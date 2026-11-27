//! Declarative rows for `msg send|reply|done|defer|unsnooze` and `msg inbox|read|list|wait`.

use org_command::{Ack, Command, Example, IfMatch, Kind};

use super::schema::{
    MSG_DEFER_INPUT, MSG_DEFER_OUTPUT, MSG_DONE_INPUT, MSG_DONE_OUTPUT, MSG_INBOX_INPUT,
    MSG_INBOX_ITEM_FIELDS, MSG_LIST_INPUT, MSG_LIST_ITEM_FIELDS, MSG_READ_INPUT, MSG_READ_OUTPUT,
    MSG_REPLY_INPUT, MSG_REPLY_OUTPUT, MSG_SEND_INPUT, MSG_SEND_OUTPUT, MSG_UNSNOOZE_INPUT,
    MSG_UNSNOOZE_OUTPUT, MSG_WAIT_INPUT, MSG_WAIT_OUTPUT, READ_ERRORS, WRITE_ERRORS,
};

/// The `msg send`, `msg reply`, `msg done`, `msg defer` and `msg unsnooze` rows.
pub const MSG_SEND_COMMANDS: &[Command] = &[
    Command {
        resource: "msg",
        verb: "send",
        summary: "Send DM or #channel message",
        call: "chat.send",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: MSG_SEND_INPUT,
        content_field: Some("body"),
        output: MSG_SEND_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r##"{"to":"#general","body":"Status update for @ada"}"##,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000101","conversation_id":"0193a5c0-0000-7000-8000-000000000100","thread_id":"0193a5c0-0000-7000-8000-000000000101","local_seq":1,"origin_seq":1,"mentions":[{"employee_id":"0193a5c0-0000-7000-8000-000000000004","handle":"ada","display_name":"Ada"}],"unmatched_mentions":[],"recipient_presence":[{"member_id":"0193a5c0-0000-7000-8000-000000000004","presence":"asleep"}],"woke_agents":["0193a5c0-0000-7000-8000-000000000004"]}"#,
        },
    },
    Command {
        resource: "msg",
        verb: "reply",
        summary: "Reply to message or ask",
        call: "msg.reply",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: MSG_REPLY_INPUT,
        content_field: Some("body"),
        output: MSG_REPLY_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"id":"0193a5c0-0000-7000-8000-000000000101","body":"On it."}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000102","thread_id":"0193a5c0-0000-7000-8000-000000000101","local_seq":2,"origin_seq":2}"#,
        },
    },
    Command {
        resource: "msg",
        verb: "done",
        summary: "Mark inbox message done",
        call: "msg.done",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: MSG_DONE_INPUT,
        content_field: None,
        output: MSG_DONE_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"id":"0193a5c0-0000-7000-8000-000000000101"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000101","state":"done","resolved_at":"2026-11-21T12:03:00.000Z"}"#,
        },
    },
    Command {
        resource: "msg",
        verb: "defer",
        summary: "Snooze or defer a message",
        call: "msg.defer",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: MSG_DEFER_INPUT,
        content_field: None,
        output: MSG_DEFER_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"id":"0193a5c0-0000-7000-8000-000000000101","after_task":true}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000101","state":"snoozed","triage_gen":1,"snooze_after_task":true}"#,
        },
    },
    Command {
        resource: "msg",
        verb: "unsnooze",
        summary: "Un-snooze a message early",
        call: "msg.unsnooze",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: MSG_UNSNOOZE_INPUT,
        content_field: None,
        output: MSG_UNSNOOZE_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"id":"0193a5c0-0000-7000-8000-000000000101"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000101","state":"unread","triage_gen":2}"#,
        },
    },
];

/// The `msg inbox`, `msg read`, `msg list` and `msg wait` rows.
pub const MSG_READ_COMMANDS: &[Command] = &[
    Command {
        resource: "msg",
        verb: "inbox",
        summary: "List inbox items and delivery reasons",
        call: "chat.inbox",
        kind: Kind::List,
        input: MSG_INBOX_INPUT,
        content_field: None,
        output: MSG_INBOX_ITEM_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: "{}",
            output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000101","conversation_id":"0193a5c0-0000-7000-8000-000000000100","thread_id":"0193a5c0-0000-7000-8000-000000000101","from_member_id":"0193a5c0-0000-7000-8000-000000000002","from_display_name":"Owner","kind":"msg","priority":"normal","urgent":false,"created_at":"2026-11-21T12:00:00.000Z","state":"unread","delivered_because":"mention","local_seq":1}]"#,
        },
    },
    Command {
        resource: "msg",
        verb: "read",
        summary: "Read message or claim next unread",
        call: "msg.read",
        kind: Kind::Read,
        input: MSG_READ_INPUT,
        content_field: None,
        output: MSG_READ_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"id":"0193a5c0-0000-7000-8000-000000000101"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000101","thread_id":"0193a5c0-0000-7000-8000-000000000101","from_member_id":"0193a5c0-0000-7000-8000-000000000002","from_display_name":"Owner","to_member_id":"0193a5c0-0000-7000-8000-000000000004","kind":"msg","priority":"normal","body":"Hello","state":"read","created_at":"2026-11-21T12:00:00.000Z"}"#,
        },
    },
    Command {
        resource: "msg",
        verb: "list",
        summary: "List channel, DM, or thread messages",
        call: "chat.messages",
        kind: Kind::List,
        input: MSG_LIST_INPUT,
        content_field: None,
        output: MSG_LIST_ITEM_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: r##"{"channel":"#general"}"##,
            output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000101","thread_id":"0193a5c0-0000-7000-8000-000000000101","from_member_id":"0193a5c0-0000-7000-8000-000000000002","from_display_name":"Owner","kind":"msg","priority":"normal","body":"Hello","created_at":"2026-11-21T12:00:00.000Z"}]"#,
        },
    },
    Command {
        resource: "msg",
        verb: "wait",
        summary: "Wait for message or ask (<=90s)",
        call: "msg.wait",
        kind: Kind::Read,
        input: MSG_WAIT_INPUT,
        content_field: None,
        output: MSG_WAIT_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"timeout_ms":5000}"#,
            output: r#"{"woken":false}"#,
        },
    },
];
