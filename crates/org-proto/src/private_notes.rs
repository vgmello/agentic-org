//! Wire types and typed RPCs for per-author private notes, plans and decisions (`TASK-50`,
//! `TASK-111` ADEC-1/2/4, `L8`, `ADR-0031`, `doc-79` PN-01..06).
//!
//! Private notes live in an isolated private store outside shared task history and Git. A note
//! may belong to a task, a project, both or neither. `Debug` implementations redact `title`,
//! `why`, `alternatives`, `tags` and `body_markdown`, so private text never reaches a log.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::tasks::{TaskCommentWire, TaskView};
use crate::Call;

/// Maximum UTF-8 scalar count accepted for one private note body.
pub const PRIVATE_NOTE_MAX_CHARS: usize = 32_768;
/// Maximum UTF-8 scalar count of an explicit title.
pub const PRIVATE_NOTE_TITLE_MAX_CHARS: usize = 200;
/// Maximum UTF-8 scalar count of a title derived from the body's first non-empty line.
pub const PRIVATE_NOTE_DERIVED_TITLE_MAX_CHARS: usize = 120;
/// Maximum UTF-8 scalar count of `why` and of `alternatives`.
pub const PRIVATE_NOTE_RATIONALE_MAX_CHARS: usize = 2_000;
/// Maximum tags on one note.
pub const PRIVATE_NOTE_MAX_TAGS: usize = 8;
/// Maximum tag length in ASCII bytes (the task-label grammar `[a-z0-9._:/-]`).
pub const PRIVATE_NOTE_TAG_MAX_LEN: usize = 64;
/// Normalized tag used by sparse, updatable per-task working-checkpoint notes (`TASK-220`).
pub const PRIVATE_NOTE_CHECKPOINT_TAG: &str = "checkpoint";
/// Page size of `tasks.notes.list` when `limit` is absent.
pub const PRIVATE_NOTE_LIST_DEFAULT_LIMIT: u32 = 100;
/// Largest accepted `tasks.notes.list` page.
pub const PRIVATE_NOTE_LIST_MAX_LIMIT: u32 = 500;

/// What a private note records. Author-editable on update.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum PrivateNoteKind {
    /// Free-form working note.
    #[default]
    Note,
    /// A plan the author intends to follow.
    Plan,
    /// A decision with its title, and optionally why and the alternatives weighed.
    Decision,
}

impl PrivateNoteKind {
    /// The stored and wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::Plan => "plan",
            Self::Decision => "decision",
        }
    }

    /// Parses the stored and wire spelling.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "note" => Some(Self::Note),
            "plan" => Some(Self::Plan),
            "decision" => Some(Self::Decision),
            _ => None,
        }
    }
}

/// A content field `tasks.notes.list` may return. Identity and timestamps are always returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivateNoteField {
    Title,
    Why,
    Alternatives,
    Tags,
    BodyMarkdown,
}

impl PrivateNoteField {
    /// Every content field, in wire order.
    pub const ALL: [Self; 5] = [
        Self::Title,
        Self::Why,
        Self::Alternatives,
        Self::Tags,
        Self::BodyMarkdown,
    ];

    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Why => "why",
            Self::Alternatives => "alternatives",
            Self::Tags => "tags",
            Self::BodyMarkdown => "body_markdown",
        }
    }

    /// Parses the wire spelling.
    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|field| field.as_str() == raw)
    }
}

fn redacted<T>(value: &Option<T>) -> Option<&'static str> {
    value.as_ref().map(|_| "<redacted>")
}

fn redacted_tags(tags: &[String]) -> String {
    format!("<{} redacted>", tags.len())
}

/// Wire view of one active private note (`doc-79`). Content fields absent from a pruned
/// `tasks.notes.list` answer (`fields`) are omitted.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivateNoteWire {
    pub id: String,
    /// The task the note belongs to; `null` for a note outside any task.
    #[serde(default)]
    pub task_id: Option<String>,
    /// The project the note belongs to (a task note inherits its task's project).
    #[serde(default)]
    pub project_id: Option<String>,
    pub author_id: String,
    #[serde(default)]
    pub kind: PrivateNoteKind,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alternatives: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_markdown: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// True only when the authenticated caller is the note's author (`TASKAUTH-6`).
    pub editable: bool,
}

impl fmt::Debug for PrivateNoteWire {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrivateNoteWire")
            .field("id", &self.id)
            .field("task_id", &self.task_id)
            .field("project_id", &self.project_id)
            .field("author_id", &self.author_id)
            .field("kind", &self.kind)
            .field("revision", &self.revision)
            .field("title", &redacted(&self.title))
            .field("why", &redacted(&self.why))
            .field("alternatives", &redacted(&self.alternatives))
            .field("tags", &redacted_tags(&self.tags))
            .field("body_markdown", &redacted(&self.body_markdown))
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .field("editable", &self.editable)
            .finish()
    }
}

/// Parameters for `tasks.notes.add` (`PN-01`, ADEC-1). The author is always the bound caller.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivateNoteAddParams {
    pub command_id: String,
    /// Caller-generated canonical UUIDv7 for the new private note.
    pub note_id: String,
    /// Task UUIDv7 or office-coded key (`T-A1`); absent for a note outside any task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// Project UUIDv7; defaults to the task's project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default)]
    pub kind: PrivateNoteKind,
    /// Derived from the body's first non-empty line when absent or blank.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alternatives: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub body_markdown: String,
    /// When true, updates the caller's existing active `"checkpoint"`-tagged note on `task`
    /// (or creates one if none exists) instead of creating a duplicate note (`TASK-220`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub checkpoint: bool,
}

impl fmt::Debug for PrivateNoteAddParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrivateNoteAddParams")
            .field("command_id", &self.command_id)
            .field("note_id", &self.note_id)
            .field("task", &self.task)
            .field("project_id", &self.project_id)
            .field("kind", &self.kind)
            .field("title", &redacted(&self.title))
            .field("why", &redacted(&self.why))
            .field("alternatives", &redacted(&self.alternatives))
            .field("tags", &redacted_tags(&self.tags))
            .field("body_markdown", &"<redacted>")
            .field("checkpoint", &self.checkpoint)
            .finish()
    }
}

/// Parameters for `tasks.notes.update` (`PN-01`, `TASKAUTH-6`): an author-only patch. An absent
/// field keeps its stored value.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivateNoteUpdateParams {
    pub command_id: String,
    pub note_id: String,
    pub expected_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<PrivateNoteKind>,
    /// A blank title derives it again from the body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// A blank value clears it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// A blank value clears it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alternatives: Option<String>,
    /// Replaces every tag; `[]` clears them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_markdown: Option<String>,
}

impl fmt::Debug for PrivateNoteUpdateParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrivateNoteUpdateParams")
            .field("command_id", &self.command_id)
            .field("note_id", &self.note_id)
            .field("expected_revision", &self.expected_revision)
            .field("kind", &self.kind)
            .field("title", &redacted(&self.title))
            .field("why", &redacted(&self.why))
            .field("alternatives", &redacted(&self.alternatives))
            .field("tags", &self.tags.as_deref().map(redacted_tags))
            .field("body_markdown", &redacted(&self.body_markdown))
            .finish()
    }
}

/// Parameters for `tasks.notes.list` (`PN-02`, `TASKAUTH-4`, ADEC-2). Every filter narrows the
/// caller's visible notes: an agent sees only its own, an active person sees every author's.
/// Pages run newest first (`updated_at`, then `id`).
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivateNoteListParams {
    /// Task UUIDv7 or office-coded key (`T-A1`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<PrivateNoteKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// RFC 3339 time: only notes updated at or after it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    /// Author employee ID. People only: another agent's ID answers an agent an empty page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_id: Option<String>,
    /// 1..=[`PRIVATE_NOTE_LIST_MAX_LIMIT`]; defaults to [`PRIVATE_NOTE_LIST_DEFAULT_LIMIT`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// The `next_cursor` of the previous page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Content fields to return; empty returns them all. Omit `body_markdown` for compact recall.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<PrivateNoteField>,
}

impl fmt::Debug for PrivateNoteListParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrivateNoteListParams")
            .field("task", &self.task)
            .field("project_id", &self.project_id)
            .field("kind", &self.kind)
            .field("tag", &redacted(&self.tag))
            .field("since", &self.since)
            .field("author_id", &self.author_id)
            .field("limit", &self.limit)
            .field("cursor", &self.cursor)
            .field("fields", &self.fields)
            .finish()
    }
}

/// Response for `tasks.notes.list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksNotesListResult {
    /// The task filter's resolved UUIDv7; `null` for a list across tasks.
    #[serde(default)]
    pub task_id: Option<String>,
    pub notes: Vec<PrivateNoteWire>,
    /// Every visible note matching the filters, across all pages.
    #[serde(default)]
    pub total: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Parameters for `tasks.notes.get` (`PN-02`, `TASKAUTH-4`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksNotesGetParams {
    pub note_id: String,
}

/// Response for `tasks.notes.get`, `tasks.notes.add`, and `tasks.notes.update`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskNoteResult {
    pub note: PrivateNoteWire,
}

/// Parameters for `tasks.notes.remove` (`PN-01`, `PN-06`, `TASKAUTH-6`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksNotesRemoveParams {
    pub command_id: String,
    pub note_id: String,
    pub expected_revision: u64,
}

/// Response for `tasks.notes.remove` (revisioned tombstone metadata; content is cleared).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskNoteRemovedResult {
    pub note_id: String,
    #[serde(default)]
    pub task_id: Option<String>,
    pub revision: u64,
    pub deleted_at: String,
}

/// Parameters for `tasks.notes.publish` (`PN-05`): explicitly copies a task note's current
/// body into a new shared task comment without changing the private original's visibility.
/// A note outside any task cannot be published.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksNotesPublishParams {
    pub command_id: String,
    pub note_id: String,
    pub expected_revision: u64,
    /// Caller-generated canonical UUIDv7 for the new shared comment.
    pub comment_id: String,
}

/// Response for `tasks.notes.publish`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskNotePublishResult {
    pub note: PrivateNoteWire,
    pub comment: TaskCommentWire,
    pub task: TaskView,
}

mod legacy;
pub use legacy::{TasksNotesAddParams, TasksNotesListParams, TasksNotesUpdateParams};

/// `tasks.notes.add`
pub struct TasksNotesAdd;
impl Call for TasksNotesAdd {
    const METHOD: &'static str = "tasks.notes.add";
    type Params = PrivateNoteAddParams;
    type Result = TaskNoteResult;
}

/// `tasks.notes.list`
pub struct TasksNotesList;
impl Call for TasksNotesList {
    const METHOD: &'static str = "tasks.notes.list";
    type Params = PrivateNoteListParams;
    type Result = TasksNotesListResult;
}

/// `tasks.notes.get`
pub struct TasksNotesGet;
impl Call for TasksNotesGet {
    const METHOD: &'static str = "tasks.notes.get";
    type Params = TasksNotesGetParams;
    type Result = TaskNoteResult;
}

/// `tasks.notes.update`
pub struct TasksNotesUpdate;
impl Call for TasksNotesUpdate {
    const METHOD: &'static str = "tasks.notes.update";
    type Params = PrivateNoteUpdateParams;
    type Result = TaskNoteResult;
}

/// `tasks.notes.remove`
pub struct TasksNotesRemove;
impl Call for TasksNotesRemove {
    const METHOD: &'static str = "tasks.notes.remove";
    type Params = TasksNotesRemoveParams;
    type Result = TaskNoteRemovedResult;
}

/// `tasks.notes.publish`
pub struct TasksNotesPublish;
impl Call for TasksNotesPublish {
    const METHOD: &'static str = "tasks.notes.publish";
    type Params = TasksNotesPublishParams;
    type Result = TaskNotePublishResult;
}

#[cfg(test)]
mod tests;
