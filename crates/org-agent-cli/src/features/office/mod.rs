//! The office & owner-inbox slice: `office list|sync-status` and `inbox list`
//! (`TASK-52`, `TASK-80.4`, `IA-10`).

use org_command::{Answer, Command, CommandTable, Example, Field, Kind, Refusal, Request, Type};
use org_proto::federation::{
    OfficeDirectoryList, OfficeDirectoryListParams, OfficeDirectoryListResult, OfficeSyncStatus,
    OfficeSyncStatusParams, OfficeSyncStatusResult,
};
use org_proto::inbox::{InboxKind, InboxList, InboxListParams, InboxListResult, InboxState};
use org_proto::ErrorCode;
use serde_json::Value;

use crate::runtime::domain::{CommandCue, Domain};
use crate::runtime::input::{call_rpc, encode, opt_bool, opt_str, opt_str_list, opt_string};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport};

/// The office & owner-inbox slice as the registry composes it.
pub(crate) const DOMAIN: Domain = Domain {
    commands: &[COMMANDS],
    resources: &[
        (
            "office",
            "listing registered HQ and satellite offices and inspecting replication sync health.",
        ),
        (
            "inbox",
            "listing open or handled owner inbox items (approvals, questions, decisions, and problems).",
        ),
    ],
    cues: &[CUES],
    dispatch,
};

const READ_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotBound,
    ErrorCode::NotFound,
    ErrorCode::NotAuthorized,
    ErrorCode::Unavailable,
];

const STRING_TYPE: Type = Type::String;
const JSON_TYPE: Type = Type::Json;
const INBOX_KIND_TYPE: Type = Type::Enum(&["approval", "question", "decision", "problem"]);

const OFFICE_SUMMARY_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Office UUIDv7"),
    Field::required("organization_id", Type::String, "Organization UUIDv7"),
    Field::required("name", Type::String, "Office display name"),
    Field::required("slug", Type::String, "Office slug"),
    Field::required("role", Type::String, "hq or satellite"),
];

const OFFICE_SYNC_OUTPUT: &[Field] = &[
    Field::required("org_id", Type::String, "Organization UUIDv7"),
    Field::required("local_office", Type::String, "Local office UUIDv7"),
    Field::required("hq_office", Type::String, "HQ office UUIDv7"),
    Field::required("epoch", Type::Integer, "Current replication epoch"),
    Field::required("seq", Type::Integer, "Current organization log sequence"),
    Field::required(
        "highest_epoch",
        Type::Integer,
        "Highest observed replication epoch",
    ),
    Field::required("protocol_level", Type::Integer, "Sync protocol level"),
    Field::required(
        "restore_pending",
        Type::Boolean,
        "Whether snapshot restore is pending",
    ),
    Field::required(
        "initial_snapshot_required",
        Type::Boolean,
        "Whether initial HQ snapshot is still required",
    ),
    Field::required(
        "pending_work_count",
        Type::Integer,
        "Queued outbound work mutations",
    ),
    Field::required(
        "pending_message_count",
        Type::Integer,
        "Queued outbound messages",
    ),
];

const INBOX_LIST_INPUT: &[Field] = &[
    Field::optional(
        "state",
        Type::Enum(&["open", "handled"]),
        "Inbox state to list (default open)",
    ),
    Field::optional(
        "kind",
        Type::List(&INBOX_KIND_TYPE),
        "Filter by item kind (repeatable: approval, question, decision, problem)",
    ),
    Field::optional("project", Type::String, "Filter by project ID"),
    Field::optional("team", Type::String, "Filter by team ID"),
    Field::optional(
        "has_image",
        Type::Boolean,
        "Only items with image attachments",
    ),
    Field::optional(
        "has_file",
        Type::Boolean,
        "Only items with file attachments",
    ),
];

const INBOX_ITEM_FIELDS: &[Field] = &[
    Field::required("ref", Type::String, "Stable inbox item reference"),
    Field::required("source", Type::String, "Originating subsystem"),
    Field::required(
        "kind",
        Type::String,
        "approval, question, decision, or problem",
    ),
    Field::optional("member", Type::String, "Related member UUIDv7"),
    Field::optional("project", Type::String, "Related project UUIDv7"),
    Field::optional("team", Type::String, "Related team UUIDv7"),
    Field::required("summary", Type::String, "Concise item summary"),
    Field::optional("context", Type::String, "Supporting context"),
    Field::optional("recommendation", Type::String, "Recommended action"),
    Field::optional(
        "options",
        Type::List(&STRING_TYPE),
        "Structured answer choices, when applicable",
    ),
    Field::optional(
        "created_at_ms",
        Type::Integer,
        "Creation epoch milliseconds",
    ),
    Field::optional("expires_at_ms", Type::Integer, "Expiry epoch milliseconds"),
    Field::required("state", Type::String, "open or handled"),
    Field::optional("handled_at_ms", Type::Integer, "Handled epoch milliseconds"),
    Field::optional("outcome", Type::String, "Recorded resolution outcome"),
    Field::required("clears", Type::String, "Clearing condition"),
    Field::required("dismissable", Type::Boolean, "Whether dismissable"),
    Field::required("actions", Type::List(&JSON_TYPE), "Available item actions"),
    Field::optional(
        "attachments",
        Type::List(&JSON_TYPE),
        "Attached file metadata",
    ),
];

const COMMANDS: &[Command] = &[
    Command {
        resource: "office",
        verb: "list",
        summary: "List registered offices in the organization",
        call: "office.directory",
        kind: Kind::List,
        input: &[],
        content_field: None,
        output: OFFICE_SUMMARY_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: "{}",
            output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000001","organization_id":"0193a5c0-0000-7000-8000-000000000000","name":"HQ","slug":"hq","role":"hq"}]"#,
        },
    },
    Command {
        resource: "office",
        verb: "sync-status",
        summary: "Inspect local office replication and outbox status",
        call: "office.sync_status",
        kind: Kind::Read,
        input: &[],
        content_field: None,
        output: OFFICE_SYNC_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: "{}",
            output: r#"{"org_id":"0193a5c0-0000-7000-8000-000000000000","local_office":"0193a5c0-0000-7000-8000-000000000001","hq_office":"0193a5c0-0000-7000-8000-000000000001","epoch":1,"seq":10,"highest_epoch":1,"protocol_level":1,"restore_pending":false,"initial_snapshot_required":false,"pending_work_count":0,"pending_message_count":0}"#,
        },
    },
    Command {
        resource: "inbox",
        verb: "list",
        summary: "List owner inbox items (owner-only)",
        call: "inbox.list",
        kind: Kind::List,
        input: INBOX_LIST_INPUT,
        content_field: None,
        output: INBOX_ITEM_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: "{}",
            output: r#"[{"ref":"approval:1","source":"session","kind":"approval","summary":"Approve tool call","state":"open","clears":"answered","dismissable":false,"actions":[]}]"#,
        },
    },
];

const CUES: &[CommandCue] = &[
    (
        "office",
        "list",
        "listing HQ and enrolled satellite offices registered in the organization.",
    ),
    (
        "office",
        "sync-status",
        "checking replication epoch, log sequence, snapshot state, and pending outbound work or messages.",
    ),
    (
        "inbox",
        "list",
        "reading the owner's open or handled inbox items (approvals, questions, decisions, problems); owner-only.",
    ),
];

fn dispatch(
    transport: &mut dyn OfficeTransport,
    request: &Request,
    _table: &CommandTable,
) -> Option<Result<Answer, Refusal>> {
    match (request.command.resource, request.command.verb) {
        ("office", "list") => Some(read_office_list(transport, request)),
        ("office", "sync-status") => Some(read_office_sync_status(transport)),
        ("inbox", "list") => Some(read_inbox_list(transport, request)),
        _ => None,
    }
}

fn read_office_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    parse_cursor(request.cursor.as_deref())?;
    let res: OfficeDirectoryListResult =
        call_rpc::<OfficeDirectoryList, _>(transport, &OfficeDirectoryListParams {})?;
    let items = match encode(&res.offices, "office.directory")? {
        Value::Array(items) => items,
        _ => Vec::new(),
    };
    paginate(request, items)
}

fn read_office_sync_status<T: OfficeTransport + ?Sized>(
    transport: &mut T,
) -> Result<Answer, Refusal> {
    let res: OfficeSyncStatusResult =
        call_rpc::<OfficeSyncStatus, _>(transport, &OfficeSyncStatusParams {})?;
    Ok(Answer::completed(encode(&res, "office.sync_status")?))
}

fn parse_inbox_state(raw: Option<&str>) -> Result<InboxState, Refusal> {
    match raw {
        None | Some("open") => Ok(InboxState::Open),
        Some("handled") => Ok(InboxState::Handled),
        Some(other) => Err(Refusal::new(
            ErrorCode::InvalidRequest,
            format!("unknown inbox state `{other}`"),
            "Pass --state open or handled.",
        )),
    }
}

fn parse_inbox_kinds(raw: Option<Vec<String>>) -> Result<Option<Vec<InboxKind>>, Refusal> {
    let Some(list) = raw else {
        return Ok(None);
    };
    let mut kinds = Vec::with_capacity(list.len());
    for word in list {
        let kind = match word.as_str() {
            "approval" => InboxKind::Approval,
            "question" => InboxKind::Question,
            "decision" => InboxKind::Decision,
            "problem" => InboxKind::Problem,
            other => {
                return Err(Refusal::new(
                    ErrorCode::InvalidRequest,
                    format!("unknown inbox kind `{other}`"),
                    "Pass --kind approval, question, decision, or problem.",
                ))
            }
        };
        kinds.push(kind);
    }
    Ok(Some(kinds))
}

fn read_inbox_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = InboxListParams {
        state: parse_inbox_state(opt_str(request, "state"))?,
        kinds: parse_inbox_kinds(opt_str_list(request, "kind"))?,
        project: opt_string(request, "project"),
        team: opt_string(request, "team"),
        cursor: request.cursor.clone(),
        limit: request.limit,
        has_image: opt_bool(request, "has_image"),
        has_file: opt_bool(request, "has_file"),
    };
    let res: InboxListResult = call_rpc::<InboxList, _>(transport, &params)?;
    let data = encode(&res.items, "inbox.list")?;
    let mut answer = Answer::completed(data);
    answer.next_cursor = res.next_cursor;
    answer.total = Some(res.counts.total);
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use crate::execute_with_office;
    use crate::runtime::transport::{OfficeTransport, TransportOffice};

    struct StubOfficeTransport {
        calls: Vec<(String, Value)>,
    }

    impl OfficeTransport for StubOfficeTransport {
        fn call_method(
            &mut self,
            method: &str,
            params: Value,
        ) -> Result<Value, org_command::Refusal> {
            self.calls.push((method.to_owned(), params));
            match method {
                "office.directory" => Ok(json!({ "offices": [] })),
                "office.sync_status" => Ok(json!({
                    "org_id": "o1",
                    "local_office": "off1",
                    "hq_office": "off1",
                    "epoch": 1,
                    "seq": 2,
                    "highest_epoch": 1,
                    "protocol_level": 1,
                    "restore_pending": false,
                    "initial_snapshot_required": false,
                    "pending_work_count": 0,
                    "pending_message_count": 0
                })),
                "inbox.list" => Ok(json!({
                    "items": [],
                    "counts": {
                        "approvals": 0,
                        "questions": 0,
                        "decisions": 0,
                        "problems": 0,
                        "total": 0
                    },
                    "as_of": { "epoch": "1", "seq": 2 },
                    "degraded": []
                })),
                other => panic!("unexpected method `{other}`"),
            }
        }
    }

    #[test]
    fn office_and_inbox_commands_dispatch_expected_rpcs() {
        let mut office = TransportOffice(StubOfficeTransport { calls: Vec::new() });
        for line in [
            &["office", "list"][..],
            &["office", "sync-status"],
            &[
                "inbox", "list", "--state", "open", "--kind", "problem", "--limit", "10",
            ],
        ] {
            let args: Vec<String> = line.iter().map(|s| (*s).to_owned()).collect();
            let out = execute_with_office("agent-os", &args, &mut office);
            assert_eq!(out.exit_code, 0, "{line:?}: {}", out.stdout);
        }
        let methods: Vec<&str> = office.0.calls.iter().map(|(m, _)| m.as_str()).collect();
        assert_eq!(
            methods,
            vec!["office.directory", "office.sync_status", "inbox.list"]
        );
        assert_eq!(office.0.calls[2].1["limit"], json!(10));
        assert_eq!(office.0.calls[2].1["kinds"], json!(["problem"]));
    }
}
