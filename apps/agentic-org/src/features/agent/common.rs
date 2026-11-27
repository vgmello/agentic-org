//! Shared helpers for operator-owned `agent` subcommands (`bind`, `renew`, `sessions`).

use std::io;

use clap::error::ErrorKind;
use clap::Parser;
use org_agent_cli::OfficeTransport;
use org_cli_shell::context::{ColorMode, ExecutionContext, OutputFormat};
use org_command::Refusal;
use org_host_setup::ApprovedOfficeChannel;
use serde_json::Value;

use crate::error::CliError;

/// Adapts an approved operator host channel into [`OfficeTransport`].
pub(super) struct ChannelTransport(pub(super) ApprovedOfficeChannel);

impl OfficeTransport for ChannelTransport {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        self.0
            .call_method(method, params)
            .map_err(org_agent_cli::api_refusal)
    }
}

/// Converts a transport [`Refusal`] into a structured [`CliError::Office`].
pub(super) fn refusal_to_cli(refusal: Refusal) -> CliError {
    CliError::Office {
        code: format!("office::{}", org_command::code_name(refusal.code)),
        message: refusal.message,
        hint: (!refusal.hint.is_empty()).then_some(refusal.hint),
        exit: 1,
    }
}

/// Flags common to operator subcommands that may be repeated after the verb.
pub(super) struct InnerModes<'a> {
    pub(super) json: bool,
    pub(super) quiet: bool,
    pub(super) format: Option<OutputFormat>,
    pub(super) dry_run: bool,
    pub(super) no_input: bool,
    pub(super) no_color: bool,
    pub(super) color: Option<&'a str>,
    pub(super) help_cmd: &'static str,
}

impl InnerModes<'_> {
    fn has_any(&self) -> bool {
        self.json
            || self.quiet
            || self.format.is_some()
            || self.dry_run
            || self.no_input
            || self.no_color
            || self.color.is_some()
    }
}

fn validate_merged_modes(modes: &org_cli_shell::context::Modes) -> Result<(), CliError> {
    if modes.quiet && modes.json {
        return Err(CliError::usage(
            "--quiet and --json cannot be used together",
        ));
    }
    if modes.quiet && modes.format.is_some() {
        return Err(CliError::usage(
            "--quiet and --format cannot be used together",
        ));
    }
    if modes.json && modes.format.is_some_and(|f| f != OutputFormat::Json) {
        return Err(CliError::usage(
            "--json cannot be combined with a non-json --format",
        ));
    }
    Ok(())
}

/// Merges flags passed after the verb into the active [`ExecutionContext`].
pub(super) fn merge_inner_modes(
    inner: &InnerModes<'_>,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    if !inner.has_any() {
        return Ok(());
    }
    let mut modes = ctx.modes().clone();
    modes.json |= inner.json;
    modes.quiet |= inner.quiet;
    if let Some(fmt) = inner.format {
        if modes.format.is_some_and(|existing| existing != fmt) {
            return Err(CliError::usage(
                "--format is given more than once with conflicting values",
            ));
        }
        modes.format = Some(fmt);
    }
    modes.dry_run |= inner.dry_run;
    modes.no_input |= inner.no_input;
    modes.no_color |= inner.no_color;
    if let Some(color) = inner.color {
        modes.color = ColorMode::from_flag(color).ok_or_else(|| {
            CliError::usage("--color is auto, always or never").with_help(inner.help_cmd)
        })?;
    }
    validate_merged_modes(&modes)?;
    if modes.json || modes.quiet || modes.format.is_some() {
        modes.ignore_env_format = true;
    }
    *ctx = ExecutionContext::detect(modes);
    Ok(())
}

/// Parses a verb's clap struct, rendering help cleanly and converting clap errors to usage exit 2.
pub(super) fn parse_verb_args<A: Parser>(
    program: &str,
    raw_args: &[String],
    ctx: &mut ExecutionContext,
) -> Result<Option<A>, CliError> {
    let argv = std::iter::once(program.to_owned()).chain(raw_args.iter().cloned());
    match A::try_parse_from(argv) {
        Ok(args) => Ok(Some(args)),
        Err(error) if error.kind() == ErrorKind::DisplayHelp => {
            let styled = error.render();
            let text = if ctx.stdout_color() {
                styled.ansi().to_string()
            } else {
                styled.to_string()
            };
            ctx.out(&text)?;
            Ok(None)
        }
        Err(error) => Err(clap_usage_error(error, raw_args, ctx)),
    }
}

fn clap_usage_error(
    error: clap::Error,
    raw_args: &[String],
    ctx: &mut ExecutionContext,
) -> CliError {
    if raw_args.iter().any(|arg| arg == "--json") && !ctx.modes().json {
        let mut modes = ctx.modes().clone();
        modes.json = true;
        *ctx = ExecutionContext::detect(modes);
    }
    let rendered = error.render().to_string();
    let mut lines = rendered.lines().filter(|line| !line.trim().is_empty());
    let message = lines
        .next()
        .unwrap_or("the command line could not be read")
        .trim_start_matches("error: ")
        .to_owned();
    let help = lines.collect::<Vec<_>>().join("\n");
    let usage = CliError::usage(message);
    if help.is_empty() {
        usage
    } else {
        usage.with_help(help)
    }
}

/// Validates a non-empty agent handle or UUIDv7 with no control characters.
pub(super) fn validate_member(raw: &str) -> Result<&str, CliError> {
    if raw.chars().any(char::is_control) {
        return Err(
            CliError::usage("Missing or invalid agent reference for --member.")
                .with_help("Pass an agent @handle or employee UUIDv7 to --member."),
        );
    }
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(
            CliError::usage("Missing or invalid agent reference for --member.")
                .with_help("Pass an agent @handle or employee UUIDv7 to --member."),
        );
    }
    Ok(trimmed)
}

/// Merges an outer selector (e.g. from `sign-in --office`) with an inner verb flag.
pub(super) fn merge_selector<'a>(
    flag: &str,
    outer: Option<&'a str>,
    inner: Option<&'a str>,
) -> Result<Option<&'a str>, CliError> {
    match (outer, inner) {
        (Some(a), Some(b)) if a != b => {
            Err(CliError::usage(format!("{flag} is given more than once")))
        }
        (outer, inner) => Ok(inner.or(outer)),
    }
}

/// Opens a grant-bound office channel for the selected instance/office.
pub(super) fn open_channel(
    instance: Option<&str>,
    office: Option<&str>,
) -> Result<ChannelTransport, CliError> {
    let channel = org_host_setup::open_for_command(instance, office, &mut io::stderr())?;
    Ok(ChannelTransport(channel))
}
