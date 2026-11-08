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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloResult {
    pub api_version: u32,
    pub server_version: String,
    /// The machine, for display, e.g. "This Mac".
    pub host: String,
    /// Set when an office server answers; absent when this machine's host answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub office: Option<OfficeHello>,
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
/// starting: its startup discards are running; retry shortly).
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
/// connection until it closes. No filter or unsubscribe yet (a filter would be an optional
/// param, which is additive). A subscriber also receives events for its own actions.
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
/// `unsupported_version` and `invalid_request`; the message says why), `not_ready` (the host
/// is still starting: its startup discards are running; retry shortly). A host older than
/// this call answers `unknown_method`. Show the office's `status_reason`, not this call's
/// error code: the code says only why the call failed.
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
