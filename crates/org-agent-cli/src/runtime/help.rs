//! `agent-os --help`: the quickstart it opens with in a shell ([`quickstart`]), and the
//! plain-text help of `org-command` enriched with `Use when:` guidance ([`augment_help`]), which
//! the feature slices declare and the registry hands in ([`Guidance`]). JSON, YAML and compact
//! answers and every error envelope are left untouched.

use org_command::{Ack, Command, CommandTable, Output};

use crate::runtime::domain::{command_cue, resource_cue, Domain, ResourceCue};

/// The help `org-command` prints for `--durable`.
const DURABLE_OPTION_HELP: &str = "Answer only after the drive has it";

/// The four lines that open `agent-os --help` in a shell with no office binding.
const QUICKSTART_UNBOUND: &str = "Agent quickstart: this shell has no office binding.
  Sign in first: agent-os login --as @<handle>
  Rules, commands and guides without the office: agent-os context --offline --format yaml
  Office-started sessions are bound already and get their rules on turn 1.
";

/// The four lines that open `agent-os --help` in a bound shell: an office-started session
/// (`AGENT_OS_*`) or a workspace signed in with `agent-os login`.
const QUICKSTART_BOUND: &str = "Agent quickstart: this shell is bound to its office (an office-started session or a signed-in workspace).
  Rules, commands, guides and must-follow processes: agent-os context --format yaml
  Office-started sessions get them on turn 1 and `agent-os login` returns them in `data.context`: re-run only after context compaction.
  Then: agent-os whoami, agent-os <resource> --help, and agent-os skill list for guides.
";

/// The quickstart `agent-os --help` opens with: how to get bound when `bound` is false, and
/// when `agent-os context` is worth running when it is true.
pub(crate) fn quickstart(bound: bool) -> &'static str {
    if bound {
        QUICKSTART_BOUND
    } else {
        QUICKSTART_UNBOUND
    }
}

const TOP_USE_WHEN: &str = "running bound agent or signed-in operator organization commands \
(tasks, messages, channels, repos, worktrees, processes, HR, sessions).";

/// A verb `agent-os` runs outside the table, listed with its resource's verbs.
pub(crate) struct ExtraVerb {
    pub(crate) resource: &'static str,
    pub(crate) verb: &'static str,
    pub(crate) summary: &'static str,
    pub(crate) use_when: &'static str,
}

/// Everything the help text adds to what `org-command` prints, as the slices declare it.
pub(crate) struct Guidance {
    pub(crate) domains: &'static [Domain],
    /// Resource guidance that differs for a program other than `agent-os` (the operator's
    /// `agentic-org`), where the agent-only verbs do not exist.
    pub(crate) operator_resources: &'static [ResourceCue],
    /// Verbs only `agent-os` runs, outside the table (`session attach`).
    pub(crate) agent_verbs: &'static [ExtraVerb],
    /// Verbs only `agentic-org` runs, outside the table (`agent bind`).
    pub(crate) operator_verbs: &'static [ExtraVerb],
    /// What `agent-os --help` closes with: the top-level verbs outside the table.
    pub(crate) agent_footer: &'static str,
}

impl Guidance {
    fn resource(&self, program: &str, resource: &str) -> Option<&'static str> {
        if program != "agent-os" {
            if let Some((_, cue)) = self
                .operator_resources
                .iter()
                .find(|(name, _)| *name == resource)
            {
                return Some(cue);
            }
        }
        resource_cue(self.domains, resource)
    }

    fn command(&self, resource: &str, verb: &str) -> Option<&'static str> {
        command_cue(self.domains, resource, verb)
    }

    fn extra_verbs<'a>(
        &'a self,
        program: &'a str,
        resource: &'a str,
    ) -> impl Iterator<Item = &'a ExtraVerb> + 'a {
        let verbs = if program == "agent-os" {
            self.agent_verbs
        } else {
            self.operator_verbs
        };
        verbs.iter().filter(move |extra| extra.resource == resource)
    }
}

pub(crate) enum HelpTarget<'a> {
    Top,
    Resource(&'a str),
    Command(&'static Command),
}

/// `--help`, or its short form `-h` (as `org-command` reads them).
fn is_help(arg: &str) -> bool {
    arg == "--help" || arg == "-h"
}

/// Whether the words after a command ask for its help: `--help` or `-h` as a word of its own,
/// never as the value of a flag that takes one (`--body --help` sends the text `--help`). Flags
/// are stepped over the way the parser reads them ([`Command::flag_takes_value`]).
pub(crate) fn asks_for_help(command: &Command, rest: &[String]) -> bool {
    let mut at = 0;
    while let Some(arg) = rest.get(at) {
        at += 1;
        if is_help(arg) {
            return true;
        }
        let Some(name) = arg.strip_prefix("--") else {
            continue;
        };
        if command.flag_takes_value(name) == Some(true) {
            at += 1;
        }
    }
    false
}

pub(crate) fn help_target<'a>(
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
    guidance: &Guidance,
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
        HelpTarget::Top => augment_top_help(table, guidance, program, &output.stdout),
        HelpTarget::Resource(resource) => {
            augment_resource_help(table, guidance, program, resource, &output.stdout)
        }
        HelpTarget::Command(command) => {
            augment_command_help(guidance, program, command, &output.stdout)
        }
    };
    output
}

fn augment_top_help(
    table: &CommandTable,
    guidance: &Guidance,
    program: &str,
    help: &str,
) -> String {
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
                if let Some(cue) = guidance.resource(program, resource) {
                    out.push_str(&format!("  {:width$}  Use when: {cue}\n", ""));
                }
            }
        }
    }
    if program == "agent-os" {
        out.push_str(guidance.agent_footer);
    }
    out
}

fn augment_resource_help(
    table: &CommandTable,
    guidance: &Guidance,
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
    for extra in guidance.extra_verbs(program, resource) {
        width = width.max(extra.verb.len());
    }
    let mut out = String::with_capacity(help.len() + 1024);
    let mut in_verbs = false;
    for (idx, line) in help.lines().enumerate() {
        if in_verbs && line.trim().is_empty() {
            for extra in guidance.extra_verbs(program, resource) {
                out.push_str(&format!("  {:width$}  {}\n", extra.verb, extra.summary));
                out.push_str(&format!("  {:width$}  Use when: {}\n", "", extra.use_when));
            }
            in_verbs = false;
        }
        out.push_str(line);
        out.push('\n');
        if idx == 0 {
            if let Some(cue) = guidance.resource(program, resource) {
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
                if let Some(cue) = guidance.command(resource, verb) {
                    out.push_str(&format!("  {:width$}  Use when: {cue}\n", ""));
                }
            }
        }
    }
    out
}

/// What `--durable` does for `command`, as `agent-os` dispatches it: always on for a write the
/// office acknowledges only once durable, refused for one it acknowledges at commit
/// (`transport::refuse_durable`).
fn durable_help(command: &Command) -> Option<&'static str> {
    match command.ack()? {
        Ack::Durable => Some("Always on: this command answers only after the drive has it"),
        Ack::Committed => {
            Some("Refused: this command is acknowledged at commit (`ack: committed` in --schema)")
        }
    }
}

fn augment_command_help(
    guidance: &Guidance,
    program: &str,
    command: &Command,
    help: &str,
) -> String {
    let durable = durable_help(command);
    let cue = guidance.command(command.resource, command.verb);
    let mut out = String::with_capacity(help.len() + 256);
    for (idx, line) in help.lines().enumerate() {
        match durable {
            Some(text) if line.trim_start().starts_with("--durable ") => {
                out.push_str(&line.replace(DURABLE_OPTION_HELP, text));
            }
            _ => out.push_str(line),
        }
        out.push('\n');
        let Some(cue) = cue else {
            continue;
        };
        // Line 0 is `Usage: ...`, line 1 is blank, line 2 is `{summary}`.
        if idx == 2 {
            out.push_str(&format!("Use when: {cue}\n"));
        }
    }
    if program != "agent-os" && command.resource == "hr" && command.verb == "hire" {
        out.push_str(
            "\nOperator workspace binding:\n  --bind-workspace <dir>  Immediately bind a local directory or Git repo to the newly hired agent (outside Git, no approval code).\n",
        );
    }
    out
}

#[cfg(test)]
mod tests;
