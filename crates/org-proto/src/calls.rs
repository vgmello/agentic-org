use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

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
/// Errors: `invalid_name`, `already_exists`, `storage_error`.
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscribeParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_position: Option<crate::org::Position>,
}

impl Call for Subscribe {
    const METHOD: &'static str = "subscribe";
    type Params = SubscribeParams;
    type Result = Empty;
}

/// `office.list` (host): every office on this machine, with how to reach each one.
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
