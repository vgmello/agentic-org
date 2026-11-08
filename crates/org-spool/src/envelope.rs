//! Versioned Envelope contract (`v = 1`) and strict untrusted-input validation.

use std::collections::BTreeMap;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::time::{format_iso8601_utc, is_valid_iso8601_utc};
use crate::ulid::{generate_ulid, is_valid_ulid};

pub const ENVELOPE_VERSION: u32 = 1;
pub const MAX_BODY_BYTES: usize = 256 * 1024;
pub const MAX_ENVELOPE_BYTES: usize = 512 * 1024;
/// Subjects this CLI sends are at most this many bytes (`msg notify` shows 80 characters).
pub const MAX_SUBJECT_BYTES: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageKind {
    Msg,
    Request,
    Reply,
}

impl MessageKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Msg => "msg",
            Self::Request => "request",
            Self::Reply => "reply",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessagePriority {
    Normal,
    High,
    Urgent,
}

impl MessagePriority {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::High => "high",
            Self::Urgent => "urgent",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "normal" => Ok(Self::Normal),
            "high" => Ok(Self::High),
            "urgent" => Ok(Self::Urgent),
            other => Err(format!(
                "invalid priority '{other}' (expected normal, high, or urgent)"
            )),
        }
    }
}

/// The v1 message envelope stored in spool files.
/// Unknown JSON fields are ignored on deserialization for forward compatibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u32,
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: MessageKind,
    pub reply_to: Option<String>,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub subject: Option<String>,
    pub body: String,
    #[serde(default)]
    pub meta: BTreeMap<String, Value>,
}

pub use serde_json::Value as MetaValue;

pub fn json_quote(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

impl Envelope {
    pub fn to_json_string(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string(self).map_err(|e| format!("serialize envelope: {e}"))
    }

    pub fn list_to_json_string(list: &[Self]) -> Result<String, String> {
        serde_json::to_string(list).map_err(|e| format!("serialize envelope list: {e}"))
    }

    /// Return the effective priority of this envelope: `meta.priority` ("normal" | "high" |
    /// "urgent"; anything else, e.g. from a v1-old sender, counts as normal), except that a
    /// `request` (`ask`) is at least `Urgent`.
    pub fn effective_priority(&self) -> MessagePriority {
        let explicit = self
            .meta
            .get("priority")
            .and_then(|v| v.as_str())
            .and_then(|p| MessagePriority::parse(p).ok())
            .unwrap_or(MessagePriority::Normal);
        if self.kind == MessageKind::Request && explicit == MessagePriority::Normal {
            MessagePriority::Urgent
        } else {
            explicit
        }
    }

    /// Returns `true` iff this message is urgent: `kind == Request` (an `ask`) or priority
    /// `High | Urgent` (the locked notify contract and ADR-0003 §2.3).
    pub fn is_urgent(&self) -> bool {
        self.kind == MessageKind::Request
            || matches!(
                self.effective_priority(),
                MessagePriority::High | MessagePriority::Urgent
            )
    }

    /// Set `meta.priority` (the Phase A spool carries priority in `meta`, ADR-0003 §2.3).
    pub fn set_priority(&mut self, priority: MessagePriority) {
        self.meta.insert(
            "priority".to_string(),
            Value::String(priority.as_str().to_string()),
        );
    }

    pub fn new_msg(
        from: impl Into<String>,
        to: impl Into<String>,
        subject: Option<String>,
        body: impl Into<String>,
        meta: BTreeMap<String, Value>,
    ) -> Result<Self, String> {
        let env = Self {
            v: ENVELOPE_VERSION,
            id: generate_ulid(),
            from: from.into(),
            to: to.into(),
            kind: MessageKind::Msg,
            reply_to: None,
            created_at: format_iso8601_utc(SystemTime::now()),
            expires_at: None,
            subject,
            body: body.into(),
            meta,
        };
        env.validate_outgoing()?;
        Ok(env)
    }

    pub fn new_request(
        from: impl Into<String>,
        to: impl Into<String>,
        subject: Option<String>,
        body: impl Into<String>,
        meta: BTreeMap<String, Value>,
        timeout: Duration,
    ) -> Result<Self, String> {
        let now = SystemTime::now();
        let expires = now.checked_add(timeout).unwrap_or(now);
        let env = Self {
            v: ENVELOPE_VERSION,
            id: generate_ulid(),
            from: from.into(),
            to: to.into(),
            kind: MessageKind::Request,
            reply_to: None,
            created_at: format_iso8601_utc(now),
            expires_at: Some(format_iso8601_utc(expires)),
            subject,
            body: body.into(),
            meta,
        };
        env.validate_outgoing()?;
        Ok(env)
    }

    pub fn new_reply(
        &self,
        from: impl Into<String>,
        subject: Option<String>,
        body: impl Into<String>,
        meta: BTreeMap<String, Value>,
    ) -> Result<Self, String> {
        let reply_subject = subject.or_else(|| {
            self.subject.as_ref().map(|s| {
                let derived = if s.starts_with("Re: ") {
                    s.clone()
                } else {
                    format!("Re: {s}")
                };
                // The original (maybe from a v1-old sender) may be at or over the cap.
                truncate_at_char_boundary(derived, MAX_SUBJECT_BYTES)
            })
        });
        let env = Self {
            v: ENVELOPE_VERSION,
            id: generate_ulid(),
            from: from.into(),
            to: self.from.clone(),
            kind: MessageKind::Reply,
            reply_to: Some(self.id.clone()),
            created_at: format_iso8601_utc(SystemTime::now()),
            expires_at: None,
            subject: reply_subject,
            body: body.into(),
            meta,
        };
        env.validate_outgoing()?;
        Ok(env)
    }

    /// Validate all structural and security invariants of the envelope.
    pub fn validate(&self) -> Result<(), String> {
        if self.v != ENVELOPE_VERSION {
            return Err(format!(
                "unsupported envelope version v={} (expected v={ENVELOPE_VERSION})",
                self.v
            ));
        }
        if !is_valid_ulid(&self.id) {
            return Err(format!("invalid ULID id '{}'", self.id));
        }
        validate_agent_id(&self.from, "from")?;
        validate_agent_id(&self.to, "to")?;

        if self.kind == MessageKind::Reply {
            match &self.reply_to {
                Some(r) if is_valid_ulid(r) => {}
                Some(r) => return Err(format!("invalid reply_to ULID '{r}'")),
                None => return Err("kind=reply requires reply_to".into()),
            }
        } else if let Some(r) = &self.reply_to {
            if !is_valid_ulid(r) {
                return Err(format!("invalid reply_to ULID '{r}'"));
            }
        }

        if !is_valid_iso8601_utc(&self.created_at) {
            return Err(format!(
                "created_at '{}' must be YYYY-MM-DDTHH:MM:SS.mmmZ",
                self.created_at
            ));
        }
        if let Some(exp) = &self.expires_at {
            if !is_valid_iso8601_utc(exp) {
                return Err(format!(
                    "expires_at '{exp}' must be YYYY-MM-DDTHH:MM:SS.mmmZ"
                ));
            }
        }
        if self.body.len() > MAX_BODY_BYTES {
            return Err(format!(
                "body size {} bytes exceeds maximum {} bytes (256 KB)",
                self.body.len(),
                MAX_BODY_BYTES
            ));
        }
        Ok(())
    }

    /// Stricter checks for envelopes this CLI creates: on top of [`Envelope::validate`], the
    /// subject is at most [`MAX_SUBJECT_BYTES`] and `meta.priority` must be `normal`, `high` or
    /// `urgent`. Envelopes read from the spool are NOT held to this (a v1-old sender may have
    /// put anything in `meta`), so an unknown priority there is treated as `normal` instead of
    /// dead-lettering the message. (The serialized size is checked by [`Envelope::to_json_bytes`].)
    pub fn validate_outgoing(&self) -> Result<(), String> {
        self.validate()?;
        if let Some(subject) = &self.subject {
            if subject.len() > MAX_SUBJECT_BYTES {
                return Err(format!(
                    "subject size {} bytes exceeds maximum {MAX_SUBJECT_BYTES} bytes",
                    subject.len()
                ));
            }
        }
        if let Some(p_val) = self.meta.get("priority") {
            let Some(p_str) = p_val.as_str() else {
                return Err("meta.priority must be a string (normal, high, or urgent)".into());
            };
            MessagePriority::parse(p_str)?;
        }
        Ok(())
    }

    pub fn is_expired_at(&self, now_iso: &str) -> bool {
        match &self.expires_at {
            Some(exp) => exp.as_str() <= now_iso,
            None => false,
        }
    }

    /// Serialize for the spool. Refuses an envelope larger than [`MAX_ENVELOPE_BYTES`] once
    /// serialized (escaping can make a body under [`MAX_BODY_BYTES`] up to 6x larger), which
    /// every reader would dead-letter.
    pub fn to_json_bytes(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|e| format!("serialize envelope: {e}"))?;
        if bytes.len() > MAX_ENVELOPE_BYTES {
            return Err(format!(
                "envelope size {} bytes exceeds limit {MAX_ENVELOPE_BYTES} bytes (512 KB) once serialized",
                bytes.len()
            ));
        }
        Ok(bytes)
    }

    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_ENVELOPE_BYTES {
            return Err(format!(
                "envelope size {} bytes exceeds limit {} bytes",
                bytes.len(),
                MAX_ENVELOPE_BYTES
            ));
        }
        let s = std::str::from_utf8(bytes).map_err(|e| format!("invalid UTF-8: {e}"))?;
        let env: Self = serde_json::from_str(s).map_err(|e| format!("invalid JSON: {e}"))?;
        env.validate()?;
        Ok(env)
    }
}

/// Cut `s` to at most `max` bytes without splitting a character.
fn truncate_at_char_boundary(mut s: String, max: usize) -> String {
    if s.len() > max {
        let mut end = max;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
    }
    s
}

pub fn validate_agent_id(id: &str, field: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 128 {
        return Err(format!("{field} agent id must be 1..=128 characters"));
    }
    if id.starts_with('.') || id.starts_with('-') || id.contains("..") {
        return Err(format!("{field} agent id '{id}' is not allowed"));
    }
    if !id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err(format!(
            "{field} agent id '{id}' may only contain ASCII letters, digits, '-' or '_'"
        ));
    }
    Ok(())
}
