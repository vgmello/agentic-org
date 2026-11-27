//! Operator-owned `agentic-org repo github <configure|status|revoke|bind|unbind>` subcommands
//! (`TASK-272`).
//!
//! Private key PEMs and webhook secrets are read strictly from regular files (`O_NOFOLLOW`, never
//! symlinks), `--private-key-stdin`, or an echo-off terminal prompt — never from CLI flags.

use std::io::{self, Read};
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use clap_complete::engine::ArgValueCompleter;
use org_agent_cli::OfficeTransport;
use org_cli_shell::context::ExecutionContext;
use org_cli_shell::error::ShellError;
use org_host::human::SecretEntryError;
use org_proto::github::{
    GitHubAppConfigure, GitHubAppConfigureParams, GitHubAppRevoke, GitHubAppStatus, GitHubRepoBind,
    GitHubRepoBindParams, GitHubRepoUnbind, GitHubRepoUnbindParams, SecretText,
};
use org_proto::{Call, Empty};
use serde_json::Value;
use zeroize::Zeroizing;

use super::common::{
    merge_inner_modes, merge_selector, open_channel, parse_verb_args, refusal_to_cli,
    validate_repo_ref, InnerModes,
};
use crate::args::{FormatArg, InstanceArgs};
use crate::completers;
use crate::error::CliError;
use crate::selection;
use crate::view::{self, column, Column, Show};

const MAX_PEM_BYTES: u64 = 4096;
const MAX_WEBHOOK_SECRET_BYTES: u64 = 2048;

const ABOUT: &str = "Manage an office's wrapped GitHub App integration and repository bindings\n\
Use when: configuring a GitHub App private key and webhook secret in the office vault, checking \
non-secret status, binding org repositories to GitHub owner/repo slugs, or revoking App credentials.";

const EXAMPLES: &str = "Examples:
  agentic-org repo github configure --app-id 12345 --installation-id 67890 --private-key-file ./app.pem --webhook-secret-file ./webhook.secret
  agentic-org repo github status --json
  agentic-org repo github bind --repo backend --owner acme --name backend
  agentic-org repo github unbind --repo backend
  agentic-org repo github revoke";

const STATUS_FIELDS: [Column; 7] = [
    column("office_id", "office_id", Show::Id),
    column("configured", "configured", Show::Status),
    column("app_id", "app_id", Show::Plain),
    column("installation_id", "installation_id", Show::Plain),
    column("api_base_url", "api_base_url", Show::Plain),
    column("webhook_configured", "webhook_configured", Show::Status),
    column("updated_at", "updated_at", Show::Dim),
];

const BINDING_FIELDS: [Column; 5] = [
    column("repository_id", "repository_id", Show::Id),
    column("repository_key", "repository_key", Show::Name),
    column("full_name", "full_name", Show::Plain),
    column("installation_id", "installation_id", Show::Plain),
    column("bound_at", "bound_at", Show::Dim),
];

const UNBIND_FIELDS: [Column; 2] = [
    column("repository_id", "repository_id", Show::Id),
    column("unbound", "unbound", Show::Status),
];

#[derive(Parser, Debug)]
#[command(
    name = "agentic-org repo github",
    about = ABOUT,
    after_help = EXAMPLES,
    disable_version_flag = true,
    max_term_width = 100
)]
struct GitHubArgs {
    #[command(subcommand)]
    action: GitHubAction,
    /// The office to act on [default: AGENTIC_ORG_OFFICE, agentic-org.toml, saved context, or host default]
    #[arg(long, global = true, value_name = "OFFICE_ID", add = ArgValueCompleter::new(completers::offices))]
    office: Option<String>,
    #[command(flatten)]
    instance: InstanceArgs,
    /// Print the result as JSON on standard output, and errors as JSON on standard error
    #[arg(long, global = true)]
    json: bool,
    /// Print only the office or repository ID
    #[arg(short, long, global = true, conflicts_with = "json")]
    quiet: bool,
    /// Output format (overrides AGENT_OS_FORMAT): json, compact, yaml, tsv or table
    #[arg(
        long,
        global = true,
        value_enum,
        value_name = "FORMAT",
        conflicts_with = "quiet"
    )]
    format: Option<FormatArg>,
    /// Never prompt; fail instead when a value or confirmation is missing
    #[arg(long, global = true)]
    no_input: bool,
    /// Show what would change, and change nothing
    #[arg(long, global = true)]
    dry_run: bool,
    /// When to color the output
    #[arg(long, global = true, value_name = "WHEN")]
    color: Option<String>,
    /// Same as --color never
    #[arg(long, global = true, hide = true)]
    no_color: bool,
}

#[derive(Subcommand, Debug)]
enum GitHubAction {
    /// Store wrapped GitHub App RSA private key and optional webhook secret in the office vault
    Configure {
        /// Numeric GitHub App ID
        #[arg(long, value_name = "APP_ID")]
        app_id: u64,
        /// Default numeric GitHub App installation ID
        #[arg(long, value_name = "INSTALLATION_ID")]
        installation_id: u64,
        /// Optional GitHub Enterprise HTTPS REST API base URL (default: https://api.github.com)
        #[arg(long, value_name = "URL")]
        api_base_url: Option<String>,
        /// Read the GitHub App RSA private key PEM from a regular file (never a symlink)
        #[arg(long, value_name = "PATH", conflicts_with = "private_key_stdin")]
        private_key_file: Option<PathBuf>,
        /// Read the GitHub App RSA private key PEM from standard input
        #[arg(long, conflicts_with = "private_key_file")]
        private_key_stdin: bool,
        /// Read the GitHub webhook HMAC-SHA256 secret from a regular file (never a symlink)
        #[arg(long, value_name = "PATH", conflicts_with = "webhook_secret_prompt")]
        webhook_secret_file: Option<PathBuf>,
        /// Prompt for the GitHub webhook HMAC-SHA256 secret on the controlling terminal
        #[arg(long, conflicts_with = "webhook_secret_file")]
        webhook_secret_prompt: bool,
        /// Non-secret account or organization hint label
        #[arg(long, value_name = "LABEL")]
        account_hint: Option<String>,
    },
    /// Show non-secret GitHub App configuration status and repository bindings
    Status,
    /// Revoke wrapped GitHub App credentials and purge cached installation tokens
    Revoke,
    /// Bind a registered organization repository to a GitHub owner/repo slug
    Bind {
        /// Registered repository ID or key
        #[arg(long, value_name = "REPO")]
        repo: String,
        /// GitHub owner/organization login
        #[arg(long, value_name = "OWNER")]
        owner: String,
        /// GitHub repository name
        #[arg(long, value_name = "NAME")]
        name: String,
        /// Optional override GitHub App installation ID for this repository
        #[arg(long, value_name = "INSTALLATION_ID")]
        installation_id: Option<u64>,
    },
    /// Remove a repository's GitHub owner/repo binding
    Unbind {
        /// Registered repository ID or key
        #[arg(long, value_name = "REPO")]
        repo: String,
    },
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
    let Some(args) = parse_verb_args::<GitHubArgs>("agentic-org repo github", raw_args, ctx)?
    else {
        return Ok(());
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
            help_cmd: "agentic-org repo github --help",
        },
        ctx,
    )?;

    let instance = merge_selector(
        "--instance",
        default_instance,
        args.instance.name.as_deref(),
    )?;
    let office = merge_selector("--office", default_office, args.office.as_deref())?;
    let target = selection::select(instance, office)?;
    let mut transport = open_channel(target.explicit_instance(), target.explicit_office())?;

    match args.action {
        GitHubAction::Configure {
            app_id,
            installation_id,
            api_base_url,
            private_key_file,
            private_key_stdin,
            webhook_secret_file,
            webhook_secret_prompt,
            account_hint,
        } => {
            let pem = read_private_key(private_key_file.as_deref(), private_key_stdin)?;
            let webhook_secret =
                read_webhook_secret(webhook_secret_file.as_deref(), webhook_secret_prompt, ctx)?;
            let params = GitHubAppConfigureParams {
                app_id,
                installation_id,
                api_base_url,
                private_key_pem: SecretText::new(pem.as_str()),
                webhook_secret: webhook_secret.map(|s| SecretText::new(s.as_str())),
                account_hint,
            };
            let raw = call_rpc::<GitHubAppConfigure>(&mut transport, &params)?;
            view::detail(ctx, &raw, &raw, "office_id", &STATUS_FIELDS)?;
        }
        GitHubAction::Status => {
            let raw = call_rpc::<GitHubAppStatus>(&mut transport, &Empty {})?;
            view::detail(ctx, &raw, &raw, "office_id", &STATUS_FIELDS)?;
        }
        GitHubAction::Revoke => {
            let raw = call_rpc::<GitHubAppRevoke>(&mut transport, &Empty {})?;
            view::detail(ctx, &raw, &raw, "office_id", &STATUS_FIELDS)?;
        }
        GitHubAction::Bind {
            repo,
            owner,
            name,
            installation_id,
        } => {
            let repository = validate_repo_ref(&repo)?;
            let params = GitHubRepoBindParams {
                repository,
                owner,
                repo: name,
                installation_id,
            };
            let raw = call_rpc::<GitHubRepoBind>(&mut transport, &params)?;
            view::detail(ctx, &raw, &raw, "repository_id", &BINDING_FIELDS)?;
        }
        GitHubAction::Unbind { repo } => {
            let repository = validate_repo_ref(&repo)?;
            let params = GitHubRepoUnbindParams { repository };
            let raw = call_rpc::<GitHubRepoUnbind>(&mut transport, &params)?;
            view::detail(ctx, &raw, &raw, "repository_id", &UNBIND_FIELDS)?;
        }
    }
    Ok(())
}

fn call_rpc<C: Call>(
    transport: &mut impl OfficeTransport,
    params: &C::Params,
) -> Result<Value, CliError> {
    let raw_params =
        serde_json::to_value(params).map_err(|e| CliError::failed("cli::encode", e.to_string()))?;
    transport
        .call_method(C::METHOD, raw_params)
        .map_err(refusal_to_cli)
}

fn read_private_key(file: Option<&Path>, from_stdin: bool) -> Result<Zeroizing<String>, CliError> {
    if let Some(path) = file {
        let fh = org_platform::fs::open_nofollow_ro(path).map_err(|_| {
            CliError::usage(
                "Could not open --private-key-file as a readable regular file (symlinks are refused).",
            )
        })?;
        return read_bounded_utf8(fh, MAX_PEM_BYTES, "--private-key-file");
    }
    if from_stdin {
        return read_bounded_utf8(io::stdin(), MAX_PEM_BYTES, "--private-key-stdin");
    }
    Err(CliError::usage(
        "`repo github configure` requires --private-key-file or --private-key-stdin.",
    )
    .with_help("Pass a PEM private key file path or pipe the PEM via --private-key-stdin."))
}

fn read_webhook_secret(
    file: Option<&Path>,
    prompt: bool,
    ctx: &ExecutionContext,
) -> Result<Option<Zeroizing<String>>, CliError> {
    if let Some(path) = file {
        let fh = org_platform::fs::open_nofollow_ro(path).map_err(|_| {
            CliError::usage(
                "Could not open --webhook-secret-file as a readable regular file (symlinks are refused).",
            )
        })?;
        return read_bounded_utf8(fh, MAX_WEBHOOK_SECRET_BYTES, "--webhook-secret-file").map(Some);
    }
    if prompt {
        if ctx.modes().no_input {
            return Err(CliError::usage(
                "--webhook-secret-prompt cannot be used with --no-input; pass --webhook-secret-file instead.",
            ));
        }
        let secret = org_host::human::terminal_secret("GitHub webhook secret: ").map_err(
            |error| match error {
                SecretEntryError::Cancelled => CliError::from(ShellError::Cancelled),
                SecretEntryError::Unavailable(why) => CliError::usage(why),
            },
        )?;
        return Ok(Some(Zeroizing::new(secret.as_str().trim().to_owned())));
    }
    Ok(None)
}

fn read_bounded_utf8<R: Read>(
    reader: R,
    max_bytes: u64,
    label: &str,
) -> Result<Zeroizing<String>, CliError> {
    let mut bytes = Zeroizing::new(Vec::new());
    reader
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| CliError::usage(format!("Could not read secret input from {label}.")))?;
    if bytes.len() as u64 > max_bytes {
        return Err(CliError::usage(format!(
            "Input from {label} exceeds {max_bytes} bytes."
        )));
    }
    let text = Zeroizing::new(
        String::from_utf8(std::mem::take(&mut *bytes))
            .map_err(|_| CliError::usage(format!("Input from {label} is not valid UTF-8.")))?,
    );
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(CliError::usage(format!("Input from {label} is empty.")));
    }
    Ok(Zeroizing::new(trimmed.to_owned()))
}
