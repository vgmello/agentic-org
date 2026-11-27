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
use org_cli_shell::context::{
    parse_env_format, prescan, resolve_color, resolve_output_format, ExecutionContext, Modes,
    OutputFormat, Prescan, OUTPUT_FORMAT_ENV,
};
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
    let explicit = ExplicitOutput {
        json: cli.global.json,
        quiet: cli.global.quiet,
        format: cli.global.format.map(args::FormatArg::into_shell),
        table_format: false,
        table_format_kind: None,
        dry_run: cli.global.dry_run,
    };
    let mut modes = cli.modes();
    let Some(top) = cli.command else {
        // `agentic-org` alone: its help, colored as standard output is (`--color always`).
        modes.ignore_env_format = true;
        let mut ctx = ExecutionContext::detect(modes);
        org_cli_shell::prompt::sync_colors(&ctx);
        let help = cli::command().render_help();
        let text = if ctx.stdout_color() {
            help.ansi().to_string()
        } else {
            help.to_string()
        };
        return write_or_closed(&mut ctx, &text);
    };
    let (top, explicit, passthrough_meta) = match prepare_modes(top, explicit, &mut modes) {
        Ok(prepared) => prepared,
        Err(error) => {
            let mut ctx = ExecutionContext::detect(modes);
            return ctx.report(&error);
        }
    };
    if !passthrough_meta && !explicit.has_output_override() {
        if let Err(bad) = parse_env_format(&|name| std::env::var_os(name)) {
            let mut ctx = ExecutionContext::detect(modes);
            let error = CliError::usage(format!(
                "{OUTPUT_FORMAT_ENV} must be json, compact, yaml, tsv or table, not `{}`",
                sanitize(&bad)
            ))
            .with_help("Unset AGENT_OS_FORMAT or override with --format, --json, or -q.");
            return ctx.report(&error);
        }
    }
    let mut ctx = ExecutionContext::detect(modes);
    org_cli_shell::prompt::sync_colors(&ctx);
    // Ctrl+C restores the cursor and exits 130. Not for `serve`, which blocks its stop
    // signals itself, nor while the host may read a password or a recovery code: that
    // prompt blocks the signals on its own thread to restore the terminal first, and a
    // handler thread would end the process with echo off.
    if !matches!(top, TopCommand::Serve(_)) && !top.reads_a_secret() {
        org_cli_shell::terminal::install_interrupt_handler();
    }
    match dispatch(top, &explicit, passthrough_meta, &mut ctx) {
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
/// output; a refusal as a JSON envelope on standard error under `--json` (or `AGENT_OS_FORMAT=json`
/// / agent-harness PTY), or clap's own message (sanitized) otherwise; exit 2.
fn clap_answer(error: &clap::Error, pre: &Prescan) -> u8 {
    let modes = Modes {
        json: pre.json,
        quiet: pre.quiet,
        format: pre.format,
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
        let mut help_modes = modes;
        help_modes.ignore_env_format = true;
        let styled = error.render();
        let text = if resolve_color(&help_modes, terminal.stdout_tty, &env) {
            styled.ansi().to_string()
        } else {
            styled.to_string()
        };
        let mut ctx = ExecutionContext::detect(help_modes);
        return write_or_closed(&mut ctx, &text);
    }
    let rendered = error.render().to_string();
    let mut err = io::stderr();
    let json_errors = pre.json
        || (!pre.quiet && resolve_output_format(&modes, &terminal, &env) == OutputFormat::Json);
    if json_errors {
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

/// Output/preview flags passed explicitly on the CLI (before or after the subcommand), kept
/// distinct from `AGENT_OS_FORMAT` / agent-harness PTY defaults so `serve`/`setup`/`recovery`
/// only refuse explicit CLI flags and explicit flags cleanly beat `AGENT_OS_FORMAT`.
#[derive(Clone, Copy, Debug, Default)]
struct ExplicitOutput {
    json: bool,
    quiet: bool,
    format: Option<OutputFormat>,
    /// True when a command carried `--format <...>` inside its trailing words.
    table_format: bool,
    /// Parsed format of `table_format` when it named a known format (`json`, `yaml`, `compact`).
    table_format_kind: Option<OutputFormat>,
    dry_run: bool,
}

impl ExplicitOutput {
    fn has_format_flag(self) -> bool {
        self.format.is_some() || self.table_format
    }

    fn has_output_override(self) -> bool {
        self.json || self.quiet || self.has_format_flag()
    }

    fn has_non_json_format(self) -> bool {
        self.format.is_some_and(|f| f != OutputFormat::Json)
            || (self.table_format && self.table_format_kind != Some(OutputFormat::Json))
    }
}

/// `serve`, `setup` and `recovery` report in prose and have no preview; `sign-in` signs in
/// and saves its context: an explicit global flag they cannot honour is refused, never ignored.
fn refuse_unsupported_modes(top: &TopCommand, explicit: &ExplicitOutput) -> Result<(), CliError> {
    let (name, refused): (&str, &[(&str, bool)]) = match top {
        TopCommand::Serve(_) => (
            "serve",
            &[
                ("--json", explicit.json),
                ("--quiet", explicit.quiet),
                ("--format", explicit.format.is_some()),
                ("--dry-run", explicit.dry_run),
            ],
        ),
        TopCommand::Setup(_) => (
            "setup",
            &[
                ("--json", explicit.json),
                ("--quiet", explicit.quiet),
                ("--format", explicit.format.is_some()),
                ("--dry-run", explicit.dry_run),
            ],
        ),
        TopCommand::Recovery(_) => (
            "recovery",
            &[
                ("--json", explicit.json),
                ("--quiet", explicit.quiet),
                ("--format", explicit.format.is_some()),
                ("--dry-run", explicit.dry_run),
            ],
        ),
        TopCommand::SignIn(_) => ("sign-in", &[("--dry-run", explicit.dry_run)]),
        _ => return Ok(()),
    };
    match refused.iter().find(|(_, given)| *given) {
        Some((flag, _)) => Err(CliError::usage(format!("`{name}` does not take {flag}"))
            .with_help(format!("agentic-org {name} --help"))),
        None => Ok(()),
    }
}

/// Whether `TopCommand::Agent(pass)` names one of the operator-local clap verbs
/// (`agent bind|renew|sessions`) rather than an office command table verb.
fn is_local_agent_verb(args: &[String]) -> bool {
    matches!(
        args.first().map(String::as_str),
        Some("bind" | "renew" | "sessions")
    )
}

fn resource_and_verb(words: &[String]) -> Option<(&str, &str)> {
    match words {
        [a, b, v, ..] if a == "task" && b == "note" => Some(("task-note", v.as_str())),
        [a, ..] if a == "who" => Some(("directory", "list")),
        [r, v, ..] => Some((r.as_str(), v.as_str())),
        _ => None,
    }
}

fn command_path_words(words: &[String]) -> usize {
    match words {
        [a, ..] if a == "who" => 1,
        [a, b, ..] if a == "task" && b == "note" => 3,
        _ => 2,
    }
}

fn office_flag_takes_value(words: &[String], flag_name: &str) -> bool {
    if matches!(
        flag_name,
        "format"
            | "fields"
            | "limit"
            | "cursor"
            | "input-json"
            | "input-file"
            | "input-format"
            | "idempotency-key"
            | "if-match"
            | "instance"
            | "office"
            | "color"
            | "name"
    ) {
        return true;
    }
    let Some((resource, verb)) = resource_and_verb(words) else {
        return false;
    };
    if resource == "hr" && verb == "hire" && flag_name == "bind-workspace" {
        return true;
    }
    org_agent_cli::runtime_table()
        .find(resource, verb)
        .is_some_and(|cmd| cmd.flag_takes_value(flag_name) == Some(true))
}

/// Whether `sign-in -- <command...>` targets one of the operator-local clap verbs
/// (`agent bind|renew|sessions`).
fn is_sign_in_local_agent(command: &[String]) -> bool {
    command.first().map(String::as_str) == Some("agent") && is_local_agent_verb(&command[1..])
}

#[derive(Default)]
struct HoistedFlags {
    json: bool,
    quiet: bool,
    format: Option<OutputFormat>,
    table_format: bool,
    table_format_kind: Option<OutputFormat>,
    passthrough_meta: bool,
}

/// Handles simple hoisted switches (`--json`, `-q`/`--quiet`, `--help`/`-h`/`--schema`/`--version`).
fn hoist_switch(
    arg: &str,
    out: &mut Vec<String>,
    hoisted: &mut HoistedFlags,
) -> Result<bool, CliError> {
    if arg.starts_with("--json=") {
        return Err(CliError::usage(
            "--json is a yes-or-no flag for JSON output and takes no value; pass a JSON object with --input-json <object>",
        ));
    }
    match arg {
        "--json" => hoisted.json = true,
        "-q" | "--quiet" => hoisted.quiet = true,
        "--help" | "-h" | "--schema" | "--version" => {
            hoisted.passthrough_meta = true;
            out.push(arg.to_owned());
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// Records a `--format` value: `table`/`tsv` are stripped into `hoisted.format` (rejecting
/// conflicting repeated shell-only values), while `org-command` formats (`json`, `yaml`,
/// `compact`, or unknown) stay in `out` for table validation.
fn hoist_format_value(
    val: &str,
    arg: &str,
    out: &mut Vec<String>,
    hoisted: &mut HoistedFlags,
) -> Result<bool, CliError> {
    match OutputFormat::parse(val) {
        Some(fmt @ (OutputFormat::Table | OutputFormat::Tsv)) => {
            check_format_agreement(hoisted.format, Some(fmt))?;
            hoisted.format = Some(fmt);
            Ok(true)
        }
        parsed => {
            hoisted.table_format = true;
            hoisted.table_format_kind = parsed;
            out.push(arg.to_owned());
            Ok(false)
        }
    }
}

/// Handles split `--format <value>`, returning `true` when the next token must be forwarded verbatim.
fn hoist_split_format(
    words: &[String],
    idx: &mut usize,
    arg: &str,
    out: &mut Vec<String>,
    hoisted: &mut HoistedFlags,
) -> Result<bool, CliError> {
    let Some(next) = words.get(*idx) else {
        hoisted.table_format = true;
        out.push(arg.to_owned());
        return Ok(false);
    };
    if hoist_format_value(next, arg, out, hoisted)? {
        *idx += 1;
        return Ok(false);
    }
    Ok(true)
}

/// Classifies one non-value word before `--`, returning `true` if the next word is its value.
fn hoist_single_word(
    words: &[String],
    idx: &mut usize,
    arg: &str,
    out: &mut Vec<String>,
    hoisted: &mut HoistedFlags,
) -> Result<bool, CliError> {
    if hoist_switch(arg, out, hoisted)? {
        return Ok(false);
    }
    if arg == "--format" {
        return hoist_split_format(words, idx, arg, out, hoisted);
    }
    if let Some(val) = arg.strip_prefix("--format=") {
        let _ = hoist_format_value(val, arg, out, hoisted)?;
        return Ok(false);
    }
    let awaiting = arg
        .strip_prefix("--")
        .is_some_and(|flag| !flag.contains('=') && office_flag_takes_value(words, flag));
    out.push(arg.to_owned());
    Ok(awaiting)
}

/// Scans `words` (`<resource> [<verb>] ...`) respecting `--` and value-taking flags, stripping
/// bare `--json`, `-q`/`--quiet`, and shell-only `--format table|tsv` into [`HoistedFlags`].
pub(crate) fn hoist_office_words(
    words: &[String],
) -> Result<(Vec<String>, HoistedFlags), CliError> {
    let mut out = Vec::with_capacity(words.len());
    let mut hoisted = HoistedFlags::default();
    let mut idx = 0;
    let mut after_terminator = false;
    let mut awaiting_value = false;
    while let Some(arg) = words.get(idx) {
        idx += 1;
        if awaiting_value || after_terminator {
            awaiting_value = false;
            out.push(arg.clone());
            continue;
        }
        if arg == "--" {
            after_terminator = true;
            out.push(arg.clone());
            continue;
        }
        awaiting_value = hoist_single_word(words, &mut idx, arg, &mut out, &mut hoisted)?;
    }
    Ok((out, hoisted))
}

fn validate_output_conflict(explicit: &ExplicitOutput) -> Result<(), CliError> {
    if explicit.json && explicit.quiet {
        return Err(CliError::usage(
            "--quiet and --json cannot be used together",
        ));
    }
    if explicit.quiet && explicit.has_format_flag() {
        return Err(CliError::usage(
            "--quiet and --format cannot be used together",
        ));
    }
    if explicit.json && explicit.has_non_json_format() {
        return Err(CliError::usage(
            "--json cannot be combined with a non-json --format",
        ));
    }
    Ok(())
}

fn check_format_agreement(
    first: Option<OutputFormat>,
    second: Option<OutputFormat>,
) -> Result<(), CliError> {
    if let (Some(a), Some(b)) = (first, second) {
        if a != b {
            return Err(CliError::usage(
                "--format is given more than once with conflicting values",
            ));
        }
    }
    Ok(())
}

fn merge_hoisted(
    hoisted: HoistedFlags,
    explicit: &mut ExplicitOutput,
    modes: &mut Modes,
) -> Result<bool, CliError> {
    if hoisted.passthrough_meta {
        modes.ignore_env_format = true;
        return Ok(true);
    }
    check_format_agreement(explicit.format, hoisted.format)?;
    check_format_agreement(explicit.format, hoisted.table_format_kind)?;
    check_format_agreement(hoisted.format, hoisted.table_format_kind)?;
    explicit.json |= hoisted.json;
    explicit.quiet |= hoisted.quiet;
    explicit.table_format |= hoisted.table_format;
    if hoisted.table_format_kind.is_some() {
        explicit.table_format_kind = hoisted.table_format_kind;
    }
    if let Some(fmt) = hoisted.format {
        explicit.format = Some(fmt);
    }
    validate_output_conflict(explicit)?;
    modes.json = explicit.json;
    modes.quiet = explicit.quiet;
    modes.format = explicit.format.or(explicit.table_format_kind);
    modes.ignore_env_format = explicit.has_output_override();
    Ok(false)
}

fn record_local_format(val: &str, found: &mut HoistedFlags) {
    match OutputFormat::parse(val) {
        Some(fmt) => found.format = Some(fmt),
        None => found.table_format = true,
    }
}

fn is_local_agent_value_flag(arg: &str) -> bool {
    matches!(
        arg,
        "--member"
            | "--workspace"
            | "--ttl"
            | "--label"
            | "--session"
            | "--limit"
            | "--history-limit"
            | "--name"
            | "--instance"
            | "--office"
            | "--color"
    )
}

/// Scans `args` of a local clap verb (`agent bind|renew|sessions`) for explicit output/help flags
/// without modifying `args`, so `-q`, `--json`, or `--format` cleanly overrides `AGENT_OS_FORMAT`.
fn scan_local_agent_flags(args: &[String]) -> HoistedFlags {
    let mut found = HoistedFlags::default();
    let mut idx = 1;
    while let Some(arg) = args.get(idx) {
        idx += 1;
        if arg == "--" {
            break;
        }
        if let Some(val) = arg.strip_prefix("--format=") {
            record_local_format(val, &mut found);
            continue;
        }
        match arg.as_str() {
            "--help" | "-h" => found.passthrough_meta = true,
            "--json" => found.json = true,
            "-q" | "--quiet" => found.quiet = true,
            "--format" => match args.get(idx) {
                Some(val) => {
                    idx += 1;
                    record_local_format(val, &mut found);
                }
                None => found.table_format = true,
            },
            other if is_local_agent_value_flag(other) => idx += 1,
            _ => {}
        }
    }
    found
}

/// Whether `top` is a passthrough host command (`serve`, `setup`, `recovery`) whose
/// arguments include `-h` or `--help` before `--`.
fn is_passthrough_host_help(top: &TopCommand) -> bool {
    let pass = match top {
        TopCommand::Serve(p) | TopCommand::Setup(p) | TopCommand::Recovery(p) => p,
        _ => return false,
    };
    pass.args
        .iter()
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| matches!(arg.as_str(), "--help" | "-h"))
}

fn hoist_sign_in_words(args: &mut features::sign_in::SignInArgs) -> Result<HoistedFlags, CliError> {
    if is_sign_in_local_agent(args.command()) {
        return Ok(scan_local_agent_flags(&args.command()[1..]));
    }
    let (cleaned, hoisted) = hoist_office_words(args.command())?;
    *args.command_mut() = cleaned;
    Ok(hoisted)
}

fn hoist_command_flags(top: &mut TopCommand) -> Result<Option<HoistedFlags>, CliError> {
    if let TopCommand::Agent(pass) = top {
        if is_local_agent_verb(&pass.args) {
            return Ok(Some(scan_local_agent_flags(&pass.args)));
        }
    }
    if let Some((resource, pass_args)) = top.office_resource_mut() {
        let mut full = Vec::with_capacity(pass_args.len() + 1);
        full.push(resource.to_owned());
        full.extend(pass_args.iter().cloned());
        let (cleaned, hoisted) = hoist_office_words(&full)?;
        *pass_args = cleaned.into_iter().skip(1).collect();
        return Ok(Some(hoisted));
    }
    if let TopCommand::Org(args) = top {
        if let Some(words) = args.office_words() {
            let (_, hoisted) = hoist_office_words(&words)?;
            return Ok(Some(hoisted));
        }
    }
    if let TopCommand::SignIn(args) = top {
        if !args.command().is_empty() {
            return Ok(Some(hoist_sign_in_words(args)?));
        }
    }
    Ok(None)
}

fn prepare_modes(
    mut top: TopCommand,
    mut explicit: ExplicitOutput,
    modes: &mut Modes,
) -> Result<(TopCommand, ExplicitOutput, bool), CliError> {
    if let Some(hoisted) = hoist_command_flags(&mut top)? {
        let passthrough = merge_hoisted(hoisted, &mut explicit, modes)?;
        return Ok((top, explicit, passthrough));
    }
    let passthrough = is_passthrough_host_help(&top);
    validate_output_conflict(&explicit)?;
    if passthrough || explicit.has_output_override() {
        modes.ignore_env_format = true;
    }
    Ok((top, explicit, passthrough))
}

/// Injects `--format yaml|compact` after the command path of an office command when `ctx` resolved
/// to YAML or Compact (via `--format` before the subcommand or `AGENT_OS_FORMAT`), without
/// touching `--help`, `--schema`, `--version`, or incomplete command paths.
fn apply_office_output_format(
    mut words: Vec<String>,
    explicit: &ExplicitOutput,
    passthrough_meta: bool,
    ctx: &ExecutionContext,
) -> Vec<String> {
    if passthrough_meta || explicit.table_format || ctx.modes().quiet {
        return words;
    }
    let path_len = command_path_words(&words);
    if words.len() < path_len {
        return words;
    }
    let fmt = match ctx.output_format() {
        OutputFormat::Yaml => Some("yaml"),
        OutputFormat::Compact => Some("compact"),
        OutputFormat::Json | OutputFormat::Tsv | OutputFormat::Table => None,
    };
    if let Some(fmt) = fmt {
        words.splice(path_len..path_len, ["--format".to_owned(), fmt.to_owned()]);
    }
    words
}

fn dispatch(
    top: TopCommand,
    explicit: &ExplicitOutput,
    passthrough_meta: bool,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    use features::*;
    refuse_unsupported_modes(&top, explicit)?;
    if let TopCommand::Agent(pass) = &top {
        if let Some(outcome) = agent::try_run(&pass.args, ctx) {
            return outcome;
        }
    }
    if let Some((resource, words)) = top.office_resource() {
        let words: Vec<String> = std::iter::once(resource.to_owned())
            .chain(words.iter().cloned())
            .collect();
        let words = apply_office_output_format(words, explicit, passthrough_meta, ctx);
        return sign_in::office_proxy::run(words, ctx);
    }
    match top {
        TopCommand::Serve(pass) => serve::run(pass.args, ctx),
        TopCommand::Setup(pass) => setup::run(&pass.args, ctx),
        TopCommand::SignIn(mut args) => {
            if let Some(outcome) = agent::try_run_after_sign_in(
                args.instance_name(),
                args.office_id(),
                args.command(),
                ctx,
            ) {
                return outcome;
            }
            if !args.command().is_empty() {
                let words = std::mem::take(args.command_mut());
                *args.command_mut() =
                    apply_office_output_format(words, explicit, passthrough_meta, ctx);
            }
            sign_in::run(args, ctx)
        }
        TopCommand::Context(args) => context::run(args, ctx),
        TopCommand::Admin(args) => admin::run(args, ctx),
        TopCommand::Org(args) => match args.office_words() {
            Some(words) => {
                let (cleaned, _) = hoist_office_words(&words)?;
                let cleaned = apply_office_output_format(cleaned, explicit, passthrough_meta, ctx);
                sign_in::office_proxy::run(cleaned, ctx)
            }
            None => org::run(args, ctx),
        },
        TopCommand::Office(args) => office::run(args, ctx),
        TopCommand::Project(args) => project::run(args, ctx),
        TopCommand::Provider(args) => provider::run(args, ctx),
        TopCommand::Release(args) => release::run(args, ctx),
        TopCommand::Service(args) => service::run(args, ctx),
        TopCommand::Backup(args) => backup::backup(args, ctx),
        TopCommand::Restore(args) => backup::restore(args, ctx),
        TopCommand::Recovery(pass) => recovery::run(pass.args, ctx),
        TopCommand::Status(args) => status::run(args, ctx),
        TopCommand::Logs(args) => diagnostics::logs(args, ctx),
        TopCommand::Doctor(args) => diagnostics::doctor(args, ctx),
        TopCommand::Support(args) => diagnostics::support(args, ctx),
        TopCommand::Uninstall(args) => diagnostics::uninstall(args, ctx),
        TopCommand::Completion(args) => completion::run(args, cli::command(), ctx),
        _ => Err(CliError::usage("this command runs over the office channel")),
    }
}
