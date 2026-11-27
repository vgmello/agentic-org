//! The index of the built-in guides (`agent-os skill list`: key and one-line description), for
//! `session boot` to answer without the office. The guides themselves live in `org-process`,
//! which serves them; `org-office-sessions`' tests hold this index to that list, entry by entry.

/// Each built-in guide's key and description, in `skill list` order.
pub const SKILL_INDEX: &[(&str, &str)] = &[
    (
        "agent-os-basics",
        "CLI grammar, envelopes, schemas, dry runs.",
    ),
    ("tasks-create", "Search first, scope, criteria, and why."),
    ("tasks-execute", "Deps, private planning, verified loops."),
    ("tasks-finalize", "Verify criteria, checklist, docs, close."),
    (
        "messaging-and-asks",
        "Inbox, replies, handoffs, asks, snoozes.",
    ),
    ("channels", "Join, summarize, read, post, ask channels."),
    (
        "code-and-worktrees",
        "Prepare worktrees and publish branches.",
    ),
    (
        "hiring-and-directory",
        "People, teams, roles, and HR hiring.",
    ),
    (
        "processes-and-standards",
        "Effective rules, precedence, and --why.",
    ),
    ("budgets", "Allowances, spend hygiene, budget holds."),
    (
        "errors-and-recovery",
        "Error codes, retries, concurrency, stalls.",
    ),
    ("batch-and-attach", "Batch JSON commands on one connection."),
    ("product-org-and-teams", "Orgs, teams, roles, hiring, caps."),
    (
        "product-projects-and-work",
        "Projects, repos, worktrees, tasks.",
    ),
    (
        "product-processes-and-standards",
        "Inheritance chains, rules, --why.",
    ),
    (
        "product-offices-and-runtimes",
        "Offices, runtimes, profiles, pools.",
    ),
    (
        "product-budgets-and-permissions",
        "Budgets, holds, presets, sandbox.",
    ),
    (
        "product-troubleshooting",
        "Offices, stalls, limits, recovery.",
    ),
    ("tess-workflows", "Briefs, batching, proposals, UI."),
];
