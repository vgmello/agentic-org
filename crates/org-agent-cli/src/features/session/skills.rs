//! The index of the built-in guides (`agent-os skill list`: key and one-line description), for
//! `session boot` to answer without the office. The guides themselves live in `org-process`,
//! which serves them; `org-office-sessions`' tests hold this index to that list, entry by entry.

/// Each built-in guide's key and description, in `skill list` order.
pub const SKILL_INDEX: &[(&str, &str)] = &[
    (
        "agent-os-basics",
        "Command grammar, the JSON envelope, help and schemas, small answers, long text, writes and dry runs.",
    ),
    (
        "tasks-create",
        "Search first, decide the scope, then write a durable why and testable acceptance criteria.",
    ),
    (
        "tasks-execute",
        "Read the task and its dependencies, plan privately against live code, start, and work in verified loops.",
    ),
    (
        "tasks-finalize",
        "Verify each criterion, tick the checklist with evidence, update docs, comment, and close.",
    ),
    (
        "messaging-and-asks",
        "Inbox, sending and replying, handoffs, structured asks and answers, snoozing, and waiting well.",
    ),
    (
        "channels",
        "Joining, summarizing with a question, reading, posting, and asking a channel.",
    ),
    (
        "code-and-worktrees",
        "Preparing a task worktree, working in it, and publishing the task branch through the office.",
    ),
    (
        "hiring-and-directory",
        "Finding people, teams and roles, who decides, and hiring agents through HR.",
    ),
    (
        "processes-and-standards",
        "Which processes apply, must-follow versus overridable, the --why chain, and project conventions.",
    ),
    (
        "budgets",
        "Checking your and your task's budget, spending well, and what a budget hold means.",
    ),
    (
        "errors-and-recovery",
        "The error codes, optimistic concurrency, safe retries, and stalled or restarted sessions.",
    ),
    (
        "batch-and-attach",
        "Running several commands over one connection, one JSON line in and one envelope out each.",
    ),
];
