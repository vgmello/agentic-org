//! `org-command` table rows for B6 messaging, channels, and status lines (`doc-28` §3.3, `TASK-45`).

use org_command::{Ack, Command, Example, Field, IfMatch, Kind, Type};
use org_proto::ErrorCode;

const READ_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotAuthorized,
    ErrorCode::NotBound,
    ErrorCode::NotFound,
    ErrorCode::Unavailable,
];

const WRITE_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotAuthorized,
    ErrorCode::NotBound,
    ErrorCode::NotFound,
    ErrorCode::UnknownRecipient,
    ErrorCode::Conflict,
    ErrorCode::AlreadyExists,
    ErrorCode::TooLarge,
    ErrorCode::Unavailable,
];

const STRING_TYPE: Type = Type::String;

const MENTION_FIELDS: &[Field] = &[
    Field::required("employee_id", Type::String, "Mentioned employee UUIDv7"),
    Field::required("handle", Type::String, "Mentioned employee handle"),
    Field::required(
        "display_name",
        Type::String,
        "Mentioned employee display name",
    ),
];
const MENTION_TYPE: Type = Type::Object(MENTION_FIELDS);

const MSG_SEND_INPUT: &[Field] = &[
    Field::required(
        "to",
        Type::String,
        "Recipient employee ID, handle, owner, #channel, or conversation UUIDv7",
    ),
    Field::required("body", Type::String, "Message body (Markdown)"),
    Field::optional("subject", Type::String, "Optional short subject"),
    Field::optional(
        "kind",
        Type::Enum(&["msg", "handoff", "announcement", "reply", "info"]),
        "Message kind (default: msg)",
    ),
    Field::optional(
        "priority",
        Type::Enum(&["normal", "high", "urgent"]),
        "Message priority (default: normal)",
    ),
    Field::optional(
        "reply_to",
        Type::String,
        "Message ID being replied to in a thread",
    ),
    Field::optional(
        "also_in_channel",
        Type::Boolean,
        "Mirror a thread reply into the main channel timeline",
    ),
    Field::optional(
        "reason",
        Type::String,
        "Announcement reason (required for --kind announcement)",
    ),
];

const MSG_SEND_OUTPUT: &[Field] = &[
    Field::required("id", Type::String, "Committed message UUIDv7"),
    Field::required("conversation_id", Type::String, "Conversation UUIDv7"),
    Field::required("thread_id", Type::String, "Thread root message ID"),
    Field::required("local_seq", Type::Integer, "Local office message sequence"),
    Field::required(
        "origin_seq",
        Type::Integer,
        "Origin office message sequence",
    ),
    Field::required(
        "mentions",
        Type::List(&MENTION_TYPE),
        "Resolved employee mentions recorded by the office",
    ),
    Field::required(
        "unmatched_mentions",
        Type::List(&STRING_TYPE),
        "@handles that did not match an active employee",
    ),
];

const MSG_REPLY_INPUT: &[Field] = &[
    Field::required("id", Type::String, "Message ID being replied to"),
    Field::required("body", Type::String, "Reply body (Markdown)"),
    Field::optional("subject", Type::String, "Optional subject override"),
    Field::optional(
        "priority",
        Type::Enum(&["normal", "high", "urgent"]),
        "Reply priority (default: normal)",
    ),
];

const MSG_REPLY_OUTPUT: &[Field] = &[
    Field::required("id", Type::String, "Committed reply UUIDv7"),
    Field::required("thread_id", Type::String, "Thread root message ID"),
    Field::required("local_seq", Type::Integer, "Local office sequence"),
    Field::required("origin_seq", Type::Integer, "Origin office sequence"),
];

const MSG_ACCEPT_INPUT: &[Field] = &[
    Field::required("id", Type::String, "Handoff message ID to accept"),
    Field::optional("reason", Type::String, "Optional acceptance note"),
];

const MSG_ACCEPT_OUTPUT: &[Field] = &[
    Field::required("id", Type::String, "Accepted handoff message ID"),
    Field::required("accepted_by", Type::String, "Accepting employee UUIDv7"),
    Field::required("accepted_at", Type::String, "RFC 3339 UTC timestamp"),
    Field::optional("reason", Type::String, "Optional acceptance note"),
];

const MSG_INBOX_INPUT: &[Field] = &[
    Field::optional(
        "all",
        Type::Boolean,
        "Include read, snoozed, and done messages in addition to unread",
    ),
    Field::optional(
        "from",
        Type::String,
        "Filter by sender employee ID or handle",
    ),
    Field::optional("thread", Type::String, "Filter by thread root message ID"),
    Field::optional(
        "because",
        Type::Enum(&["addressed", "mention", "thread_reply", "answer_candidate"]),
        "Filter by inbox delivery reason",
    ),
];

const MSG_INBOX_ITEM_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Message ID"),
    Field::required("conversation_id", Type::String, "Conversation ID"),
    Field::required("thread_id", Type::String, "Thread root ID"),
    Field::optional("reply_to", Type::String, "Replied-to message ID"),
    Field::required("from_member_id", Type::String, "Sender member ID"),
    Field::required("from_display_name", Type::String, "Sender display name"),
    Field::required("kind", Type::String, "Message kind"),
    Field::required("priority", Type::String, "Message priority"),
    Field::required("urgent", Type::Boolean, "Whether the message is urgent"),
    Field::optional("subject", Type::String, "Optional subject"),
    Field::required("created_at", Type::String, "RFC 3339 UTC timestamp"),
    Field::required("state", Type::String, "Recipient triage state"),
    Field::required(
        "delivered_because",
        Type::Enum(&["addressed", "mention", "thread_reply", "answer_candidate"]),
        "Why this item is in the caller's inbox",
    ),
    Field::required("local_seq", Type::Integer, "Local office sequence"),
];

const MSG_READ_INPUT: &[Field] = &[Field::optional(
    "id",
    Type::String,
    "Specific message ID to read (omit to claim the next unread message)",
)];

const MSG_READ_OUTPUT: &[Field] = &[
    Field::optional(
        "id",
        Type::String,
        "Read message ID (absent when inbox empty)",
    ),
    Field::optional("thread_id", Type::String, "Thread root ID"),
    Field::optional("reply_to", Type::String, "Replied-to message ID"),
    Field::optional("from_member_id", Type::String, "Sender member ID"),
    Field::optional("from_display_name", Type::String, "Sender display name"),
    Field::optional("to_member_id", Type::String, "Recipient member ID"),
    Field::optional("kind", Type::String, "Message kind"),
    Field::optional("priority", Type::String, "Message priority"),
    Field::optional("subject", Type::String, "Subject"),
    Field::optional("body", Type::String, "Full message body"),
    Field::optional("state", Type::String, "Updated recipient state"),
    Field::optional("created_at", Type::String, "RFC 3339 UTC timestamp"),
];

const MSG_LIST_INPUT: &[Field] = &[
    Field::optional(
        "conversation",
        Type::String,
        "Conversation UUIDv7 or #channel name",
    ),
    Field::optional("channel", Type::String, "Channel name or #channel"),
    Field::optional("thread", Type::String, "Filter by thread root message ID"),
    Field::optional(
        "participant",
        Type::String,
        "Filter by participant employee ID or handle",
    ),
    Field::optional("unread_only", Type::Boolean, "Return only unread messages"),
];

const MSG_LIST_ITEM_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Message ID"),
    Field::required("thread_id", Type::String, "Thread root ID"),
    Field::optional("reply_to", Type::String, "Replied-to message ID"),
    Field::required("from_member_id", Type::String, "Sender member ID"),
    Field::required("from_display_name", Type::String, "Sender display name"),
    Field::required("kind", Type::String, "Message kind"),
    Field::required("priority", Type::String, "Message priority"),
    Field::optional("subject", Type::String, "Optional subject"),
    Field::optional(
        "body",
        Type::String,
        "Message body (when reading a conversation)",
    ),
    Field::required("created_at", Type::String, "RFC 3339 UTC timestamp"),
];

const MSG_DONE_INPUT: &[Field] = &[
    Field::required("id", Type::String, "Message ID to mark done"),
    Field::optional("note", Type::String, "Optional closing note"),
];

const MSG_DEFER_INPUT: &[Field] = &[
    Field::required("id", Type::String, "Message ID to defer/snooze"),
    Field::optional(
        "until",
        Type::String,
        "RFC 3339 UTC timestamp to resurface at",
    ),
    Field::optional(
        "after_task",
        Type::Boolean,
        "Resurface after the caller finishes its current task",
    ),
    Field::optional("note", Type::String, "Optional snooze note"),
];

const MSG_DONE_OUTPUT: &[Field] = &[
    Field::required("id", Type::String, "Message ID"),
    Field::required("state", Type::String, "New lifecycle state"),
    Field::required(
        "resolved_at",
        Type::String,
        "RFC 3339 UTC resolution timestamp",
    ),
];

const MSG_DEFER_OUTPUT: &[Field] = &[
    Field::required("id", Type::String, "Message ID"),
    Field::required("state", Type::String, "New lifecycle state"),
    Field::required("triage_gen", Type::Integer, "Triage generation counter"),
    Field::optional(
        "snoozed_until",
        Type::String,
        "Snooze wake time if deferred",
    ),
    Field::required(
        "snooze_after_task",
        Type::Boolean,
        "Whether snoozed until after current task",
    ),
];

const MSG_WAIT_INPUT: &[Field] = &[
    Field::optional(
        "timeout_ms",
        Type::Integer,
        "Hold duration in milliseconds (capped at 90000)",
    ),
    Field::optional(
        "from",
        Type::String,
        "Only wake on messages from this sender",
    ),
    Field::optional("urgent_only", Type::Boolean, "Only wake on urgent messages"),
];

const MSG_WAIT_OUTPUT: &[Field] = &[
    Field::required(
        "woken",
        Type::Boolean,
        "Whether a message arrived before timeout",
    ),
    Field::optional("id", Type::String, "Arrived message ID if woken"),
    Field::optional("thread_id", Type::String, "Thread root ID if woken"),
    Field::optional("from_member_id", Type::String, "Sender member ID if woken"),
    Field::optional("subject", Type::String, "Subject if woken"),
    Field::optional("body", Type::String, "Message body if woken"),
];

const CHANNEL_LIST_INPUT: &[Field] = &[Field::optional(
    "include_archived",
    Type::Boolean,
    "Include archived channels",
)];

const CHANNEL_SUMMARY_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Channel conversation UUIDv7"),
    Field::required("kind", Type::String, "Conversation kind"),
    Field::optional("name", Type::String, "Channel name"),
    Field::optional(
        "auto_scope",
        Type::String,
        "Automatic scope if system channel",
    ),
    Field::required("archived", Type::Boolean, "Whether archived"),
    Field::required("member_count", Type::Integer, "Active member count"),
    Field::required("is_member", Type::Boolean, "Whether the caller is a member"),
];

const CHANNEL_MEMBERSHIP_INPUT: &[Field] = &[Field::required(
    "channel",
    Type::String,
    "Manual channel name (#name or name) or conversation UUIDv7",
)];

const CHANNEL_MEMBERSHIP_OUTPUT: &[Field] = &[
    Field::required(
        "conversation_id",
        Type::String,
        "Channel conversation UUIDv7",
    ),
    Field::optional("name", Type::String, "Channel name"),
    Field::required("is_member", Type::Boolean, "Current membership state"),
];

const STATUS_SET_INPUT: &[Field] = &[
    Field::required("text", Type::String, "Short status line text"),
    Field::optional(
        "ttl_ms",
        Type::Integer,
        "Optional expiry duration in milliseconds",
    ),
];

const STATUS_CLEAR_INPUT: &[Field] = &[];

const STATUS_OUTPUT: &[Field] = &[
    Field::optional(
        "status",
        Type::String,
        "Current status text (null when cleared)",
    ),
    Field::optional(
        "status_expires_at",
        Type::String,
        "RFC 3339 UTC expiry timestamp if set",
    ),
];

pub static MSG_COMMANDS: &[Command] = &[
    Command {
        resource: "msg",
        verb: "send",
        summary: "Send a message to a person, agent, or #channel with @handle mention detection",
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
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000101","conversation_id":"0193a5c0-0000-7000-8000-000000000100","thread_id":"0193a5c0-0000-7000-8000-000000000101","local_seq":1,"origin_seq":1,"mentions":[{"employee_id":"0193a5c0-0000-7000-8000-000000000004","handle":"ada","display_name":"Ada"}],"unmatched_mentions":[]}"#,
        },
    },
    Command {
        resource: "msg",
        verb: "reply",
        summary: "Reply to a message in its thread",
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
        verb: "accept",
        summary: "Accept a handoff message addressed to the caller",
        call: "msg.accept",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: MSG_ACCEPT_INPUT,
        content_field: None,
        output: MSG_ACCEPT_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"id":"0193a5c0-0000-7000-8000-000000000101","reason":"Taking ownership"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000101","accepted_by":"0193a5c0-0000-7000-8000-000000000004","accepted_at":"2026-11-21T12:02:00.000Z","reason":"Taking ownership"}"#,
        },
    },
    Command {
        resource: "msg",
        verb: "inbox",
        summary: "List inbox items addressed to the caller with delivery reasons",
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
        summary: "Read a specific message by ID or claim the next unread message",
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
        summary: "List messages in a channel, conversation, or thread",
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
        verb: "done",
        summary: "Mark a message done in the caller's triage state",
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
        summary: "Snooze or defer a message until a time or after the current task",
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
        verb: "wait",
        summary: "Hold for at most the short hold (90s) until a matching message arrives",
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
    Command {
        resource: "channel",
        verb: "list",
        summary: "List channels visible to the caller",
        call: "chat.channels",
        kind: Kind::List,
        input: CHANNEL_LIST_INPUT,
        content_field: None,
        output: CHANNEL_SUMMARY_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: "{}",
            output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000100","kind":"channel","name":"general","auto_scope":"organization","archived":false,"member_count":2,"is_member":true}]"#,
        },
    },
    Command {
        resource: "channel",
        verb: "join",
        summary: "Join a manual channel",
        call: "chat.channel_join",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: CHANNEL_MEMBERSHIP_INPUT,
        content_field: None,
        output: CHANNEL_MEMBERSHIP_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r##"{"channel":"#ops"}"##,
            output: r#"{"conversation_id":"0193a5c0-0000-7000-8000-000000000110","name":"ops","is_member":true}"#,
        },
    },
    Command {
        resource: "channel",
        verb: "leave",
        summary: "Leave a manual channel",
        call: "chat.channel_leave",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: CHANNEL_MEMBERSHIP_INPUT,
        content_field: None,
        output: CHANNEL_MEMBERSHIP_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r##"{"channel":"#ops"}"##,
            output: r#"{"conversation_id":"0193a5c0-0000-7000-8000-000000000110","name":"ops","is_member":false}"#,
        },
    },
    Command {
        resource: "status",
        verb: "set",
        summary: "Set the bound agent's short live status line",
        call: "status.set",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: STATUS_SET_INPUT,
        content_field: None,
        output: STATUS_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"text":"Reviewing API schema","ttl_ms":600000}"#,
            output: r#"{"status":"Reviewing API schema","status_expires_at":"2026-11-21T12:10:00.000Z"}"#,
        },
    },
    Command {
        resource: "status",
        verb: "clear",
        summary: "Clear the bound agent's live status line",
        call: "status.set",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: STATUS_CLEAR_INPUT,
        content_field: None,
        output: STATUS_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: "{}",
            output: r#"{}"#,
        },
    },
];
