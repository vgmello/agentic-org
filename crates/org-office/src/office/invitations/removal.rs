//! Removing a satellite office: certificate revocation and the promotion of its pending
//! origin withdrawals.

use std::collections::BTreeSet;

use org_proto::{ApiError, ErrorCode};

use crate::office::Office;
use crate::{error::OfficeError, model::OfficeRole};

use super::peers::{
    cleanup_peer_activity_fence_if_unused, peer_activity_fence_for, signal_peer_session_stops,
};
use super::{
    load_invitations, load_revocations, require_hq_owner, save_invitations, save_revocations,
    InvitationStatus, SatelliteRemovalResult,
};

impl Office {
    /// Revokes all issued certificates for `satellite_office_id` on HQ so it is immediately
    /// refused at the mTLS handshake and federation gate (`doc-67`).
    pub fn remove_satellite_office(
        &self,
        actor_member_id: &str,
        satellite_office_id: &str,
    ) -> Result<Result<SatelliteRemovalResult, ApiError>, OfficeError> {
        if let Err(err) = require_hq_owner(self, actor_member_id) {
            return Ok(Err(err));
        }
        if !org_proto::is_uuid_v7(satellite_office_id) || satellite_office_id == self.info.office_id
        {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "cannot remove HQ or invalid satellite office id",
            )));
        }
        let removed_id = satellite_office_id.to_owned();
        // Signal active sessions first so session loops stop starting new fenced work, then take
        // the exclusive per-peer activity barrier BEFORE publishing `revocations.v1.json` so every
        // already-admitted origin close or outbound send for this office completes before HQ
        // takeover or reconciliation can observe removal without blocking unrelated peers
        // (`TASK-52` P20-#2 / P25-#2).
        signal_peer_session_stops(self, &removed_id, false);
        let peer_fence = peer_activity_fence_for(self, &removed_id);
        let _barrier = peer_fence.write().unwrap_or_else(|e| e.into_inner());
        let (result, inv_save_err) = {
            let _guard = self
                .federation_io_lock
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let mut state = load_invitations(self)?;
            let mut rev = load_revocations(self)?;
            let mut newly_revoked = Vec::new();
            for inv in &mut state.invitations {
                if inv.office_id == satellite_office_id {
                    if let Some(serial) = &inv.issued_cert_serial_hex {
                        if rev.revoked_serials.insert(serial.clone()) {
                            newly_revoked.push(serial.clone());
                        }
                    }
                    inv.status = InvitationStatus::Revoked;
                }
            }
            rev.removed_offices.insert(removed_id.clone());
            save_revocations(self, &rev)?;
            // Revocation is now durable: save invitations WITHOUT skipping post-revocation relay
            // purge or live-session invalidation if `save_invitations` fails (`TASK-52` P20-#3).
            let inv_save_err = save_invitations(self, &state).err();
            let _ = self
                .replication
                .purge_relays_for_office(satellite_office_id);
            self.promoted_removed_offices
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&removed_id);
            (
                SatelliteRemovalResult {
                    office_id: removed_id.clone(),
                    revoked_serials: newly_revoked,
                },
                inv_save_err,
            )
        };
        // Still holding `_barrier`, invalidate any session that registered before the barrier.
        signal_peer_session_stops(self, &removed_id, true);
        drop(_barrier);
        cleanup_peer_activity_fence_if_unused(self, &removed_id, &peer_fence);
        // Promote any provisional HQ withdrawals for asks originated by the removed satellite
        // one journal commit at a time (`TASK-52` P15-#3 / P16-#2 / P16-#3).
        let _ = promote_pending_removed_origin_withdrawals(self);
        if let Some(err) = inv_save_err {
            return Err(err);
        }
        Ok(Ok(result))
    }

    pub fn revoked_cert_serials(&self) -> Result<BTreeSet<String>, OfficeError> {
        Ok(load_revocations(self)?.revoked_serials)
    }

    pub fn is_cert_serial_revoked(&self, serial_hex: &str) -> Result<bool, OfficeError> {
        Ok(load_revocations(self)?.revoked_serials.contains(serial_hex))
    }
}

/// Reconciles every permanently removed satellite office on HQ:
/// 1. Purges any lingering queued relays for each removed office under `federation_io_lock`
///    (retrying if a crash or storage error interrupted purge after `save_revocations`, `TASK-52` P17-#3).
/// 2. Promotes unfinalized provisional HQ withdrawals for asks originated by removed offices,
///    committing ONE withdrawal per journal entry (`TASK-52` P16-#2 / P16-#3).
pub fn promote_pending_removed_origin_withdrawals(office: &Office) -> Result<usize, OfficeError> {
    if office.info.role != OfficeRole::Hq {
        return Ok(0);
    }
    let removed = removed_satellite_offices(office)?;
    if removed.is_empty() {
        return Ok(0);
    }
    let pending: BTreeSet<String> = {
        let done = office
            .promoted_removed_offices
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        removed.difference(&done).cloned().collect()
    };
    if pending.is_empty() {
        return Ok(0);
    }
    // Re-purge every pending removed office under `federation_io_lock` first, propagating errors
    // so a crash or transient write failure never strands non-expiring relays (`TASK-52` P17-#3).
    {
        let _guard = office
            .federation_io_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        for oid in &pending {
            office.replication.purge_relays_for_office(oid)?;
        }
    }
    let mut promoted = 0usize;
    loop {
        let did_one = office
            .store
            .run(PromoteOneRemovedOriginWithdrawalOp {
                removed_offices: pending.clone(),
            })
            .map_err(OfficeError::storage)?;
        if !did_one {
            break;
        }
        promoted += 1;
    }
    let mut done = office
        .promoted_removed_offices
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for oid in pending {
        done.insert(oid);
    }
    Ok(promoted)
}

/// Promotes AT MOST ONE unfinalized provisional HQ withdrawal for asks originated by any office in
/// `removed_offices` to `reason: Some("origin-close")` per journal commit so every promoted entry
/// stays well within per-destination relay quotas and restarts resume cleanly (`TASK-52` P16-#2/#3).
struct PromoteOneRemovedOriginWithdrawalOp {
    removed_offices: BTreeSet<String>,
}

impl crate::store::open::StoreOp for PromoteOneRemovedOriginWithdrawalOp {
    type Out = Result<bool, org_store::StoreError>;

    fn run<E: crate::store::open::OfficeEngine>(self, store: &org_store::Store<E>) -> Self::Out {
        use crate::messaging::ports::MessagingRead;
        use crate::messaging::records::{encode_json, participant_answer_path};
        use org_store::{Ack, Change, Refusal};
        let removed = self.removed_offices;
        let promoted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let slot = std::sync::Arc::clone(&promoted);
        store.submit(None, false, move |uow| {
            let convs = uow
                .view()
                .conversations()
                .map_err(|_| Refusal::invalid("read conversations"))?;
            for conv in convs {
                let msgs = uow
                    .view()
                    .messages_in_conversation(&conv.id)
                    .map_err(|_| Refusal::invalid("read conversation messages"))?;
                for msg in msgs {
                    if msg.kind != "request" || !removed.contains(msg.origin_office_id.as_str()) {
                        continue;
                    }
                    let Some(mut ans) = uow
                        .view()
                        .answer_for_message(&msg.id)
                        .map_err(|_| Refusal::invalid("read ask answer"))?
                    else {
                        continue;
                    };
                    let is_provisional_withdrawal =
                        crate::messaging::asks::decode_ask_answer_file(&ans)
                            .1
                            .is_some()
                            && ans.reason.as_deref() != Some("origin-close");
                    if !is_provisional_withdrawal {
                        continue;
                    }
                    ans.reason = Some("origin-close".to_owned());
                    ans.revision = ans.revision.saturating_add(1);
                    let path = participant_answer_path(
                        &ans.conversation_id,
                        &ans.employee_id,
                        &ans.message_id,
                    );
                    slot.store(true, std::sync::atomic::Ordering::Release);
                    return Ok(Change::new(Ack::Durable).put(path, encode_json(&ans)));
                }
            }
            Ok(Change::new(Ack::Durable))
        })?;
        Ok(promoted.load(std::sync::atomic::Ordering::Acquire))
    }
}

pub(crate) fn is_satellite_office_removed(
    office: &Office,
    office_id: &str,
) -> Result<bool, OfficeError> {
    let _guard = office
        .federation_io_lock
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    is_satellite_office_removed_locked(office, office_id)
}

/// Checks `removed_offices` fail-closed when the caller already holds `office.federation_io_lock`.
pub(crate) fn is_satellite_office_removed_locked(
    office: &Office,
    office_id: &str,
) -> Result<bool, OfficeError> {
    Ok(removed_satellite_offices_locked(office)?.contains(office_id))
}

/// Loads all permanently removed satellite office IDs fail-closed when `office.federation_io_lock` is held.
pub(crate) fn removed_satellite_offices_locked(
    office: &Office,
) -> Result<BTreeSet<String>, OfficeError> {
    // Only HQ maintains the local CA `revocations.v1.json` store; satellites learn enrolled
    // offices from HQ's replicated org projection and store `org-ca.v1.der` without a local
    // revocations file (`TASK-117`). Keep HQ strictly fail-closed via `load_revocations`.
    if office.info.role == crate::OfficeRole::Satellite {
        return Ok(BTreeSet::new());
    }
    Ok(load_revocations(office)?.removed_offices)
}

/// Loads all permanently removed satellite office IDs fail-closed, acquiring `office.federation_io_lock`.
pub(crate) fn removed_satellite_offices(office: &Office) -> Result<BTreeSet<String>, OfficeError> {
    let _guard = office
        .federation_io_lock
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    removed_satellite_offices_locked(office)
}
