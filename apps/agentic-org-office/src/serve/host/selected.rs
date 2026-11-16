//! Selected owner-only native route; no legacy parser or aspect-conflict adapter.
use super::*;
use org_office::selected_tasks::{
    AcceptanceIdentity, AssignmentRequest, OwnerAccount, OwnerAuthority,
};
use org_proto::selected_tasks::{decode_native_assignment, NativeAssignmentRequest};
use org_work::selected_assignment::AssignmentCommand;

/// Closed routing for methods whose original ingress bytes must reach their
/// strict decoder. These never pass through the legacy Value-based dispatch.
pub(super) enum Route {
    Adopt,
    Stage,
    Read,
    Assignment,
}
impl Route {
    pub(super) fn from_method(method: &str) -> Option<Self> {
        use org_proto::selected_tasks as contract;
        match method {
            contract::adoption::METHOD => Some(Self::Adopt),
            contract::staging::METHOD => Some(Self::Stage),
            contract::read::METHOD => Some(Self::Read),
            contract::SET_ASSIGNMENT => Some(Self::Assignment),
            _ => None,
        }
    }
    pub(super) fn answer(self, raw: &[u8], shared: &Shared, context: &HostChannel) -> Response {
        match self {
            Self::Adopt => adopt(raw, shared, context),
            Self::Stage => stage(raw, shared, context),
            Self::Read => read(raw, shared, context),
            Self::Assignment => assignment(raw, shared, context),
        }
    }
}

/// MAC verification is done once before construction, against this live host
/// channel's key. Immutable claims retain its authenticated account, never an
/// account or employee supplied by the command body. Expiry/revocation/live
/// owner state are rechecked when the sole writer executes the operation.
struct Authority {
    claims: org_proto::GrantClaims,
    instance: String,
    channel: String,
    revocations: Arc<Mutex<Revocations>>,
}
impl OwnerAuthority for Authority {
    fn verify_live(
        &self,
        identity: &AcceptanceIdentity,
    ) -> Result<OwnerAccount, org_office::OfficeError> {
        let denied = || org_office::OfficeError::Invalid("not authorized");
        if self.instance != identity.instance_id
            || self.claims.office_id != identity.office_id
            || self.claims.org_id != identity.organization_id
        {
            return Err(denied());
        }
        self.live_owner()
    }
}
impl Authority {
    fn live_owner(&self) -> Result<OwnerAccount, org_office::OfficeError> {
        let denied = || org_office::OfficeError::Invalid("not authorized");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|d| u64::try_from(d.as_millis()).ok())
            .ok_or_else(denied)?;
        if self.claims.channel_id != self.channel
            || now >= self.claims.exp
            || self
                .revocations
                .lock()
                .map_err(|_| denied())?
                .contains(&self.claims.session_id)
        {
            return Err(denied());
        }
        // The service checks the protected account/member owner binding under
        // the writer barrier, whose live host admission rechecks registry owner
        // authority. An optional canonical employee profile is unrelated.
        Ok(OwnerAccount {
            account_id: self.claims.account.clone(),
            member_id: self.claims.member.clone(),
        })
    }
}
pub(super) fn assignment(raw: &[u8], shared: &Shared, context: &HostChannel) -> Response {
    // A separately decoded ID is used only for a sanitized malformed response.
    let id = serde_json::from_slice::<Request>(raw)
        .map(|r| r.id)
        .unwrap_or(org_proto::UNREADABLE_REQUEST_ID);
    let Ok(decoded) = decode_native_assignment(raw) else {
        return refuse(id, ErrorCode::InvalidRequest);
    };
    let NativeAssignmentRequest {
        id,
        principal,
        params,
    } = decoded;
    let authority = match authorize(principal, shared, context) {
        Ok(authority) => authority,
        Err(code) => return refuse(id, code),
    };
    let Some(office) = &shared.selected else {
        return refuse(id, ErrorCode::Unavailable);
    };
    let operation = params.change_id.as_str().to_owned();
    let request = AssignmentRequest {
        operation_id: operation,
        task_id: params.task_id,
        command: AssignmentCommand {
            expected_revision: params.command.expected_revision.get(),
            assigned_team_id: params.command.assigned_team_id.0,
            assignee_id: params.command.assignee_id.0,
        },
    };
    match office.set_selected_assignment(authority, request) {
        Ok(result) => match serde_json::to_value(result) {
            Ok(value) => Response {
                id,
                outcome: org_proto::Outcome::Result(value),
            },
            Err(_) => unresolved(id),
        },
        Err(org_office::OfficeError::Invalid("not authorized")) => {
            refuse(id, ErrorCode::NotAuthorized)
        }
        Err(org_office::OfficeError::Invalid("operation reused")) => {
            refuse(id, ErrorCode::OperationReused)
        }
        Err(org_office::OfficeError::Invalid("operation expired")) => {
            refuse(id, ErrorCode::Expired)
        }
        Err(_) => unresolved(id),
    }
}

pub(super) fn lookup(raw: &[u8], shared: &Shared, context: &HostChannel) -> Response {
    let id = serde_json::from_slice::<Request>(raw)
        .map(|r| r.id)
        .unwrap_or(org_proto::UNREADABLE_REQUEST_ID);
    let Ok(decoded) = org_proto::selected_tasks::decode_native_lookup(raw) else {
        return refuse(id, ErrorCode::InvalidRequest);
    };
    let authority = match authorize(decoded.principal, shared, context) {
        Ok(authority) => authority,
        Err(code) => return refuse(id, code),
    };
    let Some(office) = &shared.selected else {
        return refuse(id, ErrorCode::Unavailable);
    };
    match office.selected_assignment_result(authority, decoded.operation_id.as_str().to_owned()) {
        Ok(Some(result)) => {
            // The selected result remains versioned content inside the existing
            // operation outcome. Definitive rejection is a durable receipt too.
            use org_proto::selected_tasks::{AssignmentRefusal, LookupOutcome};
            let outcome = match result {
                org_office::selected_tasks::AssignmentOutcome::Completed { .. } => {
                    LookupOutcome::Committed { result }
                }
                org_office::selected_tasks::AssignmentOutcome::Rejected { code, current } => {
                    LookupOutcome::Rejected {
                        refusal: AssignmentRefusal {
                            message: code.clone(),
                            code,
                            current,
                        },
                    }
                }
            };
            match serde_json::to_value(outcome) {
                Ok(value) => Response {
                    id,
                    outcome: org_proto::Outcome::Result(value),
                },
                Err(_) => unresolved(id),
            }
        }
        Ok(None) => refuse(id, ErrorCode::NotFound),
        Err(org_office::OfficeError::Invalid("operation expired")) => {
            refuse(id, ErrorCode::Expired)
        }
        Err(org_office::OfficeError::Invalid("not authorized")) => {
            refuse(id, ErrorCode::NotAuthorized)
        }
        Err(_) => unresolved(id),
    }
}

pub(super) fn read(raw: &[u8], shared: &Shared, context: &HostChannel) -> Response {
    let id = serde_json::from_slice::<Request>(raw)
        .map(|r| r.id)
        .unwrap_or(org_proto::UNREADABLE_REQUEST_ID);
    let Ok(decoded) = org_proto::selected_tasks::read::decode(raw) else {
        return refuse(id, ErrorCode::InvalidRequest);
    };
    let authority = match authorize(decoded.principal, shared, context) {
        Ok(authority) => authority,
        Err(code) => return refuse(decoded.id, code),
    };
    let Some(office) = &shared.selected else {
        return refuse(decoded.id, ErrorCode::Unavailable);
    };
    match office.read_selected_tasks(authority, decoded.query) {
        Ok(result) => Response::ok::<org_proto::selected_tasks::read::Read>(decoded.id, &result)
            .unwrap_or_else(|_| unresolved(decoded.id)),
        Err(org_office::OfficeError::Invalid("not authorized")) => {
            refuse(decoded.id, ErrorCode::NotAuthorized)
        }
        Err(org_office::OfficeError::NotFound) => refuse(decoded.id, ErrorCode::NotFound),
        Err(_) => refuse(decoded.id, ErrorCode::Unavailable),
    }
}

pub(super) fn stage(raw: &[u8], shared: &Shared, context: &HostChannel) -> Response {
    let id = serde_json::from_slice::<Request>(raw)
        .map(|r| r.id)
        .unwrap_or(org_proto::UNREADABLE_REQUEST_ID);
    let Ok(decoded) = org_proto::selected_tasks::staging::decode(raw) else {
        return refuse(id, ErrorCode::InvalidRequest);
    };
    let authority = match authorize(decoded.principal, shared, context) {
        Ok(a) => a,
        Err(code) => return refuse(id, code),
    };
    let Some(office) = &shared.selected else {
        return refuse(id, ErrorCode::Unavailable);
    };
    match office.stage_selected_sources(authority, decoded.params) {
        Ok(result) => {
            Response::ok::<org_proto::selected_tasks::staging::Stage>(decoded.id, &result)
                .unwrap_or_else(|_| unresolved(decoded.id))
        }
        Err(org_office::OfficeError::Invalid("not authorized")) => {
            refuse(decoded.id, ErrorCode::NotAuthorized)
        }
        Err(_) => unresolved(decoded.id),
    }
}

pub(super) fn adopt(raw: &[u8], shared: &Shared, context: &HostChannel) -> Response {
    let fallback = serde_json::from_slice::<Request>(raw)
        .map(|r| r.id)
        .unwrap_or(org_proto::UNREADABLE_REQUEST_ID);
    let Ok(decoded) = org_proto::selected_tasks::adoption::decode(raw) else {
        return refuse(fallback, ErrorCode::InvalidRequest);
    };
    let authority = match authorize(decoded.principal, shared, context) {
        Ok(authority) => authority,
        Err(code) => return refuse(decoded.id, code),
    };
    let Some(office) = &shared.selected else {
        return refuse(decoded.id, ErrorCode::Unavailable);
    };
    match office.adopt_selected_task(authority, decoded.action) {
        Ok(result) => {
            Response::ok::<org_proto::selected_tasks::adoption::Adopt>(decoded.id, &result)
                .unwrap_or_else(|_| unresolved(decoded.id))
        }
        Err(org_office::OfficeError::Invalid("not authorized")) => {
            refuse(decoded.id, ErrorCode::NotAuthorized)
        }
        Err(org_office::OfficeError::Invalid("operation reused")) => {
            refuse(decoded.id, ErrorCode::OperationReused)
        }
        Err(org_office::OfficeError::Invalid(
            "operation expired" | "operation outside retry window",
        )) => refuse(decoded.id, ErrorCode::Expired),
        Err(org_office::OfficeError::NotFound) => refuse(decoded.id, ErrorCode::NotFound),
        Err(_) => unresolved(decoded.id),
    }
}

fn authorize(
    principal: org_proto::selected_tasks::RawPrincipal,
    shared: &Shared,
    context: &HostChannel,
) -> Result<Arc<Authority>, ErrorCode> {
    let raw_principal = principal.raw().ok_or(ErrorCode::NotAuthorized)?;
    let principal = serde_json::from_str::<org_proto::Principal>(raw_principal.get())
        .map_err(|_| ErrorCode::NotAuthorized)?;
    let Some(office_info) = shared.hello.office.as_ref() else {
        return Err(ErrorCode::NotAuthorized);
    };
    let claims = &principal.grant.claims;
    if claims.channel_id != context._channel_id
        || claims.office_id != office_info.office.id
        || claims.org_id != office_info.office.organization_id
    {
        return Err(ErrorCode::NotAuthorized);
    }
    if !matches!(&shared.auth, LaunchAuth::Grants { _key } if crate::grant::verify(_key, &principal.grant))
    {
        return Err(ErrorCode::NotAuthorized);
    }
    let Some(instance) = context.hello.instance_id.clone() else {
        return Err(ErrorCode::NotAuthorized);
    };
    let authority = Arc::new(Authority {
        claims: principal.grant.claims,
        instance,
        channel: context._channel_id.clone(),
        revocations: Arc::clone(&context._revocations),
    });
    authority
        .live_owner()
        .map_err(|_| ErrorCode::NotAuthorized)?;
    Ok(authority)
}

// A storage/preparation/publication error cannot certify a rejection. Keep the
// original operation ID and reconcile it; never invent a replacement ID.
fn unresolved(id: u64) -> Response {
    Response::error(
        id,
        ApiError::new(
            ErrorCode::Unavailable,
            "selected outcome unresolved; reconcile the original operation ID",
        ),
    )
}
