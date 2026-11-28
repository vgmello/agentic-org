//! Declarative rows for `review list|inspect|diff`.

use org_command::{Command, Example, Field, Kind, Type};

use super::schema::{
    DIFF_FILE_ITEM_TYPE, FINDING_ITEM_TYPE, READ_ERRORS, REVIEWER_ITEM_TYPE, REVIEW_FIELDS,
    REVIEW_REF_INPUT, ROUND_ITEM_TYPE, STATUS_TYPE, TARGET_KIND_TYPE,
};

const REVIEW_LIST_INPUT: &[Field] = &[
    Field::optional("project_id", Type::String, "Filter by project ID or slug"),
    Field::optional("task_id", Type::String, "Filter by task ID or T-<CODE><N>"),
    Field::optional("status", STATUS_TYPE, "Filter by review status"),
    Field::optional("target_kind", TARGET_KIND_TYPE, "Filter by target kind"),
    Field::optional(
        "reviewer_id",
        Type::String,
        "Filter by assigned reviewer UUIDv7",
    ),
    Field::optional(
        "created_by",
        Type::String,
        "Filter by creator employee UUIDv7",
    ),
];

const REVIEW_INSPECT_OUTPUT: &[Field] = &[
    Field::required("review", Type::Object(REVIEW_FIELDS), "Review summary"),
    Field::optional("rounds", Type::List(&ROUND_ITEM_TYPE), "Review rounds"),
    Field::optional(
        "reviewers",
        Type::List(&REVIEWER_ITEM_TYPE),
        "Assigned reviewers",
    ),
    Field::optional(
        "findings",
        Type::List(&FINDING_ITEM_TYPE),
        "Review findings and replies",
    ),
];

const REVIEW_DIFF_INPUT: &[Field] = &[
    Field::required(
        "review",
        Type::String,
        "Review UUIDv7 or RV-<OFFICE><N> key",
    ),
    Field::optional(
        "round",
        Type::Integer,
        "Round number (default: current round)",
    ),
    Field::optional(
        "since_round",
        Type::Integer,
        "Earlier round to diff from instead of review base",
    ),
    Field::optional(
        "path",
        Type::String,
        "Repository-relative file or dir filter",
    ),
    Field::optional("stat_only", Type::Boolean, "Return per-file summary only"),
    Field::optional(
        "context_lines",
        Type::Integer,
        "Unified diff context lines (0-20, default 3)",
    ),
];

const REVIEW_DIFF_OUTPUT: &[Field] = &[
    Field::required("review_id", Type::String, "Review UUIDv7"),
    Field::required("round_number", Type::Integer, "Diffed round number"),
    Field::optional("since_round", Type::Integer, "Base comparison round"),
    Field::required("base_oid", Type::String, "Base commit OID"),
    Field::required("head_oid", Type::String, "Head commit OID"),
    Field::required(
        "files",
        Type::List(&DIFF_FILE_ITEM_TYPE),
        "Changed file stats",
    ),
    Field::required("total_files", Type::Integer, "Total changed files"),
    Field::optional("files_truncated", Type::Boolean, "File list was capped"),
    Field::optional("unified_diff", Type::String, "Bounded unified diff"),
    Field::required("truncated", Type::Boolean, "Unified diff was truncated"),
];

pub(super) const ROWS: &[Command] = &[
    Command {
        resource: "review",
        verb: "list",
        summary: "List reviews matching optional filters",
        call: "review.list",
        kind: Kind::List,
        input: REVIEW_LIST_INPUT,
        content_field: None,
        output: REVIEW_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"status":"open"}"#,
            output: r#"[{"id":"018bce83-1200-7000-8000-000000000001","key":"RV-A1","office_code":"A","number":1,"organization_id":"018bce83-1200-7000-8000-000000000099","target_kind":"code","kind_slug":"code-review","title":"Add parser","independent":false,"status":"open","current_round":1,"pin_stale":false,"open_findings":0,"revision":1,"created_by":"018bce83-1200-7000-8000-000000000010","created_at":"2026-11-27T00:00:00.000Z","updated_at":"2026-11-27T00:00:00.000Z"}]"#,
        },
    },
    Command {
        resource: "review",
        verb: "inspect",
        summary: "Inspect a review, rounds, reviewers, and findings",
        call: "review.inspect",
        kind: Kind::Read,
        input: REVIEW_REF_INPUT,
        content_field: None,
        output: REVIEW_INSPECT_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"review":"RV-A1"}"#,
            output: r#"{"review":{"id":"018bce83-1200-7000-8000-000000000001","key":"RV-A1","office_code":"A","number":1,"organization_id":"018bce83-1200-7000-8000-000000000099","target_kind":"code","kind_slug":"code-review","title":"Add parser","independent":false,"status":"open","current_round":1,"pin_stale":false,"open_findings":0,"revision":1,"created_by":"018bce83-1200-7000-8000-000000000010","created_at":"2026-11-27T00:00:00.000Z","updated_at":"2026-11-27T00:00:00.000Z"},"rounds":[],"reviewers":[],"findings":[]}"#,
        },
    },
    Command {
        resource: "review",
        verb: "diff",
        summary: "Read bounded bare-cache diff for a code review round",
        call: "review.diff",
        kind: Kind::Read,
        input: REVIEW_DIFF_INPUT,
        content_field: None,
        output: REVIEW_DIFF_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"review":"RV-A1","stat_only":true}"#,
            output: r#"{"review_id":"018bce83-1200-7000-8000-000000000001","round_number":1,"base_oid":"0000000000000000000000000000000000000001","head_oid":"0000000000000000000000000000000000000002","files":[],"total_files":0,"truncated":false}"#,
        },
    },
];
