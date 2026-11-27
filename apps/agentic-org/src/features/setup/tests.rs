//! `setup`: its help, its refusal order and its federation flags.
use org_cli_shell::context::testing::{captured, tty};
use org_cli_shell::context::Modes;
use org_cli_shell::error::CliDiagnostic;
use org_cli_shell::terminal::Terminal;

use super::run::{run_as, SetUp, SetupError, HELP};

/// The text of a refusal, whatever its exit code.
trait Message {
    fn message(&self) -> &str;
}

impl Message for SetupError {
    fn message(&self) -> &str {
        match self {
            Self::Usage(message) | Self::Failed(message) => message,
        }
    }
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

/// X5 review V-d: the help says what setup does on every system it runs on.
#[test]
fn the_help_names_no_one_system() {
    for help in [
        HELP,
        crate::cli::command()
            .render_long_help()
            .to_string()
            .as_str(),
    ] {
        assert!(!help.contains("Mac"), "{help}");
    }
    assert!(HELP.starts_with("Set up this computer's instance"));
}

/// A user who may not own a service is refused before setup reads or makes anything:
/// before its options are even parsed, so no instance is reserved, no custody is made
/// and no service is registered. Help and the satellite refusal read nothing and are
/// answered to anyone.
#[test]
fn a_user_who_may_not_run_the_service_is_refused_before_setup_reads_anything() {
    let refusal = || Err("the service does not run as this user".to_owned());
    for list in [
        vec![],
        vec!["--name", "lab"],
        vec!["--port", "7420"],
        // Refused as the user, not as a bad option: nothing was parsed yet.
        vec!["--frobnicate"],
        vec!["--port", "not-a-port"],
    ] {
        assert_eq!(
            run_as(&args(&list), refusal, false, &mut Vec::new()),
            Err(SetupError::Failed(
                "the service does not run as this user".into()
            )),
            "{list:?}"
        );
    }
    let unasked = || -> Result<(), String> { panic!("asked who runs a help query") };
    assert_eq!(
        run_as(&args(&["--help"]), unasked, false, &mut Vec::new()),
        Ok(SetUp::Help)
    );
    let unasked = || -> Result<(), String> { panic!("asked before the satellite refusal") };
    assert!(
        run_as(&args(&["--satellite"]), unasked, false, &mut Vec::new())
            .unwrap_err()
            .message()
            .contains("setup creates HQ only")
    );
    // The composition's own check is the host's rule for this process's user.
    assert_eq!(
        org_host::Config::refuse_root().is_err(),
        org_socket::effective_uid() == 0
    );
}

#[test]
fn federation_flags_are_validated_before_setup_runs() {
    let allow = || Ok(());
    let no_bind = run_as(
        &args(&["--federation-advertise", "hq.example.com"]),
        allow,
        false,
        &mut Vec::new(),
    )
    .unwrap_err();
    let no_bind = no_bind.message();
    assert!(no_bind.contains("--federation-bind"), "{no_bind}");
    let bad_bind = run_as(
        &args(&["--federation-bind", "0.0.0.0:0"]),
        allow,
        false,
        &mut Vec::new(),
    )
    .unwrap_err();
    let bad_bind = bad_bind.message();
    assert!(bad_bind.contains("1 to 65535"), "{bad_bind}");
    let bad_adv = run_as(
        &args(&[
            "--federation-bind",
            "0.0.0.0:7443",
            "--federation-advertise",
            "hq.example.com:7443",
        ]),
        allow,
        false,
        &mut Vec::new(),
    )
    .unwrap_err();
    let bad_adv = bad_adv.message();
    assert!(bad_adv.contains("without a port"), "{bad_adv}");
    for empty_adv in ["", ",,,", "büro.example.com"] {
        assert!(
            run_as(
                &args(&[
                    "--federation-bind",
                    "0.0.0.0:7443",
                    "--federation-advertise",
                    empty_adv,
                ]),
                allow,
                false,
                &mut Vec::new(),
            )
            .is_err(),
            "--federation-advertise {empty_adv:?} should be refused"
        );
    }
    let with_cfg = run_as(
        &args(&[
            "--non-interactive",
            "--config",
            "/tmp/nonexistent.json",
            "--federation-bind",
            "0.0.0.0:7443",
        ]),
        allow,
        false,
        &mut Vec::new(),
    )
    .unwrap_err();
    let with_cfg = with_cfg.message();
    assert!(with_cfg.contains("not used with --config"), "{with_cfg}");
}

/// NB-02: a command line setup cannot use is a usage error (exit 2), decided before anyone
/// is asked who runs it; a refusal of the user stays exit 1.
#[test]
fn a_command_line_setup_cannot_use_is_exit_2() {
    let allow = || Ok(());
    for list in [
        &["--bogus"][..],
        &["--port", "0"],
        &["--port", "70000"],
        &["--port"],
        &["--name", "a", "--name", "b"],
        &["--non-interactive"],
        &["--config", "/tmp/x.json"],
        &[
            "--non-interactive",
            "--config",
            "/tmp/x.json",
            "--port",
            "7420",
        ],
        &["--federation-advertise", "hq.example.com"],
        &["--federation-bind", "0.0.0.0:0"],
        &["--satellite"],
    ] {
        let error = run_as(&args(list), allow, false, &mut Vec::new()).unwrap_err();
        assert!(matches!(error, SetupError::Usage(_)), "{list:?}: {error:?}");
        assert_eq!(
            crate::error::CliError::from(error).exit_code(),
            2,
            "{list:?}"
        );
    }
    let root = || Err("the service does not run as this user".to_owned());
    let refused = run_as(&args(&["--name", "lab"]), root, false, &mut Vec::new()).unwrap_err();
    assert_eq!(crate::error::CliError::from(refused).exit_code(), 1);
}

/// NB-02: `--json`, `-q`/`--quiet` and `--dry-run` written after setup's own words (where
/// the global parser does not read them) are refused too, before anything else is read.
#[test]
fn global_flags_after_setups_words_are_refused_not_ignored() {
    for (list, flag) in [
        (&["--name", "lab", "--json"][..], "--json"),
        (&["--name", "lab", "-q"], "--quiet"),
        (&["--port", "7421", "--quiet"], "--quiet"),
        (&["--name", "lab", "--format", "json"], "--format"),
        (&["--name", "lab", "--format=yaml"], "--format"),
        (&["--name", "lab", "--dry-run"], "--dry-run"),
        (
            &["--non-interactive", "--config", "/tmp/x.json", "--json"],
            "--json",
        ),
    ] {
        let unasked = || -> Result<(), String> { panic!("asked who runs a refused line") };
        let error = run_as(&args(list), unasked, false, &mut Vec::new()).unwrap_err();
        assert_eq!(
            error,
            SetupError::Usage(format!("`setup` does not take {flag}")),
            "{list:?}"
        );
        let error = crate::error::CliError::from(error);
        assert_eq!(error.exit_code(), 2);
        assert_eq!(error.help().as_deref(), Some("agentic-org setup --help"));
    }
}

/// B-03: setup asks for its port only when someone can be asked: never under `--no-input`,
/// `AGENTIC_ORG_NO_INPUT=1` or `--no-input` among its own words, even with every stream a
/// terminal, and never without a terminal on standard input and standard error.
#[test]
fn no_input_never_prompts_even_on_a_terminal() {
    let none: [String; 0] = [];
    let (ctx, _) = captured(Modes::default(), tty(Some(80)), &[]);
    assert!(super::may_ask(&ctx, &none), "a terminal is asked");
    let no_input = Modes {
        no_input: true,
        ..Modes::default()
    };
    let (ctx, _) = captured(no_input, tty(Some(80)), &[]);
    assert!(!super::may_ask(&ctx, &none), "--no-input");
    for value in ["1", "true"] {
        let (ctx, _) = captured(
            Modes::default(),
            tty(Some(80)),
            &[(org_cli_shell::context::NO_INPUT_ENV, value)],
        );
        assert!(!super::may_ask(&ctx, &none), "AGENTIC_ORG_NO_INPUT={value}");
    }
    let (ctx, _) = captured(Modes::default(), tty(Some(80)), &[]);
    assert!(!super::may_ask(
        &ctx,
        &args(&["--name", "lab", "--no-input"])
    ));
    let (ctx, _) = captured(Modes::default(), Terminal::piped(), &[]);
    assert!(!super::may_ask(&ctx, &none), "no terminal");
    // `--no-input` among setup's words is the global flag, not an unknown argument: the
    // line is read past it (to the bad port here).
    let allow = || Ok(());
    let error = run_as(
        &args(&["--no-input", "--port", "0"]),
        allow,
        false,
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(error.message().contains("--port must be"), "{error:?}");
}
