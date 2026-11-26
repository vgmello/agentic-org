//! Launch failure reporting and logging: control events, fail codes and redacted error causes.

use crate::startup::Control;
use org_office::OfficeError;
use org_office_launch::{FailCode, ServerEvent};
use org_proto::ApiError;

/// Logs why the office could not answer (to stderr, the host's log: `OfficeError`'s text has no
/// path, SQL or secret in it) and returns it for the connection, which tells the client only a
/// fixed text.
pub(crate) fn logged(what: &str, error: &OfficeError) -> String {
    eprintln!("agentic-org-office: {what} failed: {error}");
    log_causes(error);
    error.to_string()
}

/// The answer to a call this office does not serve yet: `what` is rebuilt on the office store
/// by increment `by`.
pub(crate) fn unsupported(what: &str, by: &str) -> ApiError {
    ApiError::new(
        org_proto::ErrorCode::Unsupported,
        format!("this office does not serve {what} yet ({by})"),
    )
}

/// The causes behind a storage failure (a path, the OS error, SQLite's text) belong in the log
/// and not on the pipe or the socket; they never hold the key or the token.
pub(crate) fn log_causes(error: &dyn std::error::Error) {
    let mut cause = std::error::Error::source(error);
    while let Some(inner) = cause {
        eprintln!("agentic-org-office:   caused by: {inner}");
        cause = inner.source();
    }
}

pub(crate) fn emit(control: &mut Control, event: &ServerEvent) {
    // The host may already be gone; the server notices at EOF.
    let _ = control.emit(event);
}

pub(crate) fn fail(control: &mut Control, code: FailCode, message: &str) -> i32 {
    // The host's log is this process's stderr: say what failed (a fixed text, never a secret).
    eprintln!("agentic-org-office: {message}");
    emit(
        control,
        &ServerEvent::Failed {
            code,
            message: message.to_owned(),
        },
    );
    1
}

pub(crate) fn fail_with(control: &mut Control, error: &OfficeError) -> i32 {
    log_causes(error);
    // `Display` never carries a path, SQL or a key.
    fail(control, fail_code(error), &error.to_string())
}

/// The code the host is told for an office error.
pub(crate) fn fail_code(error: &OfficeError) -> FailCode {
    match error {
        OfficeError::InvalidName(_) => FailCode::InvalidName,
        OfficeError::OfficeExists | OfficeError::AlreadyExists => FailCode::AlreadyExists,
        OfficeError::InUse => FailCode::InUse,
        // `NotFound` on `--office`: serving an office that does not exist is a bad argument.
        OfficeError::NotFound => FailCode::InvalidRequest,
        // Everything else `Invalid` that the office still reports describes the disk (databases
        // of another office, a folder that is not plain, a stray file): the server has already
        // checked the arguments and the first line. `invalid_request` is fatal for the host until
        // a new binary; a repaired folder must be restartable.
        OfficeError::Invalid(_) => FailCode::StorageError,
        _ => FailCode::StorageError,
    }
}
