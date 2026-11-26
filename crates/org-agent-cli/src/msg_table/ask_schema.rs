//! Field schemas of structured asks: their questions, options and outcome
//! (`msg ask`, `doc-28` §3.3).

use org_command::{Field, Type};

use super::schema::{RECIPIENT_PRESENCE_FIELD, STRING_TYPE, WOKE_AGENTS_FIELD};

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
    Field::optional(
        "questions",
        Type::List(&ASK_QUESTION_TYPE),
        "1 to 8 typed questions (choice, yes_no, free_text); or ask one with --prompt",
    ),
    Field::optional(
        "prompt",
        Type::String,
        "One question's text, instead of a questions array",
    ),
    Field::optional(
        "question_type",
        Type::Enum(&["choice", "yes_no", "free_text"]),
        "With --prompt: the question's type (default choice with --option, else free_text)",
    ),
    Field::optional(
        "option",
        Type::List(&STRING_TYPE),
        "With --prompt: one choice, as <id> or <id>=<label> (repeat for each)",
    ),
    Field::optional(
        "default_value",
        Type::String,
        "With --prompt: the default option ID, or yes or no",
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
    RECIPIENT_PRESENCE_FIELD,
    WOKE_AGENTS_FIELD,
];
