//! The `session boot` row (shorthands `boot`, `instructions overview`): everything an agent
//! needs before its first command, as one envelope (`doc-28`, `doc-96`).

use org_command::{Command, Example, Field, Kind, Type};

use super::{READ_ERRORS, STRING_TYPE};

const BOOT_INPUT: &[Field] = &[
    Field::optional(
        "project",
        Type::String,
        "Also the must-follow processes of this project (UUIDv7, P-<number> key or slug)",
    ),
    Field::optional(
        "offline",
        Type::Boolean,
        "Answer from this build alone, without the office: rules, catalogue and skills",
    ),
];

const IDENTITY_TEAM_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Team ID"),
    Field::required("name", Type::String, "Team name"),
    Field::optional("title_in_team", Type::String, "Title in the team"),
];

const IDENTITY_TEAM_TYPE: Type = Type::Object(IDENTITY_TEAM_FIELDS);

const IDENTITY_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Your employee ID"),
    Field::required("name", Type::String, "Your display name"),
    Field::optional("handle", Type::String, "Your handle (@handle)"),
    Field::required(
        "kind",
        Type::Enum(&["person", "agent", "unknown"]),
        "Employee kind",
    ),
    Field::optional("role_title", Type::String, "Your role title"),
    Field::required(
        "teams",
        Type::List(&IDENTITY_TEAM_TYPE),
        "Your teams, home team first",
    ),
];

const SKILL_INDEX_FIELDS: &[Field] = &[
    Field::required("key", Type::String, "Pass to `skill read --name`"),
    Field::required("description", Type::String, "What the guide covers"),
];

const SKILL_INDEX_TYPE: Type = Type::Object(SKILL_INDEX_FIELDS);

const MANDATORY_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Document ID"),
    Field::optional("key", Type::String, "Document key"),
    Field::required(
        "kind",
        Type::Enum(&["process", "standard"]),
        "Process or standard",
    ),
    Field::required(
        "level",
        Type::String,
        "Where it is set: organization, team or project",
    ),
    Field::optional(
        "scope_name",
        Type::String,
        "The team or project it is set for",
    ),
    Field::required("title", Type::String, "Title"),
    Field::required("revision", Type::Integer, "Document revision"),
    Field::required("body", Type::String, "The whole Markdown text, never cut"),
];

const MANDATORY_TYPE: Type = Type::Object(MANDATORY_FIELDS);

const BOOT_OUTPUT: &[Field] = &[
    Field::required(
        "mode",
        Type::Enum(&["offline", "bound"]),
        "offline: this build alone; bound: also asked the office",
    ),
    Field::optional(
        "identity",
        Type::Object(IDENTITY_FIELDS),
        "Who you are (bound only)",
    ),
    Field::required("rules", Type::String, "The standing rules of every agent"),
    Field::required(
        "catalogue",
        Type::String,
        "Every command, grouped by resource",
    ),
    Field::required(
        "skills",
        Type::List(&SKILL_INDEX_TYPE),
        "Built-in guides: read one with `skill read --name <key>`",
    ),
    Field::optional(
        "mandatory",
        Type::List(&MANDATORY_TYPE),
        "Must-follow (non-overridable) processes and standards that apply to you, whole (bound only)",
    ),
    Field::optional(
        "mandatory_truncated",
        Type::Boolean,
        "Whether must-follow documents were left out to stay within 8 KiB (bound only)",
    ),
    Field::optional(
        "mandatory_omitted",
        Type::List(&STRING_TYPE),
        "The key (or ID) of each must-follow document left out; read each with `process inspect`",
    ),
    Field::required("next_action", Type::String, "What to do next"),
];

pub(super) const SESSION_BOOT: Command = Command {
    resource: "session",
    verb: "boot",
    summary: "Load rules, catalogue, skills and must-follow processes",
    call: "session.boot",
    kind: Kind::Read,
    input: BOOT_INPUT,
    content_field: None,
    output: BOOT_OUTPUT,
    errors: READ_ERRORS,
    example: Example {
        input: r#"{"offline":true}"#,
        output: r###"{"mode":"offline","rules":"# How you work here","catalogue":"## Commands","skills":[{"key":"agent-os-basics","description":"Command grammar."}],"next_action":"Run `agent-os boot` without --offline."}"###,
    },
};
