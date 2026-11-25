//! Concise `Use when:` intent guidance for every `agent-os` (and shared `agentic-org`)
//! `--help` output without bloating the 4 KiB `CommandTable::catalogue()` launch summary.

use org_command::{Command, CommandTable, Output, Type};

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
        "registering or listing Git repositories, attaching them to projects, checking cache readiness, or pushing task branches.",
    ),
    (
        "role",
        "listing, inspecting, creating, or updating reusable role definitions in the role catalog.",
    ),
    (
        "session",
        "inspecting agent session state/usage or batching commands over a persistent socket (`session attach`).",
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
        "granting or revoking an employee's authorization to hire agents (requires `--if-match`).",
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
        "querying and sorting tasks by status, team, assignee, project, labels, dependencies, or search text.",
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

enum HelpTarget<'a> {
    Top,
    Resource(&'a str),
    Command(&'static Command),
}

fn asks_for_help(command: &Command, rest: &[String]) -> bool {
    let mut at = 0;
    while let Some(arg) = rest.get(at) {
        at += 1;
        if arg == "--help" {
            return true;
        }
        let Some(name) = arg.strip_prefix("--") else {
            continue;
        };
        let takes_value = COMMON_VALUE_OPTIONS.contains(&name)
            || command
                .input
                .iter()
                .any(|field| field.flag() == name && field.is_flag() && field.ty != Type::Boolean);
        if takes_value {
            at += 1;
        }
    }
    false
}

fn help_target<'a>(table: &CommandTable, effective_args: &'a [String]) -> Option<HelpTarget<'a>> {
    let first = effective_args.first()?;
    if first == "--help" {
        return Some(HelpTarget::Top);
    }
    if table.verbs(first).is_empty() {
        return None;
    }
    let second = effective_args.get(1)?;
    if second == "--help" {
        return Some(HelpTarget::Resource(first));
    }
    let command = table.find(first, second)?;
    if asks_for_help(command, &effective_args[2..]) {
        Some(HelpTarget::Command(command))
    } else {
        None
    }
}

/// Enriches plain-text `--help` output from `org_command::run` with concise `Use when:` guidance
/// while leaving all JSON/YAML/compact command output and error envelopes untouched.
pub(crate) fn augment_help(
    table: &CommandTable,
    program: &str,
    effective_args: &[String],
    mut output: Output,
) -> Output {
    if output.exit_code != 0 || !output.stdout.starts_with("Usage: ") {
        return output;
    }
    let Some(target) = help_target(table, effective_args) else {
        return output;
    };
    output.stdout = match target {
        HelpTarget::Top => augment_top_help(table, program, &output.stdout),
        HelpTarget::Resource(resource) => {
            augment_resource_help(table, program, resource, &output.stdout)
        }
        HelpTarget::Command(command) => augment_command_help(command, &output.stdout),
    };
    output
}

fn augment_top_help(table: &CommandTable, program: &str, help: &str) -> String {
    let width = table
        .resources()
        .iter()
        .map(|name| name.len())
        .max()
        .unwrap_or(0);
    let mut out = String::with_capacity(help.len() + 2048);
    let mut in_resources = false;
    for (idx, line) in help.lines().enumerate() {
        out.push_str(line);
        out.push('\n');
        if idx == 0 {
            out.push_str(&format!("Use when: {TOP_USE_WHEN}\n"));
            continue;
        }
        if line == "Resources:" {
            in_resources = true;
            continue;
        }
        if in_resources {
            if line.trim().is_empty() {
                in_resources = false;
            } else if let Some(resource) = line
                .strip_prefix("  ")
                .and_then(|rest| rest.split_whitespace().next())
            {
                if let Some(cue) = resource_use_when_for(program, resource) {
                    out.push_str(&format!("  {:width$}  Use when: {cue}\n", ""));
                }
            }
        }
    }
    out
}

fn augment_resource_help(
    table: &CommandTable,
    program: &str,
    resource: &str,
    help: &str,
) -> String {
    let mut width = table
        .verbs(resource)
        .iter()
        .map(|row| row.verb.len())
        .max()
        .unwrap_or(0);
    if resource == "session" && program == "agent-os" {
        width = width.max("attach".len());
    }
    let mut out = String::with_capacity(help.len() + 1024);
    let mut in_verbs = false;
    for (idx, line) in help.lines().enumerate() {
        if in_verbs && line.trim().is_empty() {
            if resource == "session" && program == "agent-os" {
                out.push_str(&format!(
                    "  {:width$}  Run one JSON command per stdin line over a persistent office socket\n",
                    "attach"
                ));
                out.push_str(&format!(
                    "  {:width$}  Use when: batching multiple agent-os calls in one shell turn without rebinding.\n",
                    ""
                ));
            }
            in_verbs = false;
        }
        out.push_str(line);
        out.push('\n');
        if idx == 0 {
            if let Some(cue) = resource_use_when_for(program, resource) {
                out.push_str(&format!("Use when: {cue}\n"));
            }
            continue;
        }
        if line == "Verbs:" {
            in_verbs = true;
            continue;
        }
        if in_verbs {
            if let Some(verb) = line
                .strip_prefix("  ")
                .and_then(|rest| rest.split_whitespace().next())
            {
                if let Some(cue) = command_use_when(resource, verb) {
                    out.push_str(&format!("  {:width$}  Use when: {cue}\n", ""));
                }
            }
        }
    }
    out
}

fn augment_command_help(command: &Command, help: &str) -> String {
    let Some(cue) = command_use_when(command.resource, command.verb) else {
        return help.to_owned();
    };
    let mut out = String::with_capacity(help.len() + cue.len() + 16);
    for (idx, line) in help.lines().enumerate() {
        out.push_str(line);
        out.push('\n');
        // Line 0 is `Usage: ...`, line 1 is blank, line 2 is `{summary}`.
        if idx == 2 {
            out.push_str(&format!("Use when: {cue}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use org_command::{Answer, Office, Refusal, Request, CATALOGUE_LIMIT};

    use super::*;
    use crate::{execute_with_office, table, SESSION_ATTACH_HELP};

    struct PanicOffice;

    impl Office for PanicOffice {
        fn call(&mut self, request: &Request) -> Result<Answer, Refusal> {
            panic!(
                "help must never contact the office: {}",
                request.command.name()
            );
        }
    }

    #[test]
    fn root_catalogue_stays_well_under_catalogue_limit() {
        let table = table::table();
        let catalogue = table.catalogue();
        assert!(
            catalogue.len() <= CATALOGUE_LIMIT,
            "root catalogue is {} bytes; limit is {CATALOGUE_LIMIT}",
            catalogue.len()
        );
        assert!(
            catalogue.len() <= 3800,
            "expected root catalogue comfortably below {CATALOGUE_LIMIT} bytes, got {}",
            catalogue.len()
        );
    }

    #[test]
    fn guidance_tables_match_live_command_table_exactly() {
        let table = table::table();
        let live_resources: BTreeSet<&str> = table.resources().into_iter().collect();
        let guided_resources: BTreeSet<&str> =
            RESOURCE_GUIDANCE.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            RESOURCE_GUIDANCE.len(),
            guided_resources.len(),
            "duplicate resource in RESOURCE_GUIDANCE"
        );
        assert_eq!(
            guided_resources, live_resources,
            "RESOURCE_GUIDANCE drifted from live table resources"
        );

        let live_commands: BTreeSet<(&str, &str)> = table
            .rows()
            .iter()
            .map(|row| (row.resource, row.verb))
            .collect();
        let guided_commands: BTreeSet<(&str, &str)> =
            COMMAND_GUIDANCE.iter().map(|(r, v, _)| (*r, *v)).collect();
        assert_eq!(
            COMMAND_GUIDANCE.len(),
            guided_commands.len(),
            "duplicate command in COMMAND_GUIDANCE"
        );
        assert_eq!(
            guided_commands, live_commands,
            "COMMAND_GUIDANCE drifted from live CommandTable rows"
        );
    }

    #[test]
    fn every_agent_os_help_tier_and_alias_includes_use_when_guidance() {
        let table = table::table();
        let mut office = PanicOffice;

        let top = execute_with_office("agent-os", &["--help".to_owned()], &mut office);
        assert_eq!(top.exit_code, 0);
        assert!(top.stdout.contains("Use when:"), "{}", top.stdout);
        for resource in table.resources() {
            let cue = resource_use_when(resource).unwrap();
            assert!(
                top.stdout.contains(cue),
                "top --help missing resource guidance for `{resource}`:\n{}",
                top.stdout
            );

            let res_help = execute_with_office(
                "agent-os",
                &[resource.to_owned(), "--help".to_owned()],
                &mut office,
            );
            assert_eq!(res_help.exit_code, 0, "{resource} --help");
            assert!(
                res_help.stdout.contains(&format!("Use when: {cue}")),
                "{resource} --help missing resource Use when:\n{}",
                res_help.stdout
            );

            for cmd in table.verbs(resource) {
                let cmd_cue = command_use_when(cmd.resource, cmd.verb).unwrap();
                assert!(
                    res_help.stdout.contains(&format!("Use when: {cmd_cue}")),
                    "{resource} --help missing verb guidance for `{}`:\n{}",
                    cmd.name(),
                    res_help.stdout
                );

                let cmd_help = execute_with_office(
                    "agent-os",
                    &[
                        cmd.resource.to_owned(),
                        cmd.verb.to_owned(),
                        "--help".to_owned(),
                    ],
                    &mut office,
                );
                assert_eq!(cmd_help.exit_code, 0, "{} --help", cmd.name());
                assert!(
                    cmd_help.stdout.contains(&format!("Use when: {cmd_cue}")),
                    "{} --help missing Use when:\n{}",
                    cmd.name(),
                    cmd_help.stdout
                );
            }
        }

        // Aliases & special transport help.
        let who_help = execute_with_office(
            "agent-os",
            &["who".to_owned(), "--help".to_owned()],
            &mut office,
        );
        assert_eq!(who_help.exit_code, 0);
        assert!(who_help.stdout.contains("Use when:"), "{}", who_help.stdout);

        let note_help = execute_with_office(
            "agent-os",
            &["task".to_owned(), "note".to_owned(), "--help".to_owned()],
            &mut office,
        );
        assert_eq!(note_help.exit_code, 0);
        assert!(
            note_help.stdout.contains("Use when:"),
            "{}",
            note_help.stdout
        );

        let note_add_help = execute_with_office(
            "agent-os",
            &[
                "task".to_owned(),
                "note".to_owned(),
                "add".to_owned(),
                "--help".to_owned(),
            ],
            &mut office,
        );
        assert_eq!(note_add_help.exit_code, 0);
        assert!(
            note_add_help.stdout.contains("Use when:"),
            "{}",
            note_add_help.stdout
        );

        assert!(
            SESSION_ATTACH_HELP.contains("Use when:"),
            "{SESSION_ATTACH_HELP}"
        );

        let session_help = execute_with_office(
            "agent-os",
            &["session".to_owned(), "--help".to_owned()],
            &mut office,
        );
        assert!(
            session_help.stdout.contains("attach") && session_help.stdout.contains("Use when:"),
            "{}",
            session_help.stdout
        );

        let human_session_help = execute_with_office(
            "agentic-org",
            &["session".to_owned(), "--help".to_owned()],
            &mut office,
        );
        assert_eq!(human_session_help.exit_code, 0);
        assert!(
            human_session_help.stdout.contains("Use when:")
                && !human_session_help.stdout.contains("attach"),
            "agentic-org session --help must not mention `attach`:\n{}",
            human_session_help.stdout
        );

        let human_top_help =
            execute_with_office("agentic-org", &["--help".to_owned()], &mut office);
        assert_eq!(human_top_help.exit_code, 0);
        assert!(
            !human_top_help.stdout.contains("session attach"),
            "agentic-org top --help must not mention `session attach`:\n{}",
            human_top_help.stdout
        );
    }

    fn parse_value_option_names(stdout: &str) -> BTreeSet<String> {
        stdout
            .lines()
            .skip_while(|line| *line != "Options:")
            .skip(1)
            .take_while(|line| !line.trim().is_empty())
            .filter_map(|line| {
                let mut tokens = line.split_whitespace();
                let name = tokens.next()?.strip_prefix("--")?;
                let arg = tokens.next()?;
                arg.starts_with('<').then(|| name.to_owned())
            })
            .collect()
    }

    #[test]
    fn common_value_options_match_live_org_command_help() {
        let table = table::table();
        let mut sources = crate::sources::StdSources;
        let mut keys = crate::keys::UuidV7KeySource;
        let mut office = PanicOffice;

        // `task list` covers All + Lists + WithInput; `task update` covers All + WithInput + Writes + Revisioned.
        let mut observed = BTreeSet::new();
        for args in [
            vec!["task".to_owned(), "list".to_owned(), "--help".to_owned()],
            vec!["task".to_owned(), "update".to_owned(), "--help".to_owned()],
        ] {
            let out = org_command::run(
                &table,
                "agent-os",
                &args,
                &mut sources,
                &mut keys,
                &mut office,
            );
            assert_eq!(out.exit_code, 0, "{args:?}");
            observed.extend(parse_value_option_names(&out.stdout));
        }

        let declared: BTreeSet<String> = COMMON_VALUE_OPTIONS
            .iter()
            .map(|name| (*name).to_owned())
            .collect();
        assert_eq!(
            COMMON_VALUE_OPTIONS.len(),
            declared.len(),
            "duplicate entry in COMMON_VALUE_OPTIONS"
        );
        assert_eq!(
            declared, observed,
            "COMMON_VALUE_OPTIONS drifted from live org_command Options:"
        );
    }

    struct RecordingOffice {
        calls: Vec<serde_json::Map<String, serde_json::Value>>,
    }

    impl Office for RecordingOffice {
        fn call(&mut self, request: &Request) -> Result<Answer, Refusal> {
            self.calls.push(request.input.clone());
            let data = serde_json::from_str(request.command.example.output)
                .expect("example output is valid JSON");
            Ok(Answer::completed(data))
        }
    }

    #[test]
    fn literal_help_flag_value_is_not_mistaken_for_help_request() {
        let mut office = RecordingOffice { calls: Vec::new() };
        let body_out = execute_with_office(
            "agent-os",
            &[
                "msg".to_owned(),
                "send".to_owned(),
                "--to".to_owned(),
                "#general".to_owned(),
                "--body".to_owned(),
                "--help".to_owned(),
            ],
            &mut office,
        );
        assert_eq!(body_out.exit_code, 0, "{}", body_out.stdout);
        let env: serde_json::Value =
            serde_json::from_str(&body_out.stdout).expect("expected JSON envelope, not help text");
        assert_eq!(env["ok"], true, "{env}");
        assert_eq!(office.calls.len(), 1);
        assert_eq!(
            office.calls[0].get("body").and_then(|v| v.as_str()),
            Some("--help")
        );

        let mut panic_office = PanicOffice;
        let json_out = execute_with_office(
            "agent-os",
            &[
                "msg".to_owned(),
                "send".to_owned(),
                "--json".to_owned(),
                "--help".to_owned(),
            ],
            &mut panic_office,
        );
        assert_eq!(json_out.exit_code, 2, "{}", json_out.stdout);
        let err_env: serde_json::Value =
            serde_json::from_str(&json_out.stdout).expect("expected JSON error envelope");
        assert_eq!(err_env["ok"], false, "{err_env}");
        assert_eq!(err_env["error"]["code"], "invalid_request", "{err_env}");

        let real_help = execute_with_office(
            "agent-os",
            &[
                "msg".to_owned(),
                "send".to_owned(),
                "--to".to_owned(),
                "#general".to_owned(),
                "--help".to_owned(),
            ],
            &mut panic_office,
        );
        assert_eq!(real_help.exit_code, 0);
        assert!(
            real_help.stdout.contains("Use when:"),
            "{}",
            real_help.stdout
        );
    }
}
