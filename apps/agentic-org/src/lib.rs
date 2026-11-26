//! `agentic-org`, the operator command line: the composition root of the vertical slices in
//! `features/` (one module per noun, one file per verb), over the shell library
//! `org-cli-shell` (output modes, sanitizing, diagnostics, prompts, configuration and the
//! completion cache) and the host library `org-host`. No agent CLI or office data ownership.
//!
//! Module graph (an arrow points at what a module uses; no slice uses another slice):
//! `lib -> cli, features, error`; `cli -> args, features::*` (their grammars);
//! `features::* -> args, completers, error, host, selection, view`;
//! `args -> completers -> selection -> host`; `completers -> host`; `selection -> error`;
//! `host -> view -> error`.

#![forbid(unsafe_code)]

mod args;
mod cli;
mod completers;
mod error;
mod features;
mod host;
mod selection;
#[cfg(test)]
mod tests;
mod view;

use std::ffi::OsString;
use std::io::{self, Write};

use clap::error::ErrorKind;
use clap::FromArgMatches;
use org_cli_shell::context::{prescan, resolve_color, ExecutionContext, Modes, Prescan};
use org_cli_shell::error::{render_diagnostic, EXIT_FAILURE};
use org_cli_shell::terminal::{sanitize, Terminal};

use cli::TopCommand;
use error::CliError;

/// The variable a shell's completion script sets to ask for candidates.
const COMPLETE_VAR: &str = "COMPLETE";

/// Runs `agentic-org` with `args` (the program's name first) and answers the exit code. A
/// shell's completion request (`COMPLETE=<shell> agentic-org -- <words>`) is answered first,
/// before any configuration is read and without contacting a host.
pub fn main_entry(args: Vec<OsString>) -> u8 {
    match std::env::var_os(COMPLETE_VAR).filter(|shell| !shell.is_empty() && shell != "0") {
        Some(shell) => complete(&shell, args),
        None => run(args),
    }
}

/// Answers a completion request: the registration script without words, the candidates for
/// the last word otherwise (clap's dynamic completion engine, over the cache the completers
/// read). Written fallibly: a closed standard output is not a failure.
fn complete(shell: &std::ffi::OsStr, mut args: Vec<OsString>) -> u8 {
    let shells = clap_complete::env::Shells::builtins();
    let name = std::path::Path::new(shell)
        .file_stem()
        .unwrap_or(shell)
        .to_string_lossy()
        .into_owned();
    let Some(completer) = shells.completer(&name) else {
        let _ = writeln!(
            io::stderr(),
            "agentic-org: completion: unknown shell `{}`",
            sanitize(&name)
        );
        return EXIT_FAILURE;
    };
    let words = args
        .iter()
        .position(|arg| arg == "--")
        .map_or(args.len(), |at| at + 1);
    let mut buf = Vec::new();
    let written = if words >= args.len() {
        completer.write_registration(
            COMPLETE_VAR,
            "agentic-org",
            "agentic-org",
            "agentic-org",
            &mut buf,
        )
    } else {
        let current = std::env::current_dir().ok();
        let mut command = cli::command();
        command.build();
        completer.write_complete(
            &mut command,
            args.split_off(words),
            current.as_deref(),
            &mut buf,
        )
    };
    if let Err(error) = written {
        let _ = writeln!(
            io::stderr(),
            "agentic-org: completion: {}",
            sanitize(&error.to_string())
        );
        return EXIT_FAILURE;
    }
    match org_cli_shell::terminal::write_out(&mut io::stdout(), &buf) {
        Ok(()) => 0,
        Err(error) => org_cli_shell::error::CliDiagnostic::exit_code(&error),
    }
}

/// `agentic-org help [<command>...]` is `agentic-org [<command>...] --help`.
fn help_routing(mut args: Vec<OsString>) -> Vec<OsString> {
    if args.get(1).is_some_and(|word| word == "help") {
        args.remove(1);
        args.push("--help".into());
    }
    args
}

/// Runs a command line that is not a completion request.
pub fn run(args: Vec<OsString>) -> u8 {
    let args = help_routing(args);
    let pre = prescan(&args);
    let mut command = cli::command();
    let parsed = command
        .try_get_matches_from_mut(args)
        .and_then(|matches| cli::Cli::from_arg_matches(&matches));
    let cli = match parsed {
        Ok(cli) => cli,
        Err(error) => return clap_answer(&error, &pre),
    };
    let mut ctx = ExecutionContext::detect(cli.modes());
    org_cli_shell::prompt::sync_colors(&ctx);
    let Some(top) = cli.command else {
        // `agentic-org` alone: its help, colored as standard output is (`--color always`).
        let help = cli::command().render_help();
        let text = if ctx.stdout_color() {
            help.ansi().to_string()
        } else {
            help.to_string()
        };
        return write_or_closed(&mut ctx, &text);
    };
    // Ctrl+C restores the cursor and exits 130. Not for `serve`, which blocks its stop
    // signals itself, nor while the host may read a password or a recovery code: that
    // prompt blocks the signals on its own thread to restore the terminal first, and a
    // handler thread would end the process with echo off.
    if !matches!(top, TopCommand::Serve(_)) && !top.reads_a_secret() {
        org_cli_shell::terminal::install_interrupt_handler();
    }
    match dispatch(top, &mut ctx) {
        Ok(()) => 0,
        Err(error) => ctx.report(&error),
    }
}

fn write_or_closed(ctx: &mut ExecutionContext, text: &str) -> u8 {
    match ctx.out(text) {
        Ok(()) => 0,
        Err(error) => ctx.report(&CliError::from(error)),
    }
}

/// Help, the version, or a command line clap refused: help and the version on standard
/// output; a refusal as a JSON envelope on standard error under `--json`, or clap's own
/// message (sanitized) otherwise; exit 2.
fn clap_answer(error: &clap::Error, pre: &Prescan) -> u8 {
    let modes = Modes {
        json: pre.json,
        quiet: pre.quiet,
        color: pre.color.unwrap_or_default(),
        no_color: pre.no_color,
        ..Modes::default()
    };
    let terminal = Terminal::detect();
    let env = |name: &str| std::env::var_os(name);
    if matches!(
        error.kind(),
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
    ) {
        let styled = error.render();
        let text = if resolve_color(&modes, terminal.stdout_tty, &env) {
            styled.ansi().to_string()
        } else {
            styled.to_string()
        };
        let mut ctx = ExecutionContext::detect(modes);
        return write_or_closed(&mut ctx, &text);
    }
    let rendered = error.render().to_string();
    let mut err = io::stderr();
    if pre.json {
        let mut lines = rendered.lines().filter(|line| !line.trim().is_empty());
        let (message, help) = if error.kind() == ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        {
            (
                "a subcommand is required".to_owned(),
                rendered.trim_end().to_owned(),
            )
        } else {
            let message = lines
                .next()
                .unwrap_or("the command line could not be read")
                .trim_start_matches("error: ")
                .to_owned();
            (message, lines.collect::<Vec<_>>().join("\n"))
        };
        let usage = CliError::usage(message).with_help(help);
        render_diagnostic(&usage, true, terminal.stderr_tty, false, &mut err);
    } else {
        let text: Vec<String> = rendered
            .lines()
            .map(|line| sanitize(line).into_owned())
            .collect();
        let _ = writeln!(err, "{}", text.join("\n").trim_end());
    }
    u8::try_from(error.exit_code()).unwrap_or(EXIT_FAILURE)
}

/// `serve`, `setup` and `recovery` report in prose and have no preview; `sign-in` signs in
/// and saves its context: a global flag they cannot honour is refused, never ignored.
fn refuse_unsupported_modes(top: &TopCommand, ctx: &ExecutionContext) -> Result<(), CliError> {
    let (name, refused): (&str, &[(&str, bool)]) = match top {
        TopCommand::Serve(_) => (
            "serve",
            &[
                ("--json", ctx.modes().json),
                ("--quiet", ctx.modes().quiet),
                ("--dry-run", ctx.modes().dry_run),
            ],
        ),
        TopCommand::Setup(_) => (
            "setup",
            &[
                ("--json", ctx.modes().json),
                ("--quiet", ctx.modes().quiet),
                ("--dry-run", ctx.modes().dry_run),
            ],
        ),
        TopCommand::Recovery(_) => (
            "recovery",
            &[
                ("--json", ctx.modes().json),
                ("--quiet", ctx.modes().quiet),
                ("--dry-run", ctx.modes().dry_run),
            ],
        ),
        TopCommand::SignIn(_) => ("sign-in", &[("--dry-run", ctx.modes().dry_run)]),
        _ => return Ok(()),
    };
    match refused.iter().find(|(_, given)| *given) {
        Some((flag, _)) => Err(CliError::usage(format!("`{name}` does not take {flag}"))
            .with_help(format!("agentic-org {name} --help"))),
        None => Ok(()),
    }
}

fn dispatch(top: TopCommand, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    use features::*;
    refuse_unsupported_modes(&top, ctx)?;
    if let Some((resource, words)) = top.office_resource() {
        let words = std::iter::once(resource.to_owned())
            .chain(words.iter().cloned())
            .collect();
        return sign_in::office_proxy::run(words, ctx);
    }
    match top {
        TopCommand::Serve(pass) => serve::run(pass.args, ctx),
        TopCommand::Setup(pass) => setup::run(&pass.args, ctx),
        TopCommand::SignIn(args) => sign_in::run(args, ctx),
        TopCommand::Context(args) => context::run(args, ctx),
        TopCommand::Admin(args) => admin::run(args, ctx),
        TopCommand::Org(args) => match args.office_words() {
            Some(words) => sign_in::office_proxy::run(words, ctx),
            None => org::run(args, ctx),
        },
        TopCommand::Office(args) => office::run(args, ctx),
        TopCommand::Project(args) => project::run(args, ctx),
        TopCommand::Provider(args) => provider::run(args, ctx),
        TopCommand::Release(args) => release::run(args, ctx),
        TopCommand::Service(args) => service::run(args, ctx),
        TopCommand::Recovery(pass) => recovery::run(pass.args, ctx),
        TopCommand::Status(args) => status::run(args, ctx),
        TopCommand::Completion(args) => completion::run(args, cli::command(), ctx),
        _ => Err(CliError::usage("this command runs over the office channel")),
    }
}
