//! Built-in product skills served by `agent-os skill list` and `agent-os skill read`
//! (`doc-34` Decision 4, `doc-28` §3.5, `FC-111`).

/// One built-in product skill explaining how to operate inside Agentic Org.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuiltinSkill {
    /// Stable lowercase skill key passed to `agent-os skill read --name <key>`.
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

const BUILTIN_SKILLS: &[BuiltinSkill] = &[
    BuiltinSkill {
        key: "agent-os-basics",
        title: "Using agent-os",
        description: "Grammar, JSON envelopes, dry-run checks, idempotency, and session attach.",
        source: "built_in",
        body_markdown: "\
# Using `agent-os`

Every organization action runs through `agent-os <resource> <verb> [flags]` and returns one JSON envelope: `{\"ok\":true,\"data\":...,\"meta\":...,\"error\":null}`.

- Discover commands with `agent-os --help`, `agent-os <resource> --help`, or `agent-os <resource> <verb> --schema`.
- Verify authorization and validation without committing by passing `--dry-run`.
- For retries after transient errors, reuse `--idempotency-key` and supply `--if-match <revision>` on revisioned updates.
- Inspect your own live session and permissions with `agent-os session inspect` and `agent-os grant list`.
- Batch multiple commands in one shell call with `agent-os session attach`.",
    },
    BuiltinSkill {
        key: "messaging-and-asks",
        title: "Messaging, channels, and structured asks",
        description: "Reading your inbox, sending messages, asking structured questions, and suspending cleanly.",
        source: "built_in",
        body_markdown: "\
# Messaging and Structured Asks

- Check work addressed to you with `agent-os msg inbox` and read threads with `agent-os msg read --id <id>`.
- Send direct messages or channel posts with `agent-os msg send --to <target> --body <text>` (use `@handle` for mentions and `--kind handoff` for handoffs).
- Ask structured questions with `agent-os msg ask --to <person-or-agent> --prompt <text>`.
  - Leave `--required` off and provide defaults when you can continue work without blocking.
  - Set `--required` only when you genuinely cannot proceed; the office suspends your turn and resumes you in the same context once answered.
- Summarize busy channels without flooding your context using `agent-os channel summarize --channel <ch> --about <topic>`.",
    },
    BuiltinSkill {
        key: "tasks-and-worktrees",
        title: "Tasks, checklists, private notes, and Git worktrees",
        description: "Managing tasks, private scratch notes, isolated per-task worktrees, and office Git pushes.",
        source: "built_in",
        body_markdown: "\
# Tasks, Private Notes, and Git Worktrees

- List and inspect tasks with `agent-os task list` and `agent-os task inspect --task <ref>`.
- Transition status with `agent-os task transition --task <ref> --status <status>` (supply `--reason` for `blocked`).
- Track your private reasoning or plans with `agent-os task note add|list|update|remove` (private to you and people; never shown to other agents).
- Prepare your isolated per-task Git worktree with `agent-os worktree prepare --task <ref> --repo <repo>` and publish your `task/*` branch only through `agent-os repo push`.",
    },
    BuiltinSkill {
        key: "processes-and-standards",
        title: "Organization processes and standards",
        description: "Inspecting effective processes/standards, checking override chains, and adding project standards.",
        source: "built_in",
        body_markdown: "\
# Organization Processes and Standards

- List effective processes and standards for your work with `agent-os process list` (filter with `--kind process` or `--kind standard`).
- Read any document's full text with `agent-os process inspect --key <key>`, or inspect its single-chain inheritance with `agent-os process inspect --key <key> --why`.
- Non-overridable (`overridable: false`) documents are mandatory and cannot be superseded by lower scopes or workspace files.
- Agents whose home team owns a project (or sits beneath the owning team) may document new project-scoped conventions with `agent-os process create` and `agent-os process update` (overridable only; never replacing keys inherited from the organization or teams).",
    },
    BuiltinSkill {
        key: "hiring-and-directory",
        title: "Directory lookup and hiring through HR",
        description: "Finding people, teams, and roles, and drafting or confirming agent hires.",
        source: "built_in",
        body_markdown: "\
# Directory Lookup and Hiring

- Inspect the organization with `agent-os org inspect`, teams with `agent-os team list|inspect`, and employees with `agent-os person inspect` or `agent-os agent inspect`.
- Inspect available roles with `agent-os role list` and `agent-os role inspect --role <key>`.
- Authorized hirers (owner or team leads with hiring permission) hire into their subtree using `agent-os hr hire` (or `--draft` to have the HR lead prepare a proposal first).",
    },
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
