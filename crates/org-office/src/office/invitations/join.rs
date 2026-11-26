//! Satellite join requests and their approval: proof checks, idempotent replays and the
//! issued satellite certificate.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_pki::{
    cert::{
        issue_satellite_cert_from_pubkey, IssueSatelliteCertParams,
        DEFAULT_OFFICE_CERT_VALIDITY_DAYS,
    },
    invitation::{compute_check_code, normalize_check_code, verify_exporter_proof},
};
use org_proto::{ApiError, ErrorCode};
use org_store::{Actor, Id, Outcome};

use crate::office::{storage, Office};
use crate::{
    error::OfficeError,
    model::OfficeRole,
    organization::features::offices::{RegisterSatelliteOffice, SatelliteRegistered},
    store::open::RegisterSatelliteOfficeOp,
};

use super::removal::is_satellite_office_removed_locked;
use super::tls::{ensure_hq_tls_identity_locked, unwrap_hq_ca_key};
use super::{
    load_invitations, require_hq_owner, save_invitations, ApprovedSatelliteCert, InvitationStatus,
    InvitationsStateFile, JoinRequestAccepted, JoinRequestParams, StoredInvitation,
    MAX_WRONG_CHECK_CODE_ATTEMPTS,
};

impl Office {
    /// Registers an incoming TLS-exporter-authenticated satellite join request against a pending
    /// invitation on HQ and computes the deterministic 8-char Crockford check code (`doc-67`).
    ///
    /// Persists `(request_id, joining_pub_key)` so a second peer cannot overwrite a pending
    /// key, and if TLS drops after HQ approval, reconnecting with the same `(request_id,
    /// joining_pub_key)` + fresh exporter proof replays the issued certificate idempotently.
    pub fn register_satellite_join_request(
        &self,
        req: JoinRequestParams,
    ) -> Result<Result<JoinRequestAccepted, ApiError>, OfficeError> {
        if self.info.role != OfficeRole::Hq {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "only HQ accepts satellite join requests",
            )));
        }
        if req.request_id.trim().is_empty() {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "request_id is required",
            )));
        }
        let _guard = self
            .federation_io_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let now_ms = self.monotonic_now_ms();
        let mut state = load_invitations(self)?;
        let Some(inv) = state
            .invitations
            .iter_mut()
            .find(|i| i.invitation_id == req.invitation_id)
        else {
            return Ok(Err(ApiError::new(
                ErrorCode::NotFound,
                "invitation not found",
            )));
        };
        if inv.office_id != req.office_id {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "office_id does not match invitation",
            )));
        }
        if let Some(err) = verify_join_request_proof(inv, &req)? {
            return Ok(Err(err));
        }
        if is_satellite_office_removed_locked(self, &inv.office_id)? {
            return Ok(Err(ApiError::new(
                ErrorCode::Conflict,
                "office certificate was revoked",
            )));
        }
        let incoming_pub_b64 = STANDARD.encode(req.joining_pub_key);
        if inv.status == InvitationStatus::Approved {
            return replay_approved_join(self, inv, &req.request_id, &incoming_pub_b64);
        }
        if now_ms > inv.expires_at_ms
            && matches!(
                inv.status,
                InvitationStatus::Pending | InvitationStatus::AwaitingApproval
            )
        {
            inv.status = InvitationStatus::Expired;
            save_invitations(self, &state)?;
            return Ok(Err(ApiError::new(
                ErrorCode::Expired,
                "invitation has expired",
            )));
        }
        if !matches!(
            inv.status,
            InvitationStatus::Pending | InvitationStatus::AwaitingApproval
        ) {
            return Ok(Err(ApiError::new(
                ErrorCode::Conflict,
                "invitation is no longer active",
            )));
        }
        if inv.status == InvitationStatus::AwaitingApproval {
            return reuse_pending_join(inv, &req, &incoming_pub_b64);
        }
        let check_code = compute_check_code(&req.invitation_id, &req.joining_pub_key);
        inv.request_id = Some(req.request_id);
        inv.joining_pub_key_b64 = Some(incoming_pub_b64);
        inv.pop_signature_b64 = Some(STANDARD.encode(&req.pop_signature_der));
        inv.check_code = Some(check_code.clone());
        inv.status = InvitationStatus::AwaitingApproval;
        save_invitations(self, &state)?;
        Ok(Ok(JoinRequestAccepted::Pending {
            invitation_id: req.invitation_id,
            office_id: req.office_id,
            check_code,
        }))
    }

    /// Approves a satellite join request after verifying the human-entered Crockford check code.
    /// Three wrong check codes automatically cancel the invitation (`doc-67`).
    pub fn approve_satellite_invitation(
        &self,
        actor_member_id: &str,
        invitation_id: &str,
        entered_check_code: &str,
    ) -> Result<Result<ApprovedSatelliteCert, ApiError>, OfficeError> {
        if let Err(err) = require_hq_owner(self, actor_member_id) {
            return Ok(Err(err));
        }
        let _guard = self
            .federation_io_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let now_ms = self.monotonic_now_ms();
        let mut state = load_invitations(self)?;
        let Some(idx) = state
            .invitations
            .iter()
            .position(|i| i.invitation_id == invitation_id)
        else {
            return Ok(Err(ApiError::new(
                ErrorCode::NotFound,
                "invitation not found",
            )));
        };
        if is_satellite_office_removed_locked(self, &state.invitations[idx].office_id)? {
            return Ok(Err(ApiError::new(
                ErrorCode::Conflict,
                "removed office ID cannot be re-enrolled; create a new office",
            )));
        }
        if state.invitations[idx].status == InvitationStatus::Approved {
            return idempotent_approved_retry(self, &state.invitations[idx], entered_check_code);
        }
        if let Some(err) =
            check_approval_preconditions(self, &mut state, idx, now_ms, entered_check_code)?
        {
            return Ok(Err(err));
        }
        issue_and_persist_approved_cert(self, &mut state, idx, invitation_id, now_ms)
    }

    fn register_satellite_office_in_store(
        &self,
        office_id: &str,
        office_name: &str,
    ) -> Result<SatelliteRegistered, OfficeError> {
        let sat_id = Id::parse(office_id)
            .map_err(|_| OfficeError::Invalid("invalid satellite office_id"))?;
        let op_id = Id::parse(&self.next_uuid_v7())
            .map_err(|_| OfficeError::Invalid("generated operation id"))?;
        let op = RegisterSatelliteOfficeOp {
            operation_id: op_id,
            here: self.identity.clone(),
            owner: self.owner.clone(),
            by: Actor::Employee(self.owner.clone()),
            command: RegisterSatelliteOffice {
                office_id: sat_id,
                office_name: office_name.to_owned(),
            },
        };
        let reply = self.store.run(op).map_err(storage)?;
        if reply.outcome != Outcome::Completed {
            return Err(OfficeError::Invalid(
                "satellite office registration rejected",
            ));
        }
        self.store.sync().map_err(storage)?;
        serde_json::from_slice(&reply.bytes)
            .map_err(|_| OfficeError::Invalid("malformed satellite registration reply"))
    }
}

fn verify_join_request_proof(
    inv: &StoredInvitation,
    req: &JoinRequestParams,
) -> Result<Option<ApiError>, OfficeError> {
    let pub_bytes = STANDARD
        .decode(&inv.invitation_pub_key_b64)
        .map_err(|_| OfficeError::Invalid("stored invitation pubkey corrupt"))?;
    let pub_arr: [u8; 65] = pub_bytes
        .try_into()
        .map_err(|_| OfficeError::Invalid("stored invitation pubkey length"))?;
    if verify_exporter_proof(
        &pub_arr,
        &req.exporter_binding,
        &req.invitation_id,
        &req.office_id,
        &req.joining_pub_key,
        &req.exporter_proof_der,
    )
    .is_err()
    {
        return Ok(Some(ApiError::new(
            ErrorCode::NotAuthorized,
            "invitation exporter possession proof verification failed",
        )));
    }
    Ok(None)
}

fn replay_approved_join(
    office: &Office,
    inv: &StoredInvitation,
    request_id: &str,
    incoming_pub_b64: &str,
) -> Result<Result<JoinRequestAccepted, ApiError>, OfficeError> {
    let same_binding = inv.request_id.as_deref() == Some(request_id)
        && inv.joining_pub_key_b64.as_deref() == Some(incoming_pub_b64);
    if !same_binding {
        return Ok(Err(ApiError::new(
            ErrorCode::Conflict,
            "invitation already approved for a different joining key",
        )));
    }
    let cert_der = STANDARD
        .decode(inv.issued_cert_der_b64.as_deref().unwrap_or_default())
        .map_err(|_| OfficeError::Invalid("stored issued cert corrupt"))?;
    let hq_tls = ensure_hq_tls_identity_locked(office, &inv.hq_addresses)?;
    Ok(Ok(JoinRequestAccepted::AlreadyApproved {
        cert_der,
        ca_cert_der: hq_tls.ca_cert_der,
        hq_office_id: office.info.office_id.clone(),
        epoch: hq_tls.epoch,
    }))
}

fn reuse_pending_join(
    inv: &StoredInvitation,
    req: &JoinRequestParams,
    incoming_pub_b64: &str,
) -> Result<Result<JoinRequestAccepted, ApiError>, OfficeError> {
    let same_binding = inv.request_id.as_deref() == Some(req.request_id.as_str())
        && inv.joining_pub_key_b64.as_deref() == Some(incoming_pub_b64);
    if !same_binding {
        return Ok(Err(ApiError::new(
            ErrorCode::Conflict,
            "invitation already bound to a pending join request",
        )));
    }
    let check_code = inv
        .check_code
        .clone()
        .unwrap_or_else(|| compute_check_code(&req.invitation_id, &req.joining_pub_key));
    Ok(Ok(JoinRequestAccepted::Pending {
        invitation_id: req.invitation_id.clone(),
        office_id: req.office_id.clone(),
        check_code,
    }))
}

fn idempotent_approved_retry(
    office: &Office,
    inv: &StoredInvitation,
    entered_check_code: &str,
) -> Result<Result<ApprovedSatelliteCert, ApiError>, OfficeError> {
    // Preserve terminal `Approved` state on retry even after TTL (`TASK-52` M-17).
    let expected = inv.check_code.as_deref().unwrap_or_default();
    if normalize_check_code(entered_check_code).as_deref() == Some(expected) {
        if let (Some(cert_b64), Some(serial_hex)) = (
            inv.issued_cert_der_b64.as_deref(),
            inv.issued_cert_serial_hex.clone(),
        ) {
            let cert_der = STANDARD
                .decode(cert_b64)
                .map_err(|_| OfficeError::Invalid("stored issued cert corrupt"))?;
            let hq_tls = ensure_hq_tls_identity_locked(office, &inv.hq_addresses)?;
            let registered =
                office.register_satellite_office_in_store(&inv.office_id, &inv.office_name)?;
            return Ok(Ok(ApprovedSatelliteCert {
                invitation: inv.view(),
                cert_der,
                ca_cert_der: hq_tls.ca_cert_der,
                serial_hex,
                epoch: hq_tls.epoch,
                registered,
            }));
        }
    }
    Ok(Err(ApiError::new(
        ErrorCode::Conflict,
        "invitation is already approved",
    )))
}

fn check_approval_preconditions(
    office: &Office,
    state: &mut InvitationsStateFile,
    idx: usize,
    now_ms: u64,
    entered_check_code: &str,
) -> Result<Option<ApiError>, OfficeError> {
    // Caller (`approve_satellite_invitation`) already holds `federation_io_lock`.
    if is_satellite_office_removed_locked(office, &state.invitations[idx].office_id)? {
        return Ok(Some(ApiError::new(
            ErrorCode::Conflict,
            "removed office ID cannot be re-enrolled; create a new office",
        )));
    }
    if matches!(
        state.invitations[idx].status,
        InvitationStatus::Pending | InvitationStatus::AwaitingApproval
    ) && now_ms > state.invitations[idx].expires_at_ms
    {
        state.invitations[idx].status = InvitationStatus::Expired;
        save_invitations(office, state)?;
        return Ok(Some(ApiError::new(
            ErrorCode::Expired,
            "invitation has expired",
        )));
    }
    if state.invitations[idx].status != InvitationStatus::AwaitingApproval {
        return Ok(Some(ApiError::new(
            ErrorCode::Conflict,
            "invitation is not awaiting approval",
        )));
    }
    let expected = state.invitations[idx]
        .check_code
        .clone()
        .unwrap_or_default();
    if normalize_check_code(entered_check_code).as_deref() != Some(expected.as_str()) {
        state.invitations[idx].wrong_code_attempts =
            state.invitations[idx].wrong_code_attempts.saturating_add(1);
        let attempts = state.invitations[idx].wrong_code_attempts;
        if attempts >= MAX_WRONG_CHECK_CODE_ATTEMPTS {
            state.invitations[idx].status = InvitationStatus::Cancelled;
        }
        save_invitations(office, state)?;
        return Ok(Some(ApiError::new(
            ErrorCode::InvalidRequest,
            if attempts >= MAX_WRONG_CHECK_CODE_ATTEMPTS {
                "check code mismatch; invitation cancelled after 3 wrong attempts"
            } else {
                "check code mismatch"
            },
        )));
    }
    Ok(None)
}

fn issue_and_persist_approved_cert(
    office: &Office,
    state: &mut InvitationsStateFile,
    idx: usize,
    invitation_id: &str,
    now_ms: u64,
) -> Result<Result<ApprovedSatelliteCert, ApiError>, OfficeError> {
    let joining_pub_vec = STANDARD
        .decode(
            state.invitations[idx]
                .joining_pub_key_b64
                .as_deref()
                .unwrap_or_default(),
        )
        .map_err(|_| OfficeError::Invalid("stored joining pubkey corrupt"))?;
    let joining_pub: [u8; 65] = joining_pub_vec
        .try_into()
        .map_err(|_| OfficeError::Invalid("stored joining pubkey length"))?;
    let pop_sig = STANDARD
        .decode(
            state.invitations[idx]
                .pop_signature_b64
                .as_deref()
                .unwrap_or_default(),
        )
        .map_err(|_| OfficeError::Invalid("stored pop signature corrupt"))?;

    let hq_tls = ensure_hq_tls_identity_locked(office, &state.invitations[idx].hq_addresses)?;
    let ca_key = unwrap_hq_ca_key(office)?;
    let now_unix = (now_ms / 1000) as i64;
    let issued = issue_satellite_cert_from_pubkey(
        &ca_key,
        &hq_tls.ca_cert_der,
        &IssueSatelliteCertParams {
            org_id: &office.info.org_id,
            office_id: &state.invitations[idx].office_id,
            invitation_id,
            subject_pub_key_uncompressed: &joining_pub,
            pop_signature_der: &pop_sig,
            now_unix,
            validity_days: DEFAULT_OFFICE_CERT_VALIDITY_DAYS,
        },
    )
    .map_err(|_| OfficeError::Invalid("satellite key proof-of-possession or signing failed"))?;

    let registered = office.register_satellite_office_in_store(
        &state.invitations[idx].office_id,
        &state.invitations[idx].office_name,
    )?;
    // Activate HQ durable journal consumers only once a satellite is actually enrolled so an
    // unused/expired invitation never pins a standalone HQ's journal (`TASK-52` P7-#8).
    let _ = office.register_replication_consumer("org-sync-hq")?;
    let _ = office.register_replication_consumer("org-sync-message-relay")?;

    state.invitations[idx].status = InvitationStatus::Approved;
    state.invitations[idx].issued_cert_der_b64 = Some(STANDARD.encode(&issued.cert_der));
    state.invitations[idx].issued_cert_serial_hex = Some(issued.serial_hex.clone());
    let view = state.invitations[idx].view();
    save_invitations(office, state)?;

    Ok(Ok(ApprovedSatelliteCert {
        invitation: view,
        cert_der: issued.cert_der,
        ca_cert_der: hq_tls.ca_cert_der,
        serial_hex: issued.serial_hex,
        epoch: hq_tls.epoch,
        registered,
    }))
}
