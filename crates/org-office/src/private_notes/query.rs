//! Cross-task listing of private notes (`TASK-111` ADEC-2): filters, `ADR-0031` visibility,
//! newest-first keyset pages and the content fields a caller asked for.

use std::cmp::Ordering;
use std::fmt;

use org_proto::private_notes::{
    PrivateNoteField, PrivateNoteKind, PRIVATE_NOTE_LIST_DEFAULT_LIMIT, PRIVATE_NOTE_LIST_MAX_LIMIT,
};
use org_proto::{ApiError, ErrorCode};

use crate::time::{format_rfc3339_ms, parse_rfc3339_ms};

use super::PrivateNoteRecord;

fn invalid(message: &str) -> ApiError {
    ApiError::new(ErrorCode::InvalidRequest, message)
}

/// The content fields a list answer carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldSet {
    title: bool,
    why: bool,
    alternatives: bool,
    tags: bool,
    body_markdown: bool,
}

impl FieldSet {
    /// Every content field.
    pub const fn all() -> Self {
        Self {
            title: true,
            why: true,
            alternatives: true,
            tags: true,
            body_markdown: true,
        }
    }

    /// The requested fields; none requested means all of them.
    pub fn requested(fields: &[PrivateNoteField]) -> Self {
        if fields.is_empty() {
            return Self::all();
        }
        let has = |wanted| fields.contains(&wanted);
        Self {
            title: has(PrivateNoteField::Title),
            why: has(PrivateNoteField::Why),
            alternatives: has(PrivateNoteField::Alternatives),
            tags: has(PrivateNoteField::Tags),
            body_markdown: has(PrivateNoteField::BodyMarkdown),
        }
    }

    pub const fn includes(&self, field: PrivateNoteField) -> bool {
        match field {
            PrivateNoteField::Title => self.title,
            PrivateNoteField::Why => self.why,
            PrivateNoteField::Alternatives => self.alternatives,
            PrivateNoteField::Tags => self.tags,
            PrivateNoteField::BodyMarkdown => self.body_markdown,
        }
    }
}

/// Where the previous page ended: its last note's `updated_at` and `id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteCursor {
    pub updated_at: String,
    pub id: String,
}

impl NoteCursor {
    pub fn of(record: &PrivateNoteRecord) -> Self {
        Self {
            updated_at: record.updated_at.clone(),
            id: record.id.clone(),
        }
    }

    pub fn encode(&self) -> String {
        format!("{}~{}", self.updated_at, self.id)
    }

    /// Parses a `next_cursor`: a canonical millisecond UTC time and a UUIDv7.
    pub fn parse(raw: &str) -> Result<Self, ApiError> {
        let refused = || invalid("cursor is the next_cursor of a previous page");
        let (updated_at, id) = raw.split_once('~').ok_or_else(refused)?;
        let canonical = parse_rfc3339_ms(updated_at).map(format_rfc3339_ms);
        if canonical.as_deref() != Some(updated_at) || !org_proto::is_uuid_v7(id) {
            return Err(refused());
        }
        Ok(Self {
            updated_at: updated_at.to_owned(),
            id: id.to_owned(),
        })
    }
}

/// One validated `tasks.notes.list` request against an engine.
#[derive(Clone)]
pub struct NoteQuery<'a> {
    pub org_id: &'a str,
    pub caller_id: &'a str,
    pub caller_is_person: bool,
    pub task_id: Option<&'a str>,
    pub project_id: Option<&'a str>,
    pub kind: Option<PrivateNoteKind>,
    /// A normalized tag.
    pub tag: Option<&'a str>,
    /// A canonical millisecond UTC time: notes updated at or after it.
    pub since: Option<&'a str>,
    /// Author filter; another agent's ID answers an agent an empty page.
    pub author_id: Option<&'a str>,
    pub after: Option<&'a NoteCursor>,
    pub limit: usize,
}

impl fmt::Debug for NoteQuery<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NoteQuery")
            .field("org_id", &self.org_id)
            .field("caller_id", &self.caller_id)
            .field("caller_is_person", &self.caller_is_person)
            .field("task_id", &self.task_id)
            .field("project_id", &self.project_id)
            .field("kind", &self.kind)
            .field("tag", &self.tag.map(|_| "<redacted>"))
            .field("since", &self.since)
            .field("author_id", &self.author_id)
            .field("after", &self.after)
            .field("limit", &self.limit)
            .finish()
    }
}

impl NoteQuery<'_> {
    /// `ADR-0031`: the author filter the caller may apply. An agent sees only its own notes
    /// (`Some(Some(self))`), or nothing (`None`) when it asked for another author; a person
    /// sees every author's notes or the one it named.
    pub fn visible_author(&self) -> Option<Option<&str>> {
        if self.caller_is_person {
            return Some(self.author_id);
        }
        match self.author_id {
            Some(author) if author != self.caller_id => None,
            _ => Some(Some(self.caller_id)),
        }
    }

    /// Whether an active record is visible and matches every filter but the cursor.
    pub fn matches(&self, record: &PrivateNoteRecord) -> bool {
        let Some(author) = self.visible_author() else {
            return false;
        };
        record.organization_id == self.org_id
            && record.deleted_at.is_none()
            && author.is_none_or(|a| record.author_id == a)
            && self
                .task_id
                .is_none_or(|t| record.task_id.as_deref() == Some(t))
            && self
                .project_id
                .is_none_or(|p| record.project_id.as_deref() == Some(p))
            && self.kind.is_none_or(|k| record.kind == k)
            && self.tag.is_none_or(|t| record.tags.iter().any(|x| x == t))
            && self.since.is_none_or(|s| record.updated_at.as_str() >= s)
    }

    /// Whether a record comes after the cursor in newest-first order.
    pub fn is_after_cursor(&self, record: &PrivateNoteRecord) -> bool {
        self.after.is_none_or(|c| {
            (record.updated_at.as_str(), record.id.as_str())
                < (c.updated_at.as_str(), c.id.as_str())
        })
    }
}

/// One page of visible notes, newest first.
#[derive(Debug, Default)]
pub struct NotePage {
    pub notes: Vec<PrivateNoteRecord>,
    /// Every visible note matching the filters, across all pages.
    pub total: u64,
    pub next_cursor: Option<NoteCursor>,
}

impl NotePage {
    /// Cuts `limit + 1` newest-first candidates to a page, with a cursor when more follow.
    pub fn from_candidates(mut notes: Vec<PrivateNoteRecord>, total: u64, limit: usize) -> Self {
        let next_cursor = if notes.len() > limit {
            notes.truncate(limit);
            notes.last().map(NoteCursor::of)
        } else {
            None
        };
        Self {
            notes,
            total,
            next_cursor,
        }
    }
}

/// Newest first: later `updated_at`, then the larger `id`.
pub fn newest_first(a: &PrivateNoteRecord, b: &PrivateNoteRecord) -> Ordering {
    (b.updated_at.as_str(), b.id.as_str()).cmp(&(a.updated_at.as_str(), a.id.as_str()))
}

/// The page size: 1..=500, 100 when absent.
pub fn page_limit(limit: Option<u32>) -> Result<usize, ApiError> {
    let limit = limit.unwrap_or(PRIVATE_NOTE_LIST_DEFAULT_LIMIT);
    if limit == 0 || limit > PRIVATE_NOTE_LIST_MAX_LIMIT {
        return Err(invalid("limit is 1 to 500"));
    }
    Ok(limit as usize)
}

/// An RFC 3339 `since` as the canonical millisecond UTC text stored timestamps compare with.
pub fn normalize_since(raw: &str) -> Result<String, ApiError> {
    parse_rfc3339_ms(raw)
        .map(format_rfc3339_ms)
        .ok_or_else(|| invalid("since is an RFC 3339 time"))
}
