//! Creating and listing organizations for a signed-in browser (ADR-0007, ADR-0011), served
//! only on the admitted web process's control connection. Every call carries the browser's
//! pair digests and origin, exactly as `session.validate` does: the host finds the live session
//! itself and never takes an account id from the web process.
//!
//! **`organization.create` is asynchronous.** The host admits the request (validates the plan,
//! reserves the name, the slug and every id, and stages the recovery code in its own custody),
//! answers `accepted`, and creates the organization on its own thread. The same call with the
//! same `operation_id`, or [`OperationLookup`], reports progress: `accepted` while the create is
//! in flight (it starts the work again if nothing is running), `completed` with the
//! organization once its HQ office reported `created` and the host adopted it.
//!
//! Refusals: `not_authorized` (no live session, or the account is not the administrator),
//! `invalid_request` with `field` and `rule` for a plan rule, `already_exists` with `field`
//! (`name` or `slug`) before anything is stored, `operation_reused` (the `operation_id` names
//! another account's or another plan's create), `expired`, `clock_skew`, `not_found`
//! ([`OperationLookup`] for an id the account doesn't own), and `unavailable` or
//! `storage_error`, after which the same `operation_id` continues the same create.
//!
//! The recovery code is never in any of these results; only where it is ([`RecoveryState`]).
use org_proto::{selected_bootstrap::PlanV1, Call, EventKind, OfficeInfo};
use serde::{Deserialize, Serialize};

use crate::browser::{BrowserOrigin, Digest32};
use crate::journal::Journaled;
use crate::ops::RecoveryState;
use crate::HostOpResult;

/// The browser session a call acts for: the pair digests and the origin they are bound to.
/// When `local_authless` is set on a local plain-HTTP instance whose authentication is disabled,
/// the host authorizes the implicit local owner account and ignores the sentinel digests; the
/// host refuses `local_authless` whenever authentication is enabled or the listener is HTTPS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserAuth {
    pub cookie_digest: Digest32,
    pub proof_digest: Digest32,
    pub origin: BrowserOrigin,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub local_authless: bool,
}

impl BrowserAuth {
    /// Constructs a local-authless browser auth envelope for an unauthenticated local HTTP HQ.
    pub fn local_authless(origin: BrowserOrigin) -> Self {
        Self {
            cookie_digest: Digest32([0u8; 32]),
            proof_digest: Digest32([0u8; 32]),
            origin,
            local_authless: true,
        }
    }
}

/// `organization.create`: admit a create, or report on the one `operation_id` names.
pub struct Create;
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub auth: BrowserAuth,
    /// A ULID the browser minted for this attempt.
    pub operation_id: String,
    pub plan: PlanV1,
}
impl Call for Create {
    const METHOD: &'static str = "organization.create";
    type Params = CreateParams;
    type Result = HostOpResult<OrganizationView>;
}
impl Journaled for CreateParams {
    const METHOD: &'static str = Create::METHOD;
    fn operation_id(&self) -> &str {
        &self.operation_id
    }
    /// The plan only: a retry after a new sign-in is the same create.
    fn fingerprint_fields(&self) -> Result<String, serde_json::Error> {
        #[derive(Serialize)]
        struct Fields<'a> {
            plan: &'a PlanV1,
        }
        let CreateParams {
            auth: _,
            operation_id: _,
            plan,
        } = self;
        serde_json::to_string(&Fields { plan })
    }
}

/// `operation.lookup`: the state of one of the account's own host operations. It never
/// creates anything new; for a create still in flight it restarts the work if none is running.
pub struct OperationLookup;
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationLookupParams {
    pub auth: BrowserAuth,
    pub operation_id: String,
}
impl Call for OperationLookup {
    const METHOD: &'static str = "operation.lookup";
    type Params = OperationLookupParams;
    type Result = HostOpResult<OrganizationView>;
}

/// `organization.list`: the organizations the account is linked to, and its creates that are
/// still in flight, so a page opened later can resume them without a stored id.
pub struct List;
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    pub auth: BrowserAuth,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListResult {
    pub organizations: Vec<OrganizationView>,
    pub creating: Vec<CreatingOrganization>,
}
impl Call for List {
    const METHOD: &'static str = "organization.list";
    type Params = ListParams;
    type Result = ListResult;
}

/// One organization the account can open. No path, socket, key or recovery code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganizationView {
    pub id: String,
    pub name: String,
    pub slug: String,
    /// RFC 3339 UTC with milliseconds.
    pub created_at: String,
    /// The account's person in this organization.
    pub employee_id: String,
    /// The organization's offices on this host, HQ first. `endpoint` is never set.
    pub offices: Vec<OfficeInfo>,
    pub recovery: RecoveryState,
}

/// A create the account started that has not completed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatingOrganization {
    pub operation_id: String,
    pub name: String,
    pub slug: String,
}

/// `organization.open_preview`: clone and validate an existing organization's Git state branch
/// read-only (`TASK-39` / R1) before registering it on this host.
pub struct OpenPreview;
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenPreviewParams {
    pub auth: BrowserAuth,
    pub repository_remote_url: String,
    pub state_branch: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenPreviewPerson {
    pub id: String,
    pub name: String,
    pub handle: String,
    pub home_team_id: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenPreviewResult {
    pub organization_id: String,
    pub name: String,
    pub slug: String,
    pub repository_remote_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_remote_url: Option<String>,
    pub state_branch: String,
    pub teams_count: usize,
    pub projects_count: usize,
    pub agents_count: usize,
    pub people: Vec<OpenPreviewPerson>,
    pub local_only_not_restored: Vec<String>,
}
impl Call for OpenPreview {
    const METHOD: &'static str = "organization.open_preview";
    type Params = OpenPreviewParams;
    type Result = OpenPreviewResult;
}

/// `organization.open`: open an existing organization from its Git state branch into a new HQ
/// on this host (`TASK-39` / R1, `doc-61` D25), binding the administrator to `owner_member_id`.
pub struct OpenExisting;
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenExistingParams {
    pub auth: BrowserAuth,
    pub operation_id: String,
    pub repository_remote_url: String,
    pub state_branch: String,
    pub owner_member_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_owner_person: Option<org_proto::selected_bootstrap::CreatorV1>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenExistingResult {
    pub organization: OrganizationView,
    pub local_only_not_restored: Vec<String>,
}
impl Call for OpenExisting {
    const METHOD: &'static str = "organization.open";
    type Params = OpenExistingParams;
    type Result = OpenExistingResult;
}
impl Journaled for OpenExistingParams {
    const METHOD: &'static str = OpenExisting::METHOD;
    fn operation_id(&self) -> &str {
        &self.operation_id
    }
    fn fingerprint_fields(&self) -> Result<String, serde_json::Error> {
        #[derive(Serialize)]
        struct Fields<'a> {
            repository_remote_url: &'a str,
            state_branch: &'a str,
            owner_member_id: &'a str,
            new_owner_person: &'a Option<org_proto::selected_bootstrap::CreatorV1>,
        }
        let OpenExistingParams {
            auth: _,
            operation_id: _,
            repository_remote_url,
            state_branch,
            owner_member_id,
            new_owner_person,
        } = self;
        serde_json::to_string(&Fields {
            repository_remote_url,
            state_branch,
            owner_member_id,
            new_owner_person,
        })
    }
}

/// `account.changed`: what the account can open changed (an organization was linked). The web
/// process drops that account's cached sessions.
pub struct AccountChanged;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountChangedData {
    pub account_id: String,
}
impl EventKind for AccountChanged {
    const NAME: &'static str = crate::event::ACCOUNT_CHANGED;
    type Data = AccountChangedData;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal;

    fn auth(byte: u8) -> BrowserAuth {
        BrowserAuth {
            cookie_digest: Digest32([byte; 32]),
            proof_digest: Digest32([byte.wrapping_add(1); 32]),
            origin: BrowserOrigin {
                scheme: "http".into(),
                host: "127.0.0.1".into(),
                port: 8080,
            },
            local_authless: false,
        }
    }
    fn params(byte: u8, operation_id: &str) -> CreateParams {
        CreateParams {
            auth: auth(byte),
            operation_id: operation_id.into(),
            plan: PlanV1 {
                version: 1,
                name: "Example".into(),
                slug: "example".into(),
                repository_remote_url: "https://example.test/org.git".into(),
                state_branch: "org/state".into(),
            },
        }
    }

    #[test]
    fn a_create_is_the_same_operation_after_a_new_sign_in_and_another_plan_is_not() {
        let first = journal::fingerprint(&params(1, "01ARZ3NDEKTSV4RRFFQ69G5FAV")).unwrap();
        let again = journal::fingerprint(&params(9, "01ARZ3NDEKTSV4RRFFQ69G5FAW")).unwrap();
        assert_eq!(first, again);
        let mut other = params(1, "01ARZ3NDEKTSV4RRFFQ69G5FAV");
        other.plan.slug = "other".into();
        assert_ne!(first, journal::fingerprint(&other).unwrap());
        assert_eq!(
            String::from_utf8(journal::fingerprint_input(&params(1, "x")).unwrap()).unwrap(),
            "organization.create\0{\"plan\":{\"version\":1,\"name\":\"Example\",\"slug\":\"example\",\"repository_remote_url\":\"https://example.test/org.git\",\"state_branch\":\"org/state\"}}"
        );
    }

    #[test]
    fn the_wire_shapes_are_fixed_and_refuse_unknown_fields() {
        let wire = serde_json::to_value(params(1, "01ARZ3NDEKTSV4RRFFQ69G5FAV")).unwrap();
        assert_eq!(wire["auth"]["cookie_digest"], "01".repeat(32));
        assert_eq!(wire["plan"]["slug"], "example");
        let mut extra = wire.clone();
        extra["session"] = "handle".into();
        assert!(serde_json::from_value::<CreateParams>(extra).is_err());
        let mut extra = wire;
        extra["plan"]["home_team_name"] = "Core".into();
        assert!(serde_json::from_value::<CreateParams>(extra).is_err());
        assert_eq!(Create::METHOD, "organization.create");
        assert_eq!(List::METHOD, "organization.list");
        assert_eq!(OperationLookup::METHOD, "operation.lookup");
        assert_eq!(AccountChanged::NAME, "account.changed");
        let accepted: HostOpResult<OrganizationView> = HostOpResult::Accepted;
        assert_eq!(
            serde_json::to_value(&accepted).unwrap(),
            serde_json::json!({"status": "accepted"})
        );
        let view = OrganizationView {
            id: "o".into(),
            name: "Example".into(),
            slug: "example".into(),
            created_at: "2026-11-08T02:10:00.000Z".into(),
            employee_id: "e".into(),
            offices: vec![],
            recovery: RecoveryState::Pending,
        };
        let text = serde_json::to_string(&HostOpResult::<_>::Completed { result: view }).unwrap();
        assert!(text.contains("\"recovery\":\"pending\"") && !text.contains("code"));
    }
}
