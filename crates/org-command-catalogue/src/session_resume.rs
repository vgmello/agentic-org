//! Declarative command row and shorthands for `session resume` (`agent-os resume` / `agent-os me`,
//! `TASK-220`).

use org_command::{Alias, Command, Example, Field, Kind, Type};
use org_proto::ErrorCode;

use crate::shared::STRING_TYPE;

/// `resume` is `session resume`.
pub const RESUME_ALIAS: Alias = Alias {
    words: &["resume"],
    command: &["session", "resume"],
};

/// `me` is `session resume`.
pub const ME_ALIAS: Alias = Alias {
    words: &["me"],
    command: &["session", "resume"],
};

const READ_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotAuthorized,
    ErrorCode::NotBound,
    ErrorCode::Unavailable,
];

const RESUME_INPUT: &[Field] = &[
    Field::optional(
        "tasks_limit",
        Type::Integer,
        "Max open assigned tasks (1..=50, default 20)",
    ),
    Field::optional(
        "inbox_limit",
        Type::Integer,
        "Max unread inbox items (1..=25, default 10)",
    ),
    Field::optional(
        "asks_limit",
        Type::Integer,
        "Max open asks (1..=25, default 10)",
    ),
    Field::optional(
        "worktrees_limit",
        Type::Integer,
        "Max active worktrees (1..=25, default 10)",
    ),
];

const RESUME_IDENTITY_FIELDS: &[Field] = &[
    Field::required("member_id", Type::String, "Caller employee UUIDv7"),
    Field::optional("handle", Type::String, "Caller handle"),
    Field::required("name", Type::String, "Caller display name"),
    Field::required(
        "kind",
        Type::Enum(&["person", "agent", "unknown"]),
        "Employee kind",
    ),
    Field::required("office_id", Type::String, "Current office UUIDv7"),
    Field::optional(
        "session_mode",
        Type::Enum(&["managed", "external", "unknown"]),
        "Session mode",
    ),
];

const RESUME_INBOX_ITEM_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Message ID"),
    Field::required("conversation_id", Type::String, "Conversation UUIDv7"),
    Field::required("from_member_id", Type::String, "Sender member ID or office"),
    Field::required("from_display_name", Type::String, "Sender display name"),
    Field::required("kind", Type::String, "Message kind"),
    Field::required(
        "priority",
        Type::Enum(&["normal", "high", "urgent", "unknown"]),
        "Priority",
    ),
    Field::required("urgent", Type::Boolean, "Whether message is urgent"),
    Field::optional("subject", Type::String, "Optional message subject"),
    Field::required(
        "delivered_because",
        Type::Enum(&[
            "addressed",
            "thread_reply",
            "mention",
            "answer_candidate",
            "unknown",
        ]),
        "Delivery reason",
    ),
    Field::required("created_at", Type::String, "RFC 3339 creation timestamp"),
];
const RESUME_INBOX_ITEM_TYPE: Type = Type::Object(RESUME_INBOX_ITEM_FIELDS);

const RESUME_INBOX_FIELDS: &[Field] = &[
    Field::required(
        "unread_count",
        Type::Integer,
        "Total unread messages for caller",
    ),
    Field::required(
        "urgent_count",
        Type::Integer,
        "Total unread urgent messages for caller",
    ),
    Field::required(
        "items",
        Type::List(&RESUME_INBOX_ITEM_TYPE),
        "Newest unread items",
    ),
    Field::required(
        "truncated",
        Type::Boolean,
        "True when more unread items exist",
    ),
];

const RESUME_NOTE_FIELDS: &[Field] = &[
    Field::required("note_id", Type::String, "Private note UUIDv7"),
    Field::required("revision", Type::Integer, "Private note revision"),
    Field::required(
        "kind",
        Type::Enum(&["note", "plan", "decision", "unknown"]),
        "Note kind",
    ),
    Field::optional("title", Type::String, "Private note title"),
    Field::required(
        "excerpt",
        Type::String,
        "Bounded UTF-8 excerpt of caller's latest private note",
    ),
    Field::optional(
        "tags",
        Type::List(&STRING_TYPE),
        "Normalized private note tags",
    ),
    Field::required(
        "is_checkpoint",
        Type::Boolean,
        "True when tagged as checkpoint",
    ),
    Field::required("updated_at", Type::String, "RFC 3339 last update timestamp"),
];

const RESUME_TASK_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Task UUIDv7"),
    Field::required("key", Type::String, "Task key (T-<CODE><N>)"),
    Field::required("title", Type::String, "Task title"),
    Field::required(
        "status",
        Type::Enum(&["todo", "in_progress", "blocked", "done", "cancelled"]),
        "Task status",
    ),
    Field::required(
        "priority",
        Type::Enum(&["low", "normal", "high", "urgent"]),
        "Task priority",
    ),
    Field::required("revision", Type::Integer, "Current task revision"),
    Field::optional("project_id", Type::String, "Owning project UUIDv7"),
    Field::optional("project_key", Type::String, "Owning project key (P-<N>)"),
    Field::optional(
        "blocked_reason",
        Type::String,
        "Blocked reason when blocked",
    ),
    Field::optional(
        "latest_note",
        Type::Object(RESUME_NOTE_FIELDS),
        "Caller's own latest private working note pointer (omitted when none)",
    ),
];
const RESUME_TASK_TYPE: Type = Type::Object(RESUME_TASK_FIELDS);

const RESUME_ASK_FIELDS: &[Field] = &[
    Field::required("ask_id", Type::String, "Ask message ID"),
    Field::required("conversation_id", Type::String, "Conversation UUIDv7"),
    Field::required(
        "direction",
        Type::Enum(&["awaiting_caller", "sent_by_caller", "unknown"]),
        "Ask direction relative to caller",
    ),
    Field::required(
        "status",
        Type::Enum(&[
            "waiting",
            "required_pending",
            "answered",
            "timed_out",
            "withdrawn",
            "unknown",
        ]),
        "Open ask status",
    ),
    Field::required("from_member_id", Type::String, "Asker employee ID"),
    Field::required("from_display_name", Type::String, "Asker display name"),
    Field::optional("addressed_to", Type::String, "Addressed member ID"),
    Field::optional("blocks", Type::String, "Blocked task key or reference"),
    Field::required("required", Type::Boolean, "Whether answer is required"),
    Field::required("approval", Type::Boolean, "Whether ask is an approval"),
    Field::required("question_count", Type::Integer, "Number of questions"),
    Field::required(
        "prompt_excerpt",
        Type::String,
        "Bounded UTF-8 excerpt of first question",
    ),
    Field::required(
        "wait_until",
        Type::String,
        "Wait deadline RFC 3339 timestamp",
    ),
    Field::required("created_at", Type::String, "RFC 3339 creation timestamp"),
];
const RESUME_ASK_TYPE: Type = Type::Object(RESUME_ASK_FIELDS);

const RESUME_WORKTREE_FIELDS: &[Field] = &[
    Field::required("task_id", Type::String, "Task UUIDv7"),
    Field::required("task_key", Type::String, "Task key"),
    Field::required("repository_id", Type::String, "Repository UUIDv7"),
    Field::optional("repository_key", Type::String, "Repository key"),
    Field::required("branch", Type::String, "Task branch name"),
    Field::required("worktree_path", Type::String, "Local worktree path"),
    Field::optional("pushed_head", Type::String, "Last pushed commit SHA"),
];
const RESUME_WORKTREE_TYPE: Type = Type::Object(RESUME_WORKTREE_FIELDS);

const RESUME_BUDGET_PERIOD_FIELDS: &[Field] = &[
    Field::required(
        "state_summary",
        Type::Enum(&[
            "none",
            "ok",
            "warning80",
            "stop_pending",
            "held",
            "reached_cap",
            "unknown",
        ]),
        "Budget period state summary",
    ),
    Field::optional(
        "hold_state",
        Type::Enum(&["stop_pending", "held", "released", "unknown"]),
        "Hold state",
    ),
    Field::optional(
        "unit",
        Type::Enum(&["micro_usd", "tokens", "unknown"]),
        "Budget unit",
    ),
    Field::optional("effective_limit", Type::Integer, "Effective limit"),
    Field::optional("used_amount", Type::Integer, "Amount used"),
    Field::optional("remaining_amount", Type::Integer, "Amount remaining"),
    Field::optional("period_key", Type::String, "Period key"),
];

const RESUME_BUDGET_FIELDS: &[Field] = &[
    Field::required(
        "status",
        Type::Enum(&[
            "none",
            "ok",
            "warning80",
            "stop_pending",
            "held",
            "reached_cap",
            "unknown",
        ]),
        "Overall own-budget status",
    ),
    Field::optional(
        "daily",
        Type::Object(RESUME_BUDGET_PERIOD_FIELDS),
        "Daily budget summary",
    ),
    Field::optional(
        "monthly",
        Type::Object(RESUME_BUDGET_PERIOD_FIELDS),
        "Monthly budget summary",
    ),
];

const RESUME_OUTPUT: &[Field] = &[
    Field::required(
        "identity",
        Type::Object(RESUME_IDENTITY_FIELDS),
        "Caller identity",
    ),
    Field::required(
        "unread_inbox",
        Type::Object(RESUME_INBOX_FIELDS),
        "Unread inbox summary",
    ),
    Field::required(
        "active_tasks",
        Type::List(&RESUME_TASK_TYPE),
        "Open assigned tasks",
    ),
    Field::required(
        "active_tasks_total",
        Type::Integer,
        "Total open assigned tasks",
    ),
    Field::required(
        "open_asks",
        Type::List(&RESUME_ASK_TYPE),
        "Open asks sent by or awaiting caller",
    ),
    Field::required(
        "active_worktrees",
        Type::List(&RESUME_WORKTREE_TYPE),
        "Caller-owned active worktrees",
    ),
    Field::optional(
        "budget",
        Type::Object(RESUME_BUDGET_FIELDS),
        "Own budget summary when configured",
    ),
];

/// The `session resume` row slice.
pub const RESUME_COMMANDS: &[Command] = &[Command {
    resource: "session",
    verb: "resume",
    summary: "Wake/resume dashboard",
    call: "agent.resume",
    kind: Kind::Read,
    input: RESUME_INPUT,
    content_field: None,
    output: RESUME_OUTPUT,
    errors: READ_ERRORS,
    example: Example {
        input: "{}",
        output: r#"{"identity":{"member_id":"0193a5c0-0000-7000-8000-000000000003","handle":"ada","name":"Ada","kind":"agent","office_id":"0193a5c0-0000-7000-8000-000000000001","session_mode":"managed"},"unread_inbox":{"unread_count":0,"urgent_count":0,"items":[],"truncated":false},"active_tasks":[],"active_tasks_total":0,"open_asks":[],"active_worktrees":[]}"#,
    },
}];
