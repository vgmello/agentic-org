//! The run binding `agent-os` is given by its office (`doc-28` §2.8): the `AGENT_OS_*`
//! environment, the owner-only binding file, and the one mapping of an office error to the
//! refusal a command prints ([`api_refusal`], [`map_client_error`]).
//!
//! Supports both direct socket injection (`AGENT_OS_SOCKET`) and selected UUIDv7 office
//! addressing (`AGENT_OS_OFFICE` + `AGENT_OS_RUNTIME_DIR` / `ORG_OFFICE_HOME`, `doc-5` A5).

use std::env;
use std::fmt;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use org_client::ClientError;
use org_command::{Refusal, RefusalDetails};
use org_proto::location::{OFFICE_HOME_ENV, OFFICE_RUNTIME_DIR_ENV};
use org_proto::ErrorCode;

use crate::runtime::transport::hint_for;

/// Environment variable naming the office socket path (`doc-28` §2.8).
pub const SOCKET_ENV: &str = "AGENT_OS_SOCKET";
/// Environment variable naming a selected UUIDv7 office ID (`doc-5` A5).
pub const OFFICE_ENV: &str = "AGENT_OS_OFFICE";
/// Environment variable naming the private runtime socket directory for `AGENT_OS_OFFICE`.
pub const RUNTIME_DIR_ENV: &str = "AGENT_OS_RUNTIME_DIR";
/// Environment variable carrying the run's session binding token (`doc-28` §2.8).
pub const SESSION_TOKEN_ENV: &str = "AGENT_OS_SESSION_TOKEN";
/// Environment variable naming an owner-only (`0600`) file holding the run's session token
/// (fallback when a runtime scrubs `*_TOKEN` variables from tool subprocesses, e.g. Claude
/// Code `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB=1`).
pub const BINDING_FILE_ENV: &str = "AGENT_OS_BINDING_FILE";
/// Environment variable naming the bound agent's employee ID (`doc-28` §2.8).
pub const AGENT_ID_ENV: &str = "AGENT_OS_AGENT_ID";
/// Optional environment variable naming the active run's task key or UUIDv7 (`TASK-111`).
pub const TASK_ENV: &str = "AGENT_OS_TASK";
/// Optional environment variable naming the active run's project key, slug or UUIDv7 (`TASK-111`).
pub const PROJECT_ENV: &str = "AGENT_OS_PROJECT";

const MAX_BINDING_FILE_BYTES: u64 = 4096;

/// Run binding configuration supplied to `agent-os` by its office.
#[derive(Clone, Default)]
pub struct Binding {
    pub socket: Option<PathBuf>,
    pub office_id: Option<String>,
    pub runtime_dir: Option<PathBuf>,
    pub session_token: Option<String>,
    pub agent_id: Option<String>,
    /// Active task key or UUIDv7 from the run environment (`AGENT_OS_TASK`), if any.
    pub active_task: Option<String>,
    /// Active project key, slug or UUIDv7 from the run environment (`AGENT_OS_PROJECT`), if any.
    pub active_project: Option<String>,
    /// The state folder of a signed-in external workspace (`TASK-137`): its token is refreshed
    /// by challenge-response when stale or refused.
    pub external_state: Option<PathBuf>,
}

impl fmt::Debug for Binding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Binding")
            .field("socket", &self.socket)
            .field("office_id", &self.office_id)
            .field("runtime_dir", &self.runtime_dir)
            .field(
                "session_token",
                &self.session_token.as_ref().map(|_| "<redacted>"),
            )
            .field("agent_id", &self.agent_id)
            .field("active_task", &self.active_task)
            .field("active_project", &self.active_project)
            .field("external_state", &self.external_state)
            .finish()
    }
}

impl Binding {
    /// Reads the process binding from `AGENT_OS_*` environment variables. The token in
    /// `AGENT_OS_BINDING_FILE` wins over `AGENT_OS_SESSION_TOKEN`: a managed run writes the same
    /// token to both, and `agent-os exec` keeps the file fresh while its child runs.
    pub fn from_env() -> Self {
        let non_empty = |name: &str| {
            env::var(name)
                .ok()
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        let runtime_dir = non_empty(RUNTIME_DIR_ENV)
            .or_else(|| non_empty(OFFICE_RUNTIME_DIR_ENV))
            .map(PathBuf::from)
            .or_else(|| non_empty(OFFICE_HOME_ENV).map(|h| PathBuf::from(h).join("run")));
        let session_token = non_empty(BINDING_FILE_ENV)
            .and_then(|p| read_binding_file_token(&p))
            .or_else(|| non_empty(SESSION_TOKEN_ENV));
        Self {
            socket: non_empty(SOCKET_ENV).map(PathBuf::from),
            office_id: non_empty(OFFICE_ENV),
            runtime_dir,
            session_token,
            agent_id: non_empty(AGENT_ID_ENV),
            active_task: non_empty(TASK_ENV),
            active_project: non_empty(PROJECT_ENV),
            external_state: None,
        }
    }
}

/// The trimmed token of the owner-only binding file at `path`: opened through the `org-platform`
/// contract (`O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC`, a regular file only), and read within
/// [`MAX_BINDING_FILE_BYTES`]. `None` for a symbolic link, a FIFO or device, a missing, empty or
/// oversized file.
fn read_binding_file_token(path: &str) -> Option<String> {
    let file = org_platform::fs::open_nofollow_ro(Path::new(path)).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > MAX_BINDING_FILE_BYTES {
        return None;
    }
    let mut buf = String::new();
    let n = file
        .take(MAX_BINDING_FILE_BYTES + 1)
        .read_to_string(&mut buf)
        .ok()?;
    if (n as u64) > MAX_BINDING_FILE_BYTES {
        return None;
    }
    let trimmed = buf.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// Whether the process environment asks for an office binding: it names a session token or a
/// binding file, even one that cannot be read. Such a caller is bound: a command that could run
/// without the office fails closed instead (`session context`).
pub fn env_requests_binding() -> bool {
    [SESSION_TOKEN_ENV, BINDING_FILE_ENV]
        .iter()
        .any(|name| env::var_os(name).is_some_and(|value| !value.is_empty()))
}

/// The refusal of a failed connection to `office_id` (or the bound office).
pub(crate) fn map_connect_error(error: ClientError, office_id: Option<&str>) -> Refusal {
    let target = office_id.unwrap_or("bound office");
    match error {
        ClientError::NotRunning(_) | ClientError::OfficeNotReachable => Refusal::new(
            ErrorCode::NotFound,
            format!("Office `{target}` is not running or its socket was not found."),
            "Check that the office is active and AGENT_OS_OFFICE / AGENT_OS_SOCKET is correct.",
        ),
        other => map_client_error(other),
    }
}

/// The refusal of an office error, with everything the office said in its structured fields:
/// `retry_after_ms` for `rate_limited`, `field` and `rule` an input failed, and `conflict`
/// (with `retry_with.if-match`) for optimistic-concurrency conflicts (`TASK-90`). The one
/// mapping for `agent-os` and for `agentic-org`'s grant-bound channel alike.
pub fn api_refusal(api: org_proto::ApiError) -> Refusal {
    let conflict = api.conflict_detail().cloned();
    let hint = api_refusal_hint(&api, conflict.as_ref());
    let retry_with = conflict.as_ref().map(|c| {
        let mut map = serde_json::Map::new();
        map.insert("if-match".into(), serde_json::json!(c.current_revision));
        map
    });
    let details = (api.retry_after_ms.is_some()
        || api.field.is_some()
        || api.rule.is_some()
        || conflict.is_some())
    .then(|| {
        Box::new(RefusalDetails {
            retry_after_ms: api.retry_after_ms,
            field: api.field,
            rule: api.rule,
            conflict,
        })
    });
    Refusal {
        retry_with,
        details,
        ..Refusal::new(api.code, api.message, hint)
    }
}

fn api_refusal_hint(
    api: &org_proto::ApiError,
    conflict: Option<&org_proto::ConflictDetail>,
) -> String {
    if let Some(server_hint) = api.hint().filter(|h| !h.trim().is_empty()) {
        return server_hint.to_owned();
    }
    match (api.code, api.retry_after_ms, &api.field, &api.rule, conflict) {
        (ErrorCode::NotAuthorized, ..) if api.message.contains("Ask your team lead.") => {
            "Ask your team lead.".to_owned()
        }
        (ErrorCode::NotAuthorized, ..) if api.message.contains("1:1 direct message") => {
            "To hand a file to one person, send it in a 1:1 direct message with --attach."
                .to_owned()
        }
        (ErrorCode::Conflict, _, _, _, Some(c)) => format_conflict_hint(c),
        (ErrorCode::RateLimited, Some(after), ..) => format!(
            "Wait {after} ms (error.retry_after_ms), then retry the command with the same flags."
        ),
        (_, _, Some(field), Some(rule), _) => format!(
            "Fix `{field}`: it does not meet the `{rule}` rule (error.field, error.rule). Then retry."
        ),
        (_, _, Some(field), None, _) => format!("Fix `{field}` (error.field), then retry."),
        (code, ..) => hint_for(code).to_owned(),
    }
}

fn format_conflict_hint(conflict: &org_proto::ConflictDetail) -> String {
    let rev = conflict.current_revision;
    let changed = if conflict.changed_sections.is_empty() {
        String::new()
    } else {
        format!(" (changed: {})", conflict.changed_sections.join(", "))
    };
    if conflict.reread_recommended {
        format!(
            "Record is now at revision {rev}{changed}; review error.conflict.events_since, re-inspect, and retry with --if-match {rev}."
        )
    } else {
        format!(
            "Record is now at revision {rev}{changed}; review error.conflict.events_since and retry with --if-match {rev}."
        )
    }
}

/// The refusal of any `org-client` error ([`api_refusal`] for one the office answered).
pub(crate) fn map_client_error(error: ClientError) -> Refusal {
    match error {
        ClientError::Api(api) => api_refusal(api),
        ClientError::RequestTooLarge => Refusal::new(
            ErrorCode::TooLarge,
            "The request exceeded the office socket frame limit.",
            "Reduce the request size and retry.",
        ),
        // A write the office did not answer in time may have been made: the one timeout hint
        // says to retry it with the same --idempotency-key.
        ClientError::Timeout => Refusal::new(
            ErrorCode::Timeout,
            "The office did not answer before the timeout.",
            hint_for(ErrorCode::Timeout),
        ),
        ClientError::Disconnected | ClientError::Io(_) | ClientError::Protocol(_) => Refusal::new(
            ErrorCode::Unavailable,
            "The bound office could not be reached.",
            "Check that the office process is running and AGENT_OS_SOCKET is valid.",
        ),
        other => Refusal::new(
            ErrorCode::Unavailable,
            other.to_string(),
            "Check the office connection and retry.",
        ),
    }
}

#[cfg(test)]
mod tests;
