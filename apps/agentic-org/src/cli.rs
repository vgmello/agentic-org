//! The grammar of `agentic-org`: the global flags, the top-level commands and the office
//! resources it runs over the signed-in channel. Each host noun's own verbs and flags are
//! declared next to its slice (`features/<noun>/mod.rs`).

use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use org_cli_shell::context::{ColorMode, Modes};

use crate::args::{FormatArg, PassArgs};
use crate::features::{admin, backup, completion, context, diagnostics, office, org, project};
use crate::features::{provider, release, service, sign_in, status};

const ABOUT: &str = "Agentic Org\n\
Use when: operating a computer's Agentic Org host, organizations, offices, or signed-in \
operator workflows (managed AI agents use `agent-os` instead).";

const AFTER_HELP: &str = "Examples:
  agentic-org setup
  agentic-org org create \"Acme\" --slug acme --repo https://git.example.com/acme/org.git
  agentic-org org list --json
  agentic-org office remove <office-id> --org <org-id> --yes
  agentic-org backup <org-id> --out acme.backup --encrypt
  agentic-org task list --format compact
  agentic-org completion zsh > ~/.zfunc/_agentic-org

Satellite enrollment is initiated from HQ (`office invite`) and completed on the satellite \
(`office join`) and HQ (`office approve`). `agentic-org <command> --help` shows a command's \
flags and examples.";

#[derive(Parser, Debug)]
#[command(
    name = "agentic-org",
    version,
    about = ABOUT,
    after_help = AFTER_HELP,
    disable_help_subcommand = true,
    max_term_width = 100
)]
pub(crate) struct Cli {
    #[command(flatten)]
    pub(crate) global: GlobalArgs,
    #[command(subcommand)]
    pub(crate) command: Option<TopCommand>,
}

/// `--color`.
#[derive(ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ColorArg {
    #[default]
    Auto,
    Always,
    Never,
}

/// The flags every command takes.
#[derive(Args, Debug, Default)]
#[command(next_help_heading = "Global Options")]
pub(crate) struct GlobalArgs {
    /// Print the result as JSON on standard output, and errors as JSON on standard error
    #[arg(long, global = true)]
    pub(crate) json: bool,
    /// Print only IDs, one per line
    #[arg(short, long, global = true, conflicts_with = "json")]
    pub(crate) quiet: bool,
    /// Output format (overrides AGENT_OS_FORMAT): json, compact, yaml, tsv or table
    #[arg(
        long,
        global = true,
        value_enum,
        value_name = "FORMAT",
        conflicts_with = "quiet"
    )]
    pub(crate) format: Option<FormatArg>,
    /// Never prompt; fail instead when a value or a confirmation is missing
    #[arg(long, global = true)]
    pub(crate) no_input: bool,
    /// Show what would change, and change nothing
    #[arg(long, global = true)]
    pub(crate) dry_run: bool,
    /// When to color the output
    #[arg(long, global = true, value_enum, value_name = "WHEN", default_value_t = ColorArg::Auto)]
    pub(crate) color: ColorArg,
    /// Same as --color never
    #[arg(long, global = true, hide = true)]
    pub(crate) no_color: bool,
}

#[derive(Subcommand, Debug)]
pub(crate) enum TopCommand {
    /// Run the installed service host in the foreground
    /// Use when: running the host directly in a container, terminal, or service unit.
    #[command(
        verbatim_doc_comment,
        disable_help_flag = true,
        disable_version_flag = true
    )]
    Serve(PassArgs),
    /// Set up this computer's instance and register its service
    /// Use when: initializing a new instance, registering its OS service, or enabling HQ mTLS federation.
    #[command(verbatim_doc_comment, disable_help_flag = true)]
    Setup(PassArgs),
    /// Sign in from this terminal via a browser approval link (no password)
    /// Use when: authenticating a terminal or running one office command (`-- <cmd...>`) without passwords.
    #[command(verbatim_doc_comment)]
    SignIn(sign_in::SignInArgs),
    /// Show, pin or clear the saved default instance and office
    /// Use when: viewing, pinning, or clearing your default `--name`/`--office` target.
    #[command(verbatim_doc_comment)]
    Context(context::ContextArgs),
    /// Administrator recovery and HTTPS TLS rotation
    /// Use when: issuing a fresh admin registration link or rotating an HTTPS certificate pair.
    #[command(verbatim_doc_comment)]
    Admin(admin::AdminArgs),
    /// Create, list or inspect organizations on this host
    /// Use when: creating an HQ organization from a Git repo or listing visible organizations.
    #[command(verbatim_doc_comment)]
    Org(org::OrgArgs),
    /// List, restart, enroll and remove offices
    /// Use when: listing/restarting offices or running HQ/satellite mTLS enrollment and sync checks.
    #[command(verbatim_doc_comment)]
    Office(office::OfficeArgs),
    /// List, inspect or create projects
    /// Use when: discovering, inspecting, or creating organization projects and owning teams.
    #[command(verbatim_doc_comment)]
    Project(project::ProjectArgs),
    /// Manage AI provider connections and local office bindings
    /// Use when: configuring org-wide Claude/Codex/Agy connections on HQ or binding local runtimes.
    #[command(verbatim_doc_comment)]
    Provider(provider::ProviderArgs),
    /// Verify and pin a signed release bundle into this host's release store
    /// Use when: installing or upgrading prebuilt signed binaries without compiling from source.
    #[command(verbatim_doc_comment)]
    Release(release::ReleaseArgs),
    /// Back up an organization's durable data into one verifiable archive
    /// Use when: taking a consistent local backup of a stopped office's organization data, optionally passphrase-encrypted.
    #[command(verbatim_doc_comment)]
    Backup(backup::BackupArgs),
    /// Verify a backup archive, or restore it over the organization's stopped office
    /// Use when: proving a backup restores (`--verify`) or putting an organization's durable data back after a loss.
    #[command(verbatim_doc_comment)]
    Restore(backup::RestoreArgs),
    /// Start, stop or inspect the instance's registered service
    /// Use when: managing the OS background service created by `agentic-org setup`.
    #[command(verbatim_doc_comment)]
    Service(service::ServiceArgs),
    /// Protected organization recovery: status and release of the recovery package
    /// Use when: checking or releasing an organization's recovery package as its administrator.
    #[command(verbatim_doc_comment, disable_help_flag = true)]
    Recovery(PassArgs),
    /// Show this computer's instance, service, saved context and sign-in mode
    /// Use when: checking which instance and office commands act on before running them.
    #[command(verbatim_doc_comment)]
    Status(status::StatusArgs),
    /// Show recent secret-redacted host and office operational logs
    /// Use when: inspecting recent host or office stderr logs safely without leaking tokens or keys.
    #[command(verbatim_doc_comment)]
    Logs(diagnostics::LogsArgs),
    /// Run read-only host, runtime, security, and organization-state integrity diagnostics
    /// Use when: diagnosing host permissions, registry markers, sockets, AppArmor, or durable org state.
    #[command(verbatim_doc_comment)]
    Doctor(diagnostics::DoctorArgs),
    /// Build a deterministic, secret-redacted JSON diagnostic bundle
    /// Use when: collecting shareable troubleshooting diagnostics without exposing keys or databases.
    #[command(verbatim_doc_comment)]
    Support(diagnostics::SupportArgs),
    /// Cleanly uninstall an instance and clear host-registry.created
    /// Use when: removing a stopped instance's runtime, setup, and registry markers before a fresh setup.
    #[command(verbatim_doc_comment)]
    Uninstall(diagnostics::UninstallArgs),
    /// Print the shell completion script
    /// Use when: enabling <TAB> completion of commands, flags, instances and organizations.
    #[command(verbatim_doc_comment)]
    Completion(completion::CompletionArgs),
    /// Message people and agents across offices (msg send|inbox|read|reply|ask|...)
    /// Use when: sending DMs, channel messages, or replies to local or remote office agents.
    #[command(verbatim_doc_comment, disable_help_flag = true)]
    Msg(PassArgs),
    /// Manage organization messaging channels (channel list|join|leave|summarize)
    /// Use when: discovering or inspecting shared channels across offices.
    #[command(verbatim_doc_comment, disable_help_flag = true)]
    Channel(PassArgs),
    /// Hire and manage agents across HQ and satellite offices (hr hire [--bind-workspace <dir>]|list|inspect)
    /// Use when: hiring new agents onto HQ or a remote satellite office from the terminal (optionally binding a local workspace in one step).
    #[command(verbatim_doc_comment, disable_help_flag = true)]
    Hr(PassArgs),
    /// Inspect or manage hired agents (agent inspect|update|pause|offboard|bind|approve|renew|sessions|revoke|...)
    /// Use when: checking agent status, execution settings, lifecycle state, or external workspace bindings/sign-ins.
    #[command(verbatim_doc_comment, disable_help_flag = true)]
    Agent(PassArgs),
    /// Manage organization teams (team list|inspect|create|update)
    /// Use when: discovering or creating teams before hiring agents or creating projects.
    #[command(verbatim_doc_comment, disable_help_flag = true)]
    Team(PassArgs),
    /// Manage role catalog entries (role list|inspect|create|update)
    /// Use when: inspecting or creating reusable role definitions for hiring.
    #[command(verbatim_doc_comment, disable_help_flag = true)]
    Role(PassArgs),
    /// People of the organization (person ...)
    /// Use when: looking up or managing the people of the organization.
    #[command(verbatim_doc_comment, disable_help_flag = true)]
    Person(PassArgs),
    /// List people and agents with live presence and handles
    /// Use when: discovering handles and online status across HQ and satellite offices.
    #[command(verbatim_doc_comment, disable_help_flag = true)]
    Who(PassArgs),
    /// The organization directory (directory list|...)
    /// Use when: browsing the full directory of people, agents and offices.
    #[command(verbatim_doc_comment, disable_help_flag = true)]
    Directory(PassArgs),
    /// Organization membership (member ...)
    /// Use when: reviewing or changing who belongs to the organization.
    #[command(verbatim_doc_comment, disable_help_flag = true)]
    Member(PassArgs),
    /// Manage repositories, fetch origin branches and push task branches (repo register|list|attach|status|fetch|push)
    /// Use when: registering/attaching Git repos, checking office cache status, fetching newly pushed origin branches, or pushing branches.
    #[command(verbatim_doc_comment, disable_help_flag = true)]
    Repo(PassArgs),
    /// List or prepare isolated task worktrees (worktree list|prepare)
    /// Use when: listing or preparing isolated Git worktrees for task work.
    #[command(verbatim_doc_comment, disable_help_flag = true)]
    Worktree(PassArgs),
    /// Create, filter, update, assign and track tasks (task create|list|inspect|...)
    /// Use when: creating, filtering, updating, assigning, or tracking tasks over an office channel.
    #[command(verbatim_doc_comment, disable_help_flag = true)]
    Task(PassArgs),
    /// Manage task checklists (checklist add|check|edit|remove|move)
    /// Use when: adding, checking, editing, removing, or reordering task checklist steps.
    #[command(verbatim_doc_comment, disable_help_flag = true)]
    Checklist(PassArgs),
    /// Private task notes (task-note add|list|get|update|remove|publish; alias `task note`)
    /// Use when: recording private task notes or publishing a copy as a public task comment.
    #[command(verbatim_doc_comment, disable_help_flag = true, name = "task-note")]
    TaskNote(PassArgs),
    /// Agent and person sessions (session ...)
    /// Use when: inspecting or ending sessions on an office.
    #[command(verbatim_doc_comment, disable_help_flag = true)]
    Session(PassArgs),
    /// Access grants (grant ...)
    /// Use when: listing or managing the grants that let sessions act on an office.
    #[command(verbatim_doc_comment, disable_help_flag = true)]
    Grant(PassArgs),
    /// Manage organization, team, and project processes and standards (process list|inspect|create|update)
    /// Use when: reading the processes and standards that apply to a scope, their inheritance, or authoring one.
    #[command(verbatim_doc_comment, disable_help_flag = true)]
    Process(PassArgs),
    /// Agent skills (skill ...)
    /// Use when: listing or managing the skills agents can use.
    #[command(verbatim_doc_comment, disable_help_flag = true)]
    Skill(PassArgs),
    /// Spending budgets (budget ...)
    /// Use when: checking or setting what agents and teams may spend.
    #[command(verbatim_doc_comment, disable_help_flag = true)]
    Budget(PassArgs),
}

impl TopCommand {
    /// The office resource a command names and its words, when it runs over the signed-in
    /// office channel.
    pub(crate) fn office_resource(&self) -> Option<(&'static str, &[String])> {
        let (name, pass) = match self {
            Self::Msg(p) => ("msg", p),
            Self::Channel(p) => ("channel", p),
            Self::Hr(p) => ("hr", p),
            Self::Agent(p) => ("agent", p),
            Self::Team(p) => ("team", p),
            Self::Role(p) => ("role", p),
            Self::Person(p) => ("person", p),
            Self::Who(p) => ("who", p),
            Self::Directory(p) => ("directory", p),
            Self::Member(p) => ("member", p),
            Self::Repo(p) => ("repo", p),
            Self::Worktree(p) => ("worktree", p),
            Self::Task(p) => ("task", p),
            Self::Checklist(p) => ("checklist", p),
            Self::TaskNote(p) => ("task-note", p),
            Self::Session(p) => ("session", p),
            Self::Grant(p) => ("grant", p),
            Self::Process(p) => ("process", p),
            Self::Skill(p) => ("skill", p),
            Self::Budget(p) => ("budget", p),
            _ => return None,
        };
        Some((name, &pass.args))
    }

    /// Mutable counterpart of [`Self::office_resource`] for hoisting post-subcommand output flags
    /// (`--json`, `-q`, `--format`) before dispatching over the signed-in office channel.
    pub(crate) fn office_resource_mut(&mut self) -> Option<(&'static str, &mut Vec<String>)> {
        let (name, pass) = match self {
            Self::Msg(p) => ("msg", p),
            Self::Channel(p) => ("channel", p),
            Self::Hr(p) => ("hr", p),
            Self::Agent(p) => ("agent", p),
            Self::Team(p) => ("team", p),
            Self::Role(p) => ("role", p),
            Self::Person(p) => ("person", p),
            Self::Who(p) => ("who", p),
            Self::Directory(p) => ("directory", p),
            Self::Member(p) => ("member", p),
            Self::Repo(p) => ("repo", p),
            Self::Worktree(p) => ("worktree", p),
            Self::Task(p) => ("task", p),
            Self::Checklist(p) => ("checklist", p),
            Self::TaskNote(p) => ("task-note", p),
            Self::Session(p) => ("session", p),
            Self::Grant(p) => ("grant", p),
            Self::Process(p) => ("process", p),
            Self::Skill(p) => ("skill", p),
            Self::Budget(p) => ("budget", p),
            _ => return None,
        };
        Some((name, &mut pass.args))
    }

    /// `--yes` of a destructive command.
    fn yes(&self) -> bool {
        match self {
            Self::Office(args) => args.yes(),
            Self::Context(args) => args.yes(),
            Self::Admin(args) => args.yes(),
            Self::Service(args) => args.yes(),
            Self::Restore(args) => args.yes(),
            Self::Uninstall(args) => args.yes(),
            _ => false,
        }
    }

    /// Whether this run may read a secret on the terminal through the host (a password, a
    /// recovery code): the Ctrl+C handler is then left out (see `main`).
    pub(crate) fn reads_a_secret(&self) -> bool {
        match self {
            Self::Recovery(_) => true,
            Self::Admin(args) => args.reads_a_secret(),
            Self::Org(args) => args.reads_a_secret(),
            Self::Office(args) => args.reads_a_secret(),
            Self::Project(args) => args.reads_a_secret(),
            Self::Provider(args) => args.reads_a_secret(),
            Self::Backup(args) => args.reads_a_secret(),
            Self::Restore(args) => args.reads_a_secret(),
            _ => false,
        }
    }
}

impl Cli {
    /// The modes of this run.
    pub(crate) fn modes(&self) -> Modes {
        Modes {
            json: self.global.json,
            quiet: self.global.quiet,
            format: self.global.format.map(FormatArg::into_shell),
            dry_run: self.global.dry_run,
            no_input: self.global.no_input,
            yes: self.command.as_ref().is_some_and(TopCommand::yes),
            color: match self.global.color {
                ColorArg::Auto => ColorMode::Auto,
                ColorArg::Always => ColorMode::Always,
                ColorArg::Never => ColorMode::Never,
            },
            no_color: self.global.no_color,
            ignore_env_format: false,
        }
    }
}

/// The whole grammar, for parsing, help and completion.
pub(crate) fn command() -> clap::Command {
    Cli::command()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grammar_is_consistent() {
        command().debug_assert();
    }

    #[test]
    fn the_root_help_keeps_a_use_when_cue_for_every_command() {
        let help = command().render_long_help().to_string();
        assert!(help.matches("Use when:").count() >= 30, "{help}");
        assert!(help.contains("Examples:"), "{help}");
    }
}
