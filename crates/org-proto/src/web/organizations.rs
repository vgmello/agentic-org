//! Creating and listing organizations over `/api/v1` (web-api-v1 §3, "Selected
//! create-organization payload"): what the browser sends, what it reads back, and the
//! refusals the web process decides before any host call.
//!
//! The recovery code is never part of this contract; only where it is ([`RecoveryState`]).
use serde::{Deserialize, Serialize};

use super::{
    CommandEnvelope, CredentialField, CredentialRule, OfficeView, OperationId, RecoveryState,
    WebError, WebErrorCode,
};
use crate::selected_bootstrap::PlanV1;

/// The business payload of `POST /api/v1/organizations`. There is no team field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectedCreateParams {
    pub name: String,
    pub slug: String,
    pub organization_repository: RepositoryRegistration,
    pub state_branch: String,
}

/// Registration of the Git repository that holds the organization's files. Only a remote
/// is accepted, and never a secret value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryRegistration {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_url: Option<String>,
    /// A local path. Read only so its refusal can name the repository field: a repository
    /// on the host's own disk is never accepted, under any kind.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub path: Option<serde_json::Value>,
    /// A reference to an office-held credential. Not supported yet: its presence is refused,
    /// whatever its value.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub credential_ref: Option<serde_json::Value>,
}

/// A key that is present is `Some`, whatever its value: `"path": null` is still a path
/// key, and `"credential_ref": null` still a credential reference; each is refused like
/// any other value of its key.
fn present<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<serde_json::Value>, D::Error> {
    serde_json::Value::deserialize(deserializer).map(Some)
}

pub type SelectedCreateRequest = CommandEnvelope<SelectedCreateParams>;

/// A `400 invalid_request` that names the input and the rule it broke.
pub fn input_refusal(field: CredentialField, rule: CredentialRule) -> WebError {
    let mut error = WebError::new(
        WebErrorCode::InvalidRequest,
        "This doesn't meet the organization rules.",
    );
    (error.field, error.rule) = (Some(field), Some(rule));
    error
}

/// A host operation id is a canonical lowercase UUIDv7 the browser minted. Anything else is
/// refused before any host call.
pub fn host_operation_id(raw: &str) -> Result<OperationId, WebError> {
    OperationId::parse(raw.to_owned())
        .map_err(|_| input_refusal(CredentialField::OperationId, CredentialRule::Format))
}

impl SelectedCreateParams {
    /// The host's plan for this payload, or the first rule it breaks. Syntax only: nothing
    /// here contacts the remote.
    pub fn plan(&self) -> Result<PlanV1, WebError> {
        let repository = &self.organization_repository;
        let remote_required = || {
            input_refusal(
                CredentialField::OrganizationRepository,
                CredentialRule::RemoteRequired,
            )
        };
        if repository.kind != "remote" || repository.path.is_some() {
            return Err(remote_required());
        }
        if repository.credential_ref.is_some() {
            return Err(input_refusal(
                CredentialField::OrganizationRepository,
                CredentialRule::Unsupported,
            ));
        }
        let plan = PlanV1 {
            version: 1,
            name: self.name.clone(),
            slug: self.slug.clone(),
            repository_remote_url: repository.remote_url.clone().ok_or_else(remote_required)?,
            state_branch: self.state_branch.clone(),
        };
        match plan.invalid_field() {
            None => Ok(plan),
            Some((field, rule)) => {
                let named = |text: &str| serde_json::Value::String(text.to_owned());
                Err(input_refusal(
                    serde_json::from_value(named(field)).unwrap_or(CredentialField::Unknown),
                    serde_json::from_value(named(rule)).unwrap_or(CredentialRule::Unknown),
                ))
            }
        }
    }
}

/// One organization the account can open: the organization view with its slug, the
/// account's person and the organization's offices on this instance, HQ first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedOrganizationView {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub created_at: String,
    pub employee_id: String,
    pub offices: Vec<OfficeView>,
    /// State only. The recovery code must never enter the browser contract.
    pub recovery: RecoveryState,
}

/// A create the account started that has not completed. `GET /api/v1/operations/host/{id}`
/// with its `operation_id` follows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatingOrganization {
    pub operation_id: OperationId,
    pub name: String,
    pub slug: String,
}

/// `GET /api/v1/organizations`: the organizations the account is linked to, and its creates
/// still in flight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedOrganizationList {
    pub organizations: Vec<SelectedOrganizationView>,
    pub creating: Vec<CreatingOrganization>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// The shared vectors: what each request body becomes, and that each view reads back
    /// as written.
    #[test]
    fn the_vectors_decide_every_request_and_round_trip_every_view() {
        let vectors: Value = serde_json::from_str(include_str!(
            "../../test-vectors/v2/gateway-organizations.json"
        ))
        .unwrap();
        assert_eq!(vectors["schema_version"], 1);
        let cases = vectors["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 18);
        for case in cases {
            let name = &case["name"];
            let expect = &case["expect"];
            match case["category"].as_str().unwrap() {
                "create_request" => {
                    let body = serde_json::to_vec(&case["wire"]).unwrap();
                    let raw = case["wire"]["operation_id"].as_str().unwrap_or_default();
                    let decided = host_operation_id(raw).and_then(|_| {
                        super::super::decode_request::<SelectedCreateRequest>(&body)
                            .and_then(|request| request.params.plan())
                    });
                    match decided {
                        Ok(plan) => {
                            assert_eq!(expect["valid"], true, "{name}");
                            assert_eq!(serde_json::to_value(plan).unwrap(), expect["plan"]);
                        }
                        Err(error) => {
                            assert_eq!(expect["valid"], false, "{name}");
                            let error = serde_json::to_value(error).unwrap();
                            assert_eq!(error["code"], "invalid_request", "{name}");
                            assert_eq!(error.get("field"), expect.get("field"), "{name}");
                            assert_eq!(error.get("rule"), expect.get("rule"), "{name}");
                            assert_eq!(expect["http_status"], 400, "{name}");
                        }
                    }
                }
                "organization" => {
                    let view: Result<SelectedOrganizationView, _> =
                        serde_json::from_value(case["wire"].clone());
                    assert_eq!(view.is_ok(), expect["valid"] == true, "{name}");
                    if let Ok(view) = view {
                        assert_eq!(serde_json::to_value(view).unwrap(), expect["normalized"]);
                    }
                }
                "list" => {
                    let list: SelectedOrganizationList =
                        serde_json::from_value(case["wire"].clone()).unwrap();
                    assert_eq!(serde_json::to_value(list).unwrap(), expect["normalized"]);
                }
                other => panic!("unknown category {other}"),
            }
        }
    }

    #[test]
    fn a_local_path_is_refused_by_the_repository_field_under_any_kind() {
        for repository in [
            serde_json::json!({ "kind": "local", "path": "/tmp/example" }),
            serde_json::json!({ "kind": "remote", "path": "/tmp/example",
                "remote_url": "https://example.test/example.git" }),
            serde_json::json!({ "kind": "remote", "path": null,
                "remote_url": "https://example.test/example.git" }),
        ] {
            let params: SelectedCreateParams = serde_json::from_value(serde_json::json!({
                "name": "Example", "slug": "example", "state_branch": "org/state",
                "organization_repository": repository,
            }))
            .unwrap();
            let error = serde_json::to_value(params.plan().unwrap_err()).unwrap();
            assert_eq!(error["field"], "organization_repository");
            assert_eq!(error["rule"], "remote_required");
        }
    }

    #[test]
    fn a_view_never_carries_a_private_endpoint_or_a_recovery_code() {
        let view = serde_json::json!({
            "id": "0193a5c0-0000-7000-8000-000000000001", "name": "Acme", "slug": "acme",
            "created_at": "2026-11-10T12:00:00.000Z",
            "employee_id": "0193a5c0-0000-7000-8000-000000000006",
            "offices": [{"id": "0193a5c0-0000-7000-8000-000000000002",
                "organization_id": "0193a5c0-0000-7000-8000-000000000001", "name": "HQ",
                "role": "hq", "host": "This Mac", "status": "active",
                "endpoint": "/private/office.sock"}],
            "recovery": "pending", "recovery_code": "0000-0000",
        });
        let read: SelectedOrganizationView = serde_json::from_value(view).unwrap();
        let written = serde_json::to_string(&read).unwrap();
        assert!(!written.contains("endpoint") && !written.contains("recovery_code"));
    }
}
