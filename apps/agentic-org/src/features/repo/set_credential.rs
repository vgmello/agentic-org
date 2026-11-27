//! `agentic-org repo set-credential --repo <id-or-key> [--username <user>] [--token-file <path> | --token-stdin]`
//! (`TASK-135`): stores an encrypted per-repo HTTPS token in the target office credential vault.
//! Tokens are read only from a regular `--token-file` (no symlinks), `--token-stdin`, or the
//! controlling terminal (echo off) — never from command-line arguments or environment variables.

use std::io::{self, Read};
use std::path::{Path, PathBuf};

use clap::Parser;
use clap_complete::engine::ArgValueCompleter;
use org_agent_cli::OfficeTransport;
use org_cli_shell::context::ExecutionContext;
use org_cli_shell::error::ShellError;
use org_host::human::SecretEntryError;
use org_proto::git::{RepoCredentialSet, RepoCredentialSetParams};
use org_proto::Call;
use serde_json::json;
use zeroize::Zeroizing;

use super::common::{
    decode_and_render_view, merge_inner_modes, merge_selector, open_channel, parse_verb_args,
    refusal_to_cli, validate_repo_ref, validate_username, InnerModes,
};
use crate::args::{FormatArg, InstanceArgs};
use crate::completers;
use crate::error::CliError;
use crate::selection;

const MAX_TOKEN_BYTES: u64 = 4096;

const ABOUT: &str = "Store a scoped per-repository HTTPS token in an office's encrypted vault\n\
Use when: authorizing an HQ or satellite office to clone, fetch, or push a private HTTPS \
repository without relying on personal host `gh` or `~/.git-credentials`.";

const EXAMPLES: &str = "Examples:
  printf '%s' \"$REPO_TOKEN\" | agentic-org repo set-credential --repo app --token-stdin
  agentic-org repo set-credential --repo app --token-file ./token.txt --office <office_id>
  agentic-org repo set-credential --repo app --username x-access-token --json";

#[derive(Parser, Debug)]
#[command(
    name = "agentic-org repo set-credential",
    about = ABOUT,
    after_help = EXAMPLES,
    disable_version_flag = true,
    max_term_width = 100
)]
pub(crate) struct SetCredentialArgs {
    /// Repository ID or key to configure
    #[arg(long, value_name = "REPO", conflicts_with = "repo_pos")]
    pub(crate) repo: Option<String>,
    /// Repository ID or key (shorthand for --repo)
    #[arg(value_name = "REPO", hide = true)]
    pub(crate) repo_pos: Option<String>,
    /// HTTPS Basic-auth username (defaults to x-access-token)
    #[arg(long, value_name = "USERNAME")]
    pub(crate) username: Option<String>,
    /// Read the HTTPS token from a regular file (never a symlink)
    #[arg(long, value_name = "PATH", conflicts_with = "token_stdin")]
    pub(crate) token_file: Option<PathBuf>,
    /// Read the HTTPS token from standard input
    #[arg(long, conflicts_with = "token_file")]
    pub(crate) token_stdin: bool,
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

fn parse_set_args(
    raw_args: &[String],
    ctx: &mut ExecutionContext,
) -> Result<Option<SetCredentialArgs>, CliError> {
    let Some(args) =
        parse_verb_args::<SetCredentialArgs>("agentic-org repo set-credential", raw_args, ctx)?
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
            help_cmd: "agentic-org repo set-credential --help",
        },
        ctx,
    )?;
    Ok(Some(args))
}

pub(super) fn read_token_from_reader<R: Read>(
    reader: R,
    source_label: &str,
) -> Result<Zeroizing<String>, CliError> {
    let mut bytes = Zeroizing::new(Vec::new());
    reader
        .take(MAX_TOKEN_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| {
            CliError::usage(format!(
                "Could not read repository token from {source_label}."
            ))
        })?;
    if bytes.len() as u64 > MAX_TOKEN_BYTES {
        return Err(CliError::usage(format!(
            "Repository token from {source_label} exceeds {MAX_TOKEN_BYTES} bytes."
        )));
    }
    let text = Zeroizing::new(String::from_utf8(std::mem::take(&mut *bytes)).map_err(|_| {
        CliError::usage(format!(
            "Repository token from {source_label} is not valid UTF-8."
        ))
    })?);
    normalize_token(&text)
}

fn read_token_from_file(path: &Path) -> Result<Zeroizing<String>, CliError> {
    let file = org_platform::fs::open_nofollow_ro(path).map_err(|_| {
        CliError::usage(
            "Could not open --token-file as a readable regular file (symlinks are refused).",
        )
        .with_help(
            "Pass a regular file path to --token-file, or pipe the token with --token-stdin.",
        )
    })?;
    read_token_from_reader(file, "--token-file")
}

fn read_token_from_terminal() -> Result<Zeroizing<String>, CliError> {
    let secret =
        org_host::human::terminal_secret("Repository HTTPS token: ").map_err(
            |error| match error {
                SecretEntryError::Cancelled => CliError::from(ShellError::Cancelled),
                SecretEntryError::Unavailable(why) => {
                    CliError::usage(format!("Repository token could not be read: {why}"))
                        .with_help("Pass --token-stdin or --token-file <path>.")
                }
            },
        )?;
    normalize_token(&secret)
}

fn normalize_token(raw: &str) -> Result<Zeroizing<String>, CliError> {
    let trimmed = raw.trim_end_matches(['\r', '\n']);
    if trimmed.is_empty()
        || trimmed.len() as u64 > MAX_TOKEN_BYTES
        || trimmed.chars().any(char::is_control)
    {
        return Err(CliError::usage(
            "Repository HTTPS token must be non-empty and contain no control characters.",
        ));
    }
    Ok(Zeroizing::new(trimmed.to_owned()))
}

pub(super) fn read_token(
    args: &SetCredentialArgs,
    ctx: &ExecutionContext,
) -> Result<Zeroizing<String>, CliError> {
    if let Some(path) = args.token_file.as_deref() {
        return read_token_from_file(path);
    }
    if args.token_stdin {
        return read_token_from_reader(io::stdin(), "--token-stdin");
    }
    if !ctx.can_prompt() {
        let why = if ctx.modes().no_input {
            "--no-input is on"
        } else {
            "no terminal prompt is available (--json, --quiet, or non-TTY)"
        };
        return Err(
            CliError::usage(format!("A repository HTTPS token is required and {why}."))
                .with_help("Pass --token-stdin or --token-file <path>."),
        );
    }
    read_token_from_terminal()
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
    let Some(args) = parse_set_args(raw_args, ctx)? else {
        return Ok(());
    };
    let raw_repo = args
        .repo
        .as_deref()
        .or(args.repo_pos.as_deref())
        .ok_or_else(|| {
            CliError::usage("`repo set-credential` requires --repo <id-or-key>.").with_help(
                "Run `agentic-org repo set-credential --repo <id-or-key> --token-stdin`.",
            )
        })?;
    let repository_id = validate_repo_ref(raw_repo)?;
    let username = args
        .username
        .as_deref()
        .map(validate_username)
        .transpose()?;
    let instance = merge_selector(
        "--instance",
        default_instance,
        args.instance.name.as_deref(),
    )?;
    let office = merge_selector("--office", default_office, args.office.as_deref())?;
    let target = selection::select(instance, office)?;
    if ctx.modes().dry_run {
        let plan = json!({
            "action": "repo_credential_set",
            "repository_id": repository_id,
            "username": username.as_deref().unwrap_or("x-access-token"),
            "office_id": target.office(),
        });
        let summary = format!("would store encrypted HTTPS token for repository {repository_id}");
        return Ok(ctx.render_dry_run(&summary, &plan)?);
    }
    let token = read_token(&args, ctx)?;
    let mut transport = open_channel(target.explicit_instance(), target.explicit_office())?;
    execute_set(&repository_id, username, token, &mut transport, ctx)
}

pub(super) fn execute_set<T: OfficeTransport + ?Sized>(
    repository: &str,
    username: Option<String>,
    token: Zeroizing<String>,
    transport: &mut T,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    let params = RepoCredentialSetParams {
        repository: repository.to_owned(),
        username,
        token: token.as_str().to_owned(),
    };
    let raw_params = serde_json::to_value(&params)
        .map_err(|_| CliError::failed("cli::encode", "could not encode credential request"))?;
    drop(params);
    let raw = transport
        .call_method(RepoCredentialSet::METHOD, raw_params)
        .map_err(refusal_to_cli)?;
    let view = decode_and_render_view(raw, ctx)?;
    let scope = view.scope_url.as_deref().unwrap_or("repository remote");
    let label = view
        .repository_key
        .as_deref()
        .unwrap_or(view.repository_id.as_str());
    ctx.success(&format!(
        "Stored encrypted vault HTTPS credential for repository `{label}` (scope {scope}).",
    ));
    Ok(())
}
