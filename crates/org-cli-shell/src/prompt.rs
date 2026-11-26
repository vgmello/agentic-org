//! Asking the person at the terminal: confirmations and values. The only module that uses
//! `cliclack` (and `console` for its color switch). Prompts are drawn on standard error,
//! never standard output; Ctrl+C or Esc in a prompt is [`ShellError::Cancelled`] (exit 130).

use std::io;

use crate::context::{ExecutionContext, Modes};
use crate::error::ShellError;
use crate::escape::sanitize;

/// Gives the prompt library the context's color decision for each stream.
pub fn sync_colors(ctx: &ExecutionContext) {
    console::set_colors_enabled(ctx.stdout_color());
    console::set_colors_enabled_stderr(ctx.stderr_color());
}

/// What to do about a destructive action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Decision {
    /// `--yes`, or `--dry-run` (which previews and mutates nothing).
    Proceed,
    /// No one can be asked: [`ShellError::ConfirmationRequired`] (exit 2).
    Refuse,
    /// Ask on the terminal.
    Ask,
}

fn decide(modes: &Modes, can_prompt: bool) -> Decision {
    if modes.yes || modes.dry_run {
        Decision::Proceed
    } else if can_prompt {
        Decision::Ask
    } else {
        Decision::Refuse
    }
}

/// A prompt's failure: Ctrl+C / Esc (`Interrupted`) is a cancellation.
fn prompt_error(error: io::Error) -> ShellError {
    if error.kind() == io::ErrorKind::Interrupted {
        ShellError::Cancelled
    } else {
        ShellError::io("reading the answer from the terminal", error)
    }
}

/// Asks a yes/no question. Without a [promptable](ExecutionContext::can_prompt) terminal it
/// is [`ShellError::ConfirmationRequired`] naming `message`.
pub fn confirm(
    ctx: &mut ExecutionContext,
    message: &str,
    default: bool,
) -> Result<bool, ShellError> {
    if !ctx.can_prompt() {
        return Err(ShellError::ConfirmationRequired {
            action: message.to_owned(),
        });
    }
    sync_colors(ctx);
    cliclack::confirm(sanitize(message))
        .initial_value(default)
        .interact()
        .map_err(prompt_error)
}

/// Guards a destructive `action` (an imperative phrase, e.g. `remove office hq`): proceeds on
/// `--yes` or `--dry-run`; without a promptable terminal it is
/// [`ShellError::ConfirmationRequired`] (exit 2); otherwise it asks, defaulting to no, and a
/// "no" is [`ShellError::Cancelled`] (exit 130).
pub fn require_confirmation(ctx: &mut ExecutionContext, action: &str) -> Result<(), ShellError> {
    match decide(ctx.modes(), ctx.can_prompt()) {
        Decision::Proceed => Ok(()),
        Decision::Refuse => Err(ShellError::ConfirmationRequired {
            action: action.to_owned(),
        }),
        Decision::Ask => {
            if confirm(ctx, &format!("{action}?"), false)? {
                Ok(())
            } else {
                Err(ShellError::Cancelled)
            }
        }
    }
}

/// Asks for a value (`message` names it, e.g. `Office name`), offering `default` when the
/// answer is empty. Without a promptable terminal it is [`ShellError::InvalidArguments`]
/// naming the missing value.
pub fn input(
    ctx: &mut ExecutionContext,
    message: &str,
    default: Option<&str>,
) -> Result<String, ShellError> {
    if !ctx.can_prompt() {
        return Err(missing_value(message));
    }
    sync_colors(ctx);
    let mut prompt = cliclack::input(sanitize(message));
    if let Some(default) = default {
        prompt = prompt.default_input(default);
    }
    prompt.interact::<String>().map_err(prompt_error)
}

fn missing_value(message: &str) -> ShellError {
    ShellError::InvalidArguments {
        message: format!(
            "missing value for \"{message}\": pass it as a flag (there is no interactive terminal to ask)"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::testing::{captured, tty};
    use crate::error::{CliDiagnostic, EXIT_CANCELLED, EXIT_USAGE};
    use crate::terminal::Terminal;

    fn modes(yes: bool, dry_run: bool) -> Modes {
        Modes {
            yes,
            dry_run,
            ..Modes::default()
        }
    }

    #[test]
    fn decisions() {
        assert_eq!(decide(&modes(true, false), false), Decision::Proceed);
        assert_eq!(decide(&modes(true, false), true), Decision::Proceed);
        assert_eq!(decide(&modes(false, true), false), Decision::Proceed);
        assert_eq!(decide(&modes(false, false), false), Decision::Refuse);
        assert_eq!(decide(&modes(false, false), true), Decision::Ask);
    }

    #[test]
    fn yes_and_dry_run_proceed_without_a_terminal() {
        for modes in [modes(true, false), modes(false, true)] {
            let (mut context, streams) = captured(modes, Terminal::piped(), &[]);
            require_confirmation(&mut context, "remove office hq").unwrap();
            assert_eq!(streams.stdout.text(), "");
            assert_eq!(streams.stderr.text(), "");
        }
    }

    #[test]
    fn no_terminal_needs_yes() {
        let piped = captured(Modes::default(), Terminal::piped(), &[]);
        let no_input = captured(
            Modes {
                no_input: true,
                ..Modes::default()
            },
            tty(None),
            &[],
        );
        let json = captured(
            Modes {
                json: true,
                ..Modes::default()
            },
            tty(None),
            &[],
        );
        let env = captured(
            Modes::default(),
            tty(None),
            &[("AGENTIC_ORG_NO_INPUT", "1")],
        );
        for (mut context, streams) in [piped, no_input, json, env] {
            let error = require_confirmation(&mut context, "remove office hq").unwrap_err();
            match &error {
                ShellError::ConfirmationRequired { action } => {
                    assert_eq!(action, "remove office hq")
                }
                other => panic!("{other:?}"),
            }
            assert_eq!(error.exit_code(), EXIT_USAGE);
            assert_eq!(streams.stdout.text(), "", "a prompt never touches stdout");
        }
    }

    #[test]
    fn confirm_and_input_refuse_without_a_terminal() {
        let (mut context, _) = captured(Modes::default(), Terminal::piped(), &[]);
        assert!(matches!(
            confirm(&mut context, "Continue", true),
            Err(ShellError::ConfirmationRequired { .. })
        ));
        let error = input(&mut context, "Office name", Some("hq")).unwrap_err();
        assert!(matches!(error, ShellError::InvalidArguments { .. }));
        assert!(error.to_string().contains("\"Office name\""), "{error}");
        assert_eq!(error.exit_code(), EXIT_USAGE);
    }

    #[test]
    fn interrupted_prompts_are_cancelled() {
        let cancelled = prompt_error(io::ErrorKind::Interrupted.into());
        assert!(matches!(cancelled, ShellError::Cancelled));
        assert_eq!(cancelled.exit_code(), EXIT_CANCELLED);
        assert!(matches!(
            prompt_error(io::ErrorKind::UnexpectedEof.into()),
            ShellError::Io { .. }
        ));
    }

    #[test]
    fn colors_follow_the_context() {
        let (context, _) = captured(Modes::default(), Terminal::piped(), &[]);
        sync_colors(&context);
        assert!(!console::colors_enabled());
        assert!(!console::colors_enabled_stderr());
    }
}
