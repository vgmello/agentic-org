//! Wire types and typed RPCs for per-author private task notes (`TASK-50`, `L8`, `ADR-0031`,
//! `doc-79` PN-01..06).
//!
//! Private notes live in an isolated private store outside shared task history and Git.
//! `Debug` implementations redact `body_markdown` so note text never leaks into logs.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::tasks::{TaskCommentWire, TaskView};
use crate::Call;

/// Maximum UTF-8 scalar count accepted for one private task note body.
pub const PRIVATE_NOTE_MAX_CHARS: usize = 32_768;

/// Wire view of one active private task note (`doc-79`).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivateTaskNoteWire {
    pub id: String,
    pub task_id: String,
    pub author_id: String,
    pub revision: u64,
    pub body_markdown: String,
    pub created_at: String,
    pub updated_at: String,
    /// True only when the authenticated caller is the note's author (`TASKAUTH-6`).
    pub editable: bool,
}

impl fmt::Debug for PrivateTaskNoteWire {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrivateTaskNoteWire")
            .field("id", &self.id)
            .field("task_id", &self.task_id)
            .field("author_id", &self.author_id)
            .field("revision", &self.revision)
            .field("body_markdown", &"<redacted>")
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .field("editable", &self.editable)
            .finish()
    }
}

/// Parameters for `tasks.notes.add` (`PN-01`). Author is always derived from the bound caller.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksNotesAddParams {
    pub command_id: String,
    /// Task UUIDv7 or office-coded key (`T-A1`).
    pub task: String,
    /// Caller-generated canonical UUIDv7 for the new private note.
    pub note_id: String,
    pub body_markdown: String,
}

impl fmt::Debug for TasksNotesAddParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TasksNotesAddParams")
            .field("command_id", &self.command_id)
            .field("task", &self.task)
            .field("note_id", &self.note_id)
            .field("body_markdown", &"<redacted>")
            .finish()
    }
}

/// Parameters for `tasks.notes.list` (`PN-02`, `TASKAUTH-4`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksNotesListParams {
    /// Task UUIDv7 or office-coded key (`T-A1`).
    pub task: String,
    /// Optional author employee ID filter. Ignored/empty for non-self values when caller is an agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_id: Option<String>,
}

/// Response for `tasks.notes.list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksNotesListResult {
    pub task_id: String,
    pub notes: Vec<PrivateTaskNoteWire>,
}

/// Parameters for `tasks.notes.get` (`PN-02`, `TASKAUTH-4`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksNotesGetParams {
    pub note_id: String,
}

/// Response for `tasks.notes.get`, `tasks.notes.add`, and `tasks.notes.update`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskNoteResult {
    pub note: PrivateTaskNoteWire,
}

/// Parameters for `tasks.notes.update` (`PN-01`, `TASKAUTH-6`).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksNotesUpdateParams {
    pub command_id: String,
    pub note_id: String,
    pub expected_revision: u64,
    pub body_markdown: String,
}

impl fmt::Debug for TasksNotesUpdateParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TasksNotesUpdateParams")
            .field("command_id", &self.command_id)
            .field("note_id", &self.note_id)
            .field("expected_revision", &self.expected_revision)
            .field("body_markdown", &"<redacted>")
            .finish()
    }
}

/// Parameters for `tasks.notes.remove` (`PN-01`, `PN-06`, `TASKAUTH-6`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksNotesRemoveParams {
    pub command_id: String,
    pub note_id: String,
    pub expected_revision: u64,
}

/// Response for `tasks.notes.remove` (revisioned tombstone metadata; body is cleared).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskNoteRemovedResult {
    pub note_id: String,
    pub task_id: String,
    pub revision: u64,
    pub deleted_at: String,
}

/// Parameters for `tasks.notes.publish` (`PN-05`): explicitly copies a private note's current
/// body into a new shared task comment without changing the private original's visibility.
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
    pub note: PrivateTaskNoteWire,
    pub comment: TaskCommentWire,
    pub task: TaskView,
}

/// `tasks.notes.add`
pub struct TasksNotesAdd;
impl Call for TasksNotesAdd {
    const METHOD: &'static str = "tasks.notes.add";
    type Params = TasksNotesAddParams;
    type Result = TaskNoteResult;
}

/// `tasks.notes.list`
pub struct TasksNotesList;
impl Call for TasksNotesList {
    const METHOD: &'static str = "tasks.notes.list";
    type Params = TasksNotesListParams;
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
    type Params = TasksNotesUpdateParams;
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
mod tests {
    use super::*;

    #[test]
    fn private_note_debug_redacts_body_markdown() {
        let note = PrivateTaskNoteWire {
            id: "0193a5c0-0000-7000-8000-000000000010".into(),
            task_id: "0193a5c0-0000-7000-8000-000000000011".into(),
            author_id: "0193a5c0-0000-7000-8000-000000000012".into(),
            revision: 1,
            body_markdown: "TOP_SECRET_AGENT_REASONING".into(),
            created_at: "2026-11-22T00:00:00.000Z".into(),
            updated_at: "2026-11-22T00:00:00.000Z".into(),
            editable: true,
        };
        let dbg = format!("{note:?}");
        assert!(!dbg.contains("TOP_SECRET_AGENT_REASONING"));
        assert!(dbg.contains("<redacted>"));

        let add = TasksNotesAddParams {
            command_id: "0193a5c0-0000-7000-8000-000000000013".into(),
            task: "T-A1".into(),
            note_id: "0193a5c0-0000-7000-8000-000000000010".into(),
            body_markdown: "SECRET_DRAFT".into(),
        };
        assert!(!format!("{add:?}").contains("SECRET_DRAFT"));
    }
}
