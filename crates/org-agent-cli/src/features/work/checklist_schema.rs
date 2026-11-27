//! Command rows, schemas, examples, and guidance for `checklist` (`TASK-49`, `TASK-90`).

use org_command::{Ack, Command, Example, Field, IfMatch, Kind, Type};

use super::schema::{READ_ERRORS, STRING_TYPE, WRITE_ERRORS};
use crate::runtime::domain::CommandCue;

const INTEGER_TYPE: Type = Type::Integer;
const SECTION_VALUES: &[&str] = &["acceptance", "dod", "work"];
const LIST_SECTION_VALUES: &[&str] = &["acceptance", "dod", "work", "all"];

const SECTION_PROGRESS_FIELDS: &[Field] = &[
    Field::required("total", Type::Integer, "Total steps in section"),
    Field::required("done", Type::Integer, "Completed steps in section"),
    Field::required("required_total", Type::Integer, "Required steps in section"),
    Field::required(
        "required_done",
        Type::Integer,
        "Completed required steps in section",
    ),
];

const SECTION_PROGRESS_TYPE: Type = Type::Object(SECTION_PROGRESS_FIELDS);

const CHECKLIST_PROGRESS_FIELDS: &[Field] = &[
    Field::required("total", Type::Integer, "Total checklist steps"),
    Field::required("done", Type::Integer, "Completed checklist steps"),
    Field::required("required_total", Type::Integer, "Total required steps"),
    Field::required("required_done", Type::Integer, "Completed required steps"),
    Field::required(
        "acceptance",
        SECTION_PROGRESS_TYPE,
        "Acceptance section progress",
    ),
    Field::required(
        "dod",
        SECTION_PROGRESS_TYPE,
        "Definition-of-done section progress",
    ),
    Field::required("work", SECTION_PROGRESS_TYPE, "Work section progress"),
];

const CHECKLIST_ORDINAL_STEP_FIELDS: &[Field] = &[
    Field::required(
        "index",
        Type::Integer,
        "1-based step ordinal within its section",
    ),
    Field::required("section", Type::Enum(SECTION_VALUES), "Checklist section"),
    Field::required("step_id", Type::String, "Checklist step UUIDv7"),
    Field::required("text", Type::String, "Step text"),
    Field::required("required", Type::Boolean, "Whether step is required"),
    Field::required("done", Type::Boolean, "Whether step is checked"),
    Field::optional(
        "checked_by_employee_id",
        Type::String,
        "Employee who checked it",
    ),
    Field::optional("checked_at", Type::String, "RFC 3339 check timestamp"),
    Field::required("revision", Type::Integer, "Step revision"),
];

const CHECKLIST_ORDINAL_STEP_TYPE: Type = Type::Object(CHECKLIST_ORDINAL_STEP_FIELDS);

const CHECKLIST_LIST_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::optional(
        "section",
        Type::Enum(LIST_SECTION_VALUES),
        "Filter by section: acceptance, dod, work, or all (default: all)",
    ),
    Field::optional("unchecked", Type::Boolean, "Only show unchecked steps"),
];

const CHECKLIST_ADD_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::required(
        "text",
        Type::List(&STRING_TYPE),
        "Checklist step text (repeat `--text` to add multiple steps atomically)",
    ),
    Field::optional(
        "section",
        Type::Enum(SECTION_VALUES),
        "Checklist section: acceptance, dod, or work (default: work)",
    ),
    Field::optional("required", Type::Boolean, "Mark added step(s) as required"),
    Field::optional("optional", Type::Boolean, "Mark added step(s) as optional"),
];

const CHECKLIST_CHECK_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::optional("step_id", Type::String, "Checklist step UUIDv7"),
    Field::optional(
        "section",
        Type::Enum(SECTION_VALUES),
        "Checklist section when using 1-based `--index` (requires `--if-match`)",
    ),
    Field::optional(
        "index",
        Type::List(&INTEGER_TYPE),
        "1-based step ordinal within `--section` (repeatable; requires `--if-match`)",
    ),
    Field::optional("done", Type::Boolean, "Mark step(s) done (default)"),
    Field::optional("undone", Type::Boolean, "Mark step(s) not done"),
];

const CHECKLIST_EDIT_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::optional("step_id", Type::String, "Checklist step UUIDv7"),
    Field::optional(
        "section",
        Type::Enum(SECTION_VALUES),
        "Checklist section when using 1-based `--index` (requires `--if-match`)",
    ),
    Field::optional(
        "index",
        Type::Integer,
        "1-based step ordinal within `--section` (requires `--if-match`)",
    ),
    Field::optional("text", Type::String, "Updated step text"),
    Field::optional("required", Type::Boolean, "Mark step as required"),
    Field::optional("optional", Type::Boolean, "Mark step as optional"),
];

const CHECKLIST_REMOVE_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::optional("step_id", Type::String, "Checklist step UUIDv7"),
    Field::optional(
        "section",
        Type::Enum(SECTION_VALUES),
        "Checklist section when using 1-based `--index` (requires `--if-match`)",
    ),
    Field::optional(
        "index",
        Type::Integer,
        "1-based step ordinal within `--section` (requires `--if-match`)",
    ),
];

const CHECKLIST_MOVE_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::optional("step_id", Type::String, "Checklist step UUIDv7"),
    Field::optional(
        "section",
        Type::Enum(SECTION_VALUES),
        "Source checklist section when using 1-based `--index` (requires `--if-match`)",
    ),
    Field::optional(
        "index",
        Type::Integer,
        "1-based step ordinal within `--section` (requires `--if-match`)",
    ),
    Field::optional(
        "to_section",
        Type::Enum(SECTION_VALUES),
        "Optional destination checklist section",
    ),
    Field::required(
        "position",
        Type::Integer,
        "Zero-based target position in destination section",
    ),
];

const CHECKLIST_LIST_OUTPUT: &[Field] = &[
    Field::required("task_id", Type::String, "Task UUIDv7"),
    Field::required("key", Type::String, "Task key"),
    Field::required("revision", Type::Integer, "Current task revision"),
    Field::required(
        "progress",
        Type::Object(CHECKLIST_PROGRESS_FIELDS),
        "Per-section checklist progress",
    ),
    Field::required(
        "steps",
        Type::List(&CHECKLIST_ORDINAL_STEP_TYPE),
        "Matching checklist steps with 1-based section ordinals",
    ),
];

const CHECKLIST_MUTATION_OUTPUT: &[Field] = &[
    Field::required("task_id", Type::String, "Task UUIDv7"),
    Field::required("key", Type::String, "Task key"),
    Field::required("revision", Type::Integer, "New task revision"),
    Field::required(
        "progress",
        Type::Object(CHECKLIST_PROGRESS_FIELDS),
        "Per-section checklist progress",
    ),
    Field::optional(
        "steps",
        Type::List(&CHECKLIST_ORDINAL_STEP_TYPE),
        "Mutated checklist step(s)",
    ),
    Field::optional(
        "removed_step_id",
        Type::String,
        "Removed checklist step UUIDv7",
    ),
];

const CHECKLIST_STEP_RECEIPT_EXAMPLE_JSON: &str = r#"{"task_id":"0193a5c0-0000-7000-8000-000000000100","key":"T-A1","revision":2,"progress":{"total":1,"done":0,"required_total":1,"required_done":0,"acceptance":{"total":0,"done":0,"required_total":0,"required_done":0},"dod":{"total":0,"done":0,"required_total":0,"required_done":0},"work":{"total":1,"done":0,"required_total":1,"required_done":0}},"steps":[{"index":1,"section":"work","step_id":"0193a5c0-0000-7000-8000-000000000200","text":"Add tests","required":true,"done":false,"revision":1}]}"#;

const CHECKLIST_REMOVE_EXAMPLE_JSON: &str = r#"{"task_id":"0193a5c0-0000-7000-8000-000000000100","key":"T-A1","revision":2,"progress":{"total":0,"done":0,"required_total":0,"required_done":0,"acceptance":{"total":0,"done":0,"required_total":0,"required_done":0},"dod":{"total":0,"done":0,"required_total":0,"required_done":0},"work":{"total":0,"done":0,"required_total":0,"required_done":0}},"removed_step_id":"0193a5c0-0000-7000-8000-000000000200"}"#;

/// The `checklist` rows.
pub(super) const COMMANDS: &[Command] = &[
    Command {
        resource: "checklist",
        verb: "list",
        summary: "List checklist steps and progress",
        call: "tasks.inspect",
        kind: Kind::Read,
        input: CHECKLIST_LIST_INPUT,
        content_field: None,
        output: CHECKLIST_LIST_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1","section":"all"}"#,
            output: CHECKLIST_STEP_RECEIPT_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "checklist",
        verb: "add",
        summary: "Add checklist step(s)",
        call: "tasks.checklist.add",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: CHECKLIST_ADD_INPUT,
        content_field: None,
        output: CHECKLIST_MUTATION_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1","text":["Add tests"],"required":true}"#,
            output: CHECKLIST_STEP_RECEIPT_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "checklist",
        verb: "check",
        summary: "Check or uncheck step(s)",
        call: "tasks.checklist.check",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Optional,
        },
        input: CHECKLIST_CHECK_INPUT,
        content_field: None,
        output: CHECKLIST_MUTATION_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1","step_id":"0193a5c0-0000-7000-8000-000000000200","done":true}"#,
            output: CHECKLIST_STEP_RECEIPT_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "checklist",
        verb: "edit",
        summary: "Edit checklist step",
        call: "tasks.checklist.edit",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Optional,
        },
        input: CHECKLIST_EDIT_INPUT,
        content_field: None,
        output: CHECKLIST_MUTATION_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1","step_id":"0193a5c0-0000-7000-8000-000000000200","text":"Add integration tests"}"#,
            output: CHECKLIST_STEP_RECEIPT_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "checklist",
        verb: "remove",
        summary: "Remove checklist step",
        call: "tasks.checklist.remove",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Optional,
        },
        input: CHECKLIST_REMOVE_INPUT,
        content_field: None,
        output: CHECKLIST_MUTATION_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1","step_id":"0193a5c0-0000-7000-8000-000000000200"}"#,
            output: CHECKLIST_REMOVE_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "checklist",
        verb: "move",
        summary: "Move checklist step",
        call: "tasks.checklist.move",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Optional,
        },
        input: CHECKLIST_MOVE_INPUT,
        content_field: None,
        output: CHECKLIST_MUTATION_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1","step_id":"0193a5c0-0000-7000-8000-000000000200","position":0}"#,
            output: CHECKLIST_STEP_RECEIPT_EXAMPLE_JSON,
        },
    },
];

/// `Use when:` guidance of each `checklist` command.
pub(super) const CUES: &[CommandCue] = &[
    (
        "checklist",
        "add",
        "appending one or more steps (repeat `--text`, optional `--section`) to a task's checklist.",
    ),
    (
        "checklist",
        "check",
        "marking checklist step(s) done (or `--undone`) by `--step-id` or `--section` + `--index` with `--if-match`.",
    ),
    (
        "checklist",
        "edit",
        "updating a step's wording or `--required`/`--optional` flag by `--step-id` or `--section` + `--index` with `--if-match`.",
    ),
    (
        "checklist",
        "list",
        "listing checklist progress and 1-based ordinals (filter with `--section` or `--unchecked`) without fetching task body or history.",
    ),
    (
        "checklist",
        "move",
        "reordering a checklist step to a zero-based `--position` (or `--to-section`) by `--step-id` or `--section` + `--index` with `--if-match`.",
    ),
    (
        "checklist",
        "remove",
        "deleting an obsolete checklist step by `--step-id` or `--section` + `--index` with `--if-match`.",
    ),
];
