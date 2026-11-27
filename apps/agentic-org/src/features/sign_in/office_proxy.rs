//! `agentic-org <resource> <verb> ...` for the office resources (`task`, `msg`, `who`, `hr`,
//! ... and `org inspect`): the command runs over the signed-in channel of the selected office
//! through the same command table as `agent-os`, and is shown for a person:
//!
//! - `--format json|yaml|compact`, `--help`, `--schema`: the table's own answer, as written;
//! - `--json`: the table's JSON envelope on standard output (also for a refusal, exit 1/2);
//! - `-q`: the IDs of the answer, one per line;
//! - a terminal: a borderless table for a list, an aligned card for a record, and a refusal
//!   as a diagnostic on standard error;
//! - a pipe: headerless, tab-separated lines (`OutputFormat::Tsv` / `Shown::Tsv`).
//!
//! The exit code is the table's: 0, 1 (refused) or 2 (a command line it cannot read). A line
//! this command cannot split (a selector given twice, a flag left without its value at the
//! end) is refused before the table as the CLI's usage error (exit 2, under `--json` the
//! CLI's error envelope on standard error), before any instance or office is reached.
//!
//! The office channel is opened on the table's first call to the office, never before: a line
//! the table refuses (exit 2) or answers alone (`--help`, `--schema`) never reaches the host.
//! `sign-in -- <command>` reads its line the same way ([`sign_in_and_execute`]).

use org_agent_cli::{OfficeTransport, TransportOffice};
use org_cli_shell::context::{ColorMode, ExecutionContext, OutputFormat};
use org_command::{Command, Office, Output, Refusal};
use org_host_setup::{ApprovedOfficeChannel, HostCliError, HostCliErrorKind};
use org_proto::ErrorCode;
use serde_json::Value;

use super::render::{self, print_answer};
use super::selectors::{
    self, command_path_len, has_flag, names_a_command, resource_and_verb, Selectors,
};
use crate::error::CliError;

/// Opens the channel on the first call: `--help` and `--schema` never sign in.
struct LazyGrantTransport {
    instance: Option<String>,
    office: Option<String>,
    channel: Option<ApprovedOfficeChannel>,
    /// The host's own answer when the channel could not be opened, as it said it.
    failed: Option<HostCliError>,
}

impl LazyGrantTransport {
    fn new(instance: Option<&str>, office: Option<&str>) -> Self {
        Self {
            instance: instance.map(str::to_owned),
            office: office.map(str::to_owned),
            channel: None,
            failed: None,
        }
    }

    fn open(&self) -> Result<ApprovedOfficeChannel, HostCliError> {
        org_host_setup::open_for_command(
            self.instance.as_deref(),
            self.office.as_deref(),
            &mut super::approval_link_stream(),
        )
    }

    /// Signs in (opens the channel) when no call has yet.
    fn sign_in(&mut self) -> Result<(), HostCliError> {
        if self.channel.is_none() {
            self.channel = Some(self.open()?);
        }
        Ok(())
    }

    /// The channel, opened on the first call. A sign-in the host refused is not tried again:
    /// every later call gets the same refusal.
    fn channel_mut(&mut self) -> Result<&mut ApprovedOfficeChannel, Refusal> {
        if self.channel.is_none() {
            if let Some(error) = &self.failed {
                return Err(refusal_for(error));
            }
            match self.open() {
                Ok(opened) => self.channel = Some(opened),
                Err(error) => {
                    let refusal = refusal_for(&error);
                    self.failed = Some(error);
                    return Err(refusal);
                }
            }
        }
        Ok(self.channel.as_mut().expect("channel initialized"))
    }
}

/// A sign-in the host refused, as the table's refusal of the call.
fn refusal_for(error: &HostCliError) -> Refusal {
    let code = match error.kind {
        HostCliErrorKind::Auth => ErrorCode::NotAuthorized,
        _ => error.code.unwrap_or(ErrorCode::Unavailable),
    };
    Refusal::new(
        code,
        error.message.clone(),
        error.help.clone().unwrap_or_else(|| {
            "Run `agentic-org sign-in` or check `--name` / `--office`.".to_owned()
        }),
    )
}

impl OfficeTransport for LazyGrantTransport {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        self.channel_mut()?
            .call_method(method, params)
            .map_err(org_agent_cli::api_refusal)
    }
}

/// How the answer is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Shown {
    /// The table's own output, as the caller asked for it (or via injected `--format`).
    AsWritten,
    /// `--format json`, printed as it is.
    Json,
    /// `--format yaml`, printed as it is.
    Yaml,
    /// `--format compact`, printed as it is.
    Compact,
    /// Consistent headerless TSV across TTY and pipe, with zero stderr prose.
    Tsv,
    /// The IDs.
    Quiet,
    /// A borderless table or aligned card (even when piped if `--format table` is active).
    Rendered,
}

pub(super) fn shown(forwarded: &[String], ctx: &ExecutionContext) -> Shown {
    // `task note` (no verb yet) is not a command: the table says so as written, and nothing is
    // added after its path.
    if !names_a_command(forwarded)
        || has_flag(
            forwarded,
            &["--format", "--help", "-h", "--schema", "--version"],
        )
    {
        return Shown::AsWritten;
    }
    if ctx.modes().quiet {
        return Shown::Quiet;
    }
    match ctx.output_format() {
        OutputFormat::Table => Shown::Rendered,
        OutputFormat::Tsv => Shown::Tsv,
        OutputFormat::Json => Shown::Json,
        OutputFormat::Yaml => Shown::Yaml,
        OutputFormat::Compact => Shown::Compact,
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
    let (bind_workspace, hire_args) = super::hr_bind::split_bind_workspace(&scanned.forwarded)?;
    let target = crate::selection::select(scanned.instance.as_deref(), scanned.office.as_deref())?;
    let mut transport =
        LazyGrantTransport::new(target.explicit_instance(), target.explicit_office());
    if let Some(workspace) = bind_workspace {
        let shown = shown(&hire_args, ctx);
        return super::hr_bind::execute_hire_and_bind(
            hire_args,
            super::hr_bind::HireBindOptions {
                workspace: &workspace,
                target_office: target.office(),
                office_selector: target.explicit_office(),
                shown,
                state_root: None,
            },
            ctx,
            &mut transport,
        );
    }
    execute(scanned.forwarded, ctx, &mut TransportOffice(transport))
}

/// `agentic-org sign-in -- <command>`: signs in to the selected office and runs `forwarded`
/// there. The table reads the line first and the host is reached only when the table asks the
/// office, so a line it refuses (a flag left without its value, a flag after `--`: exit 2)
/// never touches an instance or an office. A line the table answers alone (`--help`,
/// `--schema`) still signs in, then shows that answer. A sign-in that fails fails as the host
/// said (sign-in required: exit 4), never as a refusal of the command.
pub(super) fn sign_in_and_execute(
    forwarded: Vec<String>,
    instance: Option<&str>,
    office: Option<&str>,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    if matches!(
        resource_and_verb(&forwarded),
        Some(("agent", "bind" | "renew" | "sessions"))
    ) {
        let verb = forwarded.get(1).map_or("bind", String::as_str);
        return Err(CliError::usage(format!(
            "`agent {verb}` runs via `agentic-org agent {verb}`"
        ))
        .with_help(format!(
            "Run `agentic-org agent {verb} --help` (or `agentic-org sign-in -- agent {verb} ...`)."
        )));
    }
    let (bind_workspace, hire_args) = super::hr_bind::split_bind_workspace(&forwarded)?;
    if let Some(workspace) = bind_workspace {
        let shown = shown(&hire_args, ctx);
        let mut transport = LazyGrantTransport::new(instance, office);
        let result = super::hr_bind::execute_hire_and_bind(
            hire_args,
            super::hr_bind::HireBindOptions {
                workspace: &workspace,
                target_office: office,
                office_selector: office,
                shown,
                state_root: None,
            },
            ctx,
            &mut transport,
        );
        if let Some(error) = transport.failed.take() {
            return Err(error.into());
        }
        return result;
    }
    let mut transport = TransportOffice(LazyGrantTransport::new(instance, office));
    let answered = ask(forwarded, ctx, &mut transport);
    let grant = &mut transport.0;
    if let Some(error) = grant.failed.take() {
        return Err(error.into());
    }
    if answered.output.exit_code == 0 {
        grant.sign_in()?;
    }
    show(&answered, ctx)
}

/// Splits `words` and applies the global flags written after the resource. A line the scan
/// refuses (a selector given twice, a flag left without its value) fails here, before an
/// instance is selected or an office is asked.
fn prepare(words: &[String], ctx: &mut ExecutionContext) -> Result<Selectors, CliError> {
    let scanned = selectors::split(words).map_err(CliError::usage)?;
    hoist(&scanned, ctx)?;
    Ok(scanned)
}

/// The table's answer to a line, not yet shown.
struct Answered {
    shown: Shown,
    command: Option<&'static Command>,
    output: Output,
}

/// Runs `forwarded` against `office` and shows the answer.
pub(super) fn execute(
    forwarded: Vec<String>,
    ctx: &mut ExecutionContext,
    office: &mut dyn Office,
) -> Result<(), CliError> {
    let answered = ask(forwarded, ctx, office);
    show(&answered, ctx)
}

/// Runs `forwarded` against `office`.
fn ask(mut forwarded: Vec<String>, ctx: &ExecutionContext, office: &mut dyn Office) -> Answered {
    let shown = shown(&forwarded, ctx);
    let command = resource_and_verb(&forwarded)
        .and_then(|(resource, verb)| org_agent_cli::runtime_table().find(resource, verb));
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
        Shown::Yaml => added.extend(["--format".to_owned(), "yaml".to_owned()]),
        Shown::Json | Shown::Quiet | Shown::Rendered | Shown::Tsv => {
            added.extend(["--format".to_owned(), "json".to_owned()]);
        }
    }
    let at = command_path_len(&forwarded);
    forwarded.splice(at..at, added);
    let output = org_agent_cli::execute_with_office("agentic-org", &forwarded, office);
    Answered {
        shown,
        command,
        output,
    }
}

/// Shows the table's answer as the run asked.
fn show(answered: &Answered, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let Answered {
        shown,
        command,
        output,
    } = answered;
    match shown {
        Shown::Quiet | Shown::Rendered | Shown::Tsv => {
            render::answer(output, *command, *shown == Shown::Quiet, ctx)
        }
        Shown::AsWritten | Shown::Json | Shown::Yaml | Shown::Compact => {
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

    /// `task note` without its verb is not a command yet: it is answered as written, nothing
    /// is added after its path, and the table names the missing verb rather than reading an
    /// added `--format` as one. The office is never asked.
    #[test]
    fn a_task_note_without_its_verb_is_answered_as_written() {
        use org_cli_shell::error::CliDiagnostic;
        for terminal in [Terminal::piped(), tty(Some(80))] {
            let (mut ctx, _streams) = captured(Modes::default(), terminal, &[]);
            let forwarded = words(&["task", "note"]);
            assert_eq!(shown(&forwarded, &ctx), Shown::AsWritten);
            let mut office = Recording::default();
            let answered = ask(forwarded, &ctx, &mut office);
            assert_eq!(answered.output.exit_code, 2, "{}", answered.output.stdout);
            assert!(
                answered.output.stdout.contains("`task-note` needs a verb"),
                "{}",
                answered.output.stdout
            );
            assert!(office.0.is_empty(), "{:?}", office.0);
            let error = show(&answered, &mut ctx).unwrap_err();
            assert_eq!(error.exit_code(), 2);
        }
    }

    /// TASK-137 B1: the operator shorthand (`agent approve <code>`, `agent sessions @<handle>`,
    /// `agent revoke @<handle> [<session>]`) reaches the office whatever the run adds after
    /// the command path (`--format json` for terminal, pipe/`OutputFormat::Tsv`, or `--json`),
    /// through both ways in. So does the exact line `agent-os exec` runs.
    #[test]
    fn the_operator_shorthand_reaches_the_office_through_the_added_format() {
        let json = Modes {
            json: true,
            ..Modes::default()
        };
        let exec_line =
            org_agent_cli::external_auth::operator_approve_args("ABCD-EFGH", 3_600, true);
        /// A line, the verb it names and the input fields the office must receive.
        type Case = (
            Vec<String>,
            &'static str,
            Vec<(&'static str, serde_json::Value)>,
        );
        let cases: Vec<Case> = vec![
            (
                words(&["agent", "approve", "ABCD-EFGH"]),
                "approve",
                vec![("code", "ABCD-EFGH".into())],
            ),
            (
                words(&["agent", "approve", "K7QF-9XM2", "--ttl", "8h", "--takeover"]),
                "approve",
                vec![
                    ("code", "K7QF-9XM2".into()),
                    ("ttl", "8h".into()),
                    ("takeover", true.into()),
                ],
            ),
            (
                exec_line,
                "approve",
                vec![
                    ("code", "ABCD-EFGH".into()),
                    ("ttl", "3600s".into()),
                    ("takeover", true.into()),
                ],
            ),
            (
                words(&["agent", "sessions", "@ada", "--limit", "50"]),
                "sessions",
                vec![("member", "@ada".into()), ("history_limit", 50.into())],
            ),
            (
                words(&["agent", "revoke", "@ada"]),
                "revoke",
                vec![("member", "@ada".into())],
            ),
            (
                words(&["agent", "revoke", "@ada", "ext-0193a5c0"]),
                "revoke",
                vec![
                    ("member", "@ada".into()),
                    ("session", "ext-0193a5c0".into()),
                ],
            ),
        ];
        for (line, verb, inputs) in cases {
            for (modes, terminal) in [
                (Modes::default(), Terminal::piped()),
                (Modes::default(), tty(Some(80))),
                (json.clone(), Terminal::piped()),
            ] {
                for through_prepare in [true, false] {
                    let (mut ctx, _streams) = captured(modes.clone(), terminal, &[]);
                    let forwarded = if through_prepare {
                        prepare(&line, &mut ctx).unwrap().forwarded
                    } else {
                        line.clone()
                    };
                    let mut office = Recording::default();
                    let answered = ask(forwarded, &ctx, &mut office);
                    assert_ne!(
                        answered.output.exit_code, 2,
                        "{line:?} ({modes:?}): {}",
                        answered.output.stdout
                    );
                    assert_eq!(office.0.len(), 1, "{line:?} never reached the office");
                    let request = &office.0[0];
                    assert_eq!(request.command.resource, "agent");
                    assert_eq!(request.command.verb, verb);
                    for (field, value) in &inputs {
                        assert_eq!(&request.input[*field], value, "{line:?}: {field}");
                    }
                }
            }
        }
    }

    /// B-02: a value that reads `-h` or `--help` is text, not a request for help: the answer
    /// is shown as the run asked (`-q`, a table, or `Shown::Tsv` in a pipe), never the table's
    /// help as written.
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
                Shown::Tsv,
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

    struct StubListOffice;

    impl Office for StubListOffice {
        fn call(&mut self, _request: &Request) -> Result<Answer, Refusal> {
            Ok(Answer {
                status: org_command::Status::Waiting,
                next_cursor: Some("c2".to_owned()),
                ..Answer::completed(serde_json::json!([
                    {
                        "id": "0193a5c0-0000-7000-8000-000000000001",
                        "name": "Ada",
                        "handle": "ada",
                        "kind": "agent",
                        "owner": false,
                        "agent_status": "active",
                        "role_title": "Engineer",
                        "presence": "online"
                    }
                ]))
            })
        }
    }

    /// P1-2: `shown` dispatches strictly on `ctx.output_format()`: `--format table` renders a
    /// borderless table even when piped, and `--format tsv` produces identical TSV across TTY and
    /// pipe with zero stderr prose.
    #[test]
    fn shown_honors_table_when_piped_and_tsv_consistently_with_zero_stderr_prose() {
        let cmd = words(&["who"]);

        // Piped `--format table` -> `Shown::Rendered` -> borderless UPPERCASE table.
        let table_modes = Modes {
            format: Some(OutputFormat::Table),
            ..Modes::default()
        };
        let (mut ctx, streams) = captured(table_modes, Terminal::piped(), &[]);
        assert_eq!(shown(&cmd, &ctx), Shown::Rendered);
        execute(cmd.clone(), &mut ctx, &mut StubListOffice).unwrap();
        let piped_table = streams.stdout.text();
        assert!(
            piped_table.contains("HANDLE") && piped_table.contains("ada"),
            "piped --format table must render borderless table:\n{piped_table}"
        );

        // `OutputFormat::Tsv` on TTY and pipe -> `Shown::Tsv`, byte-identical stdout, zero stderr.
        let tsv_modes = Modes {
            format: Some(OutputFormat::Tsv),
            ..Modes::default()
        };
        let (mut tty_ctx, tty_streams) = captured(tsv_modes.clone(), tty(Some(80)), &[]);
        let (mut pipe_ctx, pipe_streams) = captured(tsv_modes, Terminal::piped(), &[]);
        assert_eq!(shown(&cmd, &tty_ctx), Shown::Tsv);
        assert_eq!(shown(&cmd, &pipe_ctx), Shown::Tsv);
        execute(cmd.clone(), &mut tty_ctx, &mut StubListOffice).unwrap();
        execute(cmd.clone(), &mut pipe_ctx, &mut StubListOffice).unwrap();
        assert_eq!(tty_streams.stdout.text(), pipe_streams.stdout.text());
        assert!(tty_streams.stdout.text().contains('\t'));
        assert!(
            tty_streams.stderr.text().is_empty(),
            "TSV on TTY must emit zero stderr prose: {:?}",
            tty_streams.stderr.text()
        );
        assert!(pipe_streams.stderr.text().is_empty());

        // `Json`, `Yaml`, and `Compact` dispatch directly or `AsWritten` when `--format` is set.
        let (json_ctx, _) = captured(
            Modes {
                format: Some(OutputFormat::Json),
                ..Modes::default()
            },
            Terminal::piped(),
            &[],
        );
        assert_eq!(shown(&cmd, &json_ctx), Shown::Json);
        let (yaml_ctx, _) = captured(
            Modes {
                format: Some(OutputFormat::Yaml),
                ..Modes::default()
            },
            Terminal::piped(),
            &[],
        );
        assert_eq!(shown(&cmd, &yaml_ctx), Shown::Yaml);
        assert_eq!(
            shown(&words(&["who", "--format", "yaml"]), &yaml_ctx),
            Shown::AsWritten
        );
    }
}
