//! Session and enrollment RPC dispatch, caller authorization, and the accepted-command cache.

use org_proto::sessions::{
    MemberSessionApprovalDecide, MemberSessionClearStall, MemberSessionGrants,
    MemberSessionInspect, MemberSessionInterrupt, MemberSessionList, MemberSessionStart,
    MemberSessionSteer, MemberSessionStop, MemberSessionTranscript, ProviderCredentialRevoke,
    ProviderCredentialStatus, ProviderEnrollmentCancel, ProviderEnrollmentStart,
    ProviderEnrollmentStatus, ProviderEnrollmentSubmit,
};
use org_proto::{ApiError, Call, ErrorCode, Request, Response};

use crate::credentials::EnrollmentVaultError;

use super::{CachedRpcResponse, SessionManager, MAX_COMMAND_CACHE_ENTRIES};

impl SessionManager {
    /// Dispatches all C3 session & provider-enrollment RPCs with caller-scoped `command_id` replay.
    pub fn dispatch_rpc(&self, member: Option<&str>, request: &Request) -> Option<Response> {
        self.dispatch_rpc_inner(member, false, true, request)
    }

    /// Dispatches an HQ-authenticated, mTLS-proxied session-control RPC on a satellite office
    /// (`TASK-143` AC #3). Never dispatches provider-enrollment RPCs.
    pub fn dispatch_hq_proxied_rpc(
        &self,
        caller_id: &str,
        caller_is_owner: bool,
        request: &Request,
    ) -> Option<Response> {
        self.dispatch_rpc_inner(Some(caller_id), caller_is_owner, false, request)
    }

    pub(super) fn dispatch_rpc_inner(
        &self,
        member: Option<&str>,
        hq_attested_owner: bool,
        allow_enrollment: bool,
        request: &Request,
    ) -> Option<Response> {
        if !is_c3_rpc_method(&request.method) {
            return None;
        }
        let Some(caller_id) = member else {
            return Some(Response::error(
                request.id,
                ApiError::new(ErrorCode::NotAuthorized, "authenticated caller required"),
            ));
        };
        if let Err(err) = self.authorize_rpc_caller(caller_id, hq_attested_owner, &request.method) {
            return Some(Response::error(request.id, err));
        }
        let cache_meta = command_cache_meta(request);
        if let Some((cid, fp)) = cache_meta.as_ref() {
            if let Some(replayed) =
                self.lookup_command_cache(caller_id, &request.method, cid, fp, request.id)
            {
                return Some(replayed);
            }
        }
        let resp = self
            .dispatch_session_rpcs(Some(caller_id), hq_attested_owner, request)
            .or_else(|| {
                allow_enrollment
                    .then(|| self.dispatch_enrollment_rpcs(Some(caller_id), request))
                    .flatten()
            })?;
        if let (Some((cid, fp)), org_proto::Outcome::Result(_)) = (cache_meta, &resp.outcome) {
            self.store_command_cache(caller_id, &request.method, cid, fp, &resp);
        }
        Some(resp)
    }

    pub(super) fn lookup_command_cache(
        &self,
        caller_id: &str,
        method: &str,
        cid: &str,
        fp: &str,
        request_id: u64,
    ) -> Option<Response> {
        let key = (caller_id.to_owned(), method.to_owned(), cid.to_owned());
        let cached = self
            .command_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&key)
            .cloned()?;
        if cached.fingerprint != fp {
            return Some(Response::error(
                request_id,
                ApiError::new(
                    ErrorCode::Conflict,
                    "command_id already used with different parameters",
                ),
            ));
        }
        let mut replay = cached.response;
        replay.id = request_id;
        Some(replay)
    }

    pub(super) fn store_command_cache(
        &self,
        caller_id: &str,
        method: &str,
        cid: String,
        fp: String,
        resp: &Response,
    ) {
        let mut cache = self.command_cache.lock().unwrap_or_else(|p| p.into_inner());
        if cache.len() >= MAX_COMMAND_CACHE_ENTRIES {
            cache.clear();
        }
        cache.insert(
            (caller_id.to_owned(), method.to_owned(), cid),
            CachedRpcResponse {
                fingerprint: fp,
                response: resp.clone(),
            },
        );
    }

    pub(super) fn authorize_rpc_caller(
        &self,
        caller_id: &str,
        hq_attested_owner: bool,
        method: &str,
    ) -> Result<(), ApiError> {
        match method {
            "provider.enrollment_start"
            | "provider.enrollment_status"
            | "provider.enrollment_submit"
            | "provider.enrollment_cancel"
            | "provider.credential_status"
            | "provider.credential_revoke"
            | "member.session_approval"
            | "member.session_clear_stall" => {
                self.authorize_owner_only_inner(caller_id, hq_attested_owner)
            }
            "member.session_start"
            | "member.session_stop"
            | "member.session_interrupt"
            | "member.session_steer" => {
                self.authorize_owner_or_person_inner(caller_id, hq_attested_owner)
            }
            _ => Ok(()),
        }
    }

    pub(super) fn dispatch_session_rpcs(
        &self,
        member: Option<&str>,
        hq_attested_owner: bool,
        request: &Request,
    ) -> Option<Response> {
        if request.is::<MemberSessionStart>() {
            return Some(handle_call::<MemberSessionStart>(
                request,
                member,
                |m, p| self.start_session_inner(m, hq_attested_owner, &p),
            ));
        }
        if request.is::<MemberSessionStop>() {
            return Some(handle_call::<MemberSessionStop>(request, member, |m, p| {
                self.authorize_owner_or_person_inner(m, hq_attested_owner)?;
                self.authorize_chief_of_staff_target_inner(m, hq_attested_owner, &p.employee_id)?;
                self.stop_session(&p.employee_id, p.reason.as_deref().unwrap_or("stopped"))
            }));
        }
        if let Some(response) = self.dispatch_turn_control_rpcs(member, hq_attested_owner, request)
        {
            return Some(response);
        }
        if request.is::<MemberSessionInspect>() {
            return Some(handle_call::<MemberSessionInspect>(
                request,
                member,
                |m, p| self.inspect_session_inner(m, hq_attested_owner, p.employee_id.as_deref()),
            ));
        }
        if request.is::<MemberSessionGrants>() {
            return Some(handle_call::<MemberSessionGrants>(
                request,
                member,
                |m, p| self.list_grants_inner(m, hq_attested_owner, p.employee_id.as_deref()),
            ));
        }
        if request.is::<MemberSessionApprovalDecide>() {
            return Some(handle_call::<MemberSessionApprovalDecide>(
                request,
                member,
                |m, p| {
                    self.decide_approval_inner(
                        m,
                        hq_attested_owner,
                        &p.employee_id,
                        &p.approval_id,
                        &p.decision,
                        p.reason,
                    )
                },
            ));
        }
        if request.is::<MemberSessionClearStall>() {
            return Some(handle_call::<MemberSessionClearStall>(
                request,
                member,
                |m, p| self.clear_stall_lock_inner(m, hq_attested_owner, &p.employee_id),
            ));
        }
        if request.is::<MemberSessionList>() {
            return Some(handle_call::<MemberSessionList>(request, member, |m, p| {
                self.list_sessions_inner(m, hq_attested_owner, &p)
            }));
        }
        if request.is::<MemberSessionTranscript>() {
            return Some(handle_call::<MemberSessionTranscript>(
                request,
                member,
                |m, p| self.read_session_transcript_inner(m, hq_attested_owner, &p),
            ));
        }
        None
    }

    /// A person's live turn controls (`TASK-103`), under `member.session_stop`'s policy: the
    /// owner or an active person, and only the owner on the Chief of Staff's sessions.
    fn dispatch_turn_control_rpcs(
        &self,
        member: Option<&str>,
        hq_attested_owner: bool,
        request: &Request,
    ) -> Option<Response> {
        if request.is::<MemberSessionInterrupt>() {
            return Some(handle_call::<MemberSessionInterrupt>(
                request,
                member,
                |m, p| {
                    self.authorize_owner_or_person_inner(m, hq_attested_owner)?;
                    self.authorize_chief_of_staff_target_inner(
                        m,
                        hq_attested_owner,
                        &p.employee_id,
                    )?;
                    self.interrupt_session(&p.employee_id)
                },
            ));
        }
        if request.is::<MemberSessionSteer>() {
            return Some(handle_call::<MemberSessionSteer>(
                request,
                member,
                // The steer checks its caller itself (and, on the Chief of Staff, the partition).
                |m, p| self.steer_session_inner(m, hq_attested_owner, &p.employee_id, &p.text),
            ));
        }
        None
    }

    pub(super) fn dispatch_enrollment_rpcs(
        &self,
        member: Option<&str>,
        request: &Request,
    ) -> Option<Response> {
        if request.is::<ProviderEnrollmentStart>() {
            return Some(handle_call::<ProviderEnrollmentStart>(
                request,
                member,
                |m, p| {
                    self.authorize_owner_only(m)?;
                    let provider =
                        self.resolve_connection_provider(&p.connection_id, p.provider)?;
                    self.enrollment
                        .start(m, &p.connection_id, &provider)
                        .map_err(map_vault_err)
                },
            ));
        }
        if request.is::<ProviderEnrollmentStatus>() {
            return Some(handle_call::<ProviderEnrollmentStatus>(
                request,
                member,
                |m, p| {
                    self.authorize_owner_only(m)?;
                    self.enrollment
                        .status(m, &p.connection_id, &p.login_id)
                        .map_err(map_vault_err)
                },
            ));
        }
        if request.is::<ProviderEnrollmentSubmit>() {
            return Some(handle_call::<ProviderEnrollmentSubmit>(
                request,
                member,
                |m, p| {
                    self.authorize_owner_only(m)?;
                    self.enrollment
                        .submit_code(m, &p.connection_id, &p.login_id, &p.code)
                        .map_err(map_vault_err)
                },
            ));
        }
        if request.is::<ProviderEnrollmentCancel>() {
            return Some(handle_call::<ProviderEnrollmentCancel>(
                request,
                member,
                |m, p| {
                    self.authorize_owner_only(m)?;
                    self.enrollment
                        .cancel(m, &p.connection_id, &p.login_id)
                        .map_err(map_vault_err)
                },
            ));
        }
        if request.is::<ProviderCredentialStatus>() {
            return Some(handle_call::<ProviderCredentialStatus>(
                request,
                member,
                |m, p| {
                    self.authorize_owner_only(m)?;
                    self.enrollment
                        .vault()
                        .status(&p.connection_id)
                        .map_err(map_vault_err)
                },
            ));
        }
        if request.is::<ProviderCredentialRevoke>() {
            return Some(handle_call::<ProviderCredentialRevoke>(
                request,
                member,
                |m, p| {
                    self.authorize_owner_only(m)?;
                    self.enrollment
                        .vault()
                        .revoke(&p.connection_id)
                        .map_err(map_vault_err)
                },
            ));
        }
        None
    }

    pub(super) fn resolve_connection_provider(
        &self,
        connection_id: &str,
        explicit: Option<String>,
    ) -> Result<String, ApiError> {
        if let Some(conn) = self
            .office
            .load_provider_connection(connection_id)
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "could not read connection"))?
        {
            return Ok(conn.provider_type);
        }
        explicit.ok_or_else(|| ApiError::new(ErrorCode::NotFound, "provider connection not found"))
    }
}

pub(super) fn is_c3_rpc_method(method: &str) -> bool {
    matches!(
        method,
        "member.session_start"
            | "member.session_stop"
            | "member.session_interrupt"
            | "member.session_steer"
            | "member.session_inspect"
            | "member.session_list"
            | "member.session_transcript"
            | "member.session_grants"
            | "member.session_approval"
            | "member.session_clear_stall"
            | "provider.enrollment_start"
            | "provider.enrollment_status"
            | "provider.enrollment_submit"
            | "provider.enrollment_cancel"
            | "provider.credential_status"
            | "provider.credential_revoke"
    )
}

pub(super) fn command_cache_meta(request: &Request) -> Option<(String, String)> {
    let cid = request.params.get("command_id")?.as_str()?.to_owned();
    // Hash params with SHA-256 and zeroize the serialized buffer so plaintext OAuth codes or
    // instructions are never retained in `command_cache` (`ENROLL-3`).
    let raw = zeroize::Zeroizing::new(serde_json::to_vec(&request.params).unwrap_or_default());
    let digest = ring::digest::digest(&ring::digest::SHA256, &raw);
    let mut fp = String::with_capacity(64);
    for b in digest.as_ref() {
        fp.push_str(&format!("{b:02x}"));
    }
    Some((cid, fp))
}

pub(super) fn handle_call<C: Call>(
    request: &Request,
    member: Option<&str>,
    f: impl FnOnce(&str, C::Params) -> Result<C::Result, ApiError>,
) -> Response {
    let Some(member_id) = member else {
        return Response::error(
            request.id,
            ApiError::new(ErrorCode::NotBound, "connection is not bound"),
        );
    };
    let Ok(params) = request.params::<C>() else {
        return Response::error(
            request.id,
            ApiError::new(
                ErrorCode::InvalidRequest,
                format!("invalid params for `{}`", C::METHOD),
            ),
        );
    };
    match f(member_id, params) {
        Ok(res) => Response::ok::<C>(request.id, &res).unwrap_or_else(|_| {
            Response::error(
                request.id,
                ApiError::new(ErrorCode::StorageError, "could not encode response"),
            )
        }),
        Err(err) => Response::error(request.id, err),
    }
}

pub(super) fn map_vault_err(err: EnrollmentVaultError) -> ApiError {
    let (code, msg) = match err {
        EnrollmentVaultError::InvalidInput(m) => (ErrorCode::InvalidRequest, m),
        EnrollmentVaultError::NotAuthorized(m) => (ErrorCode::NotAuthorized, m),
        EnrollmentVaultError::Conflict(m) => (ErrorCode::Conflict, m),
        EnrollmentVaultError::NotFound(m) => (ErrorCode::NotFound, m),
        EnrollmentVaultError::Expired(m) => (ErrorCode::Expired, m),
        EnrollmentVaultError::MissingSecretsKey
        | EnrollmentVaultError::Crypto(_)
        | EnrollmentVaultError::Io(_) => (
            ErrorCode::Unavailable,
            "provider credential operation unavailable".to_owned(),
        ),
    };
    ApiError::new(code, msg)
}
