use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::types::{OfficeInfo, Organization};

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
    pub office: OfficeInfo,
}

impl Call for Hello {
    const METHOD: &'static str = "hello";
    type Params = Empty;
    type Result = HelloResult;
}

/// `organization.create`: create an empty organization on this office server.
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

/// `organization.list`: every organization this office server hosts.
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

impl Call for Subscribe {
    const METHOD: &'static str = "subscribe";
    type Params = Empty;
    type Result = Empty;
}
