use super::*;

fn rendered(diag: &dyn CliDiagnostic, json: bool, pretty: bool, color: bool) -> String {
    let mut buffer = Vec::new();
    render_diagnostic(diag, json, pretty, color, &mut buffer);
    String::from_utf8(buffer).unwrap()
}

fn all() -> Vec<ShellError> {
    vec![
        ShellError::MissingRequiredFlags {
            flags: vec!["--name".into(), "--office".into()],
        },
        ShellError::InvalidArguments {
            message: "bad".into(),
        },
        ShellError::ConfirmationRequired {
            action: "remove office hq".into(),
        },
        ShellError::AuthRequired {
            message: "sign in".into(),
            help: Some("Run `agentic-org login`.".into()),
        },
        ShellError::InvalidConfig {
            message: "bad config".into(),
            help: None,
        },
        ShellError::Cancelled,
        ShellError::OutputClosed,
        ShellError::io("reading x", io::Error::other("boom")),
    ]
}

#[test]
fn codes_and_exit_codes() {
    let table: Vec<(String, u8)> = all()
        .iter()
        .map(|e| (e.code().into_owned(), e.exit_code()))
        .collect();
    let expected = [
        ("cli::usage::missing_flags", 2),
        ("cli::usage", 2),
        ("cli::confirmation_required", 2),
        ("cli::auth::required", 4),
        ("cli::config::invalid", 1),
        ("cli::cancelled", 130),
        ("cli::output_closed", 0),
        ("cli::io", 1),
    ];
    let expected: Vec<(String, u8)> = expected
        .iter()
        .map(|(c, n)| ((*c).to_owned(), *n))
        .collect();
    assert_eq!(table, expected);
}

#[test]
fn messages_and_help() {
    let errors = all();
    assert_eq!(
        errors[0].to_string(),
        "missing required flags: --name, --office"
    );
    let one = ShellError::MissingRequiredFlags {
        flags: vec!["--name".into()],
    };
    assert_eq!(one.to_string(), "missing required flag: --name");
    assert_eq!(
        errors[2].to_string(),
        "remove office hq: confirmation required"
    );
    assert_eq!(errors[2].help().as_deref(), Some(CONFIRMATION_HELP));
    assert_eq!(
        errors[3].help().as_deref(),
        Some("Run `agentic-org login`.")
    );
    assert_eq!(errors[4].help(), None);
    assert_eq!(errors[7].to_string(), "reading x: boom");
    assert!(std::error::Error::source(&errors[7]).is_some());
}

#[test]
fn silence() {
    assert!(ShellError::OutputClosed.silent(false));
    assert!(ShellError::OutputClosed.silent(true));
    assert!(ShellError::Cancelled.silent(false));
    assert!(!ShellError::Cancelled.silent(true));
    assert!(!ShellError::InvalidArguments {
        message: "x".into()
    }
    .silent(false));
    assert_eq!(rendered(&ShellError::OutputClosed, true, false, false), "");
    assert_eq!(rendered(&ShellError::Cancelled, false, false, true), "");
    let cancelled = rendered(&ShellError::Cancelled, true, false, false);
    assert_eq!(
        cancelled,
        "{\"error\":{\"code\":\"cli::cancelled\",\"message\":\"cancelled\",\"help\":null,\"exit_code\":130}}\n"
    );
}

#[test]
fn json_envelope_compact_and_pretty() {
    let error = ShellError::ConfirmationRequired {
        action: "remove office hq".into(),
    };
    let compact = rendered(&error, true, false, true);
    assert!(
        compact.ends_with("}\n") && compact.matches('\n').count() == 1,
        "{compact}"
    );
    assert!(!compact.contains('\u{1b}'), "no color in JSON: {compact}");
    let value: serde_json::Value = serde_json::from_str(&compact).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"error": {
            "code": "cli::confirmation_required",
            "message": "remove office hq: confirmation required",
            "help": CONFIRMATION_HELP,
            "exit_code": 2,
        }})
    );
    // Key order is the envelope's, not alphabetical.
    let positions: Vec<usize> = ["\"code\"", "\"message\"", "\"help\"", "\"exit_code\""]
        .iter()
        .map(|key| compact.find(key).unwrap())
        .collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "{compact}");
    let pretty = rendered(&error, true, true, false);
    assert!(pretty.lines().count() > 3, "{pretty}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&pretty).unwrap(),
        value
    );
}

#[test]
fn json_escapes_c1_and_separators() {
    let error = ShellError::InvalidArguments {
        message: "a\u{9b}31m\u{2028}b".into(),
    };
    let text = rendered(&error, true, false, false);
    assert!(text.contains("\\u009b31m\\u2028b"), "{text}");
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["error"]["message"], "a\u{9b}31m\u{2028}b");
}

#[test]
fn human_form_plain_and_colored() {
    let error = ShellError::AuthRequired {
        message: "not signed in".into(),
        help: Some("Run login.".into()),
    };
    assert_eq!(
        rendered(&error, false, false, false),
        "× [cli::auth::required] not signed in\n  help: Run login.\n"
    );
    let colored = rendered(&error, false, false, true);
    assert!(colored.contains("\u{1b}["), "{colored}");
    assert!(colored.contains("[cli::auth::required] not signed in"));
    let no_help = ShellError::InvalidArguments {
        message: "bad".into(),
    };
    assert_eq!(
        rendered(&no_help, false, false, false),
        "× [cli::usage] bad\n"
    );
}

#[test]
fn human_form_is_sanitized() {
    struct Hostile;
    impl fmt::Display for Hostile {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("name \x1b]0;pwned\x07 \u{202e}x\ny")
        }
    }
    impl CliDiagnostic for Hostile {
        fn code(&self) -> Cow<'static, str> {
            Cow::Borrowed("x::\x1b[2J")
        }
        fn help(&self) -> Option<String> {
            Some("\u{9b}0m".into())
        }
        fn exit_code(&self) -> u8 {
            EXIT_FAILURE
        }
    }
    let text = rendered(&Hostile, false, false, false);
    assert!(!text.contains('\u{1b}') && !text.contains('\u{9b}') && !text.contains('\u{202e}'));
    assert_eq!(text.lines().count(), 2, "only the help adds a line: {text}");
    assert!(text.contains("\\u{1b}]0;pwned\\u{7}"), "{text}");
}

#[test]
fn rendering_ignores_a_closed_stream() {
    struct Closed;
    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
    }
    render_diagnostic(&ShellError::Cancelled, true, true, true, &mut Closed);
}

#[test]
fn a_failing_display_does_not_panic() {
    struct Broken;
    impl fmt::Display for Broken {
        fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
            Err(fmt::Error)
        }
    }
    impl CliDiagnostic for Broken {
        fn code(&self) -> Cow<'static, str> {
            Cow::Borrowed("x::broken")
        }
        fn exit_code(&self) -> u8 {
            EXIT_FAILURE
        }
    }
    assert_eq!(rendered(&Broken, false, false, false), "× [x::broken] \n");
    assert!(rendered(&Broken, true, false, false).contains("\"code\":\"x::broken\""));
}
