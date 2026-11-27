use std::fmt;

use serde::{Deserialize, Serialize};

/// Why a request failed. Unknown codes from a newer server decode as [`ErrorCode::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ErrorCode {
    /// The request couldn't be read (bad JSON, a bad envelope, or params of the wrong shape), or
    /// it can be read but makes no sense: a value outside its documented limits (e.g. an
    /// agent's `title` or `home`), a call that gives nothing to change, `Unknown` or a
    /// non-canonical value where a known one is required, an agent-only call on a person (or
    /// the other way round), or a call on a member in the wrong state (e.g. editing or
    /// offboarding an agent that is already offboarded). In [`crate::msg`] and
    /// [`crate::directory`], a number out of range (a `limit`, `timeout_ms`, `wait_ms` or
    /// `ttl_ms`) is `invalid_request`, while a string, body, meta or note over its size limit
    /// is `too_large`.
    InvalidRequest,
    /// The server doesn't know the method.
    UnknownMethod,
    /// A name breaks its rule. Each kind of name (an organization, a team, a person, an agent, a
    /// project, an area) has its own rule; an organization's is [`crate::OrganizationName::parse`].
    /// An agent's `messaging_name` that breaks its format is also `invalid_name`, and so is a
    /// person's name or an agent's `messaging_name` that looks like an address (a `run:` prefix,
    /// or 26 letters and digits, the shape of a member id).
    InvalidName,
    /// Something with that name or id already exists: an organization or a sibling team with
    /// that name (names compare ignoring case), another agent with that `messaging_name`
    /// (ignoring case), a person's name or `messaging_name` that is a reserved word (`owner`,
    /// `office`), a project with that key or name (ignoring case), a live area of the project
    /// with that name (ignoring case), a membership, project-team link or work-item link that
    /// already exists, or a client-made id that's taken.
    AlreadyExists,
    /// The server couldn't read or write its storage. Partial state is cleaned up.
    StorageError,
    /// A loop: a team would own itself or a team above it, or someone would report to
    /// themselves, directly or through others.
    Cycle,
    /// A team still owns teams, so it can't be deleted.
    NotEmpty,
    /// The lead isn't a member of the team.
    NotAMember,
    /// The command would remove the owner, add the owner to a team, make it a lead, or give it
    /// someone to report to.
    OwnerProtected,
    /// Something the command names doesn't exist (any more).
    NotFound,
    /// The `expected_version` is stale: the thing changed since the caller saw it. Also
    /// `office.restart` on an office that isn't `offline` or `unavailable` (e.g. it's already
    /// starting or active).
    Conflict,
    /// `subscribe { from_position }` can't resume from there; take a fresh `org.snapshot`.
    SnapshotRequired,
    /// The command is too old for HQ to accept (its id's time is past the dedup window).
    /// Applies to every organization command.
    Expired,
    /// The command's id is dated too far ahead of HQ's clock. Applies to every organization
    /// command.
    ClockSkew,
    /// A message or task is addressed to someone who isn't an agent or person of the
    /// organization: no member at all, an offboarded agent, or one whose status this office
    /// can't read.
    UnknownRecipient,
    /// The caller isn't allowed to make this call, e.g. the host's check of the peer's code
    /// signature failed for `office.restart`, an app-only command
    /// (`member.set_run_mode`, `project.create`) arrived on a connection that is
    /// not the app's, `msg.bind` refused a member (it fails closed), or
    /// `org.snapshot` or `subscribe` came on an office server connection that isn't bound (ADR-0003 §2.1.4;
    /// unlike `who`, which answers that with `not_bound`).
    NotAuthorized,
    /// What was asked for exists but can't serve now, e.g. the office isn't running. Trying again later may work. For `office.restart`, though, it
    /// means the office's reason is one a restart can't fix (`unsupported_version` or
    /// `invalid_request`); the message says why. Clients show an office's `status_reason`, not
    /// this code: the code says only why the call failed.
    Unavailable,
    /// The host (or office) is still starting; retry shortly. Temporary, unlike
    /// `unavailable`: e.g. `organization.create` or `office.restart` while
    /// the host's startup discards run.
    NotReady,
    /// An `ask`'s deadline (`expires_at`) passed without a reply, including when the
    /// recipient's office reports that the request expired.
    Timeout,
    /// A string, body, meta or note is over its size limit, in bytes (or, for a status line,
    /// characters; see the limits in [`crate::msg`] and [`crate::directory`]). A number out of
    /// range is `invalid_request` instead. Never stored as a write's answer.
    TooLarge,
    /// The connection hasn't bound with `msg.bind` (ADR-0003: an agent bound as itself; a
    /// person or the owner acts through a grant instead), or the bound member was moved out of
    /// this office or removed. Also ends held `msg.wait` and `msg.ask` calls when the connection
    /// re-binds. (`org.snapshot` and `subscribe` on an office server's unbound connection are
    /// `not_authorized` instead.) Never stored as a write's answer: a retry may succeed.
    NotBound,
    /// A task was to be completed while required checklist steps are unticked and no reason to
    /// complete it anyway was given; the message names the steps. Clients branch on it ("Mark
    /// done anyway…"). `tasks.transition` answers it (`TASK-88`), and the web gateway's task
    /// routes answer it as `409`. A build that doesn't know it reads it as
    /// [`ErrorCode::Unknown`].
    ChecklistIncomplete,
    /// `hello` offered no version this server speaks, or a connection that needs a v2
    /// `hello` didn't start with one (X-02 §3). The server closes the connection after it and
    /// never stores it. Not the office status reason of the same name
    /// ([`crate::OfficeStatusReason::UnsupportedVersion`]), which says why an office can't start.
    UnsupportedVersion,
    /// A known operation id (`command_id`, `change_id`, `msg.send`'s `id`, or a host op's
    /// `operation_id`) came with different params (its fingerprint differs) or from a
    /// different principal than the stored one. Use a new id for a different operation
    /// (X-02 §4.2).
    OperationReused,
    /// A reserved organization create was cancelled or abandoned after its window expired.
    Cancelled,
    /// A throttle key is locked; the error carries `retry_after_ms`.
    RateLimited,
    /// The protocol has this call; this office does not serve it yet. Not temporary, unlike
    /// `unavailable` and `not_ready`: retrying the same call against the same build answers
    /// the same. Never stored as a write's answer. A build that doesn't know it reads it as
    /// [`ErrorCode::Unknown`].
    Unsupported,
    /// A code this build doesn't know (sent by a newer server).
    #[serde(other)]
    Unknown,
}

/// An error returned by the server. `message` is for people; it never holds paths or SQL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
    /// With `rate_limited`: how long until a retry can succeed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
    /// With `invalid_request` for a credential rule: the input field (`login`, `password`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// With `field`: the rule it failed (`format`, `taken`, `too_short`, `too_long`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
}

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retry_after_ms: None,
            field: None,
            rule: None,
        }
    }

    /// `rate_limited` with the delay until a retry can succeed.
    pub fn rate_limited(retry_after_ms: u64) -> Self {
        let mut error = Self::new(ErrorCode::RateLimited, "too many attempts; try again later");
        error.retry_after_ms = Some(retry_after_ms);
        error
    }

    /// `invalid_request` naming the input field and the rule it failed.
    pub fn rule(field: &str, rule: &str) -> Self {
        let mut error = Self::new(ErrorCode::InvalidRequest, "a credential rule was not met");
        error.field = Some(field.to_owned());
        error.rule = Some(rule.to_owned());
        error
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for ApiError {}

/// Reading a typed result out of a [`crate::Response`] failed.
#[derive(Debug)]
pub enum CallError {
    /// The server answered with an error.
    Api(ApiError),
    /// The server's result didn't match the call's result type. The serde error can quote the
    /// result's values (e.g. a message body), so never log it for `msg.*` results.
    Decode(serde_json::Error),
}

impl fmt::Display for CallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CallError::Api(e) => write!(f, "server error: {e}"),
            CallError::Decode(e) => write!(f, "unexpected result: {e}"),
        }
    }
}

impl std::error::Error for CallError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_is_its_own_code_on_the_wire() {
        let error = ApiError::new(ErrorCode::Unsupported, "not served by this office yet");
        let wire = serde_json::to_value(&error).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({ "code": "unsupported", "message": "not served by this office yet" })
        );
        assert_eq!(serde_json::from_value::<ApiError>(wire).unwrap(), error);
    }

    #[test]
    fn a_code_this_build_does_not_know_still_reads_as_unknown() {
        let code: ErrorCode = serde_json::from_str("\"some_newer_code\"").unwrap();
        assert_eq!(code, ErrorCode::Unknown);
    }
}
