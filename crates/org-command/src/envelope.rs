//! The one answer every command prints: `ok`, `data`, `meta`, `error`, in that order.

use org_proto::ErrorCode;
use serde_json::{json, Value};

use crate::node::Node;
use crate::office::{Answer, Refusal, Status};
use crate::shape::SCHEMA_DIALECT;
use crate::table::Ack;

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

/// The envelope of a command that was not refused. `data` is already in its printed order.
pub(crate) fn success(data: Node, answer: &Answer, dry_run: bool) -> Node {
    let mut meta = vec![("status".to_owned(), Node::text(answer.status.as_str()))];
    if let Some(id) = &answer.operation_id {
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
    if dry_run {
        meta.push(("dry_run".to_owned(), Node::Bool(true)));
    }
    envelope(true, data, meta, Node::Null)
}

/// The envelope of a command that made no call and has data to show (`--schema`).
pub(crate) fn local(data: Node) -> Node {
    success(data, &Answer::completed(Value::Null), false)
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
                            "retry_with": {"type": "object"}
                        }
                    }
                }
            }
        ]
    })
}
