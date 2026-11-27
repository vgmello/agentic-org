//! The files beside `task.md` that change without it (`doc-79`), and the office's task
//! counter (`doc-80`):
//!
//! - `comments/<comment-id>.md`: one append-only comment, YAML front matter and Markdown;
//! - `checklist.yaml`: the ordered checklist;
//! - `operations.yaml`: watchers, owner questions and waiting spans;
//! - `local/offices/<office-id>/task-counter.yaml`: the counter and its reservation.
//!
//! Each holds `schema_version: 1` and every field of its records. A YAML file is one strict
//! mapping of at most 64 KiB (`org_docs::parse_mapping`); a comment is a bounded source
//! document (`org_docs::SourceDoc`).

use org_docs::MAX_FRONT_MATTER_BYTES;
use org_proto::attachments::{AttachmentTargetKind, TASK_COMMENT_ATTACHMENTS_MAX};
use org_proto::tasks::ChecklistSection;
use org_store::Id;
use serde_json::{json, Value};

use super::attachments::{encode_attachments, read_attachments};
use super::{
    is_valid_office_code, ChecklistStepRecord, OfficeTaskCounterRecord, TaskCommentRecord,
    TaskWatcherRecord, SCHEMA_VERSION,
};
use crate::attachments::records::AttachmentRecord;
use crate::organization::records::{
    is_stable_id,
    yaml::{self, Fields, Reader},
    RecordError,
};

fn id_value(id: Option<&Id>) -> Value {
    id.map_or(Value::Null, |id| json!(id.as_str()))
}

fn stable(reader: &mut Reader, key: &str) -> Result<Id, RecordError> {
    let text = reader.text(key)?;
    if !is_stable_id(&text) {
        return Err(reader.error(key, "not a lowercase UUIDv7"));
    }
    Id::parse(&text).map_err(|_| reader.error(key, "not an ID"))
}

fn optional_stable(reader: &mut Reader, key: &str) -> Result<Option<Id>, RecordError> {
    match reader.optional_text(key)? {
        None => Ok(None),
        Some(text) if is_stable_id(&text) => Ok(Id::parse(&text).ok()),
        Some(_) => Err(reader.error(key, "not a lowercase UUIDv7")),
    }
}

fn schema(reader: &mut Reader) -> Result<(), RecordError> {
    if reader.number("schema_version")? == SCHEMA_VERSION {
        Ok(())
    } else {
        Err(reader.error("schema_version", "not 1"))
    }
}

/// One YAML mapping within the bound its reader keeps, or why it would not be.
fn bounded(fields: &Fields, what: &str) -> Result<Vec<u8>, String> {
    let bytes = yaml::encode(fields);
    if bytes.len() > MAX_FRONT_MATTER_BYTES {
        return Err(format!(
            "the task's {what} would be {} bytes, over the {MAX_FRONT_MATTER_BYTES}-byte limit",
            bytes.len()
        ));
    }
    Ok(bytes)
}

// ---- comments/<comment-id>.md ----

/// A decoded `comments/<comment-id>.md`: the comment and its comment-scoped attachments.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CommentDoc {
    pub comment: TaskCommentRecord,
    pub attachments: Vec<AttachmentRecord>,
}

pub fn encode_comment(
    comment: &TaskCommentRecord,
    attachments: &[AttachmentRecord],
) -> Result<Vec<u8>, String> {
    if attachments.len() > TASK_COMMENT_ATTACHMENTS_MAX {
        return Err(format!(
            "a task comment carries at most {TASK_COMMENT_ATTACHMENTS_MAX} attachments"
        ));
    }
    let mut fields = Fields::new();
    fields.insert("schema_version".into(), json!(SCHEMA_VERSION));
    fields.insert("id".into(), json!(comment.id.as_str()));
    fields.insert("task_id".into(), json!(comment.task_id.as_str()));
    fields.insert("author_id".into(), json!(comment.author_id.as_str()));
    fields.insert("created_at".into(), json!(comment.created_at));
    fields.insert("position".into(), json!(comment.position));
    fields.insert("attachments".into(), encode_attachments(attachments));
    let front = yaml::encode(&fields);
    if front.len() > MAX_FRONT_MATTER_BYTES {
        return Err(format!(
            "the comment's front matter would be {} bytes, over the {MAX_FRONT_MATTER_BYTES}-byte limit",
            front.len()
        ));
    }
    if comment.body_markdown.len() > org_docs::MAX_SOURCE_BODY_BYTES {
        return Err("the comment is over the body limit".into());
    }
    Ok(yaml::encode_document(&fields, &comment.body_markdown))
}

pub fn decode_comment(bytes: &[u8]) -> Result<CommentDoc, RecordError> {
    let (mut r, body) = yaml::decode_source_document(bytes)?;
    schema(&mut r)?;
    let comment = TaskCommentRecord {
        id: stable(&mut r, "id")?,
        task_id: stable(&mut r, "task_id")?,
        author_id: stable(&mut r, "author_id")?,
        created_at: r.text("created_at")?,
        position: r.number("position")?,
        body_markdown: body,
    };
    let attachments = read_attachments(
        &mut r,
        AttachmentTargetKind::TaskComment,
        &comment.id,
        TASK_COMMENT_ATTACHMENTS_MAX,
    )?;
    r.finish()?;
    Ok(CommentDoc {
        comment,
        attachments,
    })
}

// ---- checklist.yaml ----

/// `checklist.yaml`: the steps section by section (`acceptance`, `dod`, `work`; `TASK-88`),
/// each in its order; a step's position is its index among its section's steps.
pub fn encode_checklist(steps: &[ChecklistStepRecord]) -> Result<Vec<u8>, String> {
    let mut sorted: Vec<&ChecklistStepRecord> = steps.iter().collect();
    sorted.sort_by_key(|s| (s.section, s.position));
    let items: Vec<Value> = sorted
        .into_iter()
        .map(|s| {
            json!({
                "id": s.id.as_str(),
                "section": s.section.as_str(),
                "text": s.text,
                "required": s.required,
                "done": s.done,
                "linked_task_id": id_value(s.linked_task_id.as_ref()),
                "checked_by_employee_id": id_value(s.checked_by_employee_id.as_ref()),
                "checked_at": s.checked_at,
                "revision": s.revision,
            })
        })
        .collect();
    let mut fields = Fields::new();
    fields.insert("schema_version".into(), json!(SCHEMA_VERSION));
    fields.insert("checklist".into(), Value::Array(items));
    bounded(&fields, "checklist")
}

/// A step's `section`: a file written before `TASK-88` has none, and its steps are work.
fn section(item: &mut Reader) -> Result<ChecklistSection, RecordError> {
    if !item.has("section") {
        return Ok(ChecklistSection::Work);
    }
    let text = item.text("section")?;
    ChecklistSection::parse(&text)
        .ok_or_else(|| item.error("section", "not acceptance, dod or work"))
}

/// The steps of `checklist.yaml`, ordered by section and then position: each step's position
/// is how many steps of its section the file lists before it.
pub fn decode_checklist(bytes: &[u8]) -> Result<Vec<ChecklistStepRecord>, RecordError> {
    let mut r = yaml::decode(bytes)?;
    schema(&mut r)?;
    let mut steps = Vec::new();
    let mut next = [0u64; ChecklistSection::ALL.len()];
    for mut item in r.list("checklist")? {
        let id = stable(&mut item, "id")?;
        let section = section(&mut item)?;
        let slot = &mut next[section as usize];
        let position = *slot;
        *slot += 1;
        let step = ChecklistStepRecord {
            id,
            section,
            position,
            text: item.text("text")?,
            required: item.flag("required")?,
            done: item.flag("done")?,
            linked_task_id: optional_stable(&mut item, "linked_task_id")?,
            checked_by_employee_id: optional_stable(&mut item, "checked_by_employee_id")?,
            checked_at: item.optional_text("checked_at")?,
            revision: item.number("revision")?,
        };
        item.finish()?;
        // What the SQLite schema checks, decided for both engines alike.
        if step.revision == 0 {
            return Err(RecordError::new(
                "checklist",
                "a step's revision is at least 1",
            ));
        }
        if !step.done && (step.checked_by_employee_id.is_some() || step.checked_at.is_some()) {
            let reason = "an unchecked step has no checker or check time";
            return Err(RecordError::new("checklist", reason));
        }
        if steps.iter().any(|s: &ChecklistStepRecord| s.id == step.id) {
            return Err(RecordError::new("checklist", "a step is listed twice"));
        }
        steps.push(step);
    }
    r.finish()?;
    steps.sort_by_key(|s| (s.section, s.position));
    Ok(steps)
}

// ---- operations.yaml ----

/// `operations.yaml`. Owner questions and waiting spans are always written empty: no office
/// records them yet, and one that finds any refuses the file rather than lose them.
pub fn encode_operations(watchers: &[TaskWatcherRecord]) -> Result<Vec<u8>, String> {
    let mut sorted: Vec<&TaskWatcherRecord> = watchers.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    let items: Vec<Value> = sorted
        .into_iter()
        .map(|w| {
            json!({
                "id": w.id.as_str(),
                "watcher_kind": w.watcher_kind,
                "watcher_id": id_value(w.watcher_id.as_ref()),
            })
        })
        .collect();
    let mut fields = Fields::new();
    fields.insert("schema_version".into(), json!(SCHEMA_VERSION));
    fields.insert("watchers".into(), Value::Array(items));
    fields.insert("owner_questions".into(), json!([]));
    fields.insert("waiting_spans".into(), json!([]));
    bounded(&fields, "operations")
}

pub fn decode_operations(bytes: &[u8]) -> Result<Vec<TaskWatcherRecord>, RecordError> {
    let mut r = yaml::decode(bytes)?;
    schema(&mut r)?;
    let mut watchers: Vec<TaskWatcherRecord> = Vec::new();
    for mut item in r.list("watchers")? {
        let watcher = TaskWatcherRecord {
            id: stable(&mut item, "id")?,
            watcher_kind: item.text("watcher_kind")?,
            watcher_id: optional_stable(&mut item, "watcher_id")?,
        };
        match (watcher.watcher_kind.as_str(), &watcher.watcher_id) {
            ("employee", Some(_)) | ("owner", None) => {}
            _ => return Err(item.error("watcher_kind", "does not agree with watcher_id")),
        }
        item.finish()?;
        if watchers.iter().any(|w| w.id == watcher.id) {
            return Err(RecordError::new("watchers", "a watcher is listed twice"));
        }
        watchers.push(watcher);
    }
    for key in ["owner_questions", "waiting_spans"] {
        if !r.list(key)?.is_empty() {
            let reason = "not supported by this office yet; refused rather than lost";
            return Err(RecordError::new(key, reason));
        }
    }
    r.finish()?;
    Ok(watchers)
}

// ---- local/offices/<office-id>/task-counter.yaml ----

pub fn encode_counter(office_id: &Id, counter: &OfficeTaskCounterRecord) -> Vec<u8> {
    let mut fields = Fields::new();
    fields.insert("schema_version".into(), json!(SCHEMA_VERSION));
    fields.insert("office_id".into(), json!(office_id.as_str()));
    fields.insert("office_code".into(), json!(counter.office_code));
    fields.insert("last_number".into(), json!(counter.last_number));
    fields.insert("reserved_through".into(), json!(counter.reserved_through));
    yaml::encode(&fields)
}

/// The office the counter is for, and the counter.
pub fn decode_counter(bytes: &[u8]) -> Result<(Id, OfficeTaskCounterRecord), RecordError> {
    let mut r = yaml::decode(bytes)?;
    schema(&mut r)?;
    let office_id = stable(&mut r, "office_id")?;
    let counter = OfficeTaskCounterRecord {
        office_code: r.text("office_code")?,
        last_number: r.number("last_number")?,
        reserved_through: r.number("reserved_through")?,
    };
    if !is_valid_office_code(&counter.office_code) {
        return Err(r.error("office_code", "not an office code"));
    }
    if counter.reserved_through < counter.last_number {
        return Err(r.error("reserved_through", "below last_number"));
    }
    r.finish()?;
    Ok((office_id, counter))
}

#[cfg(test)]
#[path = "children_tests.rs"]
mod tests;
