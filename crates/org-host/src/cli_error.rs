//! The typed failure of a host operation that the operator command line composes (TASK-188):
//! one human sentence, the service's wire code when it answered one, what to do next when
//! known, and the class of failure the command line turns into its exit code. Wording around
//! it, sanitization and exit codes are the command line's; nothing here prints.
//!
//! Classes: [`HostCliErrorKind::Auth`] for not signed in or not allowed (a wire
//! `not_authorized` or `not_bound`, a wrong login or password, a declined, expired or refused
//! sign-in approval, a password the system may not send); [`HostCliErrorKind::Refused`] for a
//! service or office that said no (a wire code, or a lookup among what it listed that matched
//! nothing, with no code); [`HostCliErrorKind::Usage`] for input the person gave that is invalid,
//! or a selection only they can make (`pass --org`); [`HostCliErrorKind::Cancelled`] for a
//! cancelled password entry; [`HostCliErrorKind::Runtime`] for everything else.

use std::fmt;
use std::io;

use org_proto::{ApiError, ErrorCode};
use serde::Serialize;

/// The message of a write to the command's output that failed because its reader went away
/// (`BrokenPipe`): see [`HostCliError::is_output_closed`].
pub const OUTPUT_CLOSED: &str = "standard output closed";

/// What the terminal's password entry answers when the person cancelled it (Ctrl+C, or
/// end of input at the prompt); [`HostCliError::password`] makes it a cancellation.
pub(crate) const PASSWORD_CANCELLED: &str = "password entry cancelled";

/// The class of a [`HostCliError`]; the command line maps each to its exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostCliErrorKind {
    /// Not signed in, or not allowed (exit 4).
    Auth,
    /// The host or an office refused (exit 1).
    Refused,
    /// The person's input is invalid, or only they can choose (exit 2).
    Usage,
    /// The host is not running, I/O, protocol, or anything else (exit 1).
    Runtime,
    /// The person cancelled (exit 130).
    Cancelled,
}

/// A failed host operation. See the module docs for the classes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCliError {
    pub kind: HostCliErrorKind,
    /// The wire code, when the service answered one.
    pub code: Option<ErrorCode>,
    /// One human sentence; never `Debug` text.
    pub message: String,
    /// What to do next, when known.
    pub help: Option<String>,
}

impl HostCliError {
    fn new(kind: HostCliErrorKind, code: Option<ErrorCode>, message: impl Into<String>) -> Self {
        Self {
            kind,
            code,
            message: message.into(),
            help: None,
        }
    }

    /// Not signed in or not allowed, with no wire code.
    pub fn auth(message: impl Into<String>) -> Self {
        Self::new(HostCliErrorKind::Auth, None, message)
    }

    /// The service answered `code` (or, with `None`, refused without one): `Auth` for the
    /// sign-in codes ([`kind_of`]), `Refused` for every other.
    pub fn refused(code: impl Into<Option<ErrorCode>>, message: impl Into<String>) -> Self {
        let code = code.into();
        let kind = code.map_or(HostCliErrorKind::Refused, kind_of);
        Self::new(kind, code, message)
    }

    /// Invalid input, caught before the service is asked.
    pub fn usage(message: impl Into<String>) -> Self {
        Self::new(HostCliErrorKind::Usage, None, message)
    }

    /// Anything else.
    pub fn runtime(message: impl Into<String>) -> Self {
        Self::new(HostCliErrorKind::Runtime, None, message)
    }

    /// The person cancelled.
    pub fn cancelled() -> Self {
        Self::new(HostCliErrorKind::Cancelled, None, "cancelled")
    }

    /// The command's output was closed by its reader: a `Runtime` error whose message is
    /// [`OUTPUT_CLOSED`]. The command line treats it as a quiet success.
    pub fn output_closed() -> Self {
        Self::runtime(OUTPUT_CLOSED)
    }

    /// Whether this is [`HostCliError::output_closed`].
    pub fn is_output_closed(&self) -> bool {
        self.kind == HostCliErrorKind::Runtime
            && self.code.is_none()
            && self.message == OUTPUT_CLOSED
    }

    /// This error, with what to do next.
    #[must_use]
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// This error, naming the wire code the service answered; its kind is unchanged.
    #[must_use]
    pub fn with_code(mut self, code: ErrorCode) -> Self {
        self.code = Some(code);
        self
    }

    /// A failed password entry: [`PASSWORD_CANCELLED`] is a cancellation, anything else
    /// (no terminal, a read error) is `Runtime`.
    pub(crate) fn password(why: String) -> Self {
        if why == PASSWORD_CANCELLED {
            Self::new(HostCliErrorKind::Cancelled, None, why)
        } else {
            Self::runtime(why)
        }
    }

    /// A failed write of the command's own output: a closed reader is
    /// [`HostCliError::output_closed`], any other failure `Runtime`.
    pub(crate) fn output(error: io::Error) -> Self {
        if error.kind() == io::ErrorKind::BrokenPipe {
            Self::output_closed()
        } else {
            Self::runtime(format!("writing output: {error}"))
        }
    }
}

impl fmt::Display for HostCliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for HostCliError {}

/// Internal steps that still fail with a sentence are `Runtime`.
impl From<String> for HostCliError {
    fn from(message: String) -> Self {
        Self::runtime(message)
    }
}

impl From<&str> for HostCliError {
    fn from(message: &str) -> Self {
        Self::runtime(message)
    }
}

/// So that a `Result<_, String>` helper can still use `?` on a typed step (its message).
impl From<HostCliError> for String {
    fn from(error: HostCliError) -> Self {
        error.message
    }
}

/// The class of a wire refusal: `not_authorized` and `not_bound` are `Auth`, every other
/// code `Refused`.
pub fn kind_of(code: ErrorCode) -> HostCliErrorKind {
    match code {
        ErrorCode::NotAuthorized | ErrorCode::NotBound => HostCliErrorKind::Auth,
        _ => HostCliErrorKind::Refused,
    }
}

/// The wire spelling of a unit enum value (`not_authorized`, `ack_pending`, `satellite`), or
/// `unknown` for a value that serializes to anything but a string.
pub fn wire_name<T: Serialize + ?Sized>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        _ => "unknown".to_owned(),
    }
}

/// The service refused `action` with `code`, in its wire spelling.
pub fn refused_by(action: &str, code: ErrorCode) -> HostCliError {
    HostCliError::refused(
        code,
        format!("the service refused {action}: {}", wire_name(&code)),
    )
}

/// An office refused `action` with `error`: its wire code and its message (which is for
/// people and holds no paths).
pub fn office_refused(action: &str, error: &ApiError) -> HostCliError {
    HostCliError::refused(
        error.code,
        format!(
            "the office refused {action} ({}): {}",
            wire_name(&error.code),
            error.message
        ),
    )
}

/// What a refused `account.verify` says: a throttle or an unavailable service is the
/// service's refusal; any other answer is `wrong`, a sign-in the person cannot make.
pub(crate) fn sign_in_refused(code: ErrorCode, wrong: &str) -> HostCliError {
    match code {
        ErrorCode::RateLimited => {
            HostCliError::refused(code, "too many failed sign-ins; try again later")
        }
        ErrorCode::Unavailable => {
            HostCliError::refused(code, "the service cannot sign anyone in now; try again")
        }
        code => HostCliError::auth(wrong).with_code(code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Every code this build knows, for the classification and wording checks.
    const CODES: [ErrorCode; 28] = [
        ErrorCode::InvalidRequest,
        ErrorCode::UnknownMethod,
        ErrorCode::InvalidName,
        ErrorCode::AlreadyExists,
        ErrorCode::StorageError,
        ErrorCode::Cycle,
        ErrorCode::NotEmpty,
        ErrorCode::NotAMember,
        ErrorCode::OwnerProtected,
        ErrorCode::NotFound,
        ErrorCode::Conflict,
        ErrorCode::SnapshotRequired,
        ErrorCode::Expired,
        ErrorCode::ClockSkew,
        ErrorCode::UnknownRecipient,
        ErrorCode::NotAuthorized,
        ErrorCode::Unavailable,
        ErrorCode::NotReady,
        ErrorCode::Timeout,
        ErrorCode::TooLarge,
        ErrorCode::NotBound,
        ErrorCode::ChecklistIncomplete,
        ErrorCode::UnsupportedVersion,
        ErrorCode::OperationReused,
        ErrorCode::Cancelled,
        ErrorCode::RateLimited,
        ErrorCode::Unsupported,
        ErrorCode::Unknown,
    ];

    #[test]
    fn only_the_sign_in_codes_are_auth_and_every_other_code_is_refused() {
        for code in CODES {
            let error = refused_by("the thing", code);
            let expected = if matches!(code, ErrorCode::NotAuthorized | ErrorCode::NotBound) {
                HostCliErrorKind::Auth
            } else {
                HostCliErrorKind::Refused
            };
            assert_eq!(error.kind, expected, "{code:?}");
            assert_eq!(kind_of(code), expected, "{code:?}");
            assert_eq!(error.code, Some(code));
            assert_eq!(
                HostCliError::refused(code, "a sentence").kind,
                expected,
                "{code:?}"
            );
        }
        let without = HostCliError::refused(None, "nothing matched");
        assert_eq!(
            (without.kind, without.code),
            (HostCliErrorKind::Refused, None)
        );
        let wrong_password =
            HostCliError::auth("invalid login or password").with_code(ErrorCode::InvalidRequest);
        assert_eq!(wrong_password.kind, HostCliErrorKind::Auth);
        assert_eq!(wrong_password.code, Some(ErrorCode::InvalidRequest));
    }

    #[test]
    fn refusals_name_the_wire_spelling_and_never_debug_text() {
        for code in CODES {
            let wire = wire_name(&code);
            assert!(
                wire.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "{wire}"
            );
            let debug = format!("{code:?}");
            let by = refused_by("organization creation", code);
            assert_eq!(
                by.message,
                format!("the service refused organization creation: {wire}")
            );
            let office = office_refused(
                "listing projects",
                &ApiError::new(code, "a sentence for people"),
            );
            assert_eq!(
                office.message,
                format!("the office refused listing projects ({wire}): a sentence for people")
            );
            for message in [&by.message, &office.message] {
                assert!(!message.contains(&debug), "{message}");
                assert!(
                    !message.contains("Some(") && !message.contains("None"),
                    "{message}"
                );
            }
        }
        assert_eq!(wire_name(&ErrorCode::NotAuthorized), "not_authorized");
        assert_eq!(wire_name(&ErrorCode::Unknown), "unknown");
        assert_eq!(wire_name(&42), "unknown");
    }

    #[test]
    fn constructors_conversions_and_display() {
        let usage = HostCliError::usage("office name cannot be empty").with_help("pass a name");
        assert_eq!(usage.kind, HostCliErrorKind::Usage);
        assert_eq!(usage.help.as_deref(), Some("pass a name"));
        assert_eq!(usage.to_string(), "office name cannot be empty");
        assert_eq!(HostCliError::cancelled().kind, HostCliErrorKind::Cancelled);
        let runtime: HostCliError = String::from("the service is not running").into();
        assert_eq!(runtime.kind, HostCliErrorKind::Runtime);
        assert_eq!(HostCliError::from("io").kind, HostCliErrorKind::Runtime);
        let back: String = HostCliError::auth("not allowed").into();
        assert_eq!(back, "not allowed");
        let as_error: &dyn std::error::Error = &usage;
        assert_eq!(as_error.to_string(), "office name cannot be empty");
    }

    #[test]
    fn a_cancelled_password_entry_is_a_cancellation_and_other_failures_are_runtime() {
        let cancelled = HostCliError::password(PASSWORD_CANCELLED.to_owned());
        assert_eq!(cancelled.kind, HostCliErrorKind::Cancelled);
        let no_tty = HostCliError::password("password requires a controlling terminal".into());
        assert_eq!(no_tty.kind, HostCliErrorKind::Runtime);
    }

    #[test]
    fn a_closed_reader_is_distinguishable_from_other_write_failures() {
        struct Closed(io::ErrorKind);
        impl Write for Closed {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::from(self.0))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let write = |kind| writeln!(Closed(kind), "a line").map_err(HostCliError::output);
        let closed = write(io::ErrorKind::BrokenPipe).unwrap_err();
        assert!(closed.is_output_closed(), "{closed:?}");
        assert_eq!(closed.message, OUTPUT_CLOSED);
        let other = write(io::ErrorKind::Other).unwrap_err();
        assert!(!other.is_output_closed(), "{other:?}");
        assert_eq!(other.kind, HostCliErrorKind::Runtime);
        assert!(!HostCliError::refused(None, OUTPUT_CLOSED).is_output_closed());
    }
}
