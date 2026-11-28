//! The office & owner-inbox slice: `office list|sync-status` and `inbox list`
//! (`TASK-52`, `TASK-80.4`, `IA-10`).

use org_command::{Answer, CommandTable, Refusal, Request};
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
    commands: org_command_catalogue::office::ALL,
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
            "review" => InboxKind::Review,
            other => {
                return Err(Refusal::new(
                    ErrorCode::InvalidRequest,
                    format!("unknown inbox kind `{other}`"),
                    "Pass --kind approval, question, decision, problem, or review.",
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
