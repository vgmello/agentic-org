//! `agentic-org agent sessions [--member @<handle>] [--pending] [--limit <n>] [--json]`
//! (`TASK-248`): lists pending external workspace login requests across the office (or for one
//! agent) with masked `code_prefix` (`XXXX-••••`) and server-computed `workspace_familiarity`,
//! or shows an agent's active external lease and recent sign-in history.

use clap::Parser;
use clap_complete::engine::ArgValueCompleter;
use org_agent_cli::OfficeTransport;
use org_cli_shell::context::ExecutionContext;
use org_proto::external_auth::{
    MemberExternalPending, MemberExternalPendingParams, MemberExternalSessions,
    MemberExternalSessionsParams,
};
use org_proto::Call;

use super::common::{
    merge_inner_modes, merge_selector, open_channel, parse_verb_args, refusal_to_cli,
    validate_member, InnerModes,
};
use crate::args::InstanceArgs;
use crate::completers;
use crate::error::CliError;
use crate::selection;
use crate::view::{self, column, Column, Show};

const ABOUT: &str =
    "List pending external workspace sign-in requests or an agent's external sessions\n\
Use when: reviewing pending `agent-os login` requests (`--pending`, showing masked code prefix \
and workspace familiarity) or checking an agent's active external lease and sign-in history.";

const EXAMPLES: &str = "Examples:
  agentic-org agent sessions --pending
  agentic-org agent sessions --pending --member @ada --json
  agentic-org agent sessions --member @ada
  agentic-org agent sessions @ada --limit 50";

#[derive(Parser, Debug)]
#[command(
    name = "agentic-org agent sessions",
    about = ABOUT,
    after_help = EXAMPLES,
    disable_version_flag = true,
    max_term_width = 100
)]
pub(crate) struct SessionsArgs {
    /// Agent @handle, handle, or employee UUIDv7 (required unless --pending is given)
    #[arg(long, value_name = "MEMBER", conflicts_with = "member_pos")]
    pub(crate) member: Option<String>,
    /// Agent @handle, handle, or employee UUIDv7 (shorthand for --member)
    #[arg(value_name = "MEMBER", hide = true)]
    pub(crate) member_pos: Option<String>,
    /// List pending external workspace sign-in requests awaiting approval
    #[arg(long)]
    pub(crate) pending: bool,
    /// Most recent history entries to return (1 to 200, default 20)
    #[arg(long, alias = "history-limit", value_name = "N")]
    pub(crate) limit: Option<u32>,
    /// The office to act on [default: AGENTIC_ORG_OFFICE, agentic-org.toml, saved context, or host default]
    #[arg(long, value_name = "OFFICE_ID", add = ArgValueCompleter::new(completers::offices))]
    pub(crate) office: Option<String>,
    #[command(flatten)]
    pub(crate) instance: InstanceArgs,
    /// Print the result as JSON on standard output, and errors as JSON on standard error
    #[arg(long)]
    pub(crate) json: bool,
    /// Print only IDs, one per line
    #[arg(short, long, conflicts_with = "json")]
    pub(crate) quiet: bool,
    /// Never prompt; fail instead when a value or confirmation is missing
    #[arg(long)]
    pub(crate) no_input: bool,
    /// When to color the output
    #[arg(long, value_name = "WHEN")]
    pub(crate) color: Option<String>,
    /// Same as --color never
    #[arg(long, hide = true)]
    pub(crate) no_color: bool,
}

const PENDING_COLUMNS: [Column; 8] = [
    column("REQUEST_ID", "request_id", Show::Id),
    column("HANDLE", "handle", Show::Name),
    column("CODE", "code_prefix", Show::Plain),
    column("FAMILIARITY", "workspace_familiarity", Show::Status),
    column("WORKSPACE", "workspace_label", Show::Plain),
    column("HOST", "hostname", Show::Dim),
    column("MANAGED_TURN", "managed_turn_active", Show::Plain),
    column("EXPIRES_AT", "expires_at", Show::Dim),
];

const ACTIVE_FIELDS: [Column; 9] = [
    column("session_id", "active.session_id", Show::Id),
    column("employee_id", "employee_id", Show::Id),
    column("handle", "active.handle", Show::Name),
    column("workspace_label", "active.workspace_label", Show::Plain),
    column("pubkey_fingerprint", "active.pubkey_fingerprint", Show::Dim),
    column("approved_by", "active.approved_by", Show::Dim),
    column("approved_at", "active.approved_at", Show::Dim),
    column("expires_at", "active.expires_at", Show::Dim),
    column("active", "active.active", Show::Plain),
];

const HISTORY_COLUMNS: [Column; 5] = [
    column("AT", "at", Show::Dim),
    column("EVENT", "event", Show::Status),
    column("SESSION_ID", "session_id", Show::Id),
    column("ACTOR_ID", "actor_id", Show::Dim),
    column("DETAIL", "detail", Show::Plain),
];

enum Mode {
    Pending { member: Option<String> },
    MemberSessions { member: String, limit: Option<u32> },
}

fn parse_sessions_args(
    raw_args: &[String],
    ctx: &mut ExecutionContext,
) -> Result<Option<SessionsArgs>, CliError> {
    let Some(args) = parse_verb_args::<SessionsArgs>("agentic-org agent sessions", raw_args, ctx)?
    else {
        return Ok(None);
    };
    merge_inner_modes(
        &InnerModes {
            json: args.json,
            quiet: args.quiet,
            dry_run: false,
            no_input: args.no_input,
            no_color: args.no_color,
            color: args.color.as_deref(),
            help_cmd: "agentic-org agent sessions --help",
        },
        ctx,
    )?;
    if ctx.modes().dry_run {
        return Err(CliError::usage("`agent sessions` does not take --dry-run")
            .with_help("agentic-org agent sessions --help"));
    }
    Ok(Some(args))
}

fn prepare_mode(args: &SessionsArgs) -> Result<Mode, CliError> {
    let member = match args.member.as_deref().or(args.member_pos.as_deref()) {
        Some(raw) => Some(validate_member(raw)?.to_owned()),
        None => None,
    };
    if args.pending {
        if args.limit.is_some() {
            return Err(CliError::usage("--limit cannot be used with --pending")
                .with_help("Drop --limit when listing pending sign-in requests."));
        }
        return Ok(Mode::Pending { member });
    }
    let Some(member) = member else {
        return Err(CliError::usage(
            "`agent sessions` requires --member @<handle> unless --pending is given.",
        )
        .with_help(
            "Pass --member @<handle>, or pass --pending to list pending sign-in requests.",
        ));
    };
    if let Some(limit) = args.limit {
        if !(1..=200).contains(&limit) {
            return Err(
                CliError::usage("--limit must be between 1 and 200").with_help("Pass --limit 20.")
            );
        }
    }
    Ok(Mode::MemberSessions {
        member,
        limit: args.limit,
    })
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
    let Some(args) = parse_sessions_args(raw_args, ctx)? else {
        return Ok(());
    };
    let instance = merge_selector(
        "--instance",
        default_instance,
        args.instance.name.as_deref(),
    )?;
    let office = merge_selector("--office", default_office, args.office.as_deref())?;
    let target = selection::select(instance, office)?;
    let mode = prepare_mode(&args)?;
    let mut transport = open_channel(target.explicit_instance(), target.explicit_office())?;
    execute_mode(&mode, &mut transport, ctx)
}

#[cfg(test)]
pub(crate) fn execute_with_transport<T: OfficeTransport + ?Sized>(
    args: &SessionsArgs,
    transport: &mut T,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    let mode = prepare_mode(args)?;
    execute_mode(&mode, transport, ctx)
}

fn execute_mode<T: OfficeTransport + ?Sized>(
    mode: &Mode,
    transport: &mut T,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    match mode {
        Mode::Pending { member } => execute_pending(member.clone(), transport, ctx),
        Mode::MemberSessions { member, limit } => {
            execute_member_sessions(member.clone(), *limit, transport, ctx)
        }
    }
}

fn execute_pending<T: OfficeTransport + ?Sized>(
    employee_id: Option<String>,
    transport: &mut T,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    let params = MemberExternalPendingParams { employee_id };
    let raw_params =
        serde_json::to_value(params).map_err(|e| CliError::failed("cli::encode", e.to_string()))?;
    let raw = transport
        .call_method(MemberExternalPending::METHOD, raw_params)
        .map_err(refusal_to_cli)?;
    let rows = view::items(&raw, "pending");
    view::list(
        ctx,
        rows,
        &raw,
        "request_id",
        &PENDING_COLUMNS,
        "No pending external sign-in requests.",
    )
}

fn execute_member_sessions<T: OfficeTransport + ?Sized>(
    employee_id: String,
    limit: Option<u32>,
    transport: &mut T,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    let params = MemberExternalSessionsParams { employee_id, limit };
    let raw_params =
        serde_json::to_value(params).map_err(|e| CliError::failed("cli::encode", e.to_string()))?;
    let raw = transport
        .call_method(MemberExternalSessions::METHOD, raw_params)
        .map_err(refusal_to_cli)?;
    if ctx.modes().json {
        return view::detail(ctx, &raw, &raw, "active.session_id", &ACTIVE_FIELDS);
    }
    if ctx.modes().quiet {
        let id = view::text(view::lookup(&raw, "active.session_id"));
        if !id.is_empty() {
            ctx.out(&format!("{}\n", org_cli_shell::terminal::sanitize(&id)))?;
        }
        return Ok(());
    }
    if view::lookup(&raw, "active").is_some_and(|v| !v.is_null()) {
        view::detail(ctx, &raw, &raw, "active.session_id", &ACTIVE_FIELDS)?;
    } else {
        ctx.note("No active external session.");
    }
    let history = view::items(&raw, "history");
    if !history.is_empty() {
        view::list(
            ctx,
            history,
            &raw,
            "session_id",
            &HISTORY_COLUMNS,
            "No external session history.",
        )?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "sessions_tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) fn command() -> clap::Command {
    <SessionsArgs as clap::CommandFactory>::command()
}
