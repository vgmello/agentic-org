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
    /// offboarding an agent that is already offboarded).
    InvalidRequest,
    /// The server doesn't know the method.
    UnknownMethod,
    /// A name breaks its rule. Each kind of name (an organization, a team, a person, an agent)
    /// has its own rule; an organization's is [`crate::OrganizationName::parse`]. An agent's
    /// `messaging_name` that breaks its format is also `invalid_name`, and so is a person's name
    /// or an agent's `messaging_name` that looks like an address (a `run:` prefix, or 26 letters
    /// and digits, the shape of a member id).
    InvalidName,
    /// Something with that name or id already exists: an organization or a sibling team with
    /// that name (names compare ignoring case), another agent with that `messaging_name`
    /// (ignoring case), a person's name or `messaging_name` that is a reserved word (`owner`,
    /// `office`), a membership or project-team link that already exists, or a
    /// client-made id that's taken.
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
    /// The command would change the built-in HR lead beyond its `runtime`, `model` and run mode,
    /// or involve it where it may not be: offboard or remove it, rename it, edit its profile,
    /// pause it, put it in a team (`team.add_member`, `member.move`), make it a lead, give it
    /// reports or a manager, or let it approve hires. Also a hire request naming it as the team
    /// member or the manager. Decided in the same place in the order as `owner_protected`.
    BuiltinProtected,
    /// Something the command names doesn't exist (any more).
    NotFound,
    /// The `expected_version` is stale: the thing changed since the caller saw it.
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
    /// organization.
    UnknownRecipient,
    /// The caller isn't allowed to make this call, e.g. the host's check of the peer's code
    /// signature failed for `office.app_token`, or an app-only command (`member.set_run_mode`)
    /// arrived on a connection that is not the app's.
    NotAuthorized,
    /// What was asked for exists but can't serve now, e.g. the office isn't running, so there's
    /// no app token to hand out. Trying again later may work.
    Unavailable,
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
    /// The server's result didn't match the call's result type.
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
