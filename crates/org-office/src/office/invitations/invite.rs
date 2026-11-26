//! Creating, listing, polling, declining and cancelling satellite invitations.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_pki::{
    invitation::{InvitationToken, NewInvitationParams},
    SigningKey,
};
use org_proto::{ApiError, ErrorCode};

use crate::error::OfficeError;
use crate::office::Office;

use super::tls::ensure_hq_tls_identity_locked;
use super::{
    is_satellite_office_removed, is_satellite_office_removed_locked, load_invitations,
    require_hq_owner, save_invitations, CreateInvitationParams, CreateInvitationResult,
    InvitationPollStatus, InvitationStatus, InvitationSummaryView, StoredInvitation,
    DEFAULT_INVITATION_TTL_SECS, MAX_INVITATION_TTL_SECS,
};

impl Office {
    /// Creates a one-time satellite invitation token on HQ (owner-only). HQ stores only the
    /// public half of the one-time invitation key (`doc-67`).
    pub fn create_satellite_invitation(
        &self,
        actor_member_id: &str,
        params: CreateInvitationParams,
    ) -> Result<Result<CreateInvitationResult, ApiError>, OfficeError> {
        if let Err(err) = require_hq_owner(self, actor_member_id) {
            return Ok(Err(err));
        }
        let office_name = params.office_name.trim().to_owned();
        if office_name.is_empty() || params.hq_addresses.is_empty() {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "office_name and at least one hq_address are required",
            )));
        }
        let office_id = match params.existing_office_id {
            Some(id) => {
                if !org_proto::is_uuid_v7(&id) || id == self.info.office_id {
                    return Ok(Err(ApiError::new(
                        ErrorCode::InvalidRequest,
                        "existing_office_id must be a valid non-HQ UUIDv7",
                    )));
                }
                if is_satellite_office_removed(self, &id)? {
                    return Ok(Err(ApiError::new(
                        ErrorCode::Conflict,
                        "removed office ID cannot be re-enrolled; create a new office",
                    )));
                }
                id
            }
            None => self.next_uuid_v7(),
        };
        let ttl_secs = params
            .ttl_secs
            .unwrap_or(DEFAULT_INVITATION_TTL_SECS)
            .clamp(60, MAX_INVITATION_TTL_SECS);
        let now_ms = self.monotonic_now_ms();
        let expires_at_ms = now_ms.saturating_add(ttl_secs.saturating_mul(1000));
        let hq_tls = self.ensure_hq_tls_identity(&params.hq_addresses)?;
        let inv_key = SigningKey::generate()
            .map_err(|_| OfficeError::Invalid("failed generating invitation key"))?;
        let invitation_id = self.next_uuid_v7();
        let token = InvitationToken::new(
            NewInvitationParams {
                invitation_id: invitation_id.clone(),
                org_id: self.info.org_id.clone(),
                org_name: self.info.org_name.clone(),
                office_id: office_id.clone(),
                office_name: office_name.clone(),
                hq_office_id: self.info.office_id.clone(),
                hq_addresses: params.hq_addresses.clone(),
                ca_cert_der: hq_tls.ca_cert_der,
                expires_at_ms,
            },
            &inv_key,
        )
        .map_err(|_| OfficeError::Invalid("failed creating invitation token"))?;
        let encoded = token
            .encode()
            .map_err(|_| OfficeError::Invalid("failed encoding invitation token"))?;

        let stored = StoredInvitation {
            invitation_id,
            office_id,
            office_name,
            hq_addresses: params.hq_addresses,
            invitation_pub_key_b64: STANDARD.encode(inv_key.public_key()),
            status: InvitationStatus::Pending,
            created_at_ms: now_ms,
            expires_at_ms,
            request_id: None,
            joining_pub_key_b64: None,
            pop_signature_b64: None,
            check_code: None,
            wrong_code_attempts: 0,
            issued_cert_der_b64: None,
            issued_cert_serial_hex: None,
        };
        let view = stored.view();
        let _guard = self
            .federation_io_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut state = load_invitations(self)?;
        state.invitations.push(stored);
        save_invitations(self, &state)?;
        Ok(Ok(CreateInvitationResult {
            invitation: view,
            token: encoded,
        }))
    }

    pub fn decline_satellite_invitation(
        &self,
        actor_member_id: &str,
        invitation_id: &str,
    ) -> Result<Result<InvitationSummaryView, ApiError>, OfficeError> {
        self.transition_invitation(actor_member_id, invitation_id, InvitationStatus::Declined)
    }

    pub fn cancel_satellite_invitation(
        &self,
        actor_member_id: &str,
        invitation_id: &str,
    ) -> Result<Result<InvitationSummaryView, ApiError>, OfficeError> {
        self.transition_invitation(actor_member_id, invitation_id, InvitationStatus::Cancelled)
    }

    fn transition_invitation(
        &self,
        actor_member_id: &str,
        invitation_id: &str,
        target: InvitationStatus,
    ) -> Result<Result<InvitationSummaryView, ApiError>, OfficeError> {
        if let Err(err) = require_hq_owner(self, actor_member_id) {
            return Ok(Err(err));
        }
        let _guard = self
            .federation_io_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut state = load_invitations(self)?;
        let Some(inv) = state
            .invitations
            .iter_mut()
            .find(|i| i.invitation_id == invitation_id)
        else {
            return Ok(Err(ApiError::new(
                ErrorCode::NotFound,
                "invitation not found",
            )));
        };
        if !matches!(
            inv.status,
            InvitationStatus::Pending | InvitationStatus::AwaitingApproval
        ) {
            return Ok(Err(ApiError::new(
                ErrorCode::Conflict,
                "invitation is already terminal",
            )));
        }
        inv.status = target;
        let view = inv.view();
        save_invitations(self, &state)?;
        Ok(Ok(view))
    }

    pub fn list_satellite_invitations(
        &self,
        actor_member_id: &str,
    ) -> Result<Result<Vec<InvitationSummaryView>, ApiError>, OfficeError> {
        if let Err(err) = require_hq_owner(self, actor_member_id) {
            return Ok(Err(err));
        }
        let _guard = self
            .federation_io_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let now_ms = self.monotonic_now_ms();
        let mut state = load_invitations(self)?;
        let mut changed = false;
        for inv in &mut state.invitations {
            if now_ms > inv.expires_at_ms
                && matches!(
                    inv.status,
                    InvitationStatus::Pending | InvitationStatus::AwaitingApproval
                )
            {
                inv.status = InvitationStatus::Expired;
                changed = true;
            }
        }
        if changed {
            save_invitations(self, &state)?;
        }
        Ok(Ok(state.invitations.iter().map(|i| i.view()).collect()))
    }

    /// Polls the current state of an enrollment invitation (used by HQ's federation listener).
    pub fn poll_satellite_invitation(
        &self,
        invitation_id: &str,
    ) -> Result<InvitationPollStatus, OfficeError> {
        let _guard = self
            .federation_io_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let now_ms = self.monotonic_now_ms();
        let state = load_invitations(self)?;
        let Some(inv) = state
            .invitations
            .iter()
            .find(|i| i.invitation_id == invitation_id)
        else {
            return Ok(InvitationPollStatus::Terminal {
                code: "not_found".into(),
                message: "invitation not found".into(),
            });
        };
        if is_satellite_office_removed_locked(self, &inv.office_id)? {
            return Ok(InvitationPollStatus::Terminal {
                code: "revoked".into(),
                message: "office certificate was revoked".into(),
            });
        }
        if now_ms > inv.expires_at_ms
            && matches!(
                inv.status,
                InvitationStatus::Pending | InvitationStatus::AwaitingApproval
            )
        {
            return Ok(InvitationPollStatus::Terminal {
                code: "expired".into(),
                message: "invitation expired".into(),
            });
        }
        match inv.status {
            InvitationStatus::Pending | InvitationStatus::AwaitingApproval => {
                Ok(InvitationPollStatus::Pending)
            }
            InvitationStatus::Approved => {
                let cert_der = STANDARD
                    .decode(inv.issued_cert_der_b64.as_deref().unwrap_or_default())
                    .map_err(|_| OfficeError::Invalid("stored issued cert corrupt"))?;
                let hq_tls = ensure_hq_tls_identity_locked(self, &inv.hq_addresses)?;
                Ok(InvitationPollStatus::Approved {
                    cert_der,
                    ca_cert_der: hq_tls.ca_cert_der,
                    hq_office_id: self.info.office_id.clone(),
                    epoch: hq_tls.epoch,
                })
            }
            InvitationStatus::Declined => Ok(InvitationPollStatus::Terminal {
                code: "declined".into(),
                message: "invitation was declined at HQ".into(),
            }),
            InvitationStatus::Cancelled => Ok(InvitationPollStatus::Terminal {
                code: "cancelled".into(),
                message: "invitation was cancelled at HQ".into(),
            }),
            InvitationStatus::Expired => Ok(InvitationPollStatus::Terminal {
                code: "expired".into(),
                message: "invitation expired".into(),
            }),
            InvitationStatus::Revoked => Ok(InvitationPollStatus::Terminal {
                code: "revoked".into(),
                message: "office certificate was revoked".into(),
            }),
        }
    }
}
