//! `agentic-org <resource> <verb> ...` for the office resources (`task`, `msg`, `who`, `hr`,
//! ... and `org inspect`): the command runs over the signed-in channel of the selected office
//! through the same command table as `agent-os`, and is shown for a person:
//!
//! - `--format json|yaml|compact`, `--help`, `--schema`: the table's own answer, as written;
//! - `--json`: the table's JSON envelope on standard output (also for a refusal, exit 1/2);
//! - `-q`: the IDs of the answer, one per line;
//! - a terminal: a borderless table for a list, an aligned card for a record, and a refusal
//!   as a diagnostic on standard error;
//! - a pipe: the table's `compact` form (headerless, tab-separated lines).
//!
//! The exit code is the table's: 0, 1 (refused) or 2 (a command line it cannot read). A line
//! this command cannot split (a selector given twice, a flag left without its value at the
//! end) is refused before the table as the CLI's usage error (exit 2, under `--json` the
//! CLI's error envelope on standard error), before any instance or office is reached.

use org_agent_cli::dispatch::{hint_for, OfficeTransport, TransportOffice};
use org_cli_shell::context::{ColorMode, ExecutionContext};
use org_command::{Office, Refusal};
use org_host::setup_flow::{ApprovedOfficeChannel, HostCliErrorKind};
use org_proto::ErrorCode;
use serde_json::Value;

use super::render::{self, print_answer};
use super::selectors::{self, command_path_len, has_flag, resource_and_verb, Selectors};
use crate::error::CliError;

/// The office channel the sign-in opened.
pub(super) struct GrantTransport(pub(super) ApprovedOfficeChannel);

impl OfficeTransport for GrantTransport {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        self.0
            .call_method(method, params)
            .map_err(|api| Refusal::new(api.code, api.message, hint_for(api.code)))
    }
}

/// Opens the channel on the first call: `--help` and `--schema` never sign in.
struct LazyGrantTransport {
    instance: Option<String>,
    office: Option<String>,
    channel: Option<ApprovedOfficeChannel>,
}

impl LazyGrantTransport {
    fn channel_mut(&mut self) -> Result<&mut ApprovedOfficeChannel, Refusal> {
        if self.channel.is_none() {
            let opened = org_host::setup_flow::open_for_command(
                self.instance.as_deref(),
                self.office.as_deref(),
                &mut super::approval_link_stream(),
            )
            .map_err(|error| {
                let code = match error.kind {
                    HostCliErrorKind::Auth => ErrorCode::NotAuthorized,
                    _ => error.code.unwrap_or(ErrorCode::Unavailable),
                };
                Refusal::new(
                    code,
                    error.message,
                    error.help.unwrap_or_else(|| {
                        "Run `agentic-org sign-in` or check `--name` / `--office`.".to_owned()
                    }),
                )
            })?;
            self.channel = Some(opened);
        }
        Ok(self.channel.as_mut().expect("channel initialized"))
    }
}

impl OfficeTransport for LazyGrantTransport {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        self.channel_mut()?
            .call_method(method, params)
            .map_err(|api| Refusal::new(api.code, api.message, hint_for(api.code)))
    }
}

/// How the answer is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Shown {
    /// The table's own output, as the caller asked for it.
    AsWritten,
    /// `--format json`, printed as it is.
    Json,
    /// `--format compact`, printed as it is (a pipe).
    Compact,
    /// The IDs.
    Quiet,
    /// A table or a card (a terminal).
    Rendered,
}

pub(super) fn shown(forwarded: &[String], ctx: &ExecutionContext) -> Shown {
    let complete = forwarded.first().is_some_and(|first| first == "who") || forwarded.len() >= 2;
    if !complete
        || has_flag(
            forwarded,
            &["--format", "--help", "-h", "--schema", "--version"],
        )
    {
        Shown::AsWritten
    } else if ctx.modes().json {
        Shown::Json
    } else if ctx.modes().quiet {
        Shown::Quiet
    } else if ctx.is_tty_stdout() {
        Shown::Rendered
    } else {
        Shown::Compact
    }
}

/// Applies the global flags written after the resource to this run.
fn hoist(scanned: &Selectors, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    if !(scanned.quiet || scanned.no_input || scanned.no_color || scanned.color.is_some()) {
        return Ok(());
    }
    let mut modes = ctx.modes().clone();
    modes.quiet |= scanned.quiet;
    modes.no_input |= scanned.no_input;
    modes.no_color |= scanned.no_color;
    if let Some(color) = &scanned.color {
        modes.color = ColorMode::from_flag(color).ok_or_else(|| {
            CliError::usage("--color is auto, always or never").with_help("agentic-org --help")
        })?;
    }
    if modes.quiet && modes.json {
        return Err(CliError::usage(
            "--quiet and --json cannot be used together",
        ));
    }
    *ctx = ExecutionContext::detect(modes);
    Ok(())
}

/// Runs office command `words` (`<resource> <verb> ...`) for the selected office.
pub(crate) fn run(words: Vec<String>, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let scanned = prepare(&words, ctx)?;
    let target = crate::selection::select(scanned.instance.as_deref(), scanned.office.as_deref())?;
    let mut transport = TransportOffice(LazyGrantTransport {
        instance: target.explicit_instance().map(str::to_owned),
        office: target.explicit_office().map(str::to_owned),
        channel: None,
    });
    execute(scanned.forwarded, ctx, &mut transport)
}

/// Splits `words` and applies the global flags written after the resource. A line the scan
/// refuses (a selector given twice, a flag left without its value) fails here, before an
/// instance is selected or an office is asked.
fn prepare(words: &[String], ctx: &mut ExecutionContext) -> Result<Selectors, CliError> {
    let scanned = selectors::split(words).map_err(CliError::usage)?;
    hoist(&scanned, ctx)?;
    Ok(scanned)
}

/// Runs `forwarded` against `office` and shows the answer.
pub(super) fn execute(
    mut forwarded: Vec<String>,
    ctx: &mut ExecutionContext,
    office: &mut dyn Office,
) -> Result<(), CliError> {
    let shown = shown(&forwarded, ctx);
    let command = resource_and_verb(&forwarded)
        .and_then(|(resource, verb)| org_agent_cli::table::runtime_table().find(resource, verb));
    // The words the run adds go right after the command path, before every flag and value
    // the caller wrote and before any `--`: never at the end, where a value-taking flag left
    // without its value (`--body`) would take `--dry-run` or `--format` as its text and a dry
    // run would become a live write. Such a line stays without a value and the table refuses
    // it (exit 2) before the office is asked.
    let mut added: Vec<String> = Vec::with_capacity(3);
    // The global `--dry-run` always reaches the table, which previews a write and refuses it
    // (exit 2) for a command that changes nothing: `agentic-org --dry-run task list` never
    // runs as if it had not been given. A `--dry-run` that is a flag's value or follows `--`
    // is not the flag (`has_flag`).
    if ctx.modes().dry_run && !has_flag(&forwarded, &["--dry-run"]) {
        added.push("--dry-run".to_owned());
    }
    match shown {
        Shown::AsWritten => {}
        Shown::Compact => added.extend(["--format".to_owned(), "compact".to_owned()]),
        Shown::Json | Shown::Quiet | Shown::Rendered => {
            added.extend(["--format".to_owned(), "json".to_owned()]);
        }
    }
    let at = command_path_len(&forwarded);
    forwarded.splice(at..at, added);
    let output = org_agent_cli::execute_with_office("agentic-org", &forwarded, office);
    match shown {
        Shown::Quiet | Shown::Rendered => {
            render::answer(&output, command, shown == Shown::Quiet, ctx)
        }
        Shown::AsWritten | Shown::Json | Shown::Compact => {
            print_answer(ctx, &output.stdout, output.exit_code)
        }
    }
}

#[cfg(test)]
mod tests {
    use org_cli_shell::context::testing::{captured, tty};
    use org_cli_shell::context::Modes;
    use org_cli_shell::terminal::Terminal;
    use org_command::{Answer, Request};

    use super::*;

    /// An office that records every request it is sent and refuses it: nothing is changed.
    #[derive(Default)]
    struct Recording(Vec<Request>);

    impl Office for Recording {
        fn call(&mut self, request: &Request) -> Result<Answer, Refusal> {
            self.0.push(request.clone());
            Err(Refusal::new(
                ErrorCode::Unavailable,
                "recorded",
                "a test office",
            ))
        }
    }

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|word| (*word).to_owned()).collect()
    }

    /// B-02: `agentic-org --dry-run msg send --to ada --body "--dry-run"` is still a dry run:
    /// the body's text is not the flag, so the global `--dry-run` reaches the table and the
    /// office is asked for a preview, never a live write.
    #[test]
    fn a_body_that_reads_dry_run_does_not_turn_a_dry_run_into_a_live_write() {
        let modes = Modes {
            dry_run: true,
            ..Modes::default()
        };
        for terminal in [Terminal::piped(), tty(Some(80))] {
            let (mut ctx, _streams) = captured(modes.clone(), terminal, &[]);
            let mut office = Recording::default();
            let forwarded = words(&["msg", "send", "--to", "ada", "--body", "--dry-run"]);
            let _ = execute(forwarded, &mut ctx, &mut office);
            assert_eq!(office.0.len(), 1, "the office was not asked");
            let request = &office.0[0];
            assert!(request.dry_run, "a live write was sent: {request:?}");
            assert_eq!(request.idempotency_key, None, "{request:?}");
            assert_eq!(request.input["body"], "--dry-run");
        }
    }

    /// NB-03: the global `--dry-run` reaches a command that changes nothing too, where the
    /// table refuses it (exit 2) before the office is asked: it never runs as a plain read.
    #[test]
    fn a_dry_run_of_a_read_is_refused_and_never_runs() {
        let modes = Modes {
            dry_run: true,
            ..Modes::default()
        };
        for list in [&["task", "list"][..], &["who"], &["msg", "inbox"]] {
            let (mut ctx, _streams) = captured(modes.clone(), Terminal::piped(), &[]);
            let mut office = Recording::default();
            let error = execute(words(list), &mut ctx, &mut office).unwrap_err();
            use org_cli_shell::error::CliDiagnostic;
            assert_eq!(error.exit_code(), 2, "{list:?}");
            assert!(office.0.is_empty(), "{list:?} ran: {:?}", office.0);
        }
    }

    /// P0-1: a dry run whose line ends in a flag that takes a value, or writes a flag after
    /// `--`, is a usage error (exit 2) and the office is never asked: the added `--dry-run`
    /// and `--format` never become the missing value. Both ways in are covered: the office
    /// resources (`prepare`, then `execute`) and `sign-in -- <command>` (`execute` alone).
    #[test]
    fn a_dry_run_missing_a_value_or_with_flags_after_the_terminator_never_asks_the_office() {
        use org_cli_shell::error::CliDiagnostic;
        let modes = Modes {
            dry_run: true,
            ..Modes::default()
        };
        for list in [
            &["msg", "send", "--to", "ada", "--body"][..],
            &["task", "create", "--title"],
            &["msg", "send", "--to", "ada", "--", "--body", "x"],
            &["task", "note", "add", "--task", "T-1", "--text"],
        ] {
            for terminal in [Terminal::piped(), tty(Some(80))] {
                let mut office = Recording::default();
                let (mut ctx, _streams) = captured(modes.clone(), terminal, &[]);
                let error = prepare(&words(list), &mut ctx)
                    .and_then(|scanned| execute(scanned.forwarded, &mut ctx, &mut office))
                    .unwrap_err();
                assert_eq!(error.exit_code(), 2, "{list:?}");
                let (mut ctx, _streams) = captured(modes.clone(), terminal, &[]);
                let error = execute(words(list), &mut ctx, &mut office).unwrap_err();
                assert_eq!(error.exit_code(), 2, "sign-in -- {list:?}");
                assert!(
                    office.0.is_empty(),
                    "{list:?} reached the office: {:?}",
                    office.0
                );
            }
        }
    }

    /// The added words sit right after the command path, so a value written last keeps its
    /// own text and the request is a dry run.
    #[test]
    fn the_added_flags_go_after_the_command_path() {
        let modes = Modes {
            dry_run: true,
            ..Modes::default()
        };
        for (list, field, value) in [
            (
                &["msg", "send", "--to", "ada", "--body", "hi"][..],
                "body",
                "hi",
            ),
            (
                &["task", "create", "--title", "--format"],
                "title",
                "--format",
            ),
            (
                &["task", "note", "add", "--task", "T-1", "--text", "b"],
                "text",
                "b",
            ),
        ] {
            let (mut ctx, _streams) = captured(modes.clone(), Terminal::piped(), &[]);
            let mut office = Recording::default();
            let _ = execute(words(list), &mut ctx, &mut office);
            assert_eq!(office.0.len(), 1, "{list:?} was not sent");
            assert!(office.0[0].dry_run, "{list:?}: {:?}", office.0[0]);
            assert_eq!(office.0[0].input[field], value, "{list:?}");
        }
    }

    /// B-02: a value that reads `-h` or `--help` is text, not a request for help: the answer
    /// is shown as the run asked (`-q`, a table, compact), never the table's help as written.
    #[test]
    fn a_value_that_reads_help_is_sent_and_shown_as_asked() {
        let quiet = Modes {
            quiet: true,
            ..Modes::default()
        };
        for (modes, terminal, list, field, value, expected) in [
            (
                Modes::default(),
                Terminal::piped(),
                &["msg", "send", "--to", "ada", "--body", "-h"][..],
                "body",
                "-h",
                Shown::Compact,
            ),
            (
                Modes::default(),
                tty(Some(80)),
                &["msg", "send", "--to", "ada", "--body", "-h"],
                "body",
                "-h",
                Shown::Rendered,
            ),
            (
                quiet,
                tty(Some(80)),
                &["task", "create", "--title", "--help"],
                "title",
                "--help",
                Shown::Quiet,
            ),
        ] {
            let (mut ctx, _streams) = captured(modes, terminal, &[]);
            let forwarded = words(list);
            assert_eq!(shown(&forwarded, &ctx), expected, "{list:?}");
            let mut office = Recording::default();
            let _ = execute(forwarded, &mut ctx, &mut office);
            assert_eq!(office.0.len(), 1, "{list:?} was not sent");
            assert_eq!(office.0[0].input[field], value, "{list:?}");
        }
        // A real `--help` is still the table's own answer.
        let (ctx, _streams) = captured(Modes::default(), tty(Some(80)), &[]);
        assert_eq!(
            shown(&words(&["task", "create", "--title", "t", "--help"]), &ctx),
            Shown::AsWritten
        );
    }
}
