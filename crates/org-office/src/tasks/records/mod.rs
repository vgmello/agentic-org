//! The tasks family's records, where each lives, and the codecs between a record and its
//! file bytes (`doc-77`, `doc-79`, `doc-80`, `doc-84`, `ADR-0019`):
//!
//! - a task's folder is `state/projects/<project-id>/work/tasks/<task-id>/` for a project
//!   task and `state/teams/<team chain>/work/tasks/<task-id>/` for a projectless one
//!   ([`paths`]); it holds `task.md` ([`task_md`]), `comments/<comment-id>.md`,
//!   `checklist.yaml` and `operations.yaml` ([`children`]);
//! - a task's immutable history is `state/work-history/tasks/<task-id>/<revision>.json`,
//!   `canonical-json/2` events ([`history`]);
//! - an office's task counter is office-local, `local/offices/<office-id>/task-counter.yaml`
//!   ([`children`]), never replicated and never in Git.

use std::collections::BTreeSet;

use org_proto::tasks::{
    ChecklistSection, TaskDetailsWire, TaskDoneAnywayWire, TaskFinalSummaryWire, TaskPriority,
    TaskStatus, WorkType, TASK_FILE_PATH_MAX_LEN, TASK_LABEL_MAX_LEN, TASK_MAX_FILES_PER_ROLE,
    TASK_MAX_LABELS,
};
use org_proto::{ApiError, ErrorCode};
use org_store::Id;
use serde::{Deserialize, Serialize};

use crate::organization::records::{By, Meta};

pub mod children;
pub mod history;
pub mod paths;
pub mod task_md;

pub use children::{
    decode_checklist, decode_comment, decode_counter, decode_operations, encode_checklist,
    encode_comment, encode_counter, encode_operations,
};
pub use history::{created_change, encode_event, verify_event, EventInput, MAX_EVENT_BYTES};
pub use paths::{
    checklist_path, comment_path, counter_path, event_path, history_folder, is_counter_path,
    kind_of, operations_path, parse_path, task_dir_of, task_folder, task_md_path, Kind, Location,
    TaskDir, TaskPath,
};
pub use task_md::{decode_task_md, encode_task_md, TaskDoc};

/// The schema version of every file this family writes but its history events.
pub const SCHEMA_VERSION: u64 = 1;

/// Serializable attribution metadata on [`TaskRecord`] matching `schema.sql`'s common columns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskMeta {
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    pub created_by_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    pub updated_by_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by_id: Option<Id>,
}

impl TaskMeta {
    pub fn created(actor_id: &Id, at: &str) -> Self {
        Self {
            revision: 1,
            created_at: Some(at.to_owned()),
            created_by_kind: "employee".into(),
            created_by_id: Some(actor_id.clone()),
            updated_at: Some(at.to_owned()),
            updated_by_kind: "employee".into(),
            updated_by_id: Some(actor_id.clone()),
        }
    }

    pub fn touch(&mut self, actor_id: &Id, at: &str) {
        self.revision = self.revision.saturating_add(1);
        self.updated_at = Some(at.to_owned());
        self.updated_by_kind = "employee".into();
        self.updated_by_id = Some(actor_id.clone());
    }

    pub fn to_org_meta(&self) -> Meta {
        let to_by = |kind: &str, id: Option<&Id>| match (kind, id) {
            ("employee", Some(id)) => By::Employee(id.clone()),
            ("system", None) => By::System,
            _ => By::Unknown,
        };
        Meta {
            revision: self.revision,
            created_at: self.created_at.clone(),
            created_by: to_by(&self.created_by_kind, self.created_by_id.as_ref()),
            updated_at: self.updated_at.clone(),
            updated_by: to_by(&self.updated_by_kind, self.updated_by_id.as_ref()),
        }
    }

    pub fn from_org_meta(meta: Meta) -> Self {
        let from_by = |by: By| match by {
            By::Employee(id) => ("employee".to_owned(), Some(id)),
            By::System => ("system".to_owned(), None),
            By::Unknown => ("unknown".to_owned(), None),
        };
        let (created_by_kind, created_by_id) = from_by(meta.created_by);
        let (updated_by_kind, updated_by_id) = from_by(meta.updated_by);
        Self {
            revision: meta.revision,
            created_at: meta.created_at,
            created_by_kind,
            created_by_id,
            updated_at: meta.updated_at,
            updated_by_kind,
            updated_by_id,
        }
    }
}

/// An office's monotonic task number counter with its durable block reservation
/// (`local/offices/<office-id>/task-counter.yaml`, `doc-80`, `ADR-0019`):
/// `last_number <= reserved_through`, and no number above `reserved_through` is ever issued
/// before a new reservation is durable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeTaskCounterRecord {
    pub office_code: String,
    pub last_number: u64,
    pub reserved_through: u64,
}

/// Why a task was completed with required steps unticked, and which (`TASK-88`,
/// `task_details.done_anyway_json`): `{"reason": ..., "steps": [step IDs]}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoneAnywayRecord {
    pub reason: String,
    pub steps: Vec<Id>,
}

impl DoneAnywayRecord {
    pub fn to_wire(&self) -> TaskDoneAnywayWire {
        TaskDoneAnywayWire {
            reason: self.reason.clone(),
            steps: self.steps.iter().map(|id| id.as_str().to_owned()).collect(),
        }
    }
}

/// A task's attributed closing summary (`TASK-88`, `tasks.final_summary*`): always written by
/// an employee, so all four columns are set together or none is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinalSummaryRecord {
    pub text: String,
    pub at: String,
    pub by_id: Id,
}

impl FinalSummaryRecord {
    /// The only `final_summary_by_kind` a summary has.
    pub const BY_KIND: &'static str = "employee";

    pub fn to_wire(&self) -> TaskFinalSummaryWire {
        TaskFinalSummaryWire {
            text: self.text.clone(),
            at: self.at.clone(),
            by_kind: Self::BY_KIND.to_owned(),
            by_id: self.by_id.as_str().to_owned(),
        }
    }
}

/// Optional structured details stored alongside a task (`task_details` in `schema.sql`).
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskDetailsRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub what_happened: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default)]
    pub severity_confirmed: bool,
    #[serde(default)]
    pub reopen_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_of_task_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affects_feature_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routed_by_employee_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_into_task_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_triager_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_verifier_id: Option<Id>,
    /// Set by `tasks.transition` only (`TASK-88`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_anyway: Option<DoneAnywayRecord>,
}

impl TaskDetailsRecord {
    pub fn to_wire(&self) -> TaskDetailsWire {
        TaskDetailsWire {
            what_happened: self.what_happened.clone(),
            expected: self.expected.clone(),
            steps: self.steps.clone(),
            environment: self.environment.clone(),
            severity_confirmed: self.severity_confirmed,
            reopen_count: self.reopen_count,
            duplicate_of_task_id: self
                .duplicate_of_task_id
                .as_ref()
                .map(|id| id.as_str().to_owned()),
            closed_reason: self.closed_reason.clone(),
            affects_feature_id: self
                .affects_feature_id
                .as_ref()
                .map(|id| id.as_str().to_owned()),
            request_kind: self.request_kind.clone(),
            request_text: self.request_text.clone(),
            route_reason: self.route_reason.clone(),
            routing_source: self.routing_source.clone(),
            routed_by_employee_id: self
                .routed_by_employee_id
                .as_ref()
                .map(|id| id.as_str().to_owned()),
            merged_into_task_id: self
                .merged_into_task_id
                .as_ref()
                .map(|id| id.as_str().to_owned()),
            suggested_triager_id: self
                .suggested_triager_id
                .as_ref()
                .map(|id| id.as_str().to_owned()),
            suggested_verifier_id: self
                .suggested_verifier_id
                .as_ref()
                .map(|id| id.as_str().to_owned()),
            done_anyway: self.done_anyway.as_ref().map(DoneAnywayRecord::to_wire),
        }
    }
}

/// One repository attached to a task with its branch and optional integrated commit
/// (`task_repositories` in `schema.sql`, `doc-35`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRepositoryRecord {
    pub repository_id: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrated_commit: Option<String>,
}

/// Core unified task record (the front matter and body of `task.md`, `doc-77`, `TASK-49`,
/// `TASK-77`). In the projection `meta.revision` is the effective revision:
/// `max(task.md revision, latest history event revision)` (`doc-79`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRecord {
    pub id: Id,
    pub office_code: String,
    pub number: u64,
    pub team_id: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assigned_team_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Id>,
    #[serde(rename = "type")]
    pub task_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_type: Option<WorkType>,
    pub title: String,
    pub status: TaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee_id: Option<Id>,
    #[serde(default)]
    pub body_markdown: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub milestone_id: Option<Id>,
    pub priority: TaskPriority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank_order: Option<u64>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub reference_files: Vec<String>,
    #[serde(default)]
    pub modified_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repositories: Vec<TaskRepositoryRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<TaskDetailsRecord>,
    /// The attributed closing summary (`TASK-88`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_summary: Option<FinalSummaryRecord>,
    pub meta: TaskMeta,
}

impl TaskRecord {
    pub fn key(&self) -> String {
        format!("T-{}{}", self.office_code, self.number)
    }
}

/// One checklist step (`checklist.yaml` -> `checklist_steps`). `position` is 0-based and
/// contiguous within `section` (`TASK-88`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChecklistStepRecord {
    pub id: Id,
    #[serde(default)]
    pub section: ChecklistSection,
    pub position: u64,
    pub text: String,
    pub required: bool,
    pub done: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_task_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_by_employee_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<String>,
    pub revision: u64,
}

/// One append-only public comment (`comments/<comment-id>.md` -> `comments`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskCommentRecord {
    pub task_id: Id,
    pub id: Id,
    pub position: u64,
    pub author_id: Id,
    pub created_at: String,
    pub body_markdown: String,
}

/// One task watcher (`operations.yaml` `watchers[]` -> `watchers`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskWatcherRecord {
    pub id: Id,
    pub watcher_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watcher_id: Option<Id>,
}

/// One typed link from a task (`task.md` `links[]` -> `links`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskLinkRecord {
    pub source_id: Id,
    pub id: Id,
    /// `"depends_on"`, `"relates_to"`, or `"fixes"`.
    pub kind: String,
    pub target_kind: String,
    pub target_id: Id,
}

/// One immutable task history event (`state/work-history/tasks/<task-id>/<revision>.json`
/// -> `task_events`), as verified from its `canonical-json/2` bytes ([`history`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskEventRecord {
    pub task_id: Id,
    pub revision: u64,
    pub schema_version: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<String>,
    pub actor_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    pub change_json: String,
    pub source_metadata_json: String,
    pub payload_sha256: String,
    pub resource_profile: String,
}

pub fn is_valid_office_code(code: &str) -> bool {
    let bytes = code.as_bytes();
    (bytes.len() == 1 || bytes.len() == 2)
        && bytes
            .iter()
            .all(|b| matches!(b, b'A'..=b'H' | b'J'..=b'N' | b'P'..=b'Z'))
}

/// Parses an office-coded task key such as `T-A1` or `t-bc42` into `(office_code, number)`.
pub fn parse_task_key(raw: &str) -> Option<(String, u64)> {
    let trimmed = raw.trim();
    let rest = trimmed
        .strip_prefix("T-")
        .or_else(|| trimmed.strip_prefix("t-"))?;
    let split = rest.find(|c: char| c.is_ascii_digit())?;
    let code = rest[..split].to_ascii_uppercase();
    let num_str = &rest[split..];
    if !is_valid_office_code(&code)
        || num_str.is_empty()
        || num_str.starts_with('0')
        || !num_str.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let number = num_str.parse::<u64>().ok()?;
    (number > 0 && number <= i64::MAX as u64).then_some((code, number))
}

/// Normalizes and validates one task label (`TASK-77`), matching `schema.sql`'s `task_labels` CHECK.
pub fn normalize_label(raw: &str) -> Result<String, ApiError> {
    let label = raw.trim().to_ascii_lowercase();
    let bytes = label.as_bytes();
    let valid_char = |b: u8| {
        b.is_ascii_lowercase()
            || b.is_ascii_digit()
            || matches!(b, b'.' | b'_' | b':' | b'/' | b'-')
    };
    let valid_edge = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    if bytes.is_empty()
        || bytes.len() > TASK_LABEL_MAX_LEN
        || !valid_edge(bytes[0])
        || !valid_edge(bytes[bytes.len() - 1])
        || !bytes.iter().copied().all(valid_char)
    {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!(
                "invalid task label `{raw}`: use 1..={TASK_LABEL_MAX_LEN} lowercase letters/digits with optional . _ : / -"
            ),
        ));
    }
    Ok(label)
}

/// Normalizes, deduplicates, and sorts a label list (`TASK-77`).
pub fn normalize_labels(raw: &[String]) -> Result<Vec<String>, ApiError> {
    let mut set = BTreeSet::new();
    for item in raw {
        set.insert(normalize_label(item)?);
    }
    if set.len() > TASK_MAX_LABELS {
        return Err(ApiError::new(
            ErrorCode::TooLarge,
            format!("at most {TASK_MAX_LABELS} labels may be attached to one task"),
        ));
    }
    Ok(set.into_iter().collect())
}

/// Validates and normalizes one relative file path for `reference_files` or `modified_files`
/// (`TASK-77`), rejecting absolute paths, traversal segments, backslashes, and control characters.
pub fn normalize_file_path(raw: &str) -> Result<String, ApiError> {
    let mut trimmed = raw.trim();
    while let Some(stripped) = trimmed.strip_prefix("./") {
        trimmed = stripped;
    }
    if trimmed.is_empty() || trimmed.len() > TASK_FILE_PATH_MAX_LEN {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!("invalid task file path `{raw}`: must be 1..={TASK_FILE_PATH_MAX_LEN} bytes"),
        ));
    }
    if trimmed.starts_with('/')
        || trimmed.contains('\\')
        || trimmed.contains("..")
        || trimmed.bytes().any(|b| b < 0x20 || b == 0x7f)
    {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!(
                "invalid task file path `{raw}`: must be a relative path without `..`, backslashes, or control characters"
            ),
        ));
    }
    Ok(trimmed.to_owned())
}

/// Normalizes, deduplicates, and sorts a list of relative file paths (`TASK-77`).
pub fn normalize_file_paths(raw: &[String], role: &str) -> Result<Vec<String>, ApiError> {
    let mut set = BTreeSet::new();
    for item in raw {
        set.insert(normalize_file_path(item)?);
    }
    if set.len() > TASK_MAX_FILES_PER_ROLE {
        return Err(ApiError::new(
            ErrorCode::TooLarge,
            format!("at most {TASK_MAX_FILES_PER_ROLE} {role} files may be attached to one task"),
        ));
    }
    Ok(set.into_iter().collect())
}
