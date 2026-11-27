//! The messaging slice's error lists and the field schema the `msg` rows share (send, reply,
//! inbox, read, list, done, defer, unsnooze, wait); `msg ask`, `channel` and `status` keep theirs
//! in their own files.

use org_command::{Field, Type};
use org_proto::ErrorCode;

pub(super) const READ_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotAuthorized,
    ErrorCode::NotBound,
    ErrorCode::NotFound,
    ErrorCode::Unavailable,
];

pub(super) const WRITE_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotAuthorized,
    ErrorCode::NotBound,
    ErrorCode::NotFound,
    ErrorCode::UnknownRecipient,
    ErrorCode::Conflict,
    ErrorCode::AlreadyExists,
    ErrorCode::TooLarge,
    ErrorCode::Unsupported,
    ErrorCode::Unavailable,
];

pub(super) const STRING_TYPE: Type = Type::String;

pub(super) const MENTION_FIELDS: &[Field] = &[
    Field::required("employee_id", Type::String, "Mentioned employee UUIDv7"),
    Field::required("handle", Type::String, "Mentioned employee handle"),
    Field::required(
        "display_name",
        Type::String,
        "Mentioned employee display name",
    ),
];

pub(super) const MENTION_TYPE: Type = Type::Object(MENTION_FIELDS);

pub(super) const MSG_SEND_INPUT: &[Field] = &[
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

pub(super) const MSG_SEND_OUTPUT: &[Field] = &[
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
    RECIPIENT_PRESENCE_FIELD,
    WOKE_AGENTS_FIELD,
];

const RECIPIENT_PRESENCE_ITEM: &[Field] = &[
    Field::required("member_id", Type::String, "Recipient employee UUIDv7"),
    Field::required(
        "presence",
        Type::String,
        "Recipient presence when sent (e.g. available, busy, asleep, offline)",
    ),
    Field::optional("detail", Type::String, "What the presence means right now"),
    Field::optional(
        "away_since",
        Type::String,
        "RFC 3339 UTC time the recipient went away",
    ),
];

const RECIPIENT_PRESENCE_TYPE: Type = Type::Object(RECIPIENT_PRESENCE_ITEM);

/// Each recipient's presence when the message or ask was sent, so a sender knows who will see
/// it soon.
pub(super) const RECIPIENT_PRESENCE_FIELD: Field = Field::required(
    "recipient_presence",
    Type::List(&RECIPIENT_PRESENCE_TYPE),
    "Each recipient's presence when sent (empty when none was reported)",
);

/// The agents the office woke to handle the message or ask.
pub(super) const WOKE_AGENTS_FIELD: Field = Field::required(
    "woke_agents",
    Type::List(&STRING_TYPE),
    "Agent employee UUIDv7s the office woke for it",
);

pub(super) const MSG_REPLY_INPUT: &[Field] = &[
    Field::required("id", Type::String, "Message or ask ID being replied to"),
    Field::optional("body", Type::String, "Reply body (Markdown)"),
    Field::optional("subject", Type::String, "Optional subject override"),
    Field::optional(
        "priority",
        Type::Enum(&["normal", "high", "urgent"]),
        "Reply priority (default: normal)",
    ),
    Field::optional(
        "answer",
        Type::String,
        "JSON array of structured question answers (or object) for a structured ask",
    ),
];

pub(super) const MSG_REPLY_OUTPUT: &[Field] = &[
    Field::required("id", Type::String, "Committed reply or answered ask UUIDv7"),
    Field::optional("thread_id", Type::String, "Thread root message ID"),
    Field::optional("local_seq", Type::Integer, "Local office sequence"),
    Field::optional("origin_seq", Type::Integer, "Origin office sequence"),
    Field::optional(
        "conversation_id",
        Type::String,
        "Conversation UUIDv7 when answering an ask",
    ),
    Field::optional(
        "answer",
        Type::String,
        "Recorded answer JSON when answering an ask",
    ),
];

pub(super) const MSG_INBOX_INPUT: &[Field] = &[
    Field::optional(
        "all",
        Type::Boolean,
        "Include read, snoozed, and done messages in addition to unread",
    ),
    Field::optional(
        "state",
        Type::Enum(&["unread", "snoozed", "read", "done"]),
        "Filter by recipient lifecycle state (e.g. snoozed)",
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

pub(super) const MSG_INBOX_ITEM_FIELDS: &[Field] = &[
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

pub(super) const MSG_READ_INPUT: &[Field] = &[Field::optional(
    "id",
    Type::String,
    "Specific message ID to read (omit to claim the next unread message)",
)];

pub(super) const MSG_READ_OUTPUT: &[Field] = &[
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

pub(super) const MSG_LIST_INPUT: &[Field] = &[
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
    Field::optional(
        "unread_only",
        Type::Boolean,
        "DM history only: unread messages (in a channel use --since-read)",
    ),
    Field::optional(
        "since_read",
        Type::Boolean,
        "Only others' messages after your read marker, oldest first",
    ),
    Field::optional(
        "after_id",
        Type::String,
        "Only messages after this message UUIDv7, oldest first",
    ),
    Field::optional(
        "top_level_only",
        Type::Boolean,
        "Only thread roots (and replies also posted to the channel)",
    ),
    Field::optional(
        "headers_only",
        Type::Boolean,
        "Omit body: answer a one-line snippet and body_bytes",
    ),
];

pub(super) const MSG_LIST_ITEM_FIELDS: &[Field] = &[
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
    Field::optional(
        "snippet",
        Type::String,
        "--headers-only: body on one line, at most 160 characters",
    ),
    Field::optional(
        "body_bytes",
        Type::Integer,
        "--headers-only: full body size in bytes",
    ),
    Field::required("created_at", Type::String, "RFC 3339 UTC timestamp"),
    Field::optional("reply_count", Type::Integer, "Replies in this thread"),
    Field::optional(
        "latest_reply_at",
        Type::String,
        "RFC 3339 UTC time of the thread's latest reply",
    ),
    Field::optional("attachment_count", Type::Integer, "Attached files"),
];

pub(super) const MSG_DONE_INPUT: &[Field] = &[
    Field::required("id", Type::String, "Message ID to mark done"),
    Field::optional("note", Type::String, "Optional closing note"),
];

pub(super) const MSG_DEFER_INPUT: &[Field] = &[
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

pub(super) const MSG_DONE_OUTPUT: &[Field] = &[
    Field::required("id", Type::String, "Message ID"),
    Field::required("state", Type::String, "New lifecycle state"),
    Field::required(
        "resolved_at",
        Type::String,
        "RFC 3339 UTC resolution timestamp",
    ),
];

pub(super) const MSG_DEFER_OUTPUT: &[Field] = &[
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

pub(super) const MSG_UNSNOOZE_INPUT: &[Field] = &[Field::required(
    "id",
    Type::String,
    "Snoozed message ID to release back to unread",
)];

pub(super) const MSG_UNSNOOZE_OUTPUT: &[Field] = &[
    Field::required("id", Type::String, "Message ID"),
    Field::required("state", Type::String, "Restored lifecycle state (unread)"),
    Field::required("triage_gen", Type::Integer, "Triage generation counter"),
];

pub(super) const MSG_WAIT_INPUT: &[Field] = &[
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
    Field::optional(
        "ask",
        Type::String,
        "Wait for an answer, withdrawal, or timeout on this structured ask UUIDv7",
    ),
];

pub(super) const MSG_WAIT_OUTPUT: &[Field] = &[
    Field::required(
        "woken",
        Type::Boolean,
        "Whether a message or ask resolution arrived before the short hold ended",
    ),
    Field::optional("id", Type::String, "Arrived message ID if woken"),
    Field::optional("thread_id", Type::String, "Thread root ID if woken"),
    Field::optional("from_member_id", Type::String, "Sender member ID if woken"),
    Field::optional("subject", Type::String, "Subject if woken"),
    Field::optional("body", Type::String, "Message body if woken"),
    Field::optional(
        "status",
        Type::Enum(&[
            "answered",
            "waiting",
            "timed_out",
            "required_pending",
            "withdrawn",
        ]),
        "Ask status when called with --ask",
    ),
    Field::optional("ask_id", Type::String, "Ask UUIDv7 when called with --ask"),
    Field::optional(
        "wait_until",
        Type::String,
        "RFC 3339 UTC bounded wait deadline",
    ),
    Field::optional(
        "remaining_ms",
        Type::Integer,
        "Milliseconds left before wait_until",
    ),
    Field::optional("answer", Type::String, "Recorded answer JSON when answered"),
    Field::optional("withdrawal", Type::String, "Withdrawal JSON when withdrawn"),
    Field::optional(
        "defaults",
        Type::String,
        "Default answers JSON on non-required timeout",
    ),
    Field::optional(
        "required_notice",
        Type::String,
        "Wind-down notice on required_pending",
    ),
];
