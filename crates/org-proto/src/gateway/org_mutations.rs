//! Strict organization mutation inputs and pure native reply classification.
//! HTTP routing, sessions, grants and operation enablement belong to the gateway composition.

use std::fmt;

use serde::de::Error as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    GatewayError, GatewayErrorCode, MutationOutcome, OperationId, Scope, MAX_SAFE_INTEGER,
};
use crate::{org, Call, Empty, Request};

/// Browser versions share one nonnegative JavaScript-safe range; native versions remain u64.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct ExpectedVersion(u64);

impl ExpectedVersion {
    pub fn new(value: u64) -> Result<Self, &'static str> {
        (value <= MAX_SAFE_INTEGER)
            .then_some(Self(value))
            .ok_or("expected_version must be a nonnegative safe integer")
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

impl From<ExpectedVersion> for u64 {
    fn from(value: ExpectedVersion) -> Self {
        value.get()
    }
}

impl<'de> Deserialize<'de> for ExpectedVersion {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let number = serde_json::Number::deserialize(d)?;
        let value = number.as_u64().or_else(|| {
            let value = number.as_f64()?;
            (value.is_finite()
                && value >= 0.0
                && value <= MAX_SAFE_INTEGER as f64
                && value.fract() == 0.0)
                .then_some(value as u64)
        });
        Self::new(value.ok_or_else(|| D::Error::custom("invalid expected_version"))?)
            .map_err(D::Error::custom)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMutation {
    operation_id: OperationId,
    scope: OrganizationScope,
    params: Value,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum OrganizationScope {
    Organization { instance_id: String, org_id: String },
}

impl OrganizationScope {
    fn into_scope(self) -> Scope {
        match self {
            Self::Organization {
                instance_id,
                org_id,
            } => Scope::Organization {
                instance_id,
                org_id,
            },
        }
    }
}

fn invalid() -> GatewayError {
    GatewayError::new(
        GatewayErrorCode::InvalidRequest,
        "Invalid organization mutation.",
    )
}

macro_rules! mutations {
    ($( $variant:ident, $params:ident, $call:ident, $native:ident, $name:literal {
        $( $(#[$attr:meta])* $field:ident: $ty:ty, )*
    }; )*) => {
        /// Closed operations selected by a trusted route, never by a browser body.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        pub enum OrgOperation {
            $( #[serde(rename = $name)] $variant, )*
        }

        $(
            /// Strict browser params, without a second command identity.
            #[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
            #[serde(deny_unknown_fields)]
            pub struct $params {
                $( $(#[$attr])* pub $field: $ty, )*
            }
        )*

        enum NativeCommand {
            $( $variant(org::$native), )*
        }

        impl OrgOperation {
            pub const ALL: &'static [Self] = &[$(Self::$variant,)*];

            pub fn name(self) -> &'static str {
                match self { $( Self::$variant => $name, )* }
            }

            /// Explicit typed mapping. No string rewriting or generic RPC tunnel.
            pub fn native_method(self) -> &'static str {
                match self { $( Self::$variant => <org::$call as Call>::METHOD, )* }
            }

            /// Decode after the composition has selected/authenticated the route.
            /// All failures are sanitized and happen before native dispatch.
            pub fn decode(self, bytes: &[u8], route_scope: &Scope) -> Result<OrgMutation, GatewayError> {
                let value: Value = serde_json::from_slice(bytes).map_err(|_| invalid())?;
                if !value.is_object() {
                    return Err(invalid());
                }
                let raw: RawMutation = serde_json::from_value(value).map_err(|_| invalid())?;
                let scope = raw.scope.into_scope();
                if &scope != route_scope || !raw.params.is_object() {
                    return Err(invalid());
                }
                let operation_id = raw.operation_id;
                let native = match self {
                    $( Self::$variant => {
                        let params: $params = serde_json::from_value(raw.params).map_err(|_| invalid())?;
                        NativeCommand::$variant(org::$native {
                            command_id: operation_id.as_str().to_owned(),
                            $( $field: params.$field.into(), )*
                        })
                    }, )*
                };
                Ok(OrgMutation { operation: self, operation_id, scope, native })
            }
        }

        impl OrgMutation {
            /// The numeric RPC ID is transport correlation; command_id remains operation_id.
            pub fn native_request(&self, rpc_id: u64) -> Result<Request, serde_json::Error> {
                match &self.native {
                    $( NativeCommand::$variant(params) => Request::new::<org::$call>(rpc_id, params), )*
                }
            }
        }
    }
}

mutations! {
    TeamCreate, TeamCreateParams, TeamCreate, TeamCreateParams, "team.create" {
        team_id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        owner_team_id: Option<String>,
    };
    TeamRename, TeamRenameParams, TeamRename, TeamRenameParams, "team.rename" {
        team_id: String,
        expected_version: ExpectedVersion,
        name: String,
    };
    TeamMove, TeamMoveParams, TeamMove, TeamMoveParams, "team.move" {
        team_id: String,
        expected_version: ExpectedVersion,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        owner_team_id: Option<String>,
    };
    TeamSetLead, TeamSetLeadParams, TeamSetLead, TeamSetLeadParams, "team.set_lead" {
        team_id: String,
        expected_version: ExpectedVersion,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lead_member_id: Option<String>,
    };
    TeamDelete, TeamDeleteParams, TeamDelete, TeamDeleteParams, "team.delete" {
        team_id: String,
        expected_version: ExpectedVersion,
    };
    TeamAddMember, TeamAddMemberParams, TeamAddMember, TeamAddMemberParams, "team.add_member" {
        membership_id: String,
        team_id: String,
        member_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title_in_team: Option<String>,
    };
    TeamRemoveMember, TeamRemoveMemberParams, TeamRemoveMember, TeamRemoveMemberParams, "team.remove_member" {
        membership_id: String,
        expected_version: ExpectedVersion,
    };
    TeamSetTitle, TeamSetTitleParams, TeamSetTitle, TeamSetTitleParams, "team.set_title" {
        membership_id: String,
        expected_version: ExpectedVersion,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title_in_team: Option<String>,
    };
    MemberMove, MemberMoveParams, MemberMove, MemberMoveParams, "member.move" {
        membership_id: String,
        expected_version: ExpectedVersion,
        new_membership_id: String,
        to_team_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title_in_team: Option<String>,
    };
    MemberAddPerson, MemberAddPersonParams, MemberAddPerson, MemberAddPersonParams, "member.add_person" {
        member_id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        contact: Option<String>,
    };
    MemberUpdatePerson, MemberUpdatePersonParams, MemberUpdatePerson, MemberUpdatePersonParams, "member.update_person" {
        member_id: String,
        expected_version: ExpectedVersion,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        contact: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        clear_contact: bool,
    };
    MemberRemove, MemberRemoveParams, MemberRemove, MemberRemoveParams, "member.remove" {
        member_id: String,
        expected_version: ExpectedVersion,
    };
    MemberSetReportsTo, MemberSetReportsToParams, MemberSetReportsTo, MemberSetReportsToParams, "member.set_reports_to" {
        member_id: String,
        expected_version: ExpectedVersion,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reports_to_member_id: Option<String>,
    };
    ProjectAddTeam, ProjectAddTeamParams, ProjectAddTeam, ProjectAddTeamParams, "project.add_team" {
        project_team_id: String,
        project_id: String,
        team_id: String,
    };
    ProjectRemoveTeam, ProjectRemoveTeamParams, ProjectRemoveTeam, ProjectRemoveTeamParams, "project.remove_team" {
        project_team_id: String,
        expected_version: ExpectedVersion,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        areas_to_team_id: Option<String>,
    };
}

/// Validated mutation. Private fields keep scope and command identity together.
pub struct OrgMutation {
    operation: OrgOperation,
    operation_id: OperationId,
    scope: Scope,
    native: NativeCommand,
}

impl OrgMutation {
    pub fn operation(&self) -> OrgOperation {
        self.operation
    }
    pub fn operation_id(&self) -> &OperationId {
        &self.operation_id
    }
    pub fn scope(&self) -> &Scope {
        &self.scope
    }
    pub fn native_method(&self) -> &'static str {
        self.operation.native_method()
    }
}

impl fmt::Debug for OrgMutation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OrgMutation")
            .field("operation", &self.operation)
            .field("operation_id", &self.operation_id)
            .field("scope", &self.scope)
            .finish_non_exhaustive()
    }
}

/// Safe diagnostics for composition logging; never contains the native result or business data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrgReplyIssue {
    NoAnswer,
    Undecodable,
    CommittedWithoutPosition,
    Queued,
    UnknownStatus,
    UnknownErrorCode,
    UnexpectedRefusal,
}

#[derive(Deserialize)]
struct NativeRefusal {
    code: GatewayErrorCode,
    message: String,
}

/// Authorization recheck is deliberately outside the serializable browser outcome.
/// B1-06 must resolve it through session/grant authority before choosing an HTTP response.
///
/// ```compile_fail
/// use org_proto::gateway::org_mutations::OrgReplyClassification;
/// let _ = serde_json::to_value(OrgReplyClassification::NeedsAuthorizationRecheck);
/// ```
#[derive(Debug)]
pub enum OrgReplyClassification {
    Outcome {
        outcome: MutationOutcome<Empty>,
        issue: Option<OrgReplyIssue>,
    },
    NeedsAuthorizationRecheck,
}

/// Classify a native business reply already correlated by the transport.
/// No answer means possible dispatch, never a confirmed refusal. This function neither
/// authorizes nor logs, and never produces accepted/202 or exposes a native position.
pub fn classify_org_reply(
    operation_id: OperationId,
    reply: Option<&Value>,
) -> OrgReplyClassification {
    let unknown = |issue| OrgReplyClassification::Outcome {
        outcome: MutationOutcome::Unknown {
            operation_id: operation_id.clone(),
        },
        issue: Some(issue),
    };
    let Some(reply) = reply else {
        return unknown(OrgReplyIssue::NoAnswer);
    };
    let Some(object) = reply.as_object() else {
        return unknown(OrgReplyIssue::Undecodable);
    };
    if object.get("id").and_then(Value::as_u64).is_none() {
        return unknown(OrgReplyIssue::Undecodable);
    }
    match (object.get("result"), object.get("error")) {
        (Some(result), None) => {
            if !result.is_object() {
                return unknown(OrgReplyIssue::Undecodable);
            }
            let Ok(accepted) = org::CommandAccepted::deserialize(result) else {
                return unknown(OrgReplyIssue::Undecodable);
            };
            match accepted.status {
                org::CommandStatus::Committed if accepted.position.is_some() => {
                    OrgReplyClassification::Outcome {
                        outcome: MutationOutcome::Completed {
                            operation_id,
                            result: Empty {},
                        },
                        issue: None,
                    }
                }
                org::CommandStatus::Committed => unknown(OrgReplyIssue::CommittedWithoutPosition),
                org::CommandStatus::Queued => unknown(OrgReplyIssue::Queued),
                org::CommandStatus::Unknown => unknown(OrgReplyIssue::UnknownStatus),
            }
        }
        (None, Some(error)) => {
            if !error.is_object() {
                return unknown(OrgReplyIssue::Undecodable);
            }
            let Ok(error) = NativeRefusal::deserialize(error) else {
                return unknown(OrgReplyIssue::Undecodable);
            };
            // Native business errors cannot supply gateway session/cancellation hints.
            match &error.code {
                GatewayErrorCode::NotAuthorized => {
                    OrgReplyClassification::NeedsAuthorizationRecheck
                }
                GatewayErrorCode::Unknown => unknown(OrgReplyIssue::UnknownErrorCode),
                GatewayErrorCode::Other(_)
                | GatewayErrorCode::UnknownMethod
                | GatewayErrorCode::RateLimited => OrgReplyClassification::Outcome {
                    outcome: MutationOutcome::Rejected {
                        operation_id,
                        error: GatewayError::new(
                            GatewayErrorCode::Unavailable,
                            "The office could not accept this operation.",
                        ),
                    },
                    issue: Some(OrgReplyIssue::UnexpectedRefusal),
                },
                _ => OrgReplyClassification::Outcome {
                    outcome: MutationOutcome::Rejected {
                        operation_id,
                        error: GatewayError::new(error.code, error.message),
                    },
                    issue: None,
                },
            }
        }
        _ => unknown(OrgReplyIssue::Undecodable),
    }
}

#[cfg(test)]
mod tests;
