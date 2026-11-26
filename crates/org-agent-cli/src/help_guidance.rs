//! Concise `Use when:` intent guidance for every `agent-os` (and shared `agentic-org`)
//! `--help` output without bloating the 4 KiB `CommandTable::catalogue()` launch summary.

mod augment;

pub(crate) use augment::augment_help;

/// The four lines that open `agent-os --help` in a shell with no office binding.
const QUICKSTART_UNBOUND: &str = "Agent quickstart: this shell has no office binding.
  Sign in first: agent-os login --as @<handle>
  Rules, commands and guides without the office: agent-os boot --offline --format yaml
  Office-started sessions are bound already and get their rules on turn 1.
";

/// The four lines that open `agent-os --help` in a bound shell.
const QUICKSTART_BOUND: &str = "Agent quickstart: this shell is bound to its office.
  Rules, commands, guides and must-follow processes: agent-os boot --format yaml
  Managed sessions already have them from turn 1: re-run only after context compaction.
  Then: agent-os <resource> --help, and agent-os skill list for guides.
";

/// The quickstart `agent-os --help` opens with: how to get bound when `bound` is false, and
/// when `agent-os boot` is worth running when it is true.
pub fn quickstart(bound: bool) -> &'static str {
    if bound {
        QUICKSTART_BOUND
    } else {
        QUICKSTART_UNBOUND
    }
}

const TOP_USE_WHEN: &str = "running bound agent or signed-in operator organization commands \
(tasks, messages, channels, repos, worktrees, processes, HR, sessions).";

const COMMON_VALUE_OPTIONS: &[&str] = &[
    "format",
    "fields",
    "limit",
    "cursor",
    "json",
    "input-file",
    "input-format",
    "idempotency-key",
    "if-match",
];

/// Resource-level `Use when:` guidance, sorted by resource name.
pub const RESOURCE_GUIDANCE: &[(&str, &str)] = &[
    (
        "agent",
        "inspecting or managing agent profiles, team placement, pause/resume, or offboarding.",
    ),
    (
        "budget",
        "checking your own daily, monthly, or task budget limits and remaining allowance.",
    ),
    (
        "channel",
        "listing visible channels, joining/leaving manual channels, or summarizing channel Q&A.",
    ),
    (
        "checklist",
        "adding, checking, editing, removing, or reordering steps on a task's checklist.",
    ),
    (
        "directory",
        "listing active people and agents with handles, roles, and live presence (shorthand: `who`).",
    ),
    (
        "grant",
        "inspecting effective permission presets, tool rules, and sandbox boundaries for an agent.",
    ),
    (
        "hr",
        "listing, inspecting, drafting, confirming, or withdrawing agent hire requests.",
    ),
    (
        "member",
        "granting or revoking an employee's `may_hire` permission.",
    ),
    (
        "msg",
        "sending DMs or channel posts, asking structured questions, triaging your inbox, or waiting for replies.",
    ),
    (
        "org",
        "inspecting the full organization snapshot of teams, members, and log position.",
    ),
    (
        "person",
        "inspecting, adding, updating, moving, or deactivating human members in the organization.",
    ),
    (
        "process",
        "listing, inspecting (including `--why` inheritance), creating, or updating org/team/project processes and standards.",
    ),
    (
        "project",
        "listing, inspecting, or creating organization projects and their owning teams.",
    ),
    (
        "repo",
        "registering or listing Git repositories, attaching them to projects, checking cache readiness, fetching new origin branches, or pushing task branches.",
    ),
    (
        "role",
        "listing, inspecting, creating, or updating reusable role definitions in the role catalog.",
    ),
    (
        "session",
        "loading your rules and catalogue (`agent-os boot`), inspecting agent session state/usage, or batching commands over a persistent socket (`session attach`).",
    ),
    (
        "skill",
        "discovering and reading built-in `agent-os` product skills on demand.",
    ),
    (
        "status",
        "setting or clearing your short live status line visible to teammates in the directory.",
    ),
    (
        "task",
        "creating, filtering, inspecting, updating, transitioning, assigning, commenting on, or linking task dependencies.",
    ),
    (
        "task-note",
        "recording private working notes on a task or publishing a copy as a shared comment (alias: `task note`).",
    ),
    (
        "team",
        "listing, inspecting, creating, renaming, moving, capping, or archiving teams.",
    ),
    (
        "worktree",
        "listing or preparing isolated per-task Git worktrees before making code changes.",
    ),
];

/// Command-level `Use when:` guidance for every `(resource, verb)` row in `CommandTable`.
pub const COMMAND_GUIDANCE: &[(&str, &str, &str)] = &[
    // agent
    (
        "agent",
        "execution-get",
        "inspecting an agent's model profile, model ID, permission preset, reasoning effort, and standing system prompt.",
    ),
    (
        "agent",
        "execution-set",
        "configuring or updating an agent's model profile, model ID, permission preset, reasoning effort, and system prompt.",
    ),
    (
        "agent",
        "inspect",
        "looking up one agent's directory card, role, lifecycle status, and live presence.",
    ),
    (
        "agent",
        "move",
        "re-assigning an agent to a different home team (requires `--if-match`).",
    ),
    (
        "agent",
        "offboard",
        "permanently offboarding an agent and ending its active work (requires `--if-match`).",
    ),
    (
        "agent",
        "pause",
        "temporarily suspending an active agent without offboarding it (requires `--if-match`).",
    ),
    (
        "agent",
        "resume",
        "re-activating a paused agent so it can take turns again (requires `--if-match`).",
    ),
    (
        "agent",
        "update",
        "editing an agent's profile fields, role, reporting manager, or instructions body (requires `--if-match`).",
    ),
    // budget
    (
        "budget",
        "status",
        "checking your own daily, monthly, or task budget status before starting expensive work.",
    ),
    // channel
    (
        "channel",
        "join",
        "subscribing to a manual `#channel` so you can read and post in it.",
    ),
    (
        "channel",
        "leave",
        "leaving a manual `#channel` you no longer need to follow.",
    ),
    (
        "channel",
        "list",
        "discovering available `#channels`, their scope, member counts, and whether you are a member.",
    ),
    (
        "channel",
        "summarize",
        "answering a specific question from recent caller-visible `#channel` history without dumping every message.",
    ),
    // checklist
    (
        "checklist",
        "add",
        "appending a new acceptance or execution step to a task's checklist.",
    ),
    (
        "checklist",
        "check",
        "marking a checklist step done (or `--undone`) as you verify task progress.",
    ),
    (
        "checklist",
        "edit",
        "updating a checklist step's wording or `--required` flag.",
    ),
    (
        "checklist",
        "move",
        "reordering a checklist step to a new zero-based position.",
    ),
    (
        "checklist",
        "remove",
        "deleting an obsolete checklist step from a task.",
    ),
    // directory
    (
        "directory",
        "list",
        "finding active people and agents, `@handles`, roles, and live presence (shorthand: `agent-os who`).",
    ),
    // grant
    (
        "grant",
        "list",
        "verifying your (or a named agent's) effective permission preset, writable roots, and deny/allow rules.",
    ),
    // hr
    (
        "hr",
        "hire",
        "drafting (`--draft`), confirming (`--id`), directly hiring, or withdrawing (`--id --withdraw`) an agent hire request.",
    ),
    (
        "hr",
        "inspect",
        "checking one hire request's proposal details, state, or failed provisioning step by UUIDv7.",
    ),
    (
        "hr",
        "list",
        "reviewing pending, hired, failed, or withdrawn HR hire requests (optionally filtered by team or state).",
    ),
    // member
    (
        "member",
        "set-hiring",
        "granting (`--allow`) or revoking (`--revoke`) an employee's authorization to hire agents (requires `--if-match`).",
    ),
    // msg
    (
        "msg",
        "accept",
        "explicitly claiming ownership of a handoff message addressed to you.",
    ),
    (
        "msg",
        "ask",
        "asking typed questions (choice/yes_no/free_text) or requesting approval with bounded wait instead of free-form `msg send`.",
    ),
    (
        "msg",
        "defer",
        "snoozing an inbox message until a timestamp (`--until`) or until your current task finishes (`--after-task`).",
    ),
    (
        "msg",
        "done",
        "marking an inbox message handled so it leaves your unread queue.",
    ),
    (
        "msg",
        "inbox",
        "triaging messages specifically addressed to you (unread, mentions, thread replies) with delivery reasons.",
    ),
    (
        "msg",
        "list",
        "browsing message history in a `#channel`, DM participant conversation, or thread.",
    ),
    (
        "msg",
        "read",
        "reading a specific message's full body by `--id` or claiming the next unread inbox item.",
    ),
    (
        "msg",
        "reply",
        "replying in an existing message thread or submitting structured `--answer` JSON to an open ask.",
    ),
    (
        "msg",
        "send",
        "sending a direct message (DM to a handle/ID/owner), `#channel` post, handoff, or announcement.",
    ),
    (
        "msg",
        "unsnooze",
        "pulling a previously snoozed message back to unread before its wake trigger fires.",
    ),
    (
        "msg",
        "wait",
        "holding up to 90s for an incoming reply, mention, or structured `--ask` resolution.",
    ),
    (
        "msg",
        "withdraw",
        "cancelling an open structured ask you sent when the answer is no longer needed.",
    ),
    // org
    (
        "org",
        "inspect",
        "reading the full organization snapshot of active teams, members, and log position.",
    ),
    // person
    (
        "person",
        "add",
        "onboarding a new human member into the organization and optional home team.",
    ),
    (
        "person",
        "deactivate",
        "deactivating a departing human member in the organization (requires `--if-match`).",
    ),
    (
        "person",
        "inspect",
        "looking up one person's directory card, handle, presence, and team memberships.",
    ),
    (
        "person",
        "move",
        "moving a person to a different home team (requires `--if-match`).",
    ),
    (
        "person",
        "update",
        "updating a person's display name, contact info, manager, or team title (requires `--if-match`).",
    ),
    // process
    (
        "process",
        "create",
        "authoring a new organization, team, or project process or standard document.",
    ),
    (
        "process",
        "inspect",
        "reading a process/standard's full Markdown body (chunked with `--offset`) or tracing inheritance with `--why`.",
    ),
    (
        "process",
        "list",
        "checking which processes and mandatory/overridable standards apply to your current project or task.",
    ),
    (
        "process",
        "update",
        "publishing a new revision of an existing process or standard (requires `--if-match`).",
    ),
    // project
    (
        "project",
        "create",
        "creating a new project with an existing or newly created accountable owning team.",
    ),
    (
        "project",
        "inspect",
        "fetching one project's owning team, contributing teams, key, slug, and archive status.",
    ),
    (
        "project",
        "list",
        "discovering active (or archived) projects and their `P-<n>` keys, slugs, and owning teams.",
    ),
    // repo
    (
        "repo",
        "attach",
        "linking a registered repository and verified integration branch to a project.",
    ),
    (
        "repo",
        "fetch",
        "seeing branches pushed to the origin after your worktree was prepared: the office refreshes `origin/*` with its own credentials and never moves your checkout.",
    ),
    (
        "repo",
        "list",
        "listing registered Git repositories and their office bare-cache state (optionally by `--project`).",
    ),
    (
        "repo",
        "push",
        "pushing your committed task worktree branch to the repository origin via the office.",
    ),
    (
        "repo",
        "register",
        "registering a new remote or local Git repository in the organization.",
    ),
    (
        "repo",
        "status",
        "checking whether an office's bare Git cache for a repository is ready or failed before preparing a worktree.",
    ),
    // role
    (
        "role",
        "create",
        "defining a new reusable role template in the organization role catalog.",
    ),
    (
        "role",
        "inspect",
        "reading one catalog role's full Markdown instructions and metadata by ID, key, or name.",
    ),
    (
        "role",
        "list",
        "browsing available catalog roles before drafting an agent hire request.",
    ),
    (
        "role",
        "update",
        "updating a catalog role's name, key, or Markdown description (requires `--if-match`).",
    ),
    // session
    (
        "session",
        "boot",
        "loading the standing rules, command catalogue, guide index and your must-follow processes at once (`agent-os boot`; offline with `--offline`); a managed session already has them from turn 1.",
    ),
    (
        "session",
        "inspect",
        "checking your (or a named agent's) live session state, workspace paths, stall status, and token usage.",
    ),
    // skill
    (
        "skill",
        "list",
        "discovering built-in `agent-os` operational guides available to load on demand.",
    ),
    (
        "skill",
        "read",
        "reading the full Markdown instructions of a specific built-in skill by `--name`.",
    ),
    // status
    (
        "status",
        "clear",
        "clearing your live status line when you finish an activity or go idle.",
    ),
    (
        "status",
        "set",
        "publishing a short live status line (with optional `--ttl-ms`) so teammates see what you are doing.",
    ),
    // task
    (
        "task",
        "assign",
        "assigning or clearing a task's responsible team or employee (requires `--if-match`).",
    ),
    (
        "task",
        "comment",
        "posting a public update, review note, or handoff visible to everyone on the task (use `task-note` for private notes).",
    ),
    (
        "task",
        "create",
        "creating a new task, bug, or request with initial priority, labels, files, or dependencies.",
    ),
    (
        "task",
        "dep-add",
        "recording that a task depends on an upstream prerequisite task.",
    ),
    (
        "task",
        "dep-remove",
        "removing a direct dependency link between two tasks.",
    ),
    (
        "task",
        "deps",
        "inspecting direct and transitive upstream/downstream dependency edges around a task.",
    ),
    (
        "task",
        "history",
        "auditing the immutable revision event log for a task.",
    ),
    (
        "task",
        "inspect",
        "reading a task's full details, Markdown body, checklist, comments, dependencies, and history.",
    ),
    (
        "task",
        "labels",
        "discovering existing normalized task labels and counts before filtering or tagging tasks.",
    ),
    (
        "task",
        "list",
        "querying, sorting and paging tasks by status (or `--exclude-status`), team, assignee (or `--unassigned`), project, labels, dependencies, `--modified-file` or search text; `--count` for the total only.",
    ),
    (
        "task",
        "transition",
        "moving a task's lifecycle status (`todo`, `in_progress`, `blocked`, `done`, `cancelled`; requires `--if-match`).",
    ),
    (
        "task",
        "unwatch",
        "unsubscribing from updates on a task you no longer need to watch.",
    ),
    (
        "task",
        "update",
        "editing a task's title, body, priority, order, labels, or reference/modified files (requires `--if-match`).",
    ),
    (
        "task",
        "watch",
        "subscribing to updates on a task you care about.",
    ),
    // task-note
    (
        "task-note",
        "add",
        "recording a private working note on a task that is not visible to other agents until published.",
    ),
    (
        "task-note",
        "get",
        "reading one private task note and its current revision by `--id`.",
    ),
    (
        "task-note",
        "list",
        "listing your private working notes on a task.",
    ),
    (
        "task-note",
        "publish",
        "copying a private task note into a public task comment while keeping the private note intact (requires `--if-match`).",
    ),
    (
        "task-note",
        "remove",
        "deleting a private task note you no longer need (requires `--if-match`).",
    ),
    (
        "task-note",
        "update",
        "editing an existing private task note's text (requires `--if-match`).",
    ),
    // team
    (
        "team",
        "archive",
        "archiving an empty team that is no longer active (requires `--if-match`).",
    ),
    (
        "team",
        "create",
        "creating a new team (optionally nested under `--parent-team-id`).",
    ),
    (
        "team",
        "inspect",
        "viewing one team's metadata, lead, and active member directory cards.",
    ),
    (
        "team",
        "list",
        "listing all active teams, their parent hierarchy, and leads.",
    ),
    (
        "team",
        "move",
        "re-parenting a team under a different parent team (requires `--if-match`).",
    ),
    (
        "team",
        "set-limit",
        "updating a team's maximum active member cap (`1..=100`; requires `--if-match`).",
    ),
    (
        "team",
        "update",
        "renaming a team or changing/clearing its lead (requires `--if-match`).",
    ),
    // worktree
    (
        "worktree",
        "list",
        "listing existing task Git worktrees, branches, and paths (optionally filtered by `--task`).",
    ),
    (
        "worktree",
        "prepare",
        "creating or reusing an isolated per-task Git worktree and branch before editing repository code.",
    ),
];

/// Looks up concise `Use when:` guidance for a resource.
pub fn resource_use_when(resource: &str) -> Option<&'static str> {
    RESOURCE_GUIDANCE
        .iter()
        .find(|(name, _)| *name == resource)
        .map(|(_, cue)| *cue)
}

fn resource_use_when_for(program: &str, resource: &str) -> Option<&'static str> {
    if resource == "session" && program != "agent-os" {
        return Some("inspecting agent session state, workspace paths, stall status, and usage.");
    }
    resource_use_when(resource)
}

/// Looks up concise `Use when:` guidance for a `(resource, verb)` command.
pub fn command_use_when(resource: &str, verb: &str) -> Option<&'static str> {
    COMMAND_GUIDANCE
        .iter()
        .find(|(r, v, _)| *r == resource && *v == verb)
        .map(|(_, _, cue)| *cue)
}

#[cfg(test)]
mod tests;
