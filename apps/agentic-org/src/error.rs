//! The one error every command returns, and its exit code: `0` (a closed standard output),
//! `1` (a refusal or failure), `2` (a command line that cannot be used, a missing `--yes`),
//! `4` (not signed in or not allowed) and `130` (cancelled). `main` reports it through
//! [`org_cli_shell::context::ExecutionContext::report`]: a JSON envelope on standard error
//! under `--json`, a sanitized `× [code] message` otherwise.

use std::borrow::Cow;

use org_cli_shell::error::{
    CliDiagnostic, ShellError, EXIT_AUTH, EXIT_CANCELLED, EXIT_FAILURE, EXIT_USAGE,
};
use org_host_setup::{HostCliError, HostCliErrorKind};

#[derive(Debug, thiserror::Error)]
pub(crate) enum CliError {
    /// The shell's own: output closed, confirmation required, configuration, I/O.
    #[error(transparent)]
    Shell(#[from] ShellError),
    /// The host's answer, classified by the host.
    #[error("{0}")]
    Host(HostCliError),
    /// A command line that cannot be used. Exit 2.
    #[error("{message}")]
    Usage {
        message: String,
        help: Option<String>,
    },
    /// A failure with its own code. Exit 1.
    #[error("{message}")]
    Failed {
        code: &'static str,
        message: String,
        help: Option<String>,
    },
    /// An office command's refusal, shown on a terminal: its code (`office::<wire code>`),
    /// message and hint, and the command table's exit code (1, or 2 for a command line it
    /// could not read).
    #[error("{message}")]
    Office {
        code: String,
        message: String,
        hint: Option<String>,
        exit: u8,
    },
    /// A delegated office command printed its own answer (its envelope, or the rendering of
    /// it); only its exit code is left.
    #[error("the command exited with status {0}")]
    Exited(u8),
}

impl CliError {
    pub(crate) fn usage(message: impl Into<String>) -> Self {
        Self::Usage {
            message: message.into(),
            help: None,
        }
    }

    pub(crate) fn failed(code: &'static str, message: impl Into<String>) -> Self {
        Self::Failed {
            code,
            message: message.into(),
            help: None,
        }
    }

    pub(crate) fn with_help(self, text: impl Into<String>) -> Self {
        match self {
            Self::Usage { message, .. } => Self::Usage {
                message,
                help: Some(text.into()),
            },
            Self::Failed { code, message, .. } => Self::Failed {
                code,
                message,
                help: Some(text.into()),
            },
            other => other,
        }
    }
}

impl From<HostCliError> for CliError {
    fn from(error: HostCliError) -> Self {
        // A host operation that wrote to a standard output whose reader went away.
        if error.is_output_closed() {
            return Self::Shell(ShellError::OutputClosed);
        }
        Self::Host(error)
    }
}

/// A host operation that still answers a sentence: a runtime failure (exit 1).
impl From<String> for CliError {
    fn from(message: String) -> Self {
        Self::Host(HostCliError::runtime(message))
    }
}

/// The wire spelling of a code (`not_authorized`), never its Rust name.
fn wire_name(code: org_proto::ErrorCode) -> String {
    org_command::code_name(code)
}

fn host_code(error: &HostCliError) -> Cow<'static, str> {
    match (error.kind, error.code) {
        (HostCliErrorKind::Auth, _) => Cow::Borrowed("cli::auth::required"),
        (HostCliErrorKind::Usage, _) => Cow::Borrowed("cli::usage"),
        (HostCliErrorKind::Cancelled, _) => Cow::Borrowed("cli::cancelled"),
        (HostCliErrorKind::Refused, Some(code)) => Cow::Owned(format!("host::{}", wire_name(code))),
        (HostCliErrorKind::Refused, None) => Cow::Borrowed("host::refused"),
        (HostCliErrorKind::Runtime, _) => Cow::Borrowed("host::error"),
    }
}

fn host_exit(kind: HostCliErrorKind) -> u8 {
    match kind {
        HostCliErrorKind::Auth => EXIT_AUTH,
        HostCliErrorKind::Usage => EXIT_USAGE,
        HostCliErrorKind::Cancelled => EXIT_CANCELLED,
        HostCliErrorKind::Refused | HostCliErrorKind::Runtime => EXIT_FAILURE,
    }
}

impl CliDiagnostic for CliError {
    fn code(&self) -> Cow<'static, str> {
        match self {
            Self::Shell(error) => error.code(),
            Self::Host(error) => host_code(error),
            Self::Usage { .. } => Cow::Borrowed("cli::usage"),
            Self::Failed { code, .. } => Cow::Borrowed(code),
            Self::Office { code, .. } => Cow::Owned(code.clone()),
            Self::Exited(_) => Cow::Borrowed("cli::delegated"),
        }
    }

    fn help(&self) -> Option<String> {
        match self {
            Self::Shell(error) => error.help(),
            Self::Host(error) => error.help.clone(),
            Self::Usage { help, .. } | Self::Failed { help, .. } => help.clone(),
            Self::Office { hint, .. } => hint.clone(),
            Self::Exited(_) => None,
        }
    }

    fn exit_code(&self) -> u8 {
        match self {
            Self::Shell(error) => error.exit_code(),
            Self::Host(error) => host_exit(error.kind),
            Self::Usage { .. } => EXIT_USAGE,
            Self::Failed { .. } => EXIT_FAILURE,
            Self::Office { exit, .. } => *exit,
            Self::Exited(code) => *code,
        }
    }

    fn silent(&self, json: bool) -> bool {
        match self {
            Self::Shell(error) => error.silent(json),
            Self::Host(error) => error.kind == HostCliErrorKind::Cancelled && !json,
            Self::Exited(_) => true,
            Self::Usage { .. } | Self::Failed { .. } | Self::Office { .. } => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_errors_keep_their_class_as_the_exit_code() {
        let auth = CliError::from(HostCliError::auth("not signed in"));
        assert_eq!(
            (auth.exit_code(), auth.code()),
            (4, "cli::auth::required".into())
        );
        let refused = CliError::from(HostCliError::refused(
            org_proto::ErrorCode::Conflict,
            "the office refused it",
        ));
        assert_eq!(
            (refused.exit_code(), refused.code()),
            (1, "host::conflict".into())
        );
        let cancelled = CliError::from(HostCliError::cancelled());
        assert_eq!(cancelled.exit_code(), 130);
        assert!(cancelled.silent(false) && !cancelled.silent(true));
        assert_eq!(CliError::usage("bad").exit_code(), 2);
        assert_eq!(CliError::from("broken".to_owned()).exit_code(), 1);
        let delegated = CliError::Exited(2);
        assert_eq!(delegated.exit_code(), 2);
        assert!(delegated.silent(true));
        assert_eq!(CliError::from(ShellError::OutputClosed).exit_code(), 0);
    }
}
