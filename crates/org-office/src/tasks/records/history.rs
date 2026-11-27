//! A task's immutable history: one `canonical-json/2` event per revision at
//! `state/work-history/tasks/<task-id>/<revision>.json` (`doc-79`, `doc-84`, `ADR-0006`).
//!
//! The bytes are the canonical JSON (keys in byte order, compact, no trailing newline) the
//! office already writes for messaging's `canonical-json/2` (`yaml::canonical_json`), the
//! same bytes `org-work`'s `selected_codec` writes. `payload_sha256` is the SHA-256 of the
//! canonical JSON of the whole event with only that top-level member omitted.
//!
//! [`verify_event`] fails closed: bytes that are not exactly their canonical form, a header
//! the path does not name, an unknown member, or a checksum that does not match.

use org_store::{Id, Sha256};
use serde_json::{json, Map, Value};

use super::TaskEventRecord;
use crate::organization::records::yaml::canonical_json;

/// The largest event read (`selected-task-resources/1`).
pub const MAX_EVENT_BYTES: usize = 16 * 1024 * 1024;
/// The recorded admission profile of every event.
pub const RESOURCE_PROFILE: &str = "selected-task-resources/1";
/// The codec version of the event format.
pub const EVENT_SCHEMA_VERSION: u64 = 2;

const MEMBERS: [&str; 13] = [
    "actor_id",
    "actor_kind",
    "change",
    "occurred_at",
    "operation_id",
    "organization_id",
    "payload_sha256",
    "resource_profile",
    "revision",
    "schema_version",
    "source_metadata",
    "task_id",
    "task_kind",
];

/// What a new event says: written by an employee, at a time, for one operation.
pub struct EventInput<'a> {
    pub organization_id: &'a Id,
    pub task_id: &'a Id,
    pub revision: u64,
    pub occurred_at: &'a str,
    pub actor_id: &'a Id,
    pub operation_id: &'a str,
    /// An object with a non-empty `type`.
    pub change: Value,
}

/// The `change` of a task's revision-1 event: the facts it was created with (the front
/// matter of its first `task.md`) and its public Markdown at creation, inline.
pub fn created_change(known_facts: Map<String, Value>, body: &str) -> Value {
    json!({
        "type": "created",
        "known_facts": Value::Object(known_facts),
        "public_snapshot": {"encoding": "utf-8-markdown", "text": body},
    })
}

fn checksum(event: &Map<String, Value>) -> String {
    let mut unsigned = event.clone();
    unsigned.remove("payload_sha256");
    Sha256::of(canonical_json(&Value::Object(unsigned)).as_bytes()).to_hex()
}

/// The event's bytes and its record. The bytes are verified as a reader would before they
/// are returned, so nothing written can fail its own rebuild.
pub fn encode_event(input: EventInput<'_>) -> Result<(TaskEventRecord, Vec<u8>), String> {
    let mut event = Map::new();
    event.insert("schema_version".into(), json!(EVENT_SCHEMA_VERSION));
    event.insert("resource_profile".into(), json!(RESOURCE_PROFILE));
    event.insert(
        "organization_id".into(),
        json!(input.organization_id.as_str()),
    );
    event.insert("task_kind".into(), json!("task"));
    event.insert("task_id".into(), json!(input.task_id.as_str()));
    event.insert("revision".into(), json!(input.revision));
    event.insert("occurred_at".into(), json!(input.occurred_at));
    event.insert("actor_kind".into(), json!("employee"));
    event.insert("actor_id".into(), json!(input.actor_id.as_str()));
    event.insert("operation_id".into(), json!(input.operation_id));
    event.insert("change".into(), input.change);
    event.insert("source_metadata".into(), json!({}));
    let sum = checksum(&event);
    event.insert("payload_sha256".into(), json!(sum));
    let bytes = canonical_json(&Value::Object(event)).into_bytes();
    let record = verify_event(&bytes, input.organization_id, input.task_id, input.revision)?;
    Ok((record, bytes))
}

fn text<'a>(event: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    match event.get(key) {
        Some(Value::String(s)) if !s.is_empty() => Ok(s),
        _ => Err(format!("`{key}` is not a non-empty string")),
    }
}

fn nullable_text(event: &Map<String, Value>, key: &str) -> Result<Option<String>, String> {
    match event.get(key) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if !s.is_empty() => Ok(Some(s.clone())),
        _ => Err(format!("`{key}` is not a non-empty string or null")),
    }
}

fn is_digest(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
}

fn check_snapshot(snapshot: Option<&Value>) -> Result<(), String> {
    let Some(Value::Object(snapshot)) = snapshot else {
        return Err("`change.public_snapshot` is not an object".into());
    };
    if snapshot.get("encoding") != Some(&json!("utf-8-markdown")) {
        return Err("`change.public_snapshot.encoding` is not utf-8-markdown".into());
    }
    let blob = ["sha256", "byte_length", "resource_profile"];
    let inline = match snapshot.get("text") {
        Some(Value::String(_)) => blob.iter().all(|key| !snapshot.contains_key(*key)),
        Some(_) => false,
        None => {
            snapshot
                .get("sha256")
                .and_then(Value::as_str)
                .is_some_and(is_digest)
                && snapshot
                    .get("byte_length")
                    .and_then(Value::as_u64)
                    .is_some()
                && snapshot.get("resource_profile") == Some(&json!(RESOURCE_PROFILE))
        }
    };
    if inline {
        Ok(())
    } else {
        Err("`change.public_snapshot` is neither inline text nor a blob reference".into())
    }
}

fn check_change(change: Option<&Value>, revision: u64) -> Result<&Map<String, Value>, String> {
    let Some(Value::Object(change)) = change else {
        return Err("`change` is not an object".into());
    };
    let kind = text(change, "type")?;
    if revision == 1 && kind != "created" {
        return Err("revision 1 is the task's `created` change".into());
    }
    match kind {
        "created" => {
            if revision != 1 {
                return Err("a `created` change is only at revision 1".into());
            }
            if !matches!(change.get("known_facts"), Some(Value::Object(_))) {
                return Err("`change.known_facts` is not an object".into());
            }
            check_snapshot(change.get("public_snapshot"))?;
        }
        "assignment_changed" => {
            for side in ["from", "to"] {
                let Some(Value::Object(pair)) = change.get(side) else {
                    return Err(format!("`change.{side}` is not an object"));
                };
                for key in ["assigned_team_id", "assignee_id"] {
                    nullable_text(pair, key).map_err(|e| format!("`change.{side}`: {e}"))?;
                }
            }
        }
        // Every other change is retained, hashed and shown as it is.
        _ => {}
    }
    Ok(change)
}

fn check_actor(event: &Map<String, Value>) -> Result<(String, Option<Id>), String> {
    let kind = text(event, "actor_kind")?.to_owned();
    let actor = nullable_text(event, "actor_id")?;
    match (kind.as_str(), actor) {
        ("employee", Some(id)) => {
            let id = Id::parse(&id).map_err(|_| "`actor_id` is not an ID".to_owned())?;
            Ok((kind, Some(id)))
        }
        ("system" | "unknown", None) => Ok((kind, None)),
        _ => Err("`actor_kind` does not agree with `actor_id`".into()),
    }
}

/// The record `bytes` hold, verified against the path that names them: the organization,
/// the task and the revision. Refused unless every check passes.
pub fn verify_event(
    bytes: &[u8],
    organization_id: &Id,
    task_id: &Id,
    revision: u64,
) -> Result<TaskEventRecord, String> {
    if bytes.len() > MAX_EVENT_BYTES {
        return Err(format!("an event is at most {MAX_EVENT_BYTES} bytes"));
    }
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| "the event is not JSON".to_owned())?;
    if canonical_json(&value).as_bytes() != bytes {
        return Err("the event is not in its canonical-json/2 form".into());
    }
    let Value::Object(event) = &value else {
        return Err("the event is not an object".into());
    };
    if let Some(unknown) = event.keys().find(|k| !MEMBERS.contains(&k.as_str())) {
        return Err(format!("unknown event member `{unknown}`"));
    }
    if event.get("schema_version") != Some(&json!(EVENT_SCHEMA_VERSION)) {
        return Err("`schema_version` is not 2".into());
    }
    if text(event, "resource_profile")? != RESOURCE_PROFILE {
        return Err(format!("`resource_profile` is not {RESOURCE_PROFILE}"));
    }
    if text(event, "task_kind")? != "task" {
        return Err("`task_kind` is not task".into());
    }
    if text(event, "organization_id")? != organization_id.as_str() {
        return Err("the event is another organization's".into());
    }
    if text(event, "task_id")? != task_id.as_str() {
        return Err("`task_id` is not the task its path names".into());
    }
    let stored_revision = event.get("revision").and_then(Value::as_u64);
    if stored_revision != Some(revision) || revision == 0 || revision > i64::MAX as u64 {
        return Err("`revision` is not the revision its file name names".into());
    }
    let (actor_kind, actor_id) = check_actor(event)?;
    let occurred_at = nullable_text(event, "occurred_at")?;
    let operation_id = nullable_text(event, "operation_id")?;
    let change = check_change(event.get("change"), revision)?;
    let Some(Value::Object(metadata)) = event.get("source_metadata") else {
        return Err("`source_metadata` is not an object".into());
    };
    let stored = text(event, "payload_sha256")?;
    if !is_digest(stored) || checksum(event) != stored {
        return Err("`payload_sha256` does not match the event".into());
    }
    Ok(TaskEventRecord {
        task_id: task_id.clone(),
        revision,
        schema_version: EVENT_SCHEMA_VERSION,
        occurred_at,
        actor_kind,
        actor_id,
        operation_id,
        change_json: canonical_json(&Value::Object(change.clone())),
        source_metadata_json: canonical_json(&Value::Object(metadata.clone())),
        payload_sha256: stored.to_owned(),
        resource_profile: RESOURCE_PROFILE.to_owned(),
    })
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
