//! The one answer every command prints: `ok`, `data`, `meta`, `error`, in that order.

use org_proto::ErrorCode;
use serde_json::{json, Value};

use crate::node::Node;
#[cfg(doc)]
use crate::office::Request;
use crate::office::{Answer, Refusal, Status};
use crate::row::Ack;
use crate::shape::SCHEMA_DIALECT;

/// The spelling of a code: the one `org-proto` gives it on the wire.
pub fn code_name(code: ErrorCode) -> String {
    match serde_json::to_value(code) {
        Ok(Value::String(name)) => name,
        _ => "unknown".to_owned(),
    }
}

fn envelope(ok: bool, data: Node, meta: Vec<(String, Node)>, error: Node) -> Node {
    Node::map([
        ("ok", Node::Bool(ok)),
        ("data", data),
        ("meta", Node::Map(meta)),
        ("error", error),
    ])
}

/// `meta` of a command that was not refused. `key` is the [`Request`]'s idempotency key: it is
/// the operation ID reported when the office reports none.
pub(crate) fn success_meta(answer: &Answer, dry_run: bool, key: Option<&str>) -> Node {
    let mut meta = vec![("status".to_owned(), Node::text(answer.status.as_str()))];
    if let Some(id) = answer.operation_id.as_deref().or(key) {
        meta.push(("operation_id".to_owned(), Node::text(id)));
    }
    if let Some(ack) = answer.ack {
        meta.push(("ack".to_owned(), Node::text(ack.as_str())));
    }
    if let Some(revision) = answer.revision {
        meta.push(("revision".to_owned(), Node::Number(revision.into())));
    }
    if let Some(cursor) = &answer.next_cursor {
        meta.push(("next_cursor".to_owned(), Node::text(cursor)));
    }
    if let Some(total) = answer.total {
        meta.push(("total".to_owned(), Node::Number(total.into())));
    }
    if let Some(count) = answer.waiting_messages.filter(|&n| n > 0) {
        meta.push(("waiting_messages".to_owned(), Node::Number(count.into())));
    }
    if dry_run {
        meta.push(("dry_run".to_owned(), Node::Bool(true)));
    }
    Node::Map(meta)
}

/// The envelope of a command that was not refused. `data` is already in its printed order.
pub(crate) fn success(data: Node, meta: Node) -> Node {
    Node::map([
        ("ok", Node::Bool(true)),
        ("data", data),
        ("meta", meta),
        ("error", Node::Null),
    ])
}

/// The envelope of a command that made no call and has data to show (`--schema`).
pub(crate) fn local(data: Node) -> Node {
    success(
        data,
        success_meta(&Answer::completed(Value::Null), false, None),
    )
}

/// The envelope of a refusal or a failure.
pub(crate) fn failure(refusal: &Refusal, dry_run: bool) -> Node {
    let mut meta = Vec::new();
    if let Some(id) = &refusal.operation_id {
        meta.push(("operation_id".to_owned(), Node::text(id)));
    }
    if dry_run {
        meta.push(("dry_run".to_owned(), Node::Bool(true)));
    }
    let mut error = vec![
        ("code".to_owned(), Node::String(code_name(refusal.code))),
        ("message".to_owned(), Node::text(&refusal.message)),
        ("hint".to_owned(), Node::text(&refusal.hint)),
    ];
    if let Some(details) = &refusal.details {
        if let Some(field) = &details.field {
            error.push(("field".to_owned(), Node::text(field)));
        }
        if let Some(rule) = &details.rule {
            error.push(("rule".to_owned(), Node::text(rule)));
        }
        if let Some(after) = details.retry_after_ms {
            error.push(("retry_after_ms".to_owned(), Node::Number(after.into())));
        }
        if let Some(conflict) = &details.conflict {
            if let Ok(value) = serde_json::to_value(conflict) {
                error.push(("conflict".to_owned(), Node::from_value(&value)));
            }
        }
    }
    if let Some(retry_with) = &refusal.retry_with {
        error.push((
            "retry_with".to_owned(),
            Node::from_value(&Value::Object(retry_with.clone())),
        ));
    }
    envelope(false, Node::Null, meta, Node::Map(error))
}

/// The JSON Schema (draft 2020-12) every printed envelope validates against.
pub fn envelope_schema() -> Value {
    let statuses: Vec<&str> = Status::ALL.iter().map(|status| status.as_str()).collect();
    json!({
        "$schema": SCHEMA_DIALECT,
        "type": "object",
        "required": ["ok", "data", "meta", "error"],
        "additionalProperties": false,
        "properties": {
            "ok": {"type": "boolean"},
            "data": {},
            "meta": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "status": {"type": "string", "enum": statuses},
                    "operation_id": {"type": "string"},
                    "ack": {"type": "string", "enum": [Ack::Committed.as_str(), Ack::Durable.as_str()]},
                    "revision": {"type": "integer", "minimum": 0},
                    "next_cursor": {"type": "string"},
                    "total": {"type": "integer", "minimum": 0},
                    "waiting_messages": {"type": "integer", "minimum": 1},
                    "dry_run": {"type": "boolean"}
                }
            },
            "error": {}
        },
        "oneOf": [
            {
                "properties": {
                    "ok": {"const": true},
                    "meta": {"required": ["status"]},
                    "error": {"type": "null"}
                }
            },
            {
                "properties": {
                    "ok": {"const": false},
                    "data": {"type": "null"},
                    "error": {
                        "type": "object",
                        "required": ["code", "message", "hint"],
                        "additionalProperties": false,
                        "properties": {
                            "code": {"type": "string"},
                            "message": {"type": "string"},
                            "hint": {"type": "string"},
                            "field": {"type": "string"},
                            "rule": {"type": "string"},
                            "retry_after_ms": {"type": "integer", "minimum": 0},
                            "retry_with": {"type": "object"},
                            "conflict": conflict_schema()
                        }
                    }
                }
            }
        ]
    })
}

fn conflict_schema() -> Value {
    json!({
        "type": "object",
        "required": ["expected_revision", "current_revision"],
        "additionalProperties": false,
        "properties": {
            "expected_revision": {"type": "integer", "minimum": 0},
            "current_revision": {"type": "integer", "minimum": 0},
            "changed_sections": {"type": "array", "items": {"type": "string"}},
            "events_since": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["revision", "actor_kind", "change"],
                    "additionalProperties": false,
                    "properties": {
                        "revision": {"type": "integer", "minimum": 0},
                        "occurred_at": {"type": "string"},
                        "actor_kind": {"type": "string"},
                        "actor_id": {"type": "string"},
                        "change": {}
                    }
                }
            },
            "events_truncated": {"type": "boolean"},
            "omitted_events": {"type": "integer", "minimum": 0},
            "reread_recommended": {"type": "boolean"},
            "current_summary": {
                "type": "object",
                "required": ["title", "status", "priority", "checklist_total", "checklist_done"],
                "additionalProperties": false,
                "properties": {
                    "key": {"type": "string"},
                    "title": {"type": "string"},
                    "status": {"type": "string"},
                    "priority": {"type": "string"},
                    "assignee_id": {"type": "string"},
                    "assigned_team_id": {"type": "string"},
                    "checklist_total": {"type": "integer", "minimum": 0},
                    "checklist_done": {"type": "integer", "minimum": 0},
                    "ac_total": {"type": "integer", "minimum": 0},
                    "ac_done": {"type": "integer", "minimum": 0},
                    "dod_total": {"type": "integer", "minimum": 0},
                    "dod_done": {"type": "integer", "minimum": 0},
                    "work_steps_total": {"type": "integer", "minimum": 0},
                    "work_steps_done": {"type": "integer", "minimum": 0},
                    "comment_count": {"type": "integer", "minimum": 0},
                    "watcher_count": {"type": "integer", "minimum": 0}
                }
            }
        }
    })
}
