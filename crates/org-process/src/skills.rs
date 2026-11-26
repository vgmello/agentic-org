//! Built-in product skills served by `agent-os skill list` and `agent-os skill read`
//! (`doc-34` Decision 4, `doc-28` §3.5, `FC-111`).
//!
//! Each skill's body is a body-only Markdown file under `skills/<key>.md`, embedded at compile
//! time; the metadata stays here. The crate's tests check the budgets, the text hygiene and that
//! the files and this list match one to one.

/// Most bytes one skill body may take (4 KiB).
pub const SKILL_MAX_BYTES: usize = 4 * 1024;

/// One built-in product skill explaining how to operate inside Agentic Org.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuiltinSkill {
    /// Stable lowercase skill key passed to `agent-os skill read --name <key>`; the body is
    /// `skills/<key>.md`.
    pub key: &'static str,
    /// Human-readable title.
    pub title: &'static str,
    /// Concise one-line description shown in `agent-os skill list`.
    pub description: &'static str,
    /// Origin label (`"built_in"`).
    pub source: &'static str,
    /// Markdown body returned by `agent-os skill read`.
    pub body_markdown: &'static str,
}

const fn skill(
    key: &'static str,
    title: &'static str,
    description: &'static str,
    body_markdown: &'static str,
) -> BuiltinSkill {
    BuiltinSkill {
        key,
        title,
        description,
        source: "built_in",
        body_markdown,
    }
}

const BUILTIN_SKILLS: &[BuiltinSkill] = &[
    skill(
        "agent-os-basics",
        "Using agent-os",
        "Command grammar, the JSON envelope, help and schemas, small answers, long text, writes and dry runs.",
        include_str!("../skills/agent-os-basics.md"),
    ),
    skill(
        "tasks-create",
        "Creating tasks",
        "Search first, decide the scope, then write a durable why and testable acceptance criteria.",
        include_str!("../skills/tasks-create.md"),
    ),
    skill(
        "tasks-execute",
        "Executing a task",
        "Read the task and its dependencies, plan privately against live code, start, and work in verified loops.",
        include_str!("../skills/tasks-execute.md"),
    ),
    skill(
        "tasks-finalize",
        "Finishing a task",
        "Verify each criterion, tick the checklist with evidence, update docs, comment, and close.",
        include_str!("../skills/tasks-finalize.md"),
    ),
    skill(
        "messaging-and-asks",
        "Messages and structured asks",
        "Inbox, sending and replying, handoffs, structured asks and answers, snoozing, and waiting well.",
        include_str!("../skills/messaging-and-asks.md"),
    ),
    skill(
        "channels",
        "Channels",
        "Joining, summarizing with a question, reading, posting, and asking a channel.",
        include_str!("../skills/channels.md"),
    ),
    skill(
        "code-and-worktrees",
        "Code, worktrees and pushes",
        "Preparing a task worktree, working in it, and publishing the task branch through the office.",
        include_str!("../skills/code-and-worktrees.md"),
    ),
    skill(
        "hiring-and-directory",
        "Directory and hiring",
        "Finding people, teams and roles, who decides, and hiring agents through HR.",
        include_str!("../skills/hiring-and-directory.md"),
    ),
    skill(
        "processes-and-standards",
        "Processes and standards",
        "Which processes apply, must-follow versus overridable, the --why chain, and project conventions.",
        include_str!("../skills/processes-and-standards.md"),
    ),
    skill(
        "budgets",
        "Budgets",
        "Checking your and your task's budget, spending well, and what a budget hold means.",
        include_str!("../skills/budgets.md"),
    ),
    skill(
        "errors-and-recovery",
        "Errors and recovery",
        "The error codes, optimistic concurrency, safe retries, and stalled or restarted sessions.",
        include_str!("../skills/errors-and-recovery.md"),
    ),
    skill(
        "batch-and-attach",
        "Batching with session attach",
        "Running several commands over one connection, one JSON line in and one envelope out each.",
        include_str!("../skills/batch-and-attach.md"),
    ),
];

/// Returns all built-in product skills in stable order.
pub fn list_builtin_skills() -> &'static [BuiltinSkill] {
    BUILTIN_SKILLS
}

/// Finds a built-in product skill by exact lowercase key (trimmed, case-insensitive lookup).
pub fn find_builtin_skill(name: &str) -> Option<&'static BuiltinSkill> {
    let needle = name.trim().to_ascii_lowercase();
    BUILTIN_SKILLS.iter().find(|s| s.key == needle)
}
