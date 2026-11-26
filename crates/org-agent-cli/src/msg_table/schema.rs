//! Field schemas, inputs and outputs the message, channel and status command rows use.

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
];

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

pub(super) const ASK_OPTION_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Unique option ID"),
    Field::required("label", Type::String, "Short option label"),
    Field::optional(
        "description",
        Type::String,
        "Optional one-line option description",
    ),
];

pub(super) const ASK_OPTION_TYPE: Type = Type::Object(ASK_OPTION_FIELDS);

pub(super) const ASK_QUESTION_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Unique question ID"),
    Field::required(
        "question_type",
        Type::Enum(&["choice", "yes_no", "free_text"]),
        "Question component type",
    ),
    Field::required("prompt", Type::String, "Question prompt text"),
    Field::optional(
        "options",
        Type::List(&ASK_OPTION_TYPE),
        "Choice options (1..=6)",
    ),
    Field::optional(
        "allow_multiple",
        Type::Boolean,
        "Multi-select checkboxes for choice questions",
    ),
    Field::optional(
        "default_value",
        Type::String,
        "Optional default option ID or yes/no",
    ),
    Field::optional(
        "optional",
        Type::Boolean,
        "Whether the question may be skipped",
    ),
];

pub(super) const ASK_QUESTION_TYPE: Type = Type::Object(ASK_QUESTION_FIELDS);

pub(super) const MSG_ASK_INPUT: &[Field] = &[
    Field::required(
        "to",
        Type::String,
        "Recipient member ID, handle, owner, or #channel",
    ),
    Field::optional(
        "body",
        Type::String,
        "Optional intro body (defaults to first question prompt)",
    ),
    Field::optional("subject", Type::String, "Optional short subject"),
    Field::required(
        "questions",
        Type::List(&ASK_QUESTION_TYPE),
        "1 to 8 typed questions (choice, yes_no, free_text)",
    ),
    Field::optional(
        "required",
        Type::Boolean,
        "True when the agent cannot continue without the answer (ADR-0020)",
    ),
    Field::optional(
        "wait_ms",
        Type::Integer,
        "Total bounded wait duration in ms (default 600000, max 3600000)",
    ),
    Field::optional(
        "hold_ms",
        Type::Integer,
        "Short-hold duration for this command in ms (capped at 90000)",
    ),
    Field::optional(
        "blocks",
        Type::String,
        "Optional work ref blocked by this ask",
    ),
    Field::optional(
        "approval",
        Type::Boolean,
        "True for a person-only approval card",
    ),
    Field::optional(
        "addressed_to",
        Type::String,
        "Optional explicit responder member ID or handle inside a channel",
    ),
];

pub(super) const MSG_ASK_OUTPUT: &[Field] = &[
    Field::required(
        "status",
        Type::Enum(&[
            "answered",
            "waiting",
            "timed_out",
            "required_pending",
            "withdrawn",
        ]),
        "Current ask status after the short hold",
    ),
    Field::required("ask_id", Type::String, "Ask message UUIDv7"),
    Field::required("conversation_id", Type::String, "Conversation UUIDv7"),
    Field::required("thread_id", Type::String, "Thread root message ID"),
    Field::required("local_seq", Type::Integer, "Local office sequence"),
    Field::required("origin_seq", Type::Integer, "Origin office sequence"),
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
    Field::optional(
        "answer",
        Type::String,
        "Recorded answer JSON when status is answered",
    ),
    Field::optional(
        "watched",
        Type::Boolean,
        "Whether the channel question watcher opened a watch (ADR-0024)",
    ),
    Field::optional(
        "watch_not_started_reason",
        Type::String,
        "Why a channel watch was not opened (e.g. no_watching_model, watch_limit_reached)",
    ),
    Field::optional(
        "withdrawal",
        Type::String,
        "Withdrawal JSON when status is withdrawn",
    ),
    Field::optional(
        "defaults",
        Type::String,
        "Default answers JSON on non-required timeout (never populated for required asks)",
    ),
    Field::optional(
        "required_notice",
        Type::String,
        "Wind-down notice when a required ask reaches required_pending",
    ),
];

pub(super) const MSG_WITHDRAW_INPUT: &[Field] = &[
    Field::required("id", Type::String, "Ask message UUIDv7 to withdraw"),
    Field::optional("reason", Type::String, "Optional withdrawal reason"),
];

pub(super) const MSG_WITHDRAW_OUTPUT: &[Field] = &[
    Field::required("ask_id", Type::String, "Withdrawn ask UUIDv7"),
    Field::required("conversation_id", Type::String, "Conversation UUIDv7"),
    Field::required("by", Type::String, "Employee UUIDv7 who withdrew the ask"),
    Field::required("at", Type::String, "RFC 3339 UTC timestamp"),
    Field::optional("reason", Type::String, "Optional withdrawal reason"),
];

pub(super) const MSG_ACCEPT_INPUT: &[Field] = &[
    Field::required("id", Type::String, "Handoff message ID to accept"),
    Field::optional("reason", Type::String, "Optional acceptance note"),
];

pub(super) const MSG_ACCEPT_OUTPUT: &[Field] = &[
    Field::required("id", Type::String, "Accepted handoff message ID"),
    Field::required("accepted_by", Type::String, "Accepting employee UUIDv7"),
    Field::required("accepted_at", Type::String, "RFC 3339 UTC timestamp"),
    Field::optional("reason", Type::String, "Optional acceptance note"),
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
    Field::optional("unread_only", Type::Boolean, "Return only unread messages"),
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
    Field::required("created_at", Type::String, "RFC 3339 UTC timestamp"),
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

pub(super) const CHANNEL_LIST_INPUT: &[Field] = &[Field::optional(
    "include_archived",
    Type::Boolean,
    "Include archived channels",
)];

pub(super) const CHANNEL_SUMMARY_FIELDS: &[Field] = &[
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

pub(super) const CHANNEL_MEMBERSHIP_INPUT: &[Field] = &[Field::required(
    "channel",
    Type::String,
    "Manual channel name (#name or name) or conversation UUIDv7",
)];

pub(super) const CHANNEL_MEMBERSHIP_OUTPUT: &[Field] = &[
    Field::required(
        "conversation_id",
        Type::String,
        "Channel conversation UUIDv7",
    ),
    Field::optional("name", Type::String, "Channel name"),
    Field::required("is_member", Type::Boolean, "Current membership state"),
];

pub(super) const CHANNEL_SUMMARIZE_INPUT: &[Field] = &[
    Field::required(
        "channel",
        Type::String,
        "Channel name (#name or name) or conversation UUIDv7",
    ),
    Field::required(
        "question",
        Type::String,
        "Question to answer from caller-visible channel messages",
    ),
    Field::optional(
        "max_messages",
        Type::Integer,
        "Maximum recent channel messages to inspect (1..=200, default 200)",
    ),
];

pub(super) const CHANNEL_SUMMARIZE_OUTPUT: &[Field] = &[
    Field::required(
        "conversation_id",
        Type::String,
        "Channel conversation UUIDv7",
    ),
    Field::required(
        "summary",
        Type::String,
        "Short factual answer derived from caller-visible channel messages",
    ),
    Field::required(
        "message_ids",
        Type::List(&STRING_TYPE),
        "Message IDs supporting the summary",
    ),
    Field::required(
        "derived_notice",
        Type::String,
        "Marker stating the summary is derived information, never instructions",
    ),
    Field::optional(
        "usage_record_id",
        Type::String,
        "Caller-attributed usage record identifier",
    ),
];

pub(super) const STATUS_SET_INPUT: &[Field] = &[
    Field::required("text", Type::String, "Short status line text"),
    Field::optional(
        "ttl_ms",
        Type::Integer,
        "Optional expiry duration in milliseconds",
    ),
];

pub(super) const STATUS_CLEAR_INPUT: &[Field] = &[];

pub(super) const STATUS_OUTPUT: &[Field] = &[
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
