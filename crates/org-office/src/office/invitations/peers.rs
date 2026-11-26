//! Peer session stops, activity fences and relay admission for active peers only.

use crate::error::OfficeError;
use crate::office::Office;

use super::load_revocations;
use super::removal::is_satellite_office_removed_locked;

/// Atomically checks origin and destination revocation and enrollment under `federation_io_lock`
/// before admitting `item` into the durable relay store, preventing concurrent removal races from
/// admitting removed senders or recreating a removed office's queue and failing closed on
/// revocation read errors (`TASK-52` P16-#4 / P17-#4 / P18-#2).
pub fn admit_relay_item_if_active(
    office: &Office,
    item: crate::replication::StoredRelayItem,
) -> Result<RelayAdmission, OfficeError> {
    let _guard = office
        .federation_io_lock
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if is_satellite_office_removed_locked(office, &item.origin_office)? {
        return Ok(RelayAdmission::OriginRemoved);
    }
    if is_satellite_office_removed_locked(office, &item.destination_office)? {
        return Ok(RelayAdmission::DestinationRemoved);
    }
    let enrolled = office
        .enrolled_office_ids()?
        .iter()
        .any(|id| id == &item.destination_office);
    if !enrolled {
        return Ok(RelayAdmission::DestinationUnknown);
    }
    match office.replication.enqueue_relay_item(item) {
        Ok(()) => Ok(RelayAdmission::Admitted),
        Err(_) => Ok(RelayAdmission::QuotaExceeded),
    }
}

/// Atomically checks destination revocation under `federation_io_lock` before publishing one
/// destination's native journal relays, failing closed on revocation read errors (`TASK-52` P16-#4 / P17-#4).
pub fn publish_message_journal_relay_for_dest_checked(
    office: &Office,
    destination_office: &str,
    messages: Vec<org_sync::messages::Message>,
    relays: Vec<crate::replication::StoredRelayItem>,
    incarnation: &str,
    ordinal: u64,
    now_ms: u64,
) -> Result<usize, OfficeError> {
    let _guard = office
        .federation_io_lock
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if is_satellite_office_removed_locked(office, destination_office)? {
        return Ok(0);
    }
    office.replication.publish_message_journal_relay_for_dest(
        destination_office,
        messages,
        relays,
        incarnation,
        ordinal,
        now_ms,
    )
}

/// Atomically acknowledges a destination message relay and enqueues any forwarded receipt ONLY
/// if the receipt's target origin office has not been permanently removed, failing closed on
/// revocation read errors (`TASK-52` P16-#4 / P17-#4).
pub fn ack_message_relay_and_enqueue_receipt_checked(
    office: &Office,
    destination_office: &str,
    origin_office: &str,
    message_id: &str,
    fingerprint_hex: &str,
    receipt_item: Option<crate::replication::StoredRelayItem>,
) -> Result<bool, OfficeError> {
    let _guard = office
        .federation_io_lock
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let receipt_item = match receipt_item {
        Some(r) if is_satellite_office_removed_locked(office, &r.destination_office)? => None,
        other => other,
    };
    office.replication.ack_message_relay_and_enqueue_receipt(
        destination_office,
        origin_office,
        message_id,
        fingerprint_hex,
        receipt_item,
    )
}

/// RAII guard unregistering a live HQ peer session's stop flag on drop (`TASK-52` P19-#2).
pub struct PeerSessionGuard<'a> {
    office: &'a Office,
    peer_office_id: String,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for PeerSessionGuard<'_> {
    fn drop(&mut self) {
        let mut stops = self
            .office
            .peer_session_stops
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(flags) = stops.get_mut(&self.peer_office_id) {
            flags.retain(|f| !std::sync::Arc::ptr_eq(f, &self.stop));
            if flags.is_empty() {
                stops.remove(&self.peer_office_id);
            }
        }
    }
}

/// Registers a live HQ session's stop flag so `remove_satellite_office` can immediately
/// invalidate active sessions for that satellite before returning (`TASK-52` P19-#2).
pub fn register_peer_session_stop<'a>(
    office: &'a Office,
    peer_office_id: &str,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> PeerSessionGuard<'a> {
    let mut stops = office
        .peer_session_stops
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    stops
        .entry(peer_office_id.to_owned())
        .or_default()
        .push(std::sync::Arc::clone(&stop));
    PeerSessionGuard {
        office,
        peer_office_id: peer_office_id.to_owned(),
        stop,
    }
}

pub(super) fn signal_peer_session_stops(office: &Office, peer_office_id: &str, remove_entry: bool) {
    let mut stops = office
        .peer_session_stops
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(flags) = stops.get(peer_office_id) {
        for flag in flags {
            flag.store(true, std::sync::atomic::Ordering::Release);
        }
    }
    if remove_entry {
        stops.remove(peer_office_id);
    }
}

/// Checks fail-closed whether `peer_office_id` (or optional `cert_serial_hex`) has been revoked
/// or permanently removed in `revocations.v1.json`, synchronizing only on the leaf
/// `revocation_lock` so keepalives never block on `federation_io_lock` (`TASK-52` P24-#1).
pub fn is_peer_revoked(
    office: &Office,
    peer_office_id: &str,
    cert_serial_hex: Option<&str>,
) -> Result<bool, OfficeError> {
    let rev = load_revocations(office)?;
    Ok(rev.removed_offices.contains(peer_office_id)
        || cert_serial_hex.is_some_and(|s| rev.revoked_serials.contains(s)))
}

pub(super) fn peer_activity_fence_for(
    office: &Office,
    peer_office_id: &str,
) -> std::sync::Arc<std::sync::RwLock<()>> {
    let mut fences = office
        .peer_activity_fences
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    std::sync::Arc::clone(
        fences
            .entry(peer_office_id.to_owned())
            .or_insert_with(|| std::sync::Arc::new(std::sync::RwLock::new(()))),
    )
}

pub(super) fn cleanup_peer_activity_fence_if_unused(
    office: &Office,
    peer_office_id: &str,
    fence: &std::sync::Arc<std::sync::RwLock<()>>,
) {
    let mut fences = office
        .peer_activity_fences
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if fences.get(peer_office_id).is_some_and(|cur| {
        std::sync::Arc::ptr_eq(cur, fence) && std::sync::Arc::strong_count(cur) == 2
    }) {
        fences.remove(peer_office_id);
    }
}

/// Executes `f` under `peer_office_id`'s per-peer activity fence ONLY if `peer_office_id` (and
/// optional `cert_serial_hex`) is not revoked/removed in `revocations.v1.json`, failing closed on
/// revocation read errors (`TASK-52` P19-#1 / P19-#2 / P25-#2).
pub fn run_if_peer_active<T>(
    office: &Office,
    peer_office_id: &str,
    cert_serial_hex: Option<&str>,
    f: impl FnOnce() -> T,
) -> Result<Option<T>, OfficeError> {
    let peer_fence = peer_activity_fence_for(office, peer_office_id);
    let _fence = peer_fence.read().unwrap_or_else(|e| e.into_inner());
    if is_peer_revoked(office, peer_office_id, cert_serial_hex)? {
        return Ok(None);
    }
    Ok(Some(f()))
}

/// Result of an atomic revocation-checked relay admission (`TASK-52` P16-#4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelayAdmission {
    Admitted,
    OriginRemoved,
    DestinationRemoved,
    DestinationUnknown,
    QuotaExceeded,
}
