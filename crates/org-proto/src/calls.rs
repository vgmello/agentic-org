use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::token::AppToken;
use crate::types::{OfficeInfo, Organization};

/// The identity an office server reports in `hello`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeHello {
    /// The organization's name. Its id is `office.organization_id`.
    pub organization_name: String,
    pub office: OfficeInfo,
}

/// One method of the API: its wire name and the shapes of its params and result.
/// Params and results are JSON objects (structs), so fields can be added later.
pub trait Call {
    const METHOD: &'static str;
    type Params: Serialize + DeserializeOwned;
    type Result: Serialize + DeserializeOwned;
}

/// Params or result with no fields. Serializes as `{}`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Empty {}

/// `hello`: the first call on a connection. The client checks `api_version`.
pub struct Hello;

/// The answer to `hello`.
///
/// `#[non_exhaustive]`: fields may be added. Code outside this crate builds one with
/// [`HelloResult::host`] or [`HelloResult::office`] (and [`HelloResult::with_capabilities`]),
/// not a struct literal, so a new field never breaks it. Its fields stay public to read and
/// set. (The attribute only affects other crates; inside `org-proto` a literal still compiles.)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct HelloResult {
    pub api_version: u32,
    pub server_version: String,
    /// The machine, for display, e.g. "This Mac".
    pub host: String,
    /// Set when an office server answers; absent when this machine's host answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub office: Option<OfficeHello>,
    /// The optional calls and events this server implements beyond what every server of
    /// `api_version` has, by wire name (see [`capability`]). A client checks for one before
    /// calling it or waiting for it, and ignores names it doesn't know. Empty (and absent on
    /// the wire) for a server that implements none, including servers older than this field.
    /// Servers always send a JSON array of strings. Reading is tolerant, because `hello` must
    /// always decode: `null` reads as empty and an item that isn't a string is skipped.
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "lenient_names"
    )]
    pub capabilities: Vec<String>,
}

/// Reads an advisory list of names: `null` is empty, and non-string items are dropped.
fn lenient_names<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::Array(items)) => items
            .into_iter()
            .filter_map(|item| match item {
                serde_json::Value::String(name) => Some(name),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    })
}

/// Capability names a server may list in [`HelloResult::capabilities`]. Each one is the wire
/// name of the call or event it stands for. The three below are host calls and events: only
/// this machine's host lists them, never an office server.
pub mod capability {
    /// The host serves `office.app_token`.
    pub const OFFICE_APP_TOKEN: &str = "office.app_token";
    /// The host serves `office.restart`.
    pub const OFFICE_RESTART: &str = "office.restart";
    /// The host publishes `office.status_changed` to subscribers **and** sets `status_reason`
    /// and `next_restart_at` (on the event and in `office.list`). Listing it means both.
    pub const OFFICE_STATUS_CHANGED: &str = "office.status_changed";
}

impl HelloResult {
    /// The host's answer: [`crate::API_VERSION`], no office and no capabilities.
    pub fn host(server_version: impl Into<String>, host: impl Into<String>) -> Self {
        Self {
            api_version: crate::API_VERSION,
            server_version: server_version.into(),
            host: host.into(),
            office: None,
            capabilities: Vec::new(),
        }
    }

    /// An office server's answer: [`crate::API_VERSION`], its identity and no capabilities.
    pub fn office(
        server_version: impl Into<String>,
        host: impl Into<String>,
        office: OfficeHello,
    ) -> Self {
        Self {
            office: Some(office),
            ..Self::host(server_version, host)
        }
    }

    /// The same answer, listing `capabilities` (see [`capability`]).
    pub fn with_capabilities(mut self, capabilities: Vec<String>) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Whether the server listed `name` in its capabilities.
    pub fn supports(&self, name: &str) -> bool {
        self.capabilities.iter().any(|c| c == name)
    }
}

impl Call for Hello {
    const METHOD: &'static str = "hello";
    type Params = Empty;
    type Result = HelloResult;
}

/// `organization.create` (host): create an organization and start its HQ office on this
/// machine. The result lists that HQ.
///
/// It succeeds once the organization and its HQ office are stored. Whether the HQ's server
/// process started is reported in that office's `status` (`starting`, `active`, or
/// `unavailable`), not as an error, so a client never retries into `already_exists`.
/// Errors: `invalid_name`, `already_exists`, `storage_error`, `not_ready` (the host is still
/// starting: its startup discards are running; retry shortly), `unavailable` (too many creates
/// in progress; retry).
pub struct OrganizationCreate;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganizationCreateParams {
    pub name: String,
}

impl Call for OrganizationCreate {
    const METHOD: &'static str = "organization.create";
    type Params = OrganizationCreateParams;
    type Result = Organization;
}

/// `organization.list` (host): every organization with an office on this machine, each
/// with its offices here.
pub struct OrganizationList;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListOrganizationsResult {
    pub organizations: Vec<Organization>,
}

impl Call for OrganizationList {
    const METHOD: &'static str = "organization.list";
    type Params = Empty;
    type Result = ListOrganizationsResult;
}

/// `subscribe`: after the response, the server pushes every [`crate::Event`] on this
/// connection until it closes **or the connection binds again**: any `msg.bind`, failed or
/// not, ends the subscription, and no event follows the bind's answer. Subscribe again (from
/// the last position you got) once bound. No filter or unsubscribe yet (a filter would be an
/// optional param, which is additive). A subscriber also receives events for its own actions.
///
/// On an **office server** it needs a binding (ADR-0003 §2.1.4): on an unbound connection
/// it's `not_authorized`, like `org.snapshot`, and today only app-bound connections may
/// subscribe (an agent-bound subscriber needs its own event filter first). The **host**
/// serves it to any same-user connection without a binding (its socket is 0600), for the
/// office list and `office.status_changed`.
///
/// An office server also pushes **live events** that are not in the organization's log, for
/// what happens after the subscription: `message.received`, `message.state_changed` (headers
/// only) and `member.presence_changed` (the latest per member, at most once a second for each).
/// They are hints with no replay: take `msg.inbox` and `who` for the state, and the
/// events for the changes. If a subscriber falls too far behind, the office ends its
/// subscription by closing the connection, which looks like any other dropped connection (no
/// reason is sent). So after ANY end of a subscription (it fell behind, a re-bind, a dropped
/// connection) a client re-lists what it shows (an app-bound one
/// with `msg.list`, `msg.threads` and `msg.human_needs_you`, and `who`; `msg.inbox` is for a
/// member binding), takes `org.snapshot` only when a resume answers `snapshot_required`, and
/// subscribes again. A presence event still waiting out its gap when a subscription ends is
/// dropped; a client that subscribes again asks `who`.
pub struct Subscribe;

/// `subscribe`'s params. `{}` is still valid (live events only).
///
/// With `from_position` (an office only; the host ignores it): right after the `subscribe`
/// response, the office sends the organization changes after that position (e.g. the one an
/// [`crate::OrgSnapshot`] reflects), then live events, so nothing slips in between. If it
/// can't resume from there (the log was trimmed, a promotion started a new epoch, or the
/// position is beyond what this office has), it answers `snapshot_required`: take a fresh
/// `org.snapshot` and subscribe from its position.
///
/// With `from_work_position` (an office only): likewise for the office's work feed. The office
/// sends the `work.changed` events after that position (e.g. the first page of a
/// `work.snapshot`), then live ones. It answers `snapshot_required` if it can't: another
/// incarnation, a position beyond its feed, or one older than the feed's 30-day retention. The
/// error doesn't say which stream: a client that gave both positions takes both snapshots
/// again. See [`crate::work`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscribeParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_position: Option<crate::org::Position>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_work_position: Option<crate::work::WorkPosition>,
}

impl Call for Subscribe {
    const METHOD: &'static str = "subscribe";
    type Params = SubscribeParams;
    type Result = Empty;
}

/// `office.list` (host): every office on this machine, with how to reach each one.
///
/// To follow office status live, subscribe first, then call `office.list` on the same
/// connection; `office.status_changed` events after its response are newer than it. Host
/// requirement: on one connection, the host writes responses and events in the order the
/// state changed, so an event is never older than a response written before it.
pub struct OfficeList;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListOfficesResult {
    pub offices: Vec<OfficeInfo>,
}

impl Call for OfficeList {
    const METHOD: &'static str = "office.list";
    type Params = Empty;
    type Result = ListOfficesResult;
}

/// `office.app_token` (host, on `host.sock` only): the office's [`AppToken`], which lets the
/// desktop app bind to that office as a human.
///
/// Before answering, the host verifies the caller's code signature against the app's
/// designated requirement (ADR-0003 §2.1); only the desktop app gets a token.
/// Errors: `not_authorized` (the signature check failed), `not_found` (no such office here),
/// `unavailable` (the office isn't running, so it has no token), `not_ready` (the host is
/// still starting: its startup discards are running; retry shortly).
///
/// The result is a secret: never log it, or the response line that carries it.
///
/// The client rule: a client reads `hello`'s answer before sending an app-only call, and sends
/// nothing else on that connection until its answer arrives. First thing in the app's main,
/// before any output or threads: fstat fds 0-2 and dup2 `/dev/null` over any that is a socket;
/// the app never reads or writes an fd it didn't open.
/// The host check: the host accepts an app-only call only if it arrived after the connection's
/// accept was sampled and nothing follows its request line (otherwise `not_authorized`).
pub struct OfficeAppToken;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeAppTokenParams {
    pub office_id: String,
}

/// `office.app_token`'s result. Its `Debug` shows no token, and it isn't `Clone`, like the
/// token.
///
/// ```compile_fail,E0599
/// use org_proto::{AppToken, OfficeAppTokenResult, APP_TOKEN_BYTES};
/// let result = OfficeAppTokenResult {
///     app_token: AppToken::from_bytes([0; APP_TOKEN_BYTES]),
/// };
/// let _copy = result.clone();
/// ```
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeAppTokenResult {
    pub app_token: AppToken,
}

impl Call for OfficeAppToken {
    const METHOD: &'static str = "office.app_token";
    type Params = OfficeAppTokenParams;
    type Result = OfficeAppTokenResult;
}

/// `office.restart` (host, on `host.sock` only): the desktop app asks the host to start an
/// office that is `offline` or `unavailable` now, without waiting for the next backoff step.
/// It clears an `unavailable` status and resets the restart backoff and the give-up window
/// (the failure count goes to 0), so the office gets its full set of retries again. It
/// answers once the start is under way; the office's new status arrives as
/// `office.status_changed` events.
///
/// It refuses the two reasons a restart can't fix: `unsupported_version` and
/// `invalid_request`. Any other reason, or none, is restarted (e.g. `key_missing`,
/// `restarts_exhausted`, `stopped_by_owner`, or an `offline` office waiting for its next
/// backoff step). `key_missing` is restarted so an owner who restores the Keychain item
/// needn't restart the host; if the key is still missing, the office goes back to
/// `unavailable` with `key_missing`, with no backoff retries (seen as `office.status_changed`
/// events), not as an error from this call.
///
/// App-only: the host verifies the caller's code signature as for [`OfficeAppToken`].
/// Errors: `not_authorized` (the signature check failed), `not_found` (no such office here),
/// `conflict` (the office isn't `offline` or `unavailable`, e.g. it's already `starting` or
/// `active`), `unavailable` (only for the two reasons a restart can't fix,
/// `unsupported_version` and `invalid_request`, or an office the host couldn't start; the
/// message says why), `not_ready` (the host
/// is still starting: its startup discards are running; retry shortly). A host older than
/// this call answers `unknown_method`. Show the office's `status_reason`, not this call's
/// error code: the code says only why the call failed.
///
/// The client rule: a client reads `hello`'s answer before sending an app-only call, and sends
/// nothing else on that connection until its answer arrives. First thing in the app's main,
/// before any output or threads: fstat fds 0-2 and dup2 `/dev/null` over any that is a socket;
/// the app never reads or writes an fd it didn't open.
/// The host check: the host accepts an app-only call only if it arrived after the connection's
/// accept was sampled and nothing follows its request line (otherwise `not_authorized`).
pub struct OfficeRestart;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeRestartParams {
    pub office_id: String,
}

impl Call for OfficeRestart {
    const METHOD: &'static str = "office.restart";
    type Params = OfficeRestartParams;
    type Result = Empty;
}
