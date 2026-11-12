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
    /// Also for `from_work_position` (take a fresh `work.snapshot`). The error doesn't say
    /// which stream: a client that gave both positions takes both snapshots again.
    SnapshotRequired,
    /// The command is too old for HQ to accept (its id's time is past the dedup window).
    /// Applies to every organization command, and to every work write (its `change_id` is
    /// dated more than 29 days ago; see [`crate::work`]).
    Expired,
    /// The command's id is dated too far ahead of HQ's clock. Applies to every organization
    /// command, and to every work write (its `change_id` is more than 5 minutes ahead of the
    /// owning office's clock).
    ClockSkew,
    /// A message or task is addressed to someone who isn't an agent or person of the
    /// organization: no member at all, an offboarded agent, or one whose status this office
    /// can't read.
    UnknownRecipient,
    /// The caller isn't allowed to make this call, e.g. the host's check of the peer's code
    /// signature failed for `office.app_token` or `office.restart`, an app-only command
    /// (`member.set_run_mode`, `project.create`, `work.delete`) arrived on a connection that is
    /// not the app's, `msg.bind` refused a member or a credential (it fails closed), or
    /// `org.snapshot` or `subscribe` came on an office server connection that isn't bound (ADR-0003 §2.1.4;
    /// unlike `who`, which answers that with `not_bound`).
    NotAuthorized,
    /// What was asked for exists but can't serve now, e.g. the office isn't running, so there's
    /// no app token to hand out. Trying again later may work. For `office.restart`, though, it
    /// means the office's reason is one a restart can't fix (`unsupported_version` or
    /// `invalid_request`); the message says why. Clients show an office's `status_reason`, not
    /// this code: the code says only why the call failed.
    Unavailable,
    /// The host (or office) is still starting; retry shortly. Temporary, unlike
    /// `unavailable`: e.g. `organization.create`, `office.restart` or `office.app_token` while
    /// the host's startup discards run.
    NotReady,
    /// An `ask`'s deadline (`expires_at`) passed without a reply, including when the
    /// recipient's office reports that the request expired.
    Timeout,
    /// A string, body, meta or note is over its size limit, in bytes (or, for a status line,
    /// characters; see the limits in [`crate::msg`] and [`crate::directory`]), or a work write
    /// whose encoded params exceed [`crate::work::WORK_PARAMS_MAX_BYTES`]. A number out of range
    /// is `invalid_request` instead. Never stored as a write's answer.
    TooLarge,
    /// The connection hasn't bound with `msg.bind` (ADR-0003: an agent bound as itself, or the
    /// app bound as the owner or a person), the bound member was moved out of this office or
    /// removed, or the connection is bound app-wide and the call acts as a member. Also ends
    /// held `msg.wait` and `msg.ask` calls when the connection re-binds. Every work call
    /// ([`crate::work`]) needs a bound connection. (`org.snapshot` and `subscribe` on an office server's
    /// unbound connection are `not_authorized` instead.) Never stored as a write's answer: a retry may succeed.
    NotBound,
    /// `task.set_status { done }` (or a bug's mark-fixed) while required checklist steps are
    /// unticked and no `anyway_reason` is given; the message names the steps. Clients branch on
    /// it ("Mark done anyway…"). A build that doesn't know it reads it as [`ErrorCode::Unknown`].
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
    /// A code this build doesn't know (sent by a newer server).
    #[serde(other)]
    Unknown,
}

/// An error returned by the server. `message` is for people; it never holds paths or SQL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
}

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
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
