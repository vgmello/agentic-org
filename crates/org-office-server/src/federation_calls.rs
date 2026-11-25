//! Office RPC dispatcher for HQ satellite invitations, satellite removal/revocation, and
//! replication status (`TASK-52`, `FC-020`, `FC-024`, `FC-145`).

use org_office::{CreateInvitationParams, InvitationStatus, InvitationSummaryView, Office};
use org_proto::federation::{
    OfficeDirectoryList, OfficeDirectoryListResult, OfficeInvitationActionParams,
    OfficeInvitationActionResult, OfficeInvitationApprove, OfficeInvitationApproveResult,
    OfficeInvitationCancel, OfficeInvitationCreate, OfficeInvitationCreateResult,
    OfficeInvitationDecline, OfficeInvitationList, OfficeInvitationListResult, OfficeRemove,
    OfficeRemoveResult, OfficeSyncStatus, OfficeSyncStatusResult, OrgOfficeSummary,
    WireInvitationSummary,
};
use org_proto::{ApiError, Call, ErrorCode, OfficeRole as WireOfficeRole, Request, Response};

pub(crate) fn is_federation_method(request: &Request) -> bool {
    request.is::<OfficeInvitationCreate>()
        || request.is::<OfficeInvitationList>()
        || request.is::<OfficeInvitationApprove>()
        || request.is::<OfficeInvitationDecline>()
        || request.is::<OfficeInvitationCancel>()
        || request.is::<OfficeRemove>()
        || request.is::<OfficeSyncStatus>()
        || request.is::<OfficeDirectoryList>()
}

pub(crate) fn dispatch_federation_rpc(
    office: &Office,
    member: Option<&str>,
    request: &Request,
    hq_federation_listening: bool,
) -> Option<Response> {
    if !is_federation_method(request) {
        return None;
    }
    let Some(caller) = member else {
        return Some(Response::error(
            request.id,
            ApiError::new(ErrorCode::NotBound, "connection is not bound"),
        ));
    };
    if request.is::<OfficeInvitationCreate>() {
        return Some(run_call::<OfficeInvitationCreate>(request, |p| {
            // Require an active HQ federation TLS listener before issuing invitations (`TASK-52` H-2).
            if !hq_federation_listening {
                return Err(ApiError::new(
                    ErrorCode::Unavailable,
                    "HQ federation listener is not running; configure AGENTIC_ORG_FEDERATION_BIND",
                ));
            }
            let res = map_office(office.create_satellite_invitation(
                caller,
                CreateInvitationParams {
                    office_name: p.office_name,
                    hq_addresses: p.hq_addresses,
                    ttl_secs: p.ttl_secs,
                    existing_office_id: p.existing_office_id,
                },
            ))?;
            Ok(OfficeInvitationCreateResult {
                invitation: to_wire_summary(res.invitation),
                token: res.token.to_string(),
            })
        }));
    }
    if request.is::<OfficeInvitationList>() {
        return Some(run_call::<OfficeInvitationList>(request, |_| {
            let list = map_office(office.list_satellite_invitations(caller))?;
            Ok(OfficeInvitationListResult {
                invitations: list.into_iter().map(to_wire_summary).collect(),
            })
        }));
    }
    if request.is::<OfficeInvitationApprove>() {
        return Some(run_call::<OfficeInvitationApprove>(request, |p| {
            let approved = map_office(office.approve_satellite_invitation(
                caller,
                &p.invitation_id,
                &p.check_code,
            ))?;
            Ok(OfficeInvitationApproveResult {
                invitation: to_wire_summary(approved.invitation),
                office_id: approved.registered.office_id.as_str().to_owned(),
                slug: approved.registered.slug,
                office_code: approved.registered.office_code,
                cert_serial_hex: approved.serial_hex,
            })
        }));
    }
    if request.is::<OfficeInvitationDecline>() {
        return Some(run_call::<OfficeInvitationDecline>(
            request,
            |p: OfficeInvitationActionParams| {
                let view =
                    map_office(office.decline_satellite_invitation(caller, &p.invitation_id))?;
                Ok(OfficeInvitationActionResult {
                    invitation: to_wire_summary(view),
                })
            },
        ));
    }
    if request.is::<OfficeInvitationCancel>() {
        return Some(run_call::<OfficeInvitationCancel>(
            request,
            |p: OfficeInvitationActionParams| {
                let view =
                    map_office(office.cancel_satellite_invitation(caller, &p.invitation_id))?;
                Ok(OfficeInvitationActionResult {
                    invitation: to_wire_summary(view),
                })
            },
        ));
    }
    if request.is::<OfficeRemove>() {
        return Some(run_call::<OfficeRemove>(request, |p| {
            let removed = map_office(office.remove_satellite_office(caller, &p.office_id))?;
            Ok(OfficeRemoveResult {
                office_id: removed.office_id,
                revoked_serials: removed.revoked_serials,
            })
        }));
    }
    if request.is::<OfficeSyncStatus>() {
        return Some(run_call::<OfficeSyncStatus>(request, |_| {
            let st = office
                .sync_status()
                .map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))?;
            Ok(OfficeSyncStatusResult {
                org_id: st.org_id,
                local_office: st.local_office,
                hq_office: st.hq_office,
                epoch: st.epoch,
                seq: st.seq,
                highest_epoch: st.highest_epoch,
                protocol_level: st.protocol_level,
                restore_pending: st.restore_pending,
                initial_snapshot_required: st.initial_snapshot_required,
                pending_work_count: st.pending_work_count,
                pending_message_count: st.pending_message_count,
            })
        }));
    }
    if request.is::<OfficeDirectoryList>() {
        return Some(run_call::<OfficeDirectoryList>(request, |_| {
            let org_id = office.info().org_id.clone();
            let list = office
                .list_org_offices()
                .map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))?;
            let offices = list
                .into_iter()
                .map(|o| OrgOfficeSummary {
                    id: o.id.as_str().to_owned(),
                    organization_id: org_id.clone(),
                    name: o.name,
                    slug: o.slug,
                    role: match o.role {
                        org_office::organization_family::records::OfficeRole::Hq => {
                            WireOfficeRole::Hq
                        }
                        org_office::organization_family::records::OfficeRole::Satellite => {
                            WireOfficeRole::Satellite
                        }
                    },
                })
                .collect();
            Ok(OfficeDirectoryListResult { offices })
        }));
    }
    None
}

fn to_wire_summary(v: InvitationSummaryView) -> WireInvitationSummary {
    let status = match v.status {
        InvitationStatus::Pending => "pending",
        InvitationStatus::AwaitingApproval => "awaiting_approval",
        InvitationStatus::Approved => "approved",
        InvitationStatus::Declined => "declined",
        InvitationStatus::Cancelled => "cancelled",
        InvitationStatus::Expired => "expired",
        InvitationStatus::Revoked => "revoked",
    };
    WireInvitationSummary {
        invitation_id: v.invitation_id,
        office_id: v.office_id,
        office_name: v.office_name,
        status: status.to_owned(),
        // Never expose the expected check code through HQ's API (`TASK-52` M-18): the owner must
        // read it from the verified destination machine and type it into HQ approval.
        check_code: None,
        wrong_code_attempts: v.wrong_code_attempts,
        created_at_ms: v.created_at_ms,
        expires_at_ms: v.expires_at_ms,
        issued_cert_serial_hex: v.issued_cert_serial_hex,
    }
}

fn map_office<T>(res: Result<Result<T, ApiError>, org_office::OfficeError>) -> Result<T, ApiError> {
    res.map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))
        .and_then(|r| r)
}

fn run_call<C: Call>(
    request: &Request,
    handler: impl FnOnce(C::Params) -> Result<C::Result, ApiError>,
) -> Response {
    let params = match request.params::<C>() {
        Ok(p) => p,
        Err(_) => {
            return Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    format!("invalid parameters for `{}`", C::METHOD),
                ),
            );
        }
    };
    match handler(params) {
        Ok(result) => Response::ok::<C>(request.id, &result).unwrap_or_else(|_| {
            Response::error(
                request.id,
                ApiError::new(ErrorCode::StorageError, "failed encoding response"),
            )
        }),
        Err(err) => Response::error(request.id, err),
    }
}

#[cfg(test)]
mod directory_rpc_tests {
    use super::*;
    use org_office::{EngineChoice, TestOffice};
    use org_proto::federation::OfficeDirectoryListParams;

    #[test]
    fn office_directory_rpc_returns_registered_offices_and_requires_bound_caller() {
        let office = Office::for_test(TestOffice {
            engine: EngineChoice::Memory,
        });
        let req = Request::new::<OfficeDirectoryList>(1, &OfficeDirectoryListParams {}).unwrap();

        let unbound = dispatch_federation_rpc(&office, None, &req, false).unwrap();
        assert!(matches!(
            unbound.outcome,
            org_proto::Outcome::Error(ref e) if e.code == ErrorCode::NotBound
        ));

        let owner = office.owner_member_id().to_owned();
        let bound = dispatch_federation_rpc(&office, Some(&owner), &req, false).unwrap();
        let org_proto::Outcome::Result(val) = bound.outcome else {
            panic!("expected Ok result, got {:?}", bound.outcome);
        };
        let list: OfficeDirectoryListResult = serde_json::from_value(val).unwrap();
        assert_eq!(list.offices.len(), 1);
        assert_eq!(list.offices[0].id, office.info().office_id);
        assert_eq!(list.offices[0].organization_id, office.info().org_id);
        assert_eq!(list.offices[0].role, WireOfficeRole::Hq);
    }
}
