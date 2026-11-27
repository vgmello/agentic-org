//! Shared declarative field schemas reused across multiple `agent-os` command domains.

use org_command::{Field, Type};

/// Reusable scalar string type for list fields.
pub const STRING_TYPE: Type = Type::String;

/// Reusable untyped JSON type for list fields.
pub const JSON_TYPE: Type = Type::Json;

const CARD_TEAM_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Team ID"),
    Field::required("name", Type::String, "Team name"),
    Field::optional("title_in_team", Type::String, "Employee title in the team"),
];

const CARD_TEAM_TYPE: Type = Type::Object(CARD_TEAM_FIELDS);

/// The full directory card, with the employee's teams (`person inspect`, `agent inspect`,
/// `team inspect`).
pub const DIRECTORY_CARD_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Employee ID"),
    Field::required("name", Type::String, "Display name"),
    Field::optional("handle", Type::String, "Addressing handle"),
    Field::required(
        "kind",
        Type::Enum(&["person", "agent", "unknown"]),
        "Employee kind",
    ),
    Field::required("owner", Type::Boolean, "Whether this member is the owner"),
    Field::optional(
        "agent_status",
        Type::Enum(&["active", "paused", "offboarded", "unknown"]),
        "Agent lifecycle status",
    ),
    Field::optional("role_title", Type::String, "Agent role title"),
    Field::required(
        "presence",
        Type::Enum(&[
            "online",
            "available",
            "busy",
            "idle",
            "needs_you",
            "waiting_for_answer",
            "paused",
            "unable_to_run",
            "with_you",
            "asleep",
            "offline",
            "unknown",
        ]),
        "Current presence",
    ),
    Field::required(
        "teams",
        Type::List(&CARD_TEAM_TYPE),
        "Team membership cards",
    ),
];

/// Directory card object type for nested lists (`team inspect`).
pub const DIRECTORY_CARD_TYPE: Type = Type::Object(DIRECTORY_CARD_FIELDS);

/// The input of `person inspect` and `agent inspect`.
pub const MEMBER_INSPECT_INPUT: &[Field] = &[Field::required(
    "member",
    Type::String,
    "Employee ID, handle, or owner",
)];

/// An organization log position, as an organization command's answer carries it.
pub const POSITION_FIELDS: &[Field] = &[
    Field::required("epoch", Type::Integer, "Organization log epoch"),
    Field::required("seq", Type::Integer, "Organization log sequence"),
];

/// The answer of an organization mutation (`team.*`, `member.*`, `project.*`).
pub const MUTATION_OUTPUT: &[Field] = &[
    Field::required("id", Type::String, "Target entity UUIDv7"),
    Field::required(
        "status",
        Type::Enum(&["committed", "checked", "queued"]),
        "Office command acknowledgement status",
    ),
    Field::optional(
        "position",
        Type::Object(POSITION_FIELDS),
        "Committed organization position",
    ),
];

/// The answer of a hiring-family entity write (`role.*`, `agent.*`, `member set-hiring`,
/// `team set-limit`).
pub const ENTITY_OP_OUTPUT: &[Field] = &[
    Field::required("id", Type::String, "Target entity UUIDv7"),
    Field::required("revision", Type::Integer, "Committed record revision"),
    Field::required("generation", Type::Integer, "Organization store generation"),
];
