//! `agentic-org agent bind --member <id> --workspace <dir> [--takeover] [--ttl <dur>] [--json]`
//! (`TASK-211`): generates a fresh Ed25519 workspace keypair, authorizes an external lease via
//! `member.external_bind` under the operator's signed-in grant, and writes workspace credentials
//! strictly outside Git (`0700` dir, `0600` files) so subsequent `agent-os` commands in `<dir>`
//! authenticate immediately without an approval code.

use std::path::{Path, PathBuf};

use clap::Parser;
use clap_complete::engine::ArgValueCompleter;
use org_agent_cli::external_auth::bind::{
    parse_lease_ttl, PreparedWorkspaceBind, WorkspaceBindRequest,
};
use org_agent_cli::OfficeTransport;
use org_cli_shell::context::ExecutionContext;
use serde_json::json;

use super::common::{
    merge_inner_modes, merge_selector, open_channel, parse_verb_args, refusal_to_cli,
    validate_member, InnerModes,
};
use crate::args::{FormatArg, InstanceArgs};
use crate::completers;
use crate::error::CliError;
use crate::selection;
use crate::view::{self, column, Column, Show};

const ABOUT: &str = "Bind a local workspace directly to an agent without an approval code\n\
Use when: authorizing a local folder or Git repository to authenticate `agent-os` immediately \
as an organization agent in one operator command (no `agent-os login` approval code needed).";

const EXAMPLES: &str = "Examples:
  agentic-org agent bind --member @ada --workspace /path/to/repo
  agentic-org agent bind --member @ada --workspace . --takeover --ttl 8h
  agentic-org agent bind --member @ada --workspace /path/to/repo --json";

#[derive(Parser, Debug)]
#[command(
    name = "agentic-org agent bind",
    about = ABOUT,
    after_help = EXAMPLES,
    disable_version_flag = true,
    max_term_width = 100
)]
pub(crate) struct BindArgs {
    /// Agent @handle, handle, or employee UUIDv7 to bind to the workspace
    #[arg(long, value_name = "MEMBER")]
    pub(crate) member: String,
    /// Existing local directory or Git repository to bind
    #[arg(long, value_name = "DIR")]
    pub(crate) workspace: PathBuf,
    /// Park a live managed turn instead of refusing
    #[arg(long)]
    pub(crate) takeover: bool,
    /// Lease lifetime (e.g. 8h, 24h, 7d; default 24h)
    #[arg(long, value_name = "DURATION")]
    pub(crate) ttl: Option<String>,
    /// Short human label for the workspace (defaults to ~/…)
    #[arg(long, value_name = "TEXT")]
    pub(crate) label: Option<String>,
    /// The office to act on [default: AGENTIC_ORG_OFFICE, agentic-org.toml, saved context, or host default]
    #[arg(long, value_name = "OFFICE_ID", add = ArgValueCompleter::new(completers::offices))]
    pub(crate) office: Option<String>,
    #[command(flatten)]
    pub(crate) instance: InstanceArgs,
    /// Print the result as JSON on standard output, and errors as JSON on standard error
    #[arg(long)]
    pub(crate) json: bool,
    /// Print only the session ID
    #[arg(short, long, conflicts_with = "json")]
    pub(crate) quiet: bool,
    /// Output format (overrides AGENT_OS_FORMAT): json, compact, yaml, tsv or table
    #[arg(long, value_enum, value_name = "FORMAT", conflicts_with = "quiet")]
    pub(crate) format: Option<FormatArg>,
    /// Never prompt; fail instead when a value or confirmation is missing
    #[arg(long)]
    pub(crate) no_input: bool,
    /// Show what would change, and change nothing
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// When to color the output
    #[arg(long, value_name = "WHEN")]
    pub(crate) color: Option<String>,
    /// Same as --color never
    #[arg(long, hide = true)]
    pub(crate) no_color: bool,
}

const FIELDS: [Column; 10] = [
    column("session_id", "session_id", Show::Id),
    column("employee_id", "employee_id", Show::Id),
    column("handle", "handle", Show::Name),
    column("office_id", "office_id", Show::Dim),
    column("workspace_root", "workspace_root", Show::Plain),
    column("workspace_label", "workspace_label", Show::Plain),
    column("pubkey_fingerprint", "pubkey_fingerprint", Show::Dim),
    column("expires_at", "expires_at", Show::Dim),
    column("superseded_session_id", "superseded_session_id", Show::Dim),
    column("took_over_managed", "took_over_managed", Show::Plain),
];

fn parse_bind_args(
    raw_args: &[String],
    ctx: &mut ExecutionContext,
) -> Result<Option<BindArgs>, CliError> {
    let Some(args) = parse_verb_args::<BindArgs>("agentic-org agent bind", raw_args, ctx)? else {
        return Ok(None);
    };
    merge_inner_modes(
        &InnerModes {
            json: args.json,
            quiet: args.quiet,
            format: args.format.map(FormatArg::into_shell),
            dry_run: args.dry_run,
            no_input: args.no_input,
            no_color: args.no_color,
            color: args.color.as_deref(),
            help_cmd: "agentic-org agent bind --help",
        },
        ctx,
    )?;
    Ok(Some(args))
}

fn prepare_bind(
    args: &BindArgs,
    state_root: Option<&Path>,
) -> Result<PreparedWorkspaceBind, CliError> {
    let member = validate_member(&args.member)?;
    let ttl_secs = args
        .ttl
        .as_deref()
        .map(parse_lease_ttl)
        .transpose()
        .map_err(refusal_to_cli)?;
    PreparedWorkspaceBind::prepare(&WorkspaceBindRequest {
        member,
        workspace_dir: &args.workspace,
        label: args.label.as_deref(),
        ttl_secs,
        takeover: args.takeover,
        command_id: None,
        state_root,
    })
    .map_err(refusal_to_cli)
}

pub(crate) fn run(raw_args: &[String], ctx: &mut ExecutionContext) -> Result<(), CliError> {
    run_with_defaults(None, None, raw_args, ctx)
}

/// Runs `agentic-org sign-in [--name ...] [--office ...] -- agent bind ...`, combining any
/// outer sign-in selectors with inner `agent bind` flags and validating inputs before touching
/// the host.
pub(crate) fn run_after_sign_in(
    sign_in_instance: Option<&str>,
    sign_in_office: Option<&str>,
    raw_args: &[String],
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    run_with_defaults(sign_in_instance, sign_in_office, raw_args, ctx)
}

fn run_with_defaults(
    default_instance: Option<&str>,
    default_office: Option<&str>,
    raw_args: &[String],
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    let Some(args) = parse_bind_args(raw_args, ctx)? else {
        return Ok(());
    };
    let instance = merge_selector(
        "--instance",
        default_instance,
        args.instance.name.as_deref(),
    )?;
    let office = merge_selector("--office", default_office, args.office.as_deref())?;
    let target = selection::select(instance, office)?;
    let prepared = prepare_bind(&args, None)?;
    if ctx.modes().dry_run {
        let plan = json!({
            "action": "agent_bind",
            "member": prepared.member,
            "workspace_root": prepared.canonical_root.display().to_string(),
            "workspace_label": prepared.label,
            "ttl_secs": prepared.ttl_secs,
            "takeover": prepared.takeover,
            "office_id": target.office(),
        });
        let summary = format!(
            "would bind workspace {} to agent {}",
            prepared.canonical_root.display(),
            prepared.member
        );
        return Ok(ctx.render_dry_run(&summary, &plan)?);
    }
    let mut transport = open_channel(target.explicit_instance(), target.explicit_office())?;
    execute_prepared(prepared, &mut transport, ctx)
}

/// Runs `agent bind` against any [`OfficeTransport`] (real grant-bound channel or test transport).
#[cfg(test)]
pub(crate) fn execute_with_transport<T: OfficeTransport + ?Sized>(
    args: &BindArgs,
    transport: &mut T,
    state_root: Option<&Path>,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    let prepared = prepare_bind(args, state_root)?;
    execute_prepared(prepared, transport, ctx)
}

fn execute_prepared<T: OfficeTransport + ?Sized>(
    prepared: PreparedWorkspaceBind,
    transport: &mut T,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    let bound = prepared.execute(transport).map_err(refusal_to_cli)?;
    let json = view::json(&bound)?;
    view::detail(ctx, &json, &json, "session_id", &FIELDS)?;
    let who = bound
        .session
        .handle
        .as_deref()
        .map(|h| format!("@{h}"))
        .unwrap_or_else(|| bound.session.employee_id.clone());
    ctx.success(&format!(
        "Bound workspace {} to {} (session {}).",
        bound.workspace_root, who, bound.session.session_id
    ));
    Ok(())
}

#[cfg(test)]
#[path = "bind_tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) fn command() -> clap::Command {
    <BindArgs as clap::CommandFactory>::command()
}
