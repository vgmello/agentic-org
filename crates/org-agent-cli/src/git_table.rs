//! `org-command` command table rows for C7 repositories and worktrees (`TASK-51`, `doc-28` §3.6).

use org_command::{Ack, Command, Example, Field, IfMatch, Kind, Type};
use org_proto::ErrorCode;

const STRING_TYPE: Type = Type::String;

const READ_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotBound,
    ErrorCode::NotFound,
    ErrorCode::Unavailable,
];

const WRITE_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::InvalidName,
    ErrorCode::NotAuthorized,
    ErrorCode::NotBound,
    ErrorCode::NotFound,
    ErrorCode::AlreadyExists,
    ErrorCode::Conflict,
    ErrorCode::OperationReused,
    ErrorCode::Unavailable,
];

const REPO_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Repository UUIDv7"),
    Field::optional("key", Type::String, "Repository key"),
    Field::required("kind", Type::Enum(&["remote", "local"]), "Repository kind"),
    Field::optional("remote_url", Type::String, "Remote URL"),
    Field::optional(
        "integration_branch",
        Type::String,
        "Project integration branch",
    ),
    Field::required(
        "cache_state",
        Type::Enum(&["unprepared", "cloning", "ready", "failed"]),
        "Office cache state",
    ),
    Field::optional("cache_reason", Type::String, "Cache failure reason"),
    Field::required("revision", Type::Integer, "Record revision"),
];

const REPO_REGISTER_OUTPUT: &[Field] = &[Field::required(
    "repository",
    Type::Object(REPO_FIELDS),
    "Registered repository",
)];

const REPO_ATTACH_LINK_FIELDS: &[Field] = &[
    Field::required("project_id", Type::String, "Project UUIDv7"),
    Field::required("project_key", Type::String, "Project key"),
    Field::required("repository_id", Type::String, "Repository UUIDv7"),
    Field::optional("repository_key", Type::String, "Repository key"),
    Field::required("integration_branch", Type::String, "Integration branch"),
    Field::required("revision", Type::Integer, "Project revision"),
];

const REPO_ATTACH_OUTPUT: &[Field] = &[Field::required(
    "link",
    Type::Object(REPO_ATTACH_LINK_FIELDS),
    "Attached project repository link",
)];

const REPO_STATUS_OUTPUT: &[Field] = &[
    Field::required("office_id", Type::String, "Office UUIDv7"),
    Field::required("repository_id", Type::String, "Repository UUIDv7"),
    Field::required(
        "state",
        Type::Enum(&["unprepared", "cloning", "ready", "failed"]),
        "Cache state",
    ),
    Field::optional("bare_path", Type::String, "Bare cache path"),
    Field::optional("reason", Type::String, "Failure reason"),
];

const REPO_PUSH_OUTPUT: &[Field] = &[
    Field::required("task_id", Type::String, "Task UUIDv7"),
    Field::required("repository_id", Type::String, "Repository UUIDv7"),
    Field::required("branch", Type::String, "Pushed task branch"),
    Field::required("commit", Type::String, "Pushed commit OID"),
];

const WORKTREE_FIELDS: &[Field] = &[
    Field::required("office_id", Type::String, "Office UUIDv7"),
    Field::required("employee_id", Type::String, "Owner employee UUIDv7"),
    Field::required("task_id", Type::String, "Task UUIDv7"),
    Field::required("task_key", Type::String, "Task key"),
    Field::required("repository_id", Type::String, "Repository UUIDv7"),
    Field::optional("repository_key", Type::String, "Repository key"),
    Field::required("branch", Type::String, "Task branch"),
    Field::required("worktree_path", Type::String, "Worktree directory path"),
    Field::required("base_commit", Type::String, "Base commit OID"),
    Field::optional("pushed_head", Type::String, "Last pushed commit OID"),
    Field::required("state", Type::String, "Workspace state"),
];

const WORKTREE_PREPARE_OUTPUT: &[Field] = &[
    Field::required(
        "worktree",
        Type::Object(WORKTREE_FIELDS),
        "Prepared worktree",
    ),
    Field::required(
        "writable_roots",
        Type::List(&STRING_TYPE),
        "Sandbox writable roots",
    ),
    Field::required(
        "fixed_write_denies",
        Type::List(&STRING_TYPE),
        "Sandbox write-denied Git metadata paths",
    ),
];

const REPO_REGISTER_INPUT: &[Field] = &[
    Field::required("kind", Type::Enum(&["remote", "local"]), "remote or local"),
    Field::optional("key", Type::String, "Optional unique key"),
    Field::optional("remote_url", Type::String, "Remote HTTPS or SSH URL"),
    Field::optional("origin_path", Type::String, "Local origin directory path"),
    Field::optional("repository_id", Type::String, "Optional repository UUIDv7"),
];

const REPO_LIST_INPUT: &[Field] = &[Field::optional(
    "project",
    Type::String,
    "Filter by project ID, P-<n>, or slug",
)];

const REPO_ATTACH_INPUT: &[Field] = &[
    Field::required("project", Type::String, "Project ID, P-<n>, or slug"),
    Field::required("repo", Type::String, "Repository ID or key"),
    Field::required("branch", Type::String, "Verified integration branch"),
];

const REPO_STATUS_INPUT: &[Field] = &[Field::required(
    "repo",
    Type::String,
    "Repository ID or key",
)];

const REPO_PUSH_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task ID or T-<CODE><N> key"),
    Field::optional("repo", Type::String, "Repository ID or key"),
];

const WORKTREE_LIST_INPUT: &[Field] = &[Field::optional(
    "task",
    Type::String,
    "Filter by task ID or key",
)];

const WORKTREE_PREPARE_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task ID or T-<CODE><N> key"),
    Field::required("repo", Type::String, "Repository ID or key"),
    Field::optional(
        "base_branch",
        Type::String,
        "Base branch for projectless tasks",
    ),
];

pub const GIT_COMMANDS: &[Command] = &[
    Command {
        resource: "repo",
        verb: "register",
        summary: "Register a repository",
        call: "repo.register",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: REPO_REGISTER_INPUT,
        content_field: None,
        output: REPO_REGISTER_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"kind":"remote","key":"api","remote_url":"https://github.com/acme/api.git"}"#,
            output: r#"{"repository":{"id":"01950000-0000-7000-8000-000000000301","key":"api","kind":"remote","remote_url":"https://github.com/acme/api.git","cache_state":"unprepared","revision":1}}"#,
        },
    },
    Command {
        resource: "repo",
        verb: "list",
        summary: "List repositories",
        call: "repo.list",
        kind: Kind::List,
        input: REPO_LIST_INPUT,
        content_field: None,
        output: REPO_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: "{}",
            output: r#"[{"id":"01950000-0000-7000-8000-000000000301","key":"api","kind":"remote","remote_url":"https://github.com/acme/api.git","cache_state":"ready","revision":1}]"#,
        },
    },
    Command {
        resource: "repo",
        verb: "attach",
        summary: "Attach repo to project",
        call: "repo.attach",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: REPO_ATTACH_INPUT,
        content_field: None,
        output: REPO_ATTACH_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"project":"P-1","repo":"api","branch":"main"}"#,
            output: r#"{"link":{"project_id":"01950000-0000-7000-8000-000000000201","project_key":"P-1","repository_id":"01950000-0000-7000-8000-000000000301","repository_key":"api","integration_branch":"main","revision":2}}"#,
        },
    },
    Command {
        resource: "repo",
        verb: "status",
        summary: "Show repo cache status",
        call: "repo.status",
        kind: Kind::Read,
        input: REPO_STATUS_INPUT,
        content_field: None,
        output: REPO_STATUS_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"repo":"api"}"#,
            output: r#"{"office_id":"01950000-0000-7000-8000-000000000001","repository_id":"01950000-0000-7000-8000-000000000301","state":"ready"}"#,
        },
    },
    Command {
        resource: "repo",
        verb: "push",
        summary: "Push task branch",
        call: "git.push",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: REPO_PUSH_INPUT,
        content_field: None,
        output: REPO_PUSH_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1"}"#,
            output: r#"{"task_id":"01950000-0000-7000-8000-000000000401","repository_id":"01950000-0000-7000-8000-000000000301","branch":"task/T-A1-fix-bug","commit":"0123456789abcdef0123456789abcdef01234567"}"#,
        },
    },
    Command {
        resource: "worktree",
        verb: "list",
        summary: "List task worktrees",
        call: "worktree.list",
        kind: Kind::List,
        input: WORKTREE_LIST_INPUT,
        content_field: None,
        output: WORKTREE_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: "{}",
            output: r#"[{"office_id":"01950000-0000-7000-8000-000000000001","employee_id":"01950000-0000-7000-8000-000000000002","task_id":"01950000-0000-7000-8000-000000000401","task_key":"T-A1","repository_id":"01950000-0000-7000-8000-000000000301","branch":"task/T-A1-fix-bug","worktree_path":"/tmp/wt","base_commit":"0123456789abcdef0123456789abcdef01234567","state":"active"}]"#,
        },
    },
    Command {
        resource: "worktree",
        verb: "prepare",
        summary: "Prepare task worktree",
        call: "worktree.prepare",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: WORKTREE_PREPARE_INPUT,
        content_field: None,
        output: WORKTREE_PREPARE_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1","repo":"api"}"#,
            output: r#"{"worktree":{"office_id":"01950000-0000-7000-8000-000000000001","employee_id":"01950000-0000-7000-8000-000000000002","task_id":"01950000-0000-7000-8000-000000000401","task_key":"T-A1","repository_id":"01950000-0000-7000-8000-000000000301","branch":"task/T-A1-fix-bug","worktree_path":"/tmp/wt","base_commit":"0123456789abcdef0123456789abcdef01234567","state":"active"},"writable_roots":["/tmp/wt"],"fixed_write_denies":["/tmp/wt/.git"]}"#,
        },
    },
];
