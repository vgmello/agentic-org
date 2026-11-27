//! The messaging slice: `msg` (12 verbs), `channel` (6, with the `TASK-99` catch-up ladder) and
//! `status` (2) (B6, `TASK-45`), with handlers and guidance in each file and declarative rows in
//! `org_command_catalogue::messaging`.

mod channel;
mod msg_ask;
mod msg_ask_input;
mod msg_read;
mod msg_send;
mod status;

use org_command::{Answer, CommandTable, Request};
use org_proto::ask::AskStatus;
use org_proto::directory::RecipientPresence;
use org_proto::ErrorCode;
use serde_json::{json, Map, Value};

use crate::runtime::domain::Domain;
use crate::runtime::transport::{command_id, write_ack, OfficeTransport, Refusal};

/// The messaging slice as the registry composes it.
pub(crate) const DOMAIN: Domain = Domain {
    commands: org_command_catalogue::messaging::ALL,
    resources: &[
        ("channel", "listing visible channels and what is new in them, joining/leaving manual channels, marking a channel read, or summarizing channel Q&A and catch-ups."),
        ("msg", "sending DMs or channel posts, asking structured questions, triaging your inbox, or waiting for replies."),
        ("status", "setting or clearing your short live status line visible to teammates in the directory."),
    ],
    cues: &[
        msg_send::CUES,
        msg_ask::CUES,
        msg_read::CUES,
        channel::CUES,
        status::CUES,
    ],
    dispatch,
};

fn dispatch(
    transport: &mut dyn OfficeTransport,
    request: &Request,
    _table: &CommandTable,
) -> Option<Result<Answer, Refusal>> {
    match request.command.resource {
        "msg" => dispatch_msg(transport, request),
        "channel" => channel::try_channel(transport, request),
        "status" => match request.command.verb {
            "set" => Some(status::write_status(transport, request, false)),
            "clear" => Some(status::write_status(transport, request, true)),
            _ => None,
        },
        _ => None,
    }
}

fn dispatch_msg(
    transport: &mut dyn OfficeTransport,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    Some(match request.command.verb {
        "send" => msg_send::write_msg_send(transport, request),
        "ask" => msg_ask::write_msg_ask(transport, request),
        "withdraw" => msg_ask::write_msg_withdraw(transport, request),
        "reply" => msg_send::write_msg_reply(transport, request),
        "accept" => msg_ask::write_msg_accept(transport, request),
        "inbox" => msg_read::read_msg_inbox(transport, request),
        "read" => msg_read::read_msg_read(transport, request),
        "list" => msg_read::read_msg_list(transport, request),
        "done" => msg_send::write_msg_done(transport, request),
        "defer" => msg_send::write_msg_defer(transport, request),
        "unsnooze" => msg_send::write_msg_unsnooze(transport, request),
        "wait" => msg_read::read_msg_wait(transport, request),
        _ => return None,
    })
}

/// The message's ID: the caller's UUIDv7 key as it is, or the UUIDv7 derived from any other
/// key, so a retry with the same `--idempotency-key` is the same message and never a second one.
fn message_id_for(request: &Request) -> Result<String, Refusal> {
    command_id(request, "msg-message")
}

/// Who the office reported on and woke for a message or an ask: each recipient's presence when
/// it was sent, and the agents it woke. Always both lists, empty when there is no one.
fn insert_delivery(
    obj: &mut Map<String, Value>,
    presence: &[RecipientPresence],
    woke_agents: &[String],
) {
    obj.insert(
        "recipient_presence".into(),
        serde_json::to_value(presence).unwrap_or(Value::Array(Vec::new())),
    );
    obj.insert("woke_agents".into(), json!(woke_agents));
}

/// What an ask has come to, as `msg ask`, `msg wait` and `msg read` print it.
struct AskOutcomeRef<'a> {
    status: AskStatus,
    wait_until: Option<&'a str>,
    remaining_ms: Option<u64>,
    answer: Option<&'a org_proto::ask::AskAnswerRecord>,
    withdrawal: Option<&'a org_proto::ask::AskWithdrawalRecord>,
    defaults: &'a [org_proto::ask::AskDefaultEntry],
    required_notice: Option<&'a str>,
}

/// A write's answer with the acknowledgement its row declares (`committed` for messages,
/// channels and status lines; `--durable` never reaches them, see `transport::refuse_durable`).
fn with_ack(mut answer: Answer, request: &Request) -> Answer {
    answer.ack = Some(write_ack(request));
    answer
}

fn parse_u64_cursor(cursor: Option<&str>) -> Result<Option<u64>, Refusal> {
    let Some(raw) = cursor else {
        return Ok(None);
    };
    raw.parse::<u64>().map(Some).map_err(|_| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            "Cursor must be a non-negative integer.",
            "Pass the exact `next_cursor` returned by the previous list call.",
        )
    })
}

#[cfg(test)]
mod tests;
