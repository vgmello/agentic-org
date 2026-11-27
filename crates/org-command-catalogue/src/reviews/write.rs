//! Declarative rows for `review request|route|finding|verdict|resubmit|close`.

use org_command::{Ack, Command, Example, Field, IfMatch, Kind, Type};

use crate::shared::{JSON_TYPE, STRING_TYPE};

use super::schema::{
    ANCHOR_KIND_TYPE, FINDING_FIELDS, FINDING_ITEM_TYPE, FINDING_STATUS_TYPE, REVIEWER_ITEM_TYPE,
    REVIEW_FIELDS, ROUND_FIELDS, SAMPLE_FINDING_JSON, SAMPLE_REVIEW_JSON, SAMPLE_ROUND_JSON,
    SESSION_MODE_TYPE, SEVERITY_TYPE, TARGET_KIND_TYPE, VERDICT_FIELDS, VERDICT_TYPE, WRITE_ERRORS,
};

const REVIEW_REQUEST_INPUT: &[Field] = &[
    Field::required("title", Type::String, "Single-line review title"),
    Field::optional(
        "target_kind",
        TARGET_KIND_TYPE,
        "Target kind: code (default), doc, task, or artifact",
    ),
    Field::optional(
        "kind",
        Type::String,
        "Review kind slug (default: code-review)",
    ),
    Field::optional("category", Type::String, "Optional category slug"),
    Field::optional("body", Type::String, "Markdown review description"),
    Field::optional("project_id", Type::String, "Owning project ID or slug"),
    Field::optional("task_id", Type::String, "Associated task ID or key"),
    Field::optional("repository_id", Type::String, "Repository UUIDv7 (code)"),
    Field::optional("branch", Type::String, "Reviewed task branch (code)"),
    Field::optional(
        "target_ref",
        Type::String,
        "Doc path/ID or artifact attachment ID",
    ),
    Field::optional("base_oid", Type::String, "Optional base commit OID"),
    Field::optional(
        "expected_head_oid",
        Type::String,
        "Expected worktree HEAD commit OID",
    ),
    Field::optional(
        "independent",
        Type::Boolean,
        "Enforce independent review guards and fresh sessions",
    ),
    Field::optional("fresh", Type::Boolean, "Shorthand for --session-mode fresh"),
    Field::optional("session_mode", SESSION_MODE_TYPE, "Reviewer session mode"),
    Field::optional(
        "auto_assign",
        Type::Boolean,
        "Let the office pick a single reviewer agent automatically",
    ),
    Field::optional(
        "reviewer",
        Type::List(&STRING_TYPE),
        "Explicit reviewer employee UUIDv7 (repeatable)",
    ),
    Field::optional("team", Type::String, "Reviewer pool team UUIDv7"),
    Field::optional(
        "guidance_refs",
        Type::List(&STRING_TYPE),
        "Referenced rules or specs (repeatable)",
    ),
    Field::optional(
        "criteria",
        Type::List(&STRING_TYPE),
        "Acceptance criteria to verify (repeatable)",
    ),
];

const REVIEW_ROUND_OUTPUT: &[Field] = &[
    Field::required("review", Type::Object(REVIEW_FIELDS), "Updated review"),
    Field::required("round", Type::Object(ROUND_FIELDS), "Created round"),
];

const REVIEW_ROUTE_INPUT: &[Field] = &[
    Field::required(
        "review",
        Type::String,
        "Review UUIDv7 or RV-<OFFICE><N> key",
    ),
    Field::optional(
        "reviewer",
        Type::List(&STRING_TYPE),
        "Explicit reviewer employee UUIDv7 (repeatable)",
    ),
    Field::optional("team", Type::String, "Reviewer pool team UUIDv7"),
    Field::optional(
        "auto_assign",
        Type::Boolean,
        "Route to the office's default single reviewer agent",
    ),
    Field::optional(
        "fresh",
        Type::Boolean,
        "Force fresh sessions for routed reviewers",
    ),
    Field::optional("session_mode", SESSION_MODE_TYPE, "Reviewer session mode"),
];

const REVIEW_ROUTE_OUTPUT: &[Field] = &[
    Field::required("review", Type::Object(REVIEW_FIELDS), "Updated review"),
    Field::required(
        "reviewers",
        Type::List(&REVIEWER_ITEM_TYPE),
        "Current reviewer roster",
    ),
];

const REVIEW_FINDING_INPUT: &[Field] = &[
    Field::required(
        "review",
        Type::String,
        "Review UUIDv7 or RV-<OFFICE><N> key",
    ),
    Field::required(
        "action",
        Type::Enum(&["add", "reply"]),
        "Add a new finding or reply to an existing finding",
    ),
    Field::optional(
        "severity",
        SEVERITY_TYPE,
        "Finding severity (required when --action add)",
    ),
    Field::optional("body", Type::String, "Finding or reply markdown"),
    Field::optional(
        "anchor_kind",
        ANCHOR_KIND_TYPE,
        "Anchor kind for --action add (default: general)",
    ),
    Field::optional(
        "path",
        Type::String,
        "Repository-relative path for file_line/doc",
    ),
    Field::optional("start_line", Type::Integer, "First anchored line (1-based)"),
    Field::optional("end_line", Type::Integer, "Last anchored line (inclusive)"),
    Field::optional("doc_section", Type::String, "Doc section heading"),
    Field::optional("checklist_item_id", Type::String, "Checklist item UUIDv7"),
    Field::optional(
        "finding_id",
        Type::String,
        "Target finding UUIDv7 (required for --action reply)",
    ),
    Field::optional(
        "set_status",
        FINDING_STATUS_TYPE,
        "Update finding status on --action reply",
    ),
    Field::optional(
        "attach",
        Type::List(&STRING_TYPE),
        "Local file path to upload and attach (repeatable)",
    ),
    Field::optional(
        "attachment_id",
        Type::List(&STRING_TYPE),
        "Existing shared attachment UUIDv7 to link (repeatable)",
    ),
];

const REVIEW_FINDING_OUTPUT: &[Field] = &[
    Field::required("review", Type::Object(REVIEW_FIELDS), "Updated review"),
    Field::required(
        "finding",
        Type::Object(FINDING_FIELDS),
        "Created or updated finding",
    ),
];

const REVIEW_VERDICT_INPUT: &[Field] = &[
    Field::required(
        "review",
        Type::String,
        "Review UUIDv7 or RV-<OFFICE><N> key",
    ),
    Field::required("round", Type::Integer, "Current round number being judged"),
    Field::required("verdict", VERDICT_TYPE, "Reviewer verdict"),
    Field::optional(
        "head_oid",
        Type::String,
        "Pinned round head commit OID (code)",
    ),
    Field::optional("summary", Type::String, "Verdict summary markdown"),
    Field::optional(
        "findings",
        Type::List(&JSON_TYPE),
        "Batch of finding draft objects (via --input-json)",
    ),
    Field::optional(
        "attach",
        Type::List(&STRING_TYPE),
        "Local file path to upload and cite in verdict (repeatable)",
    ),
    Field::optional(
        "attachment_id",
        Type::List(&STRING_TYPE),
        "Existing shared attachment UUIDv7 to cite (repeatable)",
    ),
];

const REVIEW_VERDICT_OUTPUT: &[Field] = &[
    Field::required("review", Type::Object(REVIEW_FIELDS), "Updated review"),
    Field::required("verdict", Type::Object(VERDICT_FIELDS), "Recorded verdict"),
    Field::required(
        "findings",
        Type::List(&FINDING_ITEM_TYPE),
        "Findings recorded atomically with the verdict",
    ),
];

const REVIEW_RESUBMIT_INPUT: &[Field] = &[
    Field::required(
        "review",
        Type::String,
        "Review UUIDv7 or RV-<OFFICE><N> key",
    ),
    Field::optional(
        "expected_head_oid",
        Type::String,
        "Expected clean worktree HEAD commit OID (code)",
    ),
    Field::optional(
        "fresh",
        Type::Boolean,
        "Start a clean fresh reviewer validation session",
    ),
    Field::optional(
        "session_mode",
        SESSION_MODE_TYPE,
        "Session mode for next round",
    ),
    Field::optional("note", Type::String, "Author resubmission note markdown"),
];

const REVIEW_CLOSE_INPUT: &[Field] = &[
    Field::required(
        "review",
        Type::String,
        "Review UUIDv7 or RV-<OFFICE><N> key",
    ),
    Field::optional(
        "reason",
        Type::String,
        "Optional closing reason (e.g. merged)",
    ),
];

const REVIEW_CLOSE_OUTPUT: &[Field] = &[Field::required(
    "review",
    Type::Object(REVIEW_FIELDS),
    "Closed review",
)];

pub(super) const ROWS: &[Command] = &[
    Command {
        resource: "review",
        verb: "request",
        summary: "Open a review and pin round 1",
        call: "review.request",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: REVIEW_REQUEST_INPUT,
        content_field: Some("body"),
        output: REVIEW_ROUND_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"title":"Add parser","target_kind":"doc","target_ref":"docs/parser.md","auto_assign":true}"#,
            output: concat_round_example(),
        },
    },
    Command {
        resource: "review",
        verb: "route",
        summary: "Replace or reassign reviewers on an open review",
        call: "review.route",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: REVIEW_ROUTE_INPUT,
        content_field: None,
        output: REVIEW_ROUTE_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"review":"RV-A1","auto_assign":true}"#,
            output: r#"{"review":{"id":"018bce83-1200-7000-8000-000000000001","key":"RV-A1","office_code":"A","number":1,"organization_id":"018bce83-1200-7000-8000-000000000099","target_kind":"code","kind_slug":"code-review","title":"Add parser","independent":false,"status":"open","current_round":1,"pin_stale":false,"open_findings":0,"revision":2,"created_by":"018bce83-1200-7000-8000-000000000010","created_at":"2026-11-27T00:00:00.000Z","updated_at":"2026-11-27T00:00:00.000Z"},"reviewers":[]}"#,
        },
    },
    Command {
        resource: "review",
        verb: "finding",
        summary: "Raise a finding or reply to / resolve one",
        call: "review.finding",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: REVIEW_FINDING_INPUT,
        content_field: Some("body"),
        output: REVIEW_FINDING_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"review":"RV-A1","action":"add","severity":"major","body":"Handle empty input"}"#,
            output: r#"{"review":{"id":"018bce83-1200-7000-8000-000000000001","key":"RV-A1","office_code":"A","number":1,"organization_id":"018bce83-1200-7000-8000-000000000099","target_kind":"code","kind_slug":"code-review","title":"Add parser","independent":false,"status":"open","current_round":1,"pin_stale":false,"open_findings":1,"revision":2,"created_by":"018bce83-1200-7000-8000-000000000010","created_at":"2026-11-27T00:00:00.000Z","updated_at":"2026-11-27T00:00:00.000Z"},"finding":{"id":"018bce83-1200-7000-8000-000000000020","review_id":"018bce83-1200-7000-8000-000000000001","number":1,"round_number":1,"severity":"major","status":"open","anchor":{"kind":"general"},"body_markdown":"Check error path","author_id":"018bce83-1200-7000-8000-000000000011","revision":1,"created_at":"2026-11-27T00:00:00.000Z","updated_at":"2026-11-27T00:00:00.000Z"}}"#,
        },
    },
    Command {
        resource: "review",
        verb: "verdict",
        summary: "Submit a reviewer verdict and findings batch",
        call: "review.verdict",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: REVIEW_VERDICT_INPUT,
        content_field: Some("summary"),
        output: REVIEW_VERDICT_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"review":"RV-A1","round":1,"verdict":"approve","summary":"Looks good"}"#,
            output: r#"{"review":{"id":"018bce83-1200-7000-8000-000000000001","key":"RV-A1","office_code":"A","number":1,"organization_id":"018bce83-1200-7000-8000-000000000099","target_kind":"code","kind_slug":"code-review","title":"Add parser","independent":false,"status":"approved","current_round":1,"pin_stale":false,"open_findings":0,"revision":2,"created_by":"018bce83-1200-7000-8000-000000000010","created_at":"2026-11-27T00:00:00.000Z","updated_at":"2026-11-27T00:00:00.000Z"},"verdict":{"reviewer_id":"018bce83-1200-7000-8000-000000000011","round_number":1,"verdict":"approve","submitted_at":"2026-11-27T00:00:00.000Z"},"findings":[]}"#,
        },
    },
    Command {
        resource: "review",
        verb: "resubmit",
        summary: "Submit a new review round (warm or --fresh)",
        call: "review.resubmit",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: REVIEW_RESUBMIT_INPUT,
        content_field: Some("note"),
        output: REVIEW_ROUND_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"review":"RV-A1","note":"Addressed findings","fresh":true}"#,
            output: r#"{"review":{"id":"018bce83-1200-7000-8000-000000000001","key":"RV-A1","office_code":"A","number":1,"organization_id":"018bce83-1200-7000-8000-000000000099","target_kind":"code","kind_slug":"code-review","title":"Add parser","independent":false,"status":"open","current_round":2,"pin_stale":false,"open_findings":0,"revision":3,"created_by":"018bce83-1200-7000-8000-000000000010","created_at":"2026-11-27T00:00:00.000Z","updated_at":"2026-11-27T00:00:00.000Z"},"round":{"round_number":2,"session_mode":"fresh","submitted_by":"018bce83-1200-7000-8000-000000000010","submitted_at":"2026-11-27T00:00:00.000Z"}}"#,
        },
    },
    Command {
        resource: "review",
        verb: "close",
        summary: "Close a review permanently",
        call: "review.close",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: REVIEW_CLOSE_INPUT,
        content_field: None,
        output: REVIEW_CLOSE_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"review":"RV-A1","reason":"merged"}"#,
            output: r#"{"review":{"id":"018bce83-1200-7000-8000-000000000001","key":"RV-A1","office_code":"A","number":1,"organization_id":"018bce83-1200-7000-8000-000000000099","target_kind":"code","kind_slug":"code-review","title":"Add parser","independent":false,"status":"closed","current_round":1,"pin_stale":false,"open_findings":0,"closed_reason":"merged","revision":2,"created_by":"018bce83-1200-7000-8000-000000000010","created_at":"2026-11-27T00:00:00.000Z","updated_at":"2026-11-27T00:00:00.000Z"}}"#,
        },
    },
];

const fn concat_round_example() -> &'static str {
    let _ = (SAMPLE_REVIEW_JSON, SAMPLE_ROUND_JSON, SAMPLE_FINDING_JSON);
    r#"{"review":{"id":"018bce83-1200-7000-8000-000000000001","key":"RV-A1","office_code":"A","number":1,"organization_id":"018bce83-1200-7000-8000-000000000099","target_kind":"doc","kind_slug":"code-review","title":"Add parser","independent":false,"status":"open","current_round":1,"pin_stale":false,"open_findings":0,"revision":1,"created_by":"018bce83-1200-7000-8000-000000000010","created_at":"2026-11-27T00:00:00.000Z","updated_at":"2026-11-27T00:00:00.000Z"},"round":{"round_number":1,"session_mode":"warm","submitted_by":"018bce83-1200-7000-8000-000000000010","submitted_at":"2026-11-27T00:00:00.000Z"}}"#
}
