//! Explicit selected provisioning contract; the name-only legacy operation is unchanged.
use crate::{journal::Journaled, HostOpResult, SessionHandle};
use org_proto::{selected_bootstrap::PlanV1, Call};
use serde::{Deserialize, Serialize};

pub struct OrganizationProvision;
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Params {
    pub session: SessionHandle,
    pub operation_id: String,
    pub plan: PlanV1,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provisioned {
    pub organization_id: String,
    pub office_id: String,
    pub home_team_id: String,
    /// Local syntax was verified; no remote probe or publication occurred.
    pub remote_readiness: RemoteReadiness,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteReadiness {
    Unverified,
}
impl Call for OrganizationProvision {
    const METHOD: &'static str = "organization.provision.v1";
    type Params = Params;
    type Result = HostOpResult<Provisioned>;
}
impl Journaled for Params {
    const METHOD: &'static str = OrganizationProvision::METHOD;
    fn operation_id(&self) -> &str {
        &self.operation_id
    }
    fn fingerprint_fields(&self) -> Result<String, serde_json::Error> {
        #[derive(Serialize)]
        struct Fields<'a> {
            plan: &'a PlanV1,
        }
        serde_json::to_string(&Fields { plan: &self.plan })
    }
}
