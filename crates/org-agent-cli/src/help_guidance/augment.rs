//! Recognizing `--help` requests and enriching their plain-text output with `Use when:` guidance.

use org_command::{Command, CommandTable, Output, Type};

use super::{command_use_when, resource_use_when_for, COMMON_VALUE_OPTIONS, TOP_USE_WHEN};

pub(super) enum HelpTarget<'a> {
    Top,
    Resource(&'a str),
    Command(&'static Command),
}

/// `--help`, or its short form `-h` (as `org-command` reads them).
fn is_help(arg: &str) -> bool {
    arg == "--help" || arg == "-h"
}

pub(super) fn asks_for_help(command: &Command, rest: &[String]) -> bool {
    let mut at = 0;
    while let Some(arg) = rest.get(at) {
        at += 1;
        if is_help(arg) {
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

pub(super) fn help_target<'a>(
    table: &CommandTable,
    effective_args: &'a [String],
) -> Option<HelpTarget<'a>> {
    let first = effective_args.first()?;
    if is_help(first) {
        return Some(HelpTarget::Top);
    }
    if table.verbs(first).is_empty() {
        return None;
    }
    let second = effective_args.get(1)?;
    if is_help(second) {
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

pub(super) fn augment_top_help(table: &CommandTable, program: &str, help: &str) -> String {
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
    if program == "agent-os" {
        out.push_str(WORKSPACE_SIGN_IN_HELP);
    }
    out
}

/// The external lifecycle verbs (`TASK-137`), which sit outside the command table.
const WORKSPACE_SIGN_IN_HELP: &str = "
Workspace sign-in (bring your own agent):
  login   Sign this workspace in as an agent once a person approves (agent-os login --as @<handle>)
  whoami  Show the agent, office, mode (managed|external), session and expiry this shell acts as
  logout  End this workspace's external session and remove its local binding
  exec    Run one command as an agent with a session your signed-in agentic-org approves
";

pub(super) fn augment_resource_help(
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

pub(super) fn augment_command_help(command: &Command, help: &str) -> String {
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
