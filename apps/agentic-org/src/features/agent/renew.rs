//! `agentic-org agent renew --member @<handle> [--ttl <duration>] [--session <id>] [--json]`
//! (`TASK-248`): extends an agent's active external workspace lease by up to 7 days (default `7d`)
//! without re-login, strictly under the operator's signed-in human grant (`member.external_renew`).

use clap::Parser;
use clap_complete::engine::ArgValueCompleter;
use org_agent_cli::external_auth::bind::parse_lease_ttl;
use org_agent_cli::OfficeTransport;
use org_cli_shell::context::ExecutionContext;
use org_proto::external_auth::{
    ExternalSessionView, MemberExternalRenew, MemberExternalRenewParams, MAX_LEASE_TTL_SECS,
};
use org_proto::Call;
use serde_json::json;

use super::common::{
    merge_inner_modes, merge_selector, open_channel, parse_verb_args, refusal_to_cli,
    validate_member, InnerModes,
};
use crate::args::InstanceArgs;
use crate::completers;
use crate::error::CliError;
use crate::selection;
use crate::view::{self, column, Column, Show};

const ABOUT: &str = "Extend an agent's active external workspace lease without re-login\n\
Use when: renewing an active external workspace lease (up to 7 days, default 7d) before it \
expires so the workspace keeps authenticating without a new approval code.";

const EXAMPLES: &str = "Examples:
  agentic-org agent renew --member @ada --ttl 7d
  agentic-org agent renew @ada --ttl 24h
  agentic-org agent renew --member @ada --json";

#[derive(Parser, Debug)]
#[command(
    name = "agentic-org agent renew",
    about = ABOUT,
    after_help = EXAMPLES,
    disable_version_flag = true,
    max_term_width = 100
)]
pub(crate) struct RenewArgs {
    /// Agent @handle, handle, or employee UUIDv7 whose active external lease to renew
    #[arg(long, value_name = "MEMBER", conflicts_with = "member_pos")]
    pub(crate) member: Option<String>,
    /// Agent @handle, handle, or employee UUIDv7 (shorthand for --member)
    #[arg(value_name = "MEMBER", hide = true)]
    pub(crate) member_pos: Option<String>,
    /// New lease lifetime from now (e.g. 8h, 24h, 7d; default 7d, max 7d)
    #[arg(long, value_name = "DURATION")]
    pub(crate) ttl: Option<String>,
    /// Only renew if the active external session matches this session ID
    #[arg(long, value_name = "SESSION_ID")]
    pub(crate) session: Option<String>,
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

const FIELDS: [Column; 9] = [
    column("session_id", "session_id", Show::Id),
    column("employee_id", "employee_id", Show::Id),
    column("handle", "handle", Show::Name),
    column("workspace_label", "workspace_label", Show::Plain),
    column("pubkey_fingerprint", "pubkey_fingerprint", Show::Dim),
    column("approved_by", "approved_by", Show::Dim),
    column("approved_at", "approved_at", Show::Dim),
    column("expires_at", "expires_at", Show::Dim),
    column("active", "active", Show::Plain),
];

fn parse_renew_args(
    raw_args: &[String],
    ctx: &mut ExecutionContext,
) -> Result<Option<RenewArgs>, CliError> {
    let Some(args) = parse_verb_args::<RenewArgs>("agentic-org agent renew", raw_args, ctx)? else {
        return Ok(None);
    };
    merge_inner_modes(
        &InnerModes {
            json: args.json,
            quiet: args.quiet,
            dry_run: args.dry_run,
            no_input: args.no_input,
            no_color: args.no_color,
            color: args.color.as_deref(),
            help_cmd: "agentic-org agent renew --help",
        },
        ctx,
    )?;
    Ok(Some(args))
}

struct PreparedRenew {
    member: String,
    ttl_secs: u64,
    session_id: Option<String>,
}

fn prepare_renew(args: &RenewArgs) -> Result<PreparedRenew, CliError> {
    let raw_member = args
        .member
        .as_deref()
        .or(args.member_pos.as_deref())
        .ok_or_else(|| {
            CliError::usage("`agent renew` requires --member @<handle>.")
                .with_help("Run `agentic-org agent renew --member @<handle> [--ttl 7d]`.")
        })?;
    let member = validate_member(raw_member)?.to_owned();
    let ttl_secs = match args.ttl.as_deref() {
        Some(raw) => parse_lease_ttl(raw).map_err(refusal_to_cli)?,
        None => MAX_LEASE_TTL_SECS,
    };
    let session_id = match args.session.as_deref() {
        Some(raw) => Some(validate_session_id(raw)?),
        None => None,
    };
    Ok(PreparedRenew {
        member,
        ttl_secs,
        session_id,
    })
}

fn validate_session_id(raw: &str) -> Result<String, CliError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || raw.chars().any(char::is_control) {
        return Err(CliError::usage("Invalid --session ID.")
            .with_help("Pass the active external session ID (e.g. ext-...)."));
    }
    Ok(trimmed.to_owned())
}

pub(crate) fn run(raw_args: &[String], ctx: &mut ExecutionContext) -> Result<(), CliError> {
    run_with_defaults(None, None, raw_args, ctx)
}

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
    let Some(args) = parse_renew_args(raw_args, ctx)? else {
        return Ok(());
    };
    let instance = merge_selector(
        "--instance",
        default_instance,
        args.instance.name.as_deref(),
    )?;
    let office = merge_selector("--office", default_office, args.office.as_deref())?;
    let target = selection::select(instance, office)?;
    let prepared = prepare_renew(&args)?;
    if ctx.modes().dry_run {
        let plan = json!({
            "action": "agent_renew",
            "member": prepared.member,
            "ttl_secs": prepared.ttl_secs,
            "session_id": prepared.session_id,
            "office_id": target.office(),
        });
        let summary = format!(
            "would renew active external lease for {} by {}s",
            prepared.member, prepared.ttl_secs
        );
        return Ok(ctx.render_dry_run(&summary, &plan)?);
    }
    let mut transport = open_channel(target.explicit_instance(), target.explicit_office())?;
    execute_prepared(&prepared, &mut transport, ctx)
}

#[cfg(test)]
pub(crate) fn execute_with_transport<T: OfficeTransport + ?Sized>(
    args: &RenewArgs,
    transport: &mut T,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    let prepared = prepare_renew(args)?;
    execute_prepared(&prepared, transport, ctx)
}

fn execute_prepared<T: OfficeTransport + ?Sized>(
    prepared: &PreparedRenew,
    transport: &mut T,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    let params = MemberExternalRenewParams {
        employee_id: prepared.member.clone(),
        ttl_secs: Some(prepared.ttl_secs),
        session_id: prepared.session_id.clone(),
        command_id: None,
    };
    let raw_params =
        serde_json::to_value(params).map_err(|e| CliError::failed("cli::encode", e.to_string()))?;
    let raw = transport
        .call_method(MemberExternalRenew::METHOD, raw_params)
        .map_err(refusal_to_cli)?;
    let renewed: ExternalSessionView = serde_json::from_value(raw.clone())
        .map_err(|e| CliError::failed("cli::decode", e.to_string()))?;
    view::detail(ctx, &raw, &raw, "session_id", &FIELDS)?;
    let who = renewed
        .handle
        .as_deref()
        .map(|h| format!("@{h}"))
        .unwrap_or_else(|| renewed.employee_id.clone());
    ctx.success(&format!(
        "Renewed external lease {} for {} until {}.",
        renewed.session_id, who, renewed.expires_at
    ));
    Ok(())
}

#[cfg(test)]
#[path = "renew_tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) fn command() -> clap::Command {
    <RenewArgs as clap::CommandFactory>::command()
}
