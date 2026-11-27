//! Declarative rows for operator external-agent lifecycle: `agent approve|sessions|revoke`.

use org_command::{Ack, Command, Example, Field, IfMatch, Kind, Type};
use org_proto::ErrorCode;

const ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotFound,
    ErrorCode::Conflict,
    ErrorCode::NotAuthorized,
    ErrorCode::Unavailable,
];

const SESSION_VIEW: &[Field] = &[
    Field::required("session_id", Type::String, "External session (lease) ID"),
    Field::required("employee_id", Type::String, "Agent employee UUIDv7"),
    Field::optional("handle", Type::String, "Agent handle"),
    Field::required("workspace_label", Type::String, "Signed-in workspace"),
    Field::required(
        "pubkey_fingerprint",
        Type::String,
        "Workspace key fingerprint",
    ),
    Field::required("approved_by", Type::String, "Approving member UUIDv7"),
    Field::required("approved_at", Type::String, "Approval timestamp"),
    Field::required("expires_at", Type::String, "Lease expiry"),
    Field::optional("revoked_at", Type::String, "Revocation timestamp"),
    Field::required("active", Type::Boolean, "Whether the lease is active"),
    Field::optional(
        "superseded_session_id",
        Type::String,
        "Lease this approval replaced",
    ),
    Field::optional(
        "took_over_managed",
        Type::Boolean,
        "Whether a live managed turn was parked",
    ),
];

const HISTORY_ENTRY: &[Field] = &[
    Field::required("at", Type::String, "When"),
    Field::required("event", Type::String, "What happened"),
    Field::optional("session_id", Type::String, "External session ID"),
    Field::optional("actor_id", Type::String, "Who acted"),
    Field::optional("detail", Type::String, "Detail"),
];

const APPROVE_INPUT: &[Field] = &[
    Field::required(
        "code",
        Type::String,
        "Approval code printed by `agent-os login`",
    ),
    Field::optional("ttl", Type::String, "Lease lifetime (e.g. 8h, 24h, 7d)"),
    Field::optional(
        "takeover",
        Type::Boolean,
        "Park a live managed turn instead of refusing",
    ),
];

const SESSIONS_INPUT: &[Field] = &[
    Field::optional(
        "member",
        Type::String,
        "Agent @handle or employee UUIDv7 (required unless --pending is given)",
    ),
    Field::optional(
        "pending",
        Type::Boolean,
        "List pending external workspace sign-in requests awaiting approval (all agents or filtered by --member)",
    ),
    Field::optional(
        "history_limit",
        Type::Integer,
        "Most recent history entries, 1 to 200 (default 20; `--limit` works too)",
    ),
];

const REVOKE_INPUT: &[Field] = &[
    Field::required("member", Type::String, "Agent @handle or employee UUIDv7"),
    Field::optional("session", Type::String, "Only this external session ID"),
];

const PENDING_REQUEST_VIEW: &[Field] = &[
    Field::required("request_id", Type::String, "Pending request ID"),
    Field::optional("handle", Type::String, "Agent @handle"),
    Field::required("employee_id", Type::String, "Agent employee UUIDv7"),
    Field::required("office_id", Type::String, "Office UUIDv7"),
    Field::optional("office_name", Type::String, "Office display name"),
    Field::optional("hostname", Type::String, "Client machine hostname"),
    Field::optional("workspace_path", Type::String, "Client workspace root path"),
    Field::required("workspace_label", Type::String, "Workspace label"),
    Field::required(
        "pubkey_fingerprint",
        Type::String,
        "Workspace key fingerprint",
    ),
    Field::required(
        "managed_turn_active",
        Type::Boolean,
        "Whether the agent currently has a live managed turn",
    ),
    Field::required("requested_at", Type::String, "Request creation timestamp"),
    Field::required("expires_at", Type::String, "Approval code expiry timestamp"),
    Field::required(
        "code_prefix",
        Type::String,
        "Masked approval code prefix (XXXX-••••)",
    ),
    Field::required(
        "workspace_familiarity",
        Type::Enum(&["first_time", "returning_new_key", "returning_known_key"]),
        "Whether this workspace and key were approved previously",
    ),
    Field::optional("last_approved_by", Type::String, "Last approving member ID"),
    Field::optional("last_approved_at", Type::String, "Last approval timestamp"),
];

const SESSIONS_OUTPUT: &[Field] = &[
    Field::optional("employee_id", Type::String, "Agent employee UUIDv7"),
    Field::optional("active", Type::Object(SESSION_VIEW), "Active lease"),
    Field::optional(
        "history",
        Type::List(&Type::Object(HISTORY_ENTRY)),
        "Recent sign-in history",
    ),
    Field::optional(
        "pending",
        Type::List(&Type::Object(PENDING_REQUEST_VIEW)),
        "Pending external workspace sign-in requests (--pending)",
    ),
];

const REVOKE_OUTPUT: &[Field] = &[
    Field::required("employee_id", Type::String, "Agent employee UUIDv7"),
    Field::required("revoked", Type::Boolean, "Whether a lease was ended"),
    Field::optional("session_id", Type::String, "Ended external session ID"),
];

/// The operator rows of the external lifecycle.
pub const AGENT_SESSIONS_COMMANDS: &[Command] = &[
    Command {
        resource: "agent",
        verb: "approve",
        summary: "Approve an external workspace sign-in",
        call: "member.external_approve",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: APPROVE_INPUT,
        content_field: None,
        output: SESSION_VIEW,
        errors: ERRORS,
        example: Example {
            input: r#"{"code":"K7QF-9XM2","ttl":"24h"}"#,
            output: r#"{"session_id":"ext-0193a5c0-0000-7000-8000-000000000070","employee_id":"0193a5c0-0000-7000-8000-000000000051","handle":"ada","workspace_label":"~/src/app","pubkey_fingerprint":"3f9c2a7d10b4e8f1","approved_by":"0193a5c0-0000-7000-8000-000000000001","approved_at":"2026-11-26T10:00:00.000Z","expires_at":"2026-11-27T10:00:00.000Z","active":true}"#,
        },
    },
    Command {
        resource: "agent",
        verb: "sessions",
        summary: "List an agent's external sessions",
        call: "member.external_sessions",
        kind: Kind::Read,
        input: SESSIONS_INPUT,
        content_field: None,
        output: SESSIONS_OUTPUT,
        errors: ERRORS,
        example: Example {
            input: r#"{"member":"@ada"}"#,
            output: r#"{"employee_id":"0193a5c0-0000-7000-8000-000000000051","history":[{"at":"2026-11-26T10:00:00.000Z","event":"approved","session_id":"ext-0193a5c0-0000-7000-8000-000000000070"}]}"#,
        },
    },
    Command {
        resource: "agent",
        verb: "revoke",
        summary: "Revoke an agent's external session",
        call: "member.external_revoke",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: REVOKE_INPUT,
        content_field: None,
        output: REVOKE_OUTPUT,
        errors: ERRORS,
        example: Example {
            input: r#"{"member":"@ada"}"#,
            output: r#"{"employee_id":"0193a5c0-0000-7000-8000-000000000051","revoked":true,"session_id":"ext-0193a5c0-0000-7000-8000-000000000070"}"#,
        },
    },
];
