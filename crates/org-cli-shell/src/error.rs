//! Diagnostics: what every command error tells the person or the script, the exit codes, the
//! shell's own errors and the one renderer.
//!
//! Under `--json` a diagnostic is one JSON envelope on standard error,
//! `{"error":{"code":..,"message":..,"help":..,"exit_code":N}}`; otherwise it is
//! `× [code] message` with an optional `  help: ...` line, every text sanitized.

use std::borrow::Cow;
use std::fmt::{self, Write as _};
use std::io::{self, Write};

use owo_colors::OwoColorize;
use serde::Serialize;

use crate::escape::{escape_json_controls, sanitize};

/// Success, and a closed standard output (the reader went away; nothing is wrong).
pub const EXIT_OK: u8 = 0;
/// A domain, runtime, configuration or I/O failure.
pub const EXIT_FAILURE: u8 = 1;
/// A usage error: a bad or missing argument, or a confirmation `--yes` would have given.
pub const EXIT_USAGE: u8 = 2;
/// Not signed in, or not allowed.
pub const EXIT_AUTH: u8 = 4;
/// Cancelled: Ctrl+C, Esc, or a declined confirmation (128 + SIGINT).
pub const EXIT_CANCELLED: u8 = 130;

/// An error a command can report. The consumer's own error enums implement it next to
/// [`ShellError`]; [`render_diagnostic`] and `ExecutionContext::report` print it.
pub trait CliDiagnostic: fmt::Display {
    /// A stable machine-readable code, `area::kind` (e.g. `cli::usage`).
    fn code(&self) -> Cow<'static, str>;
    /// What to do about it, if there is something to say.
    fn help(&self) -> Option<String> {
        None
    }
    /// The process exit code.
    fn exit_code(&self) -> u8;
    /// True when nothing is printed for it (`json` is whether `--json` is on).
    fn silent(&self, _json: bool) -> bool {
        false
    }
}

/// The help of [`ShellError::ConfirmationRequired`].
pub const CONFIRMATION_HELP: &str =
    "Re-run with --yes to confirm, or run it in an interactive terminal.";

/// The shell's own errors.
#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    /// Values a non-interactive run must give as flags. Exit 2.
    #[error("missing required {}: {}", flag_noun(.flags.len()), .flags.join(", "))]
    MissingRequiredFlags { flags: Vec<String> },
    /// A bad argument value or combination. Exit 2.
    #[error("{message}")]
    InvalidArguments { message: String },
    /// A destructive action without `--yes` where no one can be asked. Exit 2.
    #[error("{action}: confirmation required")]
    ConfirmationRequired { action: String },
    /// Not signed in or not allowed. Exit 4.
    #[error("{message}")]
    AuthRequired {
        message: String,
        help: Option<String>,
    },
    /// A bad environment value, directory or `agentic-org.toml`. Exit 1.
    #[error("{message}")]
    InvalidConfig {
        message: String,
        help: Option<String>,
    },
    /// Ctrl+C, Esc or a declined confirmation. Exit 130; printed only under `--json`.
    #[error("cancelled")]
    Cancelled,
    /// Standard output was closed by its reader (`| head`). Exit 0; never printed.
    #[error("standard output was closed")]
    OutputClosed,
    /// Another I/O failure. Exit 1.
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: io::Error,
    },
}

fn flag_noun(count: usize) -> &'static str {
    if count == 1 {
        "flag"
    } else {
        "flags"
    }
}

impl ShellError {
    /// An [`ShellError::Io`] with what was being done.
    pub fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }
}

impl CliDiagnostic for ShellError {
    fn code(&self) -> Cow<'static, str> {
        Cow::Borrowed(match self {
            Self::MissingRequiredFlags { .. } => "cli::usage::missing_flags",
            Self::InvalidArguments { .. } => "cli::usage",
            Self::ConfirmationRequired { .. } => "cli::confirmation_required",
            Self::AuthRequired { .. } => "cli::auth::required",
            Self::InvalidConfig { .. } => "cli::config::invalid",
            Self::Cancelled => "cli::cancelled",
            Self::OutputClosed => "cli::output_closed",
            Self::Io { .. } => "cli::io",
        })
    }

    fn help(&self) -> Option<String> {
        match self {
            Self::MissingRequiredFlags { .. } => Some(
                "Pass them on the command line; prompting for them needs an interactive \
                 terminal and no --no-input, --json or --quiet."
                    .to_owned(),
            ),
            Self::ConfirmationRequired { .. } => Some(CONFIRMATION_HELP.to_owned()),
            Self::AuthRequired { help, .. } | Self::InvalidConfig { help, .. } => help.clone(),
            Self::InvalidArguments { .. }
            | Self::Cancelled
            | Self::OutputClosed
            | Self::Io { .. } => None,
        }
    }

    fn exit_code(&self) -> u8 {
        match self {
            Self::MissingRequiredFlags { .. }
            | Self::InvalidArguments { .. }
            | Self::ConfirmationRequired { .. } => EXIT_USAGE,
            Self::AuthRequired { .. } => EXIT_AUTH,
            Self::InvalidConfig { .. } | Self::Io { .. } => EXIT_FAILURE,
            Self::Cancelled => EXIT_CANCELLED,
            Self::OutputClosed => EXIT_OK,
        }
    }

    fn silent(&self, json: bool) -> bool {
        match self {
            Self::OutputClosed => true,
            Self::Cancelled => !json,
            _ => false,
        }
    }
}

#[derive(Serialize)]
struct Envelope<'a> {
    error: Body<'a>,
}

/// Field order is the order of the envelope.
#[derive(Serialize)]
struct Body<'a> {
    code: &'a str,
    message: &'a str,
    help: Option<&'a str>,
    exit_code: u8,
}

/// Writes `diag` to `err` unless it is [silent](CliDiagnostic::silent): the JSON envelope when
/// `json` (indented when `pretty`, one line otherwise), else the sanitized human form with a
/// red `×` when `color`. Best effort: a write failure is ignored, and it never panics.
pub fn render_diagnostic(
    diag: &dyn CliDiagnostic,
    json: bool,
    pretty: bool,
    color: bool,
    err: &mut dyn Write,
) {
    if diag.silent(json) {
        return;
    }
    let text = if json {
        json_text(diag, pretty)
    } else {
        human_text(diag, color)
    };
    let _ = err.write_all(text.as_bytes());
    let _ = err.flush();
}

/// The message, without the panic `to_string` raises when a `Display` implementation fails.
fn message_of(diag: &dyn CliDiagnostic) -> String {
    let mut message = String::new();
    let _ = write!(message, "{diag}");
    message
}

fn json_text(diag: &dyn CliDiagnostic, pretty: bool) -> String {
    let code = diag.code();
    let message = message_of(diag);
    let help = diag.help();
    let envelope = Envelope {
        error: Body {
            code: &code,
            message: &message,
            help: help.as_deref(),
            exit_code: diag.exit_code(),
        },
    };
    let encoded = if pretty {
        serde_json::to_string_pretty(&envelope)
    } else {
        serde_json::to_string(&envelope)
    };
    // Strings and a number always serialize; the fallback only keeps this panic-free.
    let mut text = escape_json_controls(encoded.unwrap_or_else(|_| {
        format!(
            "{{\"error\":{{\"code\":\"cli::internal\",\"message\":\"the error could not be encoded\",\"help\":null,\"exit_code\":{}}}}}",
            diag.exit_code()
        )
    }));
    text.push('\n');
    text
}

fn human_text(diag: &dyn CliDiagnostic, color: bool) -> String {
    let code = diag.code();
    let message = message_of(diag);
    let mut text = if color {
        format!("{} ", "×".red().bold())
    } else {
        String::from("× ")
    };
    text.push_str(&format!("[{}] {}\n", sanitize(&code), sanitize(&message)));
    if let Some(help) = diag.help() {
        let label = if color {
            "help:".cyan().to_string()
        } else {
            String::from("help:")
        };
        text.push_str(&format!("  {label} {}\n", sanitize(&help)));
    }
    text
}

#[cfg(test)]
mod tests;
