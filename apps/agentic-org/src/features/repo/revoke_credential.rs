//! `agentic-org repo revoke-credential --repo <id-or-key> [--office <id>] [--json]`
//! (`TASK-135`): deletes an office's stored encrypted per-repo HTTPS token.

use clap::Parser;
use clap_complete::engine::ArgValueCompleter;
use org_agent_cli::OfficeTransport;
use org_cli_shell::context::ExecutionContext;
use org_proto::git::{RepoCredentialRevoke, RepoCredentialRevokeParams};
use org_proto::Call;
use serde_json::json;

use super::common::{
    decode_and_render_view, merge_inner_modes, merge_selector, open_channel, parse_verb_args,
    refusal_to_cli, validate_repo_ref, InnerModes,
};
use crate::args::{FormatArg, InstanceArgs};
use crate::completers;
use crate::error::CliError;
use crate::selection;

const ABOUT: &str = "Revoke an office's stored per-repository HTTPS token\n\
Use when: rotating or removing a scoped per-repository HTTPS token from an HQ or satellite \
office credential vault.";

const EXAMPLES: &str = "Examples:
  agentic-org repo revoke-credential --repo app
  agentic-org repo revoke-credential --repo app --office <office_id> --json";

#[derive(Parser, Debug)]
#[command(
    name = "agentic-org repo revoke-credential",
    about = ABOUT,
    after_help = EXAMPLES,
    disable_version_flag = true,
    max_term_width = 100
)]
pub(crate) struct RevokeCredentialArgs {
    /// Repository ID or key whose stored token to revoke
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

fn parse_revoke_args(
    raw_args: &[String],
    ctx: &mut ExecutionContext,
) -> Result<Option<RevokeCredentialArgs>, CliError> {
    let Some(args) = parse_verb_args::<RevokeCredentialArgs>(
        "agentic-org repo revoke-credential",
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
            help_cmd: "agentic-org repo revoke-credential --help",
        },
        ctx,
    )?;
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
    let Some(args) = parse_revoke_args(raw_args, ctx)? else {
        return Ok(());
    };
    let raw_repo = args
        .repo
        .as_deref()
        .or(args.repo_pos.as_deref())
        .ok_or_else(|| {
            CliError::usage("`repo revoke-credential` requires --repo <id-or-key>.")
                .with_help("Run `agentic-org repo revoke-credential --repo <id-or-key>`.")
        })?;
    let repository_id = validate_repo_ref(raw_repo)?;
    let instance = merge_selector(
        "--instance",
        default_instance,
        args.instance.name.as_deref(),
    )?;
    let office = merge_selector("--office", default_office, args.office.as_deref())?;
    let target = selection::select(instance, office)?;
    if ctx.modes().dry_run {
        let plan = json!({
            "action": "repo_credential_revoke",
            "repository_id": repository_id,
            "office_id": target.office(),
        });
        let summary = format!("would revoke stored HTTPS token for repository {repository_id}");
        return Ok(ctx.render_dry_run(&summary, &plan)?);
    }
    let mut transport = open_channel(target.explicit_instance(), target.explicit_office())?;
    execute_revoke(&repository_id, &mut transport, ctx)
}

pub(super) fn execute_revoke<T: OfficeTransport + ?Sized>(
    repository: &str,
    transport: &mut T,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    let params = RepoCredentialRevokeParams {
        repository: repository.to_owned(),
    };
    let raw_params =
        serde_json::to_value(params).map_err(|e| CliError::failed("cli::encode", e.to_string()))?;
    let raw = transport
        .call_method(RepoCredentialRevoke::METHOD, raw_params)
        .map_err(refusal_to_cli)?;
    let view = decode_and_render_view(raw, ctx)?;
    let label = view
        .repository_key
        .as_deref()
        .unwrap_or(view.repository_id.as_str());
    ctx.success(&format!(
        "Revoked stored vault HTTPS credential for repository `{label}`.",
    ));
    Ok(())
}
