//! Session RPC hooks: proxying a satellite employee's session RPCs to HQ after authorizing the caller.

use std::sync::Arc;

use crate::federation_runtime::HqFederationSlot;
use crate::{federation_stream, serve};
use org_office::Office;
use org_proto::external_auth as ext;

pub(crate) struct SessionHooks {
    pub(crate) sessions: serve::SessionsFn,
    pub(crate) run_binding_verify: serve::RunBindingVerifyFn,
    pub(crate) run_binding_active: serve::RunBindingActiveFn,
    pub(crate) record_agent_command: serve::RecordCommandFn,
    pub(crate) on_agent_stop: serve::AgentLifecycleHookFn,
}

fn proxied_session_target_employee(request: &org_proto::Request) -> Option<String> {
    use org_proto::sessions::{
        MemberSessionApprovalDecide, MemberSessionClearStall, MemberSessionGrants,
        MemberSessionInspect, MemberSessionList, MemberSessionStart, MemberSessionStop,
        MemberSessionTranscript,
    };
    if request.is::<MemberSessionStart>() {
        request
            .params::<MemberSessionStart>()
            .ok()
            .map(|p| p.employee_id)
    } else if request.is::<MemberSessionStop>() {
        request
            .params::<MemberSessionStop>()
            .ok()
            .map(|p| p.employee_id)
    } else if request.is::<MemberSessionApprovalDecide>() {
        request
            .params::<MemberSessionApprovalDecide>()
            .ok()
            .map(|p| p.employee_id)
    } else if request.is::<MemberSessionClearStall>() {
        request
            .params::<MemberSessionClearStall>()
            .ok()
            .map(|p| p.employee_id)
    } else if request.is::<MemberSessionInspect>() {
        request
            .params::<MemberSessionInspect>()
            .ok()
            .and_then(|p| p.employee_id)
    } else if request.is::<MemberSessionGrants>() {
        request
            .params::<MemberSessionGrants>()
            .ok()
            .and_then(|p| p.employee_id)
    } else if request.is::<MemberSessionList>() {
        request
            .params::<MemberSessionList>()
            .ok()
            .and_then(|p| p.employee_id)
    } else if request.is::<MemberSessionTranscript>() {
        request
            .params::<MemberSessionTranscript>()
            .ok()
            .and_then(|p| p.employee_id)
    } else {
        None
    }
}

fn authorize_hq_proxied_session_caller(
    office: &Office,
    caller_id: &str,
    method: &str,
) -> Result<bool, org_proto::ApiError> {
    let is_owner = caller_id == office.owner_member_id();
    let is_active_person = is_owner
        || office
            .load_employee(caller_id)
            .ok()
            .flatten()
            .is_some_and(|e| {
                e.kind == org_office::organization_family::records::EmployeeKind::Person
                    && e.status == org_office::organization_family::records::EmployeeStatus::Active
            });
    match method {
        "member.session_approval" | "member.session_clear_stall" => {
            if is_owner {
                Ok(true)
            } else {
                Err(org_proto::ApiError::new(
                    org_proto::ErrorCode::NotAuthorized,
                    "only the organization owner may approve or clear agent sessions",
                ))
            }
        }
        _ => {
            if is_active_person {
                Ok(is_owner)
            } else {
                Err(org_proto::ApiError::new(
                    org_proto::ErrorCode::NotAuthorized,
                    "only an active person or the owner may inspect or manage remote agent sessions",
                ))
            }
        }
    }
}

fn rewrite_proxied_employee_id(
    request: &org_proto::Request,
    resolved_target_id: &str,
) -> org_proto::Request {
    let mut forwarded = request.clone();
    if let Some(obj) = forwarded.params.as_object_mut() {
        obj.insert(
            "employee_id".to_owned(),
            serde_json::Value::String(resolved_target_id.to_owned()),
        );
    }
    forwarded
}

fn try_proxy_remote_session_rpc(
    office: &Office,
    hq_fed_slot: &HqFederationSlot,
    member: Option<&str>,
    request: &org_proto::Request,
) -> Option<org_proto::Response> {
    if !matches!(office.is_hq(), Ok(true))
        || !federation_stream::is_proxied_session_method(&request.method)
    {
        return None;
    }
    let target_raw = proxied_session_target_employee(request)?;
    let target_id = office
        .org_snapshot()
        .ok()
        .and_then(|snap| {
            snap.members
                .into_iter()
                .find(|m| m.id == target_raw || m.handle.as_deref() == Some(target_raw.as_str()))
                .map(|m| m.id)
        })
        .unwrap_or(target_raw);
    let target_office = org_office::resolve_employee_office(office, &target_id).ok()?;
    if target_office == office.info().office_id {
        return None;
    }
    let Some(caller_id) = member else {
        return Some(org_proto::Response::error(
            request.id,
            org_proto::ApiError::new(org_proto::ErrorCode::NotBound, "connection is not bound"),
        ));
    };
    let caller_is_owner =
        match authorize_hq_proxied_session_caller(office, caller_id, &request.method) {
            Ok(is_owner) => is_owner,
            Err(err) => return Some(org_proto::Response::error(request.id, err)),
        };
    let Some(hq_fed) = hq_fed_slot.get() else {
        return Some(org_proto::Response::error(
            request.id,
            org_proto::ApiError::new(
                org_proto::ErrorCode::Unavailable,
                format!("satellite office `{target_office}` is not reachable (HQ federation listener disabled)"),
            ),
        ));
    };
    let forwarded = rewrite_proxied_employee_id(request, &target_id);
    // Keep the proxy deadline strictly below `agentic-org-web`'s 5 s `OFFICE_DEADLINE` so a
    // stalled satellite never times out and evicts the Web BFF's shared multiplexed channel.
    match hq_fed.proxy_session_rpc(
        &target_office,
        caller_id,
        caller_is_owner,
        &forwarded,
        std::time::Duration::from_secs(4),
    ) {
        Ok(resp) => Some(resp),
        Err(err) => Some(org_proto::Response::error(request.id, err)),
    }
}

pub(crate) fn build_session_hooks(
    office: &Arc<Office>,
    session_manager: &Arc<org_office_sessions::SessionManager>,
    hq_fed_slot: &HqFederationSlot,
) -> SessionHooks {
    SessionHooks {
        sessions: {
            let office = Arc::clone(office);
            let sessions = Arc::clone(session_manager);
            let hq_fed = Arc::clone(hq_fed_slot);
            Box::new(move |member, request| {
                if let Some(proxied) =
                    try_proxy_remote_session_rpc(&office, &hq_fed, member, request)
                {
                    return Some(proxied);
                }
                sessions.dispatch_rpc(member, request)
            })
        },
        run_binding_verify: {
            let sessions = Arc::clone(session_manager);
            Box::new(move |member_id, token, peer_pid| {
                sessions
                    .bindings()
                    .verify_peer(member_id, token, peer_pid)
                    .is_ok()
            })
        },
        run_binding_active: {
            let sessions = Arc::clone(session_manager);
            Box::new(move |member_id| sessions.bindings().has_active(member_id))
        },
        record_agent_command: {
            let sessions = Arc::clone(session_manager);
            Box::new(move |member_id, method| sessions.record_accepted_command(member_id, method))
        },
        on_agent_stop: {
            let sessions = Arc::clone(session_manager);
            Box::new(move |employee_id, reason| {
                // Pausing or offboarding ends both leases: the managed run and any external one.
                sessions.revoke_external_lease(employee_id, reason);
                let _ = sessions.stop_session(employee_id, reason);
            })
        },
    }
}

/// External (BYOA) agent auth over the office's session manager (`TASK-137`).
pub(crate) struct ExternalAuth(pub(crate) Arc<org_office_sessions::SessionManager>);

impl serve::ExternalAuthPort for ExternalAuth {
    fn login_request(
        &self,
        member_id: &str,
        params: &ext::AuthLoginRequestParams,
    ) -> Result<ext::AuthLoginRequestResult, org_proto::ApiError> {
        self.0.external_login_request(member_id, params)
    }

    fn login_poll(
        &self,
        request_id: &str,
    ) -> Result<ext::AuthLoginPollResult, org_proto::ApiError> {
        self.0.external_login_poll(request_id)
    }

    fn challenge(
        &self,
        member_id: &str,
        session_id: &str,
        purpose: ext::ChallengePurpose,
    ) -> Result<ext::AuthChallengeResult, org_proto::ApiError> {
        self.0.external_challenge(member_id, session_id, purpose)
    }

    fn exchange(
        &self,
        member_id: &str,
        params: &ext::SignedChallengeParams,
    ) -> Result<ext::AuthExchangeResult, org_proto::ApiError> {
        self.0.external_exchange(member_id, params)
    }

    fn logout(
        &self,
        member_id: &str,
        params: &ext::SignedChallengeParams,
    ) -> Result<ext::AuthLogoutResult, org_proto::ApiError> {
        self.0.external_logout(member_id, params)
    }

    fn whoami(&self, member_id: &str, external_session: Option<&str>) -> ext::AuthWhoamiResult {
        self.0.external_whoami(member_id, external_session)
    }

    fn approve(
        &self,
        actor: &str,
        params: &ext::MemberExternalApproveParams,
    ) -> Result<ext::ExternalSessionView, org_proto::ApiError> {
        self.0.external_approve(actor, params)
    }

    fn bind(
        &self,
        actor: &str,
        employee_id: &str,
        params: &ext::MemberExternalBindParams,
    ) -> Result<ext::ExternalSessionView, org_proto::ApiError> {
        self.0.external_bind(actor, employee_id, params)
    }

    fn sessions(
        &self,
        actor: &str,
        employee_id: &str,
        limit: Option<u32>,
    ) -> Result<ext::MemberExternalSessionsResult, org_proto::ApiError> {
        self.0.external_sessions(actor, employee_id, limit)
    }

    fn revoke(
        &self,
        actor: &str,
        employee_id: &str,
        session_id: Option<&str>,
    ) -> Result<ext::MemberExternalRevokeResult, org_proto::ApiError> {
        self.0.external_revoke(actor, employee_id, session_id)
    }

    fn verify(&self, member_id: &str, token: Option<&str>) -> Option<String> {
        self.0.verify_external_token(member_id, token)
    }

    fn is_active(&self, member_id: &str, session_id: &str) -> bool {
        self.0.external_binding_active(member_id, session_id)
    }
}
