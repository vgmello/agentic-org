//! Runtime presence and self-reported status lines (`ADR-0003` §2.4, `org_proto::directory::StatusSet`).
//!
//! Presence and status lines are runtime operational state (never in the organization Git log).
//! Implements [`RuntimeView`] so `who` surfaces live presence and unexpired status lines.

use std::collections::HashMap;
use std::sync::Mutex;

use org_proto::directory::{
    MemberPresence, MemberPresenceChangedData, StatusSetParams, StatusSetResult,
    DEFAULT_STATUS_TTL_MS, MAX_STATUS_TTL_MS, STATUS_MAX_BYTES, STATUS_MAX_CHARS,
};
use org_proto::{ApiError, ErrorCode};

use crate::organization::directory::{MemberRuntime, RuntimeView};
use crate::time;

/// Thread-safe in-memory runtime presence and status line store for one office.
#[derive(Default)]
pub struct OfficeRuntime {
    members: Mutex<HashMap<String, MemberRuntime>>,
}

impl OfficeRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets or clears `member_id`'s self-reported status line, validating every rule of
    /// [`org_proto::directory::StatusSet`]. Returns the wire result and the presence event
    /// to publish onto `LiveEvents`.
    pub fn set_status(
        &self,
        member_id: &str,
        params: &StatusSetParams,
        now_ms: u64,
    ) -> Result<(StatusSetResult, MemberPresenceChangedData), ApiError> {
        if let Some(ttl) = params.ttl_ms {
            if ttl > MAX_STATUS_TTL_MS {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "status ttl_ms exceeds the 24-hour maximum",
                ));
            }
        }
        let now_rfc = time::format_rfc3339_ms(now_ms);
        let cleared = params.ttl_ms == Some(0)
            || params
                .text
                .as_deref()
                .is_none_or(|text| text.trim().is_empty());

        let (status, expires_at) = if cleared {
            (None, None)
        } else {
            let trimmed = validate_status_text(params.text.as_deref().unwrap_or(""))?;
            let ttl = params.ttl_ms.unwrap_or(DEFAULT_STATUS_TTL_MS);
            let exp_ms = now_ms.saturating_add(ttl);
            (Some(trimmed), Some(time::format_rfc3339_ms(exp_ms)))
        };

        let mut guard = self.members.lock().unwrap_or_else(|p| p.into_inner());
        let entry = guard
            .entry(member_id.to_owned())
            .or_insert_with(|| MemberRuntime {
                presence: MemberPresence::Idle,
                last_seen_at: Some(now_rfc.clone()),
                status: None,
                status_expires_at: None,
                current_task_id: None,
            });
        entry.last_seen_at = Some(now_rfc.clone());
        entry.status = status.clone();
        entry.status_expires_at = expires_at.clone();

        let event = MemberPresenceChangedData {
            member_id: member_id.to_owned(),
            presence: entry.presence,
            last_seen_at: now_rfc,
            status: status.clone(),
            status_expires_at: expires_at.clone(),
            current_task_id: entry.current_task_id.clone(),
        };
        let result = StatusSetResult { status, expires_at };
        Ok((result, event))
    }

    /// Updates a member's presence (e.g. `Online`, `Idle`, `Busy`, `Offline`) and returns
    /// the resulting [`MemberPresenceChangedData`] event.
    pub fn set_presence(
        &self,
        member_id: &str,
        presence: MemberPresence,
        now_ms: u64,
    ) -> MemberPresenceChangedData {
        let now_rfc = time::format_rfc3339_ms(now_ms);
        let mut guard = self.members.lock().unwrap_or_else(|p| p.into_inner());
        let entry = guard
            .entry(member_id.to_owned())
            .or_insert_with(|| MemberRuntime {
                presence,
                last_seen_at: Some(now_rfc.clone()),
                status: None,
                status_expires_at: None,
                current_task_id: None,
            });
        entry.presence = presence;
        entry.last_seen_at = Some(now_rfc.clone());
        MemberPresenceChangedData {
            member_id: member_id.to_owned(),
            presence: entry.presence,
            last_seen_at: now_rfc,
            status: entry.status.clone(),
            status_expires_at: entry.status_expires_at.clone(),
            current_task_id: entry.current_task_id.clone(),
        }
    }
}

impl RuntimeView for OfficeRuntime {
    fn runtime_of(&self, member_id: &str) -> Option<MemberRuntime> {
        self.members
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(member_id)
            .cloned()
    }
}

fn validate_status_text(raw: &str) -> Result<String, ApiError> {
    let trimmed = raw.trim();
    if trimmed.chars().count() > STATUS_MAX_CHARS || trimmed.len() > STATUS_MAX_BYTES {
        return Err(ApiError::new(
            ErrorCode::TooLarge,
            "status text exceeds the length limit",
        ));
    }
    if trimmed.chars().any(is_disallowed_status_char) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "status text contains control, line-break, or hidden-text characters",
        ));
    }
    Ok(trimmed.to_owned())
}

fn is_disallowed_status_char(c: char) -> bool {
    if c.is_control() {
        return true;
    }
    matches!(
        c as u32,
        0x115F
            | 0x1160
            | 0x180E
            | 0x200B
            | 0x2028
            | 0x2029
            | 0x202A..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x206F
            | 0x3164
            | 0xFEFF
            | 0xFFA0
            | 0xFFF9..=0xFFFB
            | 0xE0000..=0xE007F
            | 0xE0100..=0xE01EF
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_sets_and_clears_status_lines() {
        let rt = OfficeRuntime::new();
        let now = 1_762_598_400_000_u64;
        let (res, ev) = rt
            .set_status(
                "m1",
                &StatusSetParams {
                    text: Some("  Reviewing B6  ".into()),
                    ttl_ms: Some(60_000),
                },
                now,
            )
            .unwrap();
        assert_eq!(res.status.as_deref(), Some("Reviewing B6"));
        assert!(res.expires_at.is_some());
        assert_eq!(ev.status.as_deref(), Some("Reviewing B6"));
        assert_eq!(
            rt.runtime_of("m1").unwrap().status.as_deref(),
            Some("Reviewing B6")
        );

        // Clear via empty text
        let (cleared, ev2) = rt
            .set_status("m1", &StatusSetParams::default(), now + 1000)
            .unwrap();
        assert_eq!(cleared.status, None);
        assert_eq!(ev2.status, None);

        // Reject control / hidden / overlong / bad ttl
        assert_eq!(
            rt.set_status(
                "m1",
                &StatusSetParams {
                    text: Some("line1\nline2".into()),
                    ttl_ms: None
                },
                now
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            rt.set_status(
                "m1",
                &StatusSetParams {
                    text: Some("hidden\u{202E}bidi".into()),
                    ttl_ms: None
                },
                now
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            rt.set_status(
                "m1",
                &StatusSetParams {
                    text: Some("a".repeat(STATUS_MAX_CHARS + 1)),
                    ttl_ms: None
                },
                now
            )
            .unwrap_err()
            .code,
            ErrorCode::TooLarge
        );
        assert_eq!(
            rt.set_status(
                "m1",
                &StatusSetParams {
                    text: Some("ok".into()),
                    ttl_ms: Some(MAX_STATUS_TTL_MS + 1)
                },
                now
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidRequest
        );
    }
}
