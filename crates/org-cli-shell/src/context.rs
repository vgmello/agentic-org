//! The modes of one run and the [`ExecutionContext`] that owns its standard streams.

use std::ffi::OsString;
use std::io::{self, Write};

use owo_colors::OwoColorize;

use crate::error::{render_diagnostic, CliDiagnostic, ShellError};
use crate::escape::sanitize;
use crate::terminal::{write_out, Terminal};

mod prescan;
pub mod testing;

pub use prescan::{prescan, Prescan};

/// A non-empty value other than `0` forces `--no-input` (CI, agents, wrappers).
pub const NO_INPUT_ENV: &str = "AGENTIC_ORG_NO_INPUT";

/// `--color <auto|always|never>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ColorMode {
    #[default]
    Auto,
    Always,
    Never,
}

impl ColorMode {
    /// The value of `--color`, or `None` for anything else.
    pub fn from_flag(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(Self::Auto),
            "always" => Some(Self::Always),
            "never" => Some(Self::Never),
            _ => None,
        }
    }
}

/// The global flags of one run.
#[derive(Clone, Debug, Default)]
pub struct Modes {
    /// `--json`: machine output on stdout, the error envelope on stderr, no color, no prompts.
    pub json: bool,
    /// `-q`/`--quiet`: IDs only on stdout, no notes.
    pub quiet: bool,
    /// `--dry-run`: preview, mutate nothing.
    pub dry_run: bool,
    /// `--no-input`: never prompt.
    pub no_input: bool,
    /// `--yes`: confirm destructive actions.
    pub yes: bool,
    pub color: ColorMode,
    /// `--no-color`.
    pub no_color: bool,
}

fn set(env: &dyn Fn(&str) -> Option<OsString>, name: &str) -> Option<OsString> {
    env(name).filter(|value| !value.is_empty())
}

fn forced(env: &dyn Fn(&str) -> Option<OsString>, name: &str) -> bool {
    set(env, name).is_some_and(|value| value != "0")
}

/// Whether a stream gets ANSI color. `--json` and `--no-color` turn it off, `--color always`
/// / `never` decide; under `auto` a non-empty `NO_COLOR` turns it off, a non-empty
/// `FORCE_COLOR` or `CLICOLOR_FORCE` other than `0` turns it on, and otherwise the stream must
/// be a terminal, `TERM` not `dumb` and `CLICOLOR` not `0`.
pub fn resolve_color(modes: &Modes, is_tty: bool, env: &dyn Fn(&str) -> Option<OsString>) -> bool {
    if modes.json || modes.no_color {
        return false;
    }
    match modes.color {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => {
            if set(env, "NO_COLOR").is_some() {
                false
            } else if forced(env, "FORCE_COLOR") || forced(env, "CLICOLOR_FORCE") {
                true
            } else {
                is_tty
                    && env("TERM").is_none_or(|term| term != "dumb")
                    && env("CLICOLOR").is_none_or(|clicolor| clicolor != "0")
            }
        }
    }
}

/// One run: its modes, its terminal, the color of each stream, and the streams themselves.
/// Everything a command prints goes through it.
pub struct ExecutionContext {
    modes: Modes,
    terminal: Terminal,
    stdout_color: bool,
    stderr_color: bool,
    out: Box<dyn Write>,
    err: Box<dyn Write>,
}

impl ExecutionContext {
    /// The process's environment, terminal and standard streams.
    pub fn detect(modes: Modes) -> Self {
        Self::with_streams(
            modes,
            Terminal::detect(),
            &|name| std::env::var_os(name),
            Box::new(io::stdout()),
            Box::new(io::stderr()),
        )
    }

    /// A context over given streams; `env` reads `AGENTIC_ORG_NO_INPUT` and the color
    /// variables. See [`testing::captured`] for tests.
    pub fn with_streams(
        mut modes: Modes,
        terminal: Terminal,
        env: &dyn Fn(&str) -> Option<OsString>,
        out: Box<dyn Write>,
        err: Box<dyn Write>,
    ) -> Self {
        if forced(env, NO_INPUT_ENV) {
            modes.no_input = true;
        }
        let stdout_color = resolve_color(&modes, terminal.stdout_tty, env);
        let stderr_color = resolve_color(&modes, terminal.stderr_tty, env);
        Self {
            modes,
            terminal,
            stdout_color,
            stderr_color,
            out,
            err,
        }
    }

    pub fn modes(&self) -> &Modes {
        &self.modes
    }

    pub fn terminal(&self) -> &Terminal {
        &self.terminal
    }

    pub fn is_tty_stdout(&self) -> bool {
        self.terminal.stdout_tty
    }

    pub fn is_tty_stderr(&self) -> bool {
        self.terminal.stderr_tty
    }

    pub fn stdout_color(&self) -> bool {
        self.stdout_color
    }

    pub fn stderr_color(&self) -> bool {
        self.stderr_color
    }

    /// True when someone can be asked: standard input and standard error are terminals and
    /// none of `--no-input` (or its variable), `--json` or `--quiet` is on.
    pub fn can_prompt(&self) -> bool {
        self.terminal.stdin_tty
            && self.terminal.stderr_tty
            && !self.modes.no_input
            && !self.modes.json
            && !self.modes.quiet
    }

    pub fn term_width(&self) -> Option<usize> {
        self.terminal.width
    }

    /// Writes `text` to standard output exactly as given (no newline added, not sanitized)
    /// and flushes. A closed reader is [`ShellError::OutputClosed`].
    pub fn out(&mut self, text: &str) -> Result<(), ShellError> {
        write_out(&mut *self.out, text.as_bytes())
    }

    /// Writes `text` to standard error as given; failures are ignored.
    pub(crate) fn err_raw(&mut self, text: &str) {
        let _ = self.err.write_all(text.as_bytes());
        let _ = self.err.flush();
    }

    /// Whether prose for a person (notes, success marks) stays off standard error: under
    /// `--quiet`, and under `--json`, where standard error carries only the error envelope.
    fn prose_off(&self) -> bool {
        self.modes.quiet || self.modes.json
    }

    /// One sanitized line on standard error, unless `--quiet` or `--json`. Best effort.
    pub fn note(&mut self, text: &str) {
        if self.prose_off() {
            return;
        }
        self.err_raw(&format!("{}\n", sanitize(text)));
    }

    /// `✓ text` on standard error (a green mark with color), unless `--quiet` or `--json`.
    /// Best effort.
    pub fn success(&mut self, text: &str) {
        if self.prose_off() {
            return;
        }
        let mark = if self.stderr_color {
            "✓".green().bold().to_string()
        } else {
            String::from("✓")
        };
        self.err_raw(&format!("{mark} {}\n", sanitize(text)));
    }

    /// `! text` on standard error (a yellow mark with color), also under `--quiet`. Best effort.
    pub fn warn(&mut self, text: &str) {
        let mark = if self.stderr_color {
            "!".yellow().bold().to_string()
        } else {
            String::from("!")
        };
        self.err_raw(&format!("{mark} {}\n", sanitize(text)));
    }

    /// Prints `diag` on standard error ([`render_diagnostic`] with `--json`, indented when
    /// standard error is a terminal, colored per standard error) and returns its exit code.
    pub fn report(&mut self, diag: &dyn CliDiagnostic) -> u8 {
        render_diagnostic(
            diag,
            self.modes.json,
            self.terminal.stderr_tty,
            self.stderr_color,
            &mut *self.err,
        );
        diag.exit_code()
    }
}

#[cfg(test)]
mod tests;
