//! `agentic-org repo credential-status --repo <id-or-key> [--office <id>] [--json]`
//! (`TASK-135`): inspects non-secret repository HTTPS credential status on the target office.

use clap::Parser;
use clap_complete::engine::ArgValueCompleter;
use org_agent_cli::OfficeTransport;
use org_cli_shell::context::ExecutionContext;
use org_proto::git::{RepoCredentialStatus, RepoCredentialStatusParams};
use org_proto::Call;

use super::common::{
    decode_and_render_view, merge_inner_modes, merge_selector, open_channel, parse_verb_args,
    refusal_to_cli, validate_repo_ref, InnerModes,
};
use crate::args::{FormatArg, InstanceArgs};
use crate::completers;
use crate::error::CliError;
use crate::selection;

const ABOUT: &str = "Show whether an office has a scoped HTTPS token configured for a repository\n\
Use when: checking if an HQ or satellite office vault holds an encrypted per-repo HTTPS token \
(without ever revealing secret material).";

const EXAMPLES: &str = "Examples:
  agentic-org repo credential-status --repo app
  agentic-org repo credential-status --repo app --office <office_id> --json";

#[derive(Parser, Debug)]
#[command(
    name = "agentic-org repo credential-status",
    about = ABOUT,
    after_help = EXAMPLES,
    disable_version_flag = true,
    max_term_width = 100
)]
pub(crate) struct CredentialStatusArgs {
    /// Repository ID or key to inspect
    #[arg(long, value_name = "REPO", conflicts_with = "repo_pos")]
    pub(crate) repo: Option<String>,
    /// Repository ID or key (shorthand for --repo)
    #[arg(value_name = "REPO", hide = true)]
    pub(crate) repo_pos: Option<String>,
    /// The office to act on [default: AGENTIC_ORG_OFFICE, agentic-org.toml, saved context, or host default]
    #[arg(long, value_name = "OFFICE_ID", add = ArgValueCompleter::new(completers::offices))]
    pub(crate) office: Option<String>,
    #[command(flatten)]
    pub(crate) instance: InstanceArgs,
    /// Print the result as JSON on standard output, and errors as JSON on standard error
    #[arg(long)]
    pub(crate) json: bool,
    /// Print only the repository ID
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

fn parse_status_args(
    raw_args: &[String],
    ctx: &mut ExecutionContext,
) -> Result<Option<CredentialStatusArgs>, CliError> {
    let Some(args) = parse_verb_args::<CredentialStatusArgs>(
        "agentic-org repo credential-status",
        raw_args,
        ctx,
    )?
    else {
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
            help_cmd: "agentic-org repo credential-status --help",
        },
        ctx,
    )?;
    if ctx.modes().dry_run {
        return Err(
            CliError::usage("`repo credential-status` does not take --dry-run")
                .with_help("agentic-org repo credential-status --help"),
        );
    }
    Ok(Some(args))
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
    let Some(args) = parse_status_args(raw_args, ctx)? else {
        return Ok(());
    };
    let raw_repo = args
        .repo
        .as_deref()
        .or(args.repo_pos.as_deref())
        .ok_or_else(|| {
            CliError::usage("`repo credential-status` requires --repo <id-or-key>.")
                .with_help("Run `agentic-org repo credential-status --repo <id-or-key>`.")
        })?;
    let repository_id = validate_repo_ref(raw_repo)?;
    let instance = merge_selector(
        "--instance",
        default_instance,
        args.instance.name.as_deref(),
    )?;
    let office = merge_selector("--office", default_office, args.office.as_deref())?;
    let target = selection::select(instance, office)?;
    let mut transport = open_channel(target.explicit_instance(), target.explicit_office())?;
    execute_status(&repository_id, &mut transport, ctx)
}

pub(super) fn execute_status<T: OfficeTransport + ?Sized>(
    repository: &str,
    transport: &mut T,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    let params = RepoCredentialStatusParams {
        repository: repository.to_owned(),
    };
    let raw_params =
        serde_json::to_value(params).map_err(|e| CliError::failed("cli::encode", e.to_string()))?;
    let raw = transport
        .call_method(RepoCredentialStatus::METHOD, raw_params)
        .map_err(refusal_to_cli)?;
    let _ = decode_and_render_view(raw, ctx)?;
    Ok(())
}
