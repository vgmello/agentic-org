use super::testing::{captured, tty, ClosedPipe, SharedBuf};
use super::*;

fn color(modes: &Modes, is_tty: bool, env: &[(&str, &str)]) -> bool {
    resolve_color(modes, is_tty, &|name| {
        env.iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| OsString::from(value))
    })
}

fn with(color: ColorMode) -> Modes {
    Modes {
        color,
        ..Modes::default()
    }
}

/// Modes, whether the stream is a terminal, the environment, and the expected decision.
type Case<'a> = (&'a Modes, bool, &'a [(&'a str, &'a str)], bool);

#[test]
fn color_matrix() {
    let json = Modes {
        json: true,
        color: ColorMode::Always,
        ..Modes::default()
    };
    let no_color_flag = Modes {
        no_color: true,
        color: ColorMode::Always,
        ..Modes::default()
    };
    let auto = with(ColorMode::Auto);
    #[rustfmt::skip]
    let cases: &[Case<'_>] = &[
        (&json, true, &[("FORCE_COLOR", "1")], false),
        (&no_color_flag, true, &[], false),
        (&with(ColorMode::Always), false, &[("NO_COLOR", "1"), ("TERM", "dumb")], true),
        (&with(ColorMode::Never), true, &[("FORCE_COLOR", "1")], false),
        (&auto, true, &[], true),
        (&auto, false, &[], false),
        (&auto, true, &[("NO_COLOR", "1")], false),
        (&auto, true, &[("NO_COLOR", "")], true),
        (&auto, true, &[("NO_COLOR", "1"), ("FORCE_COLOR", "1")], false),
        (&auto, false, &[("FORCE_COLOR", "1")], true),
        (&auto, false, &[("FORCE_COLOR", "0")], false),
        (&auto, false, &[("FORCE_COLOR", "")], false),
        (&auto, false, &[("CLICOLOR_FORCE", "1")], true),
        (&auto, false, &[("CLICOLOR_FORCE", "0")], false),
        (&auto, true, &[("TERM", "dumb")], false),
        (&auto, true, &[("TERM", "xterm-256color")], true),
        (&auto, true, &[("CLICOLOR", "0")], false),
        (&auto, true, &[("CLICOLOR", "1")], true),
        (&auto, true, &[("TERM", "dumb"), ("FORCE_COLOR", "1")], true),
    ];
    for (index, (modes, is_tty, env, expected)) in cases.iter().enumerate() {
        assert_eq!(
            color(modes, *is_tty, env),
            *expected,
            "case {index}: {env:?}"
        );
    }
}

#[test]
fn color_per_stream() {
    let terminal = Terminal {
        stdout_tty: false,
        stderr_tty: true,
        ..tty(None)
    };
    let (context, _) = captured(Modes::default(), terminal, &[]);
    assert!(!context.stdout_color());
    assert!(context.stderr_color());
}

#[test]
fn prompting_needs_terminals_and_no_opt_out() {
    let (context, _) = captured(Modes::default(), tty(Some(80)), &[]);
    assert!(context.can_prompt());
    let (context, _) = captured(Modes::default(), Terminal::piped(), &[]);
    assert!(!context.can_prompt());
    let no_stdin = Terminal {
        stdin_tty: false,
        ..tty(None)
    };
    assert!(!captured(Modes::default(), no_stdin, &[]).0.can_prompt());
    let no_stderr = Terminal {
        stderr_tty: false,
        ..tty(None)
    };
    assert!(!captured(Modes::default(), no_stderr, &[]).0.can_prompt());
    for modes in [
        Modes {
            no_input: true,
            ..Modes::default()
        },
        Modes {
            json: true,
            ..Modes::default()
        },
        Modes {
            quiet: true,
            ..Modes::default()
        },
    ] {
        assert!(!captured(modes, tty(None), &[]).0.can_prompt());
    }
}

#[test]
fn no_input_from_the_environment() {
    for (value, expected) in [("1", true), ("true", true), ("0", false), ("", false)] {
        let (context, _) = captured(Modes::default(), tty(None), &[(NO_INPUT_ENV, value)]);
        assert_eq!(context.modes().no_input, expected, "{value:?}");
        assert_eq!(context.can_prompt(), !expected);
    }
}

#[test]
fn accessors() {
    let (context, _) = captured(Modes::default(), tty(Some(120)), &[]);
    assert_eq!(context.term_width(), Some(120));
    assert!(context.is_tty_stdout() && context.is_tty_stderr());
    assert_eq!(context.terminal(), &tty(Some(120)));
    assert!(!context.modes().json);
}

#[test]
fn out_is_verbatim_and_notes_are_sanitized() {
    let (mut context, streams) = captured(Modes::default(), Terminal::piped(), &[]);
    context.out("raw\tline\n").unwrap();
    context.note("hello \x1b[2J");
    context.success("done");
    context.warn("careful\nnow");
    assert_eq!(streams.stdout.text(), "raw\tline\n");
    assert_eq!(
        streams.stderr.text(),
        "hello \\u{1b}[2J\n✓ done\n! careful\\u{a}now\n"
    );
}

#[test]
fn quiet_keeps_warnings_only() {
    let quiet = Modes {
        quiet: true,
        color: ColorMode::Never,
        ..Modes::default()
    };
    let (mut context, streams) = captured(quiet, tty(None), &[]);
    context.note("note");
    context.success("ok");
    context.warn("warning");
    assert_eq!(streams.stderr.text(), "! warning\n");
}

#[test]
fn marks_are_colored_only_with_color() {
    let (mut context, streams) = captured(Modes::default(), tty(None), &[]);
    context.success("ok");
    context.warn("warn");
    let text = streams.stderr.text();
    assert!(text.contains("\u{1b}[") && text.contains("ok") && text.contains("warn"));
    let never = with(ColorMode::Never);
    let (mut context, streams) = captured(never, tty(None), &[]);
    context.success("ok");
    assert_eq!(streams.stderr.text(), "✓ ok\n");
}

#[test]
fn closed_streams() {
    let mut context = ExecutionContext::with_streams(
        Modes::default(),
        Terminal::piped(),
        &|_| None,
        Box::new(ClosedPipe),
        Box::new(ClosedPipe),
    );
    assert!(matches!(context.out("x"), Err(ShellError::OutputClosed)));
    context.note("nobody reads this");
    context.warn("nor this");
    assert_eq!(context.report(&ShellError::Cancelled), 130);
}

#[test]
fn report_uses_the_modes() {
    let error = ShellError::InvalidArguments {
        message: "bad".into(),
    };
    let (mut context, streams) = captured(Modes::default(), Terminal::piped(), &[]);
    assert_eq!(context.report(&error), 2);
    assert_eq!(streams.stderr.text(), "× [cli::usage] bad\n");
    assert_eq!(streams.stdout.text(), "");

    let json = Modes {
        json: true,
        ..Modes::default()
    };
    let (mut context, streams) = captured(json.clone(), Terminal::piped(), &[]);
    assert_eq!(context.report(&error), 2);
    let text = streams.stderr.text();
    assert_eq!(
        text.lines().count(),
        1,
        "compact when stderr is piped: {text}"
    );
    let (mut context, streams) = captured(json, tty(None), &[]);
    context.report(&error);
    assert!(
        streams.stderr.text().lines().count() > 1,
        "pretty on a terminal"
    );

    let (mut context, streams) = captured(Modes::default(), tty(None), &[]);
    assert_eq!(context.report(&ShellError::Cancelled), 130);
    assert_eq!(
        streams.stderr.text(),
        "",
        "cancelled is silent for a person"
    );
}

#[test]
fn shared_buffers_share() {
    let buffer = SharedBuf::new();
    let mut clone = buffer.clone();
    clone.write_all("é".as_bytes()).unwrap();
    assert_eq!(buffer.text(), "é");
}
