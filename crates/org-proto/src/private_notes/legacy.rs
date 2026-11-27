//! Legacy task-scoped parameter subsets for `tasks.notes.*` (`TASK-50`).

use std::fmt;

use serde::{Deserialize, Serialize};

use super::{
    PrivateNoteAddParams, PrivateNoteKind, PrivateNoteListParams, PrivateNoteUpdateParams,
};

/// The task-scoped `tasks.notes.add` request `agent-os task note add` builds (`TASK-50`). Its
/// JSON is a subset of [`PrivateNoteAddParams`]; retire it when ADEC-3 moves that command.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksNotesAddParams {
    pub command_id: String,
    /// Task UUIDv7 or office-coded key (`T-A1`).
    pub task: String,
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

impl From<TasksNotesAddParams> for PrivateNoteAddParams {
    fn from(p: TasksNotesAddParams) -> Self {
        Self {
            command_id: p.command_id,
            note_id: p.note_id,
            task: Some(p.task),
            project_id: None,
            kind: PrivateNoteKind::Note,
            title: None,
            why: None,
            alternatives: None,
            tags: Vec::new(),
            body_markdown: p.body_markdown,
            checkpoint: false,
        }
    }
}

/// The task-scoped `tasks.notes.list` request `agent-os task note list` builds (`TASK-50`).
/// Its JSON is a subset of [`PrivateNoteListParams`]; retire it with ADEC-3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksNotesListParams {
    pub task: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_id: Option<String>,
}

impl From<TasksNotesListParams> for PrivateNoteListParams {
    fn from(p: TasksNotesListParams) -> Self {
        Self {
            task: Some(p.task),
            author_id: p.author_id,
            ..Self::default()
        }
    }
}

/// The body-only `tasks.notes.update` request `agent-os task note update` builds (`TASK-50`).
/// Its JSON is a subset of [`PrivateNoteUpdateParams`]; retire it with ADEC-3.
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

impl From<TasksNotesUpdateParams> for PrivateNoteUpdateParams {
    fn from(p: TasksNotesUpdateParams) -> Self {
        Self {
            command_id: p.command_id,
            note_id: p.note_id,
            expected_revision: p.expected_revision,
            kind: None,
            title: None,
            why: None,
            alternatives: None,
            tags: None,
            body_markdown: Some(p.body_markdown),
        }
    }
}
