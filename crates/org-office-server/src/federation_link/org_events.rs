//! Organization events on a satellite: pending work paths, pre-apply verification, and applying
//! HQ event frames.

use std::collections::BTreeSet;
use std::sync::Arc;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use org_office::{Office, OrgDeltaPayload};
use org_sync::organization::{apply as apply_org_event, OrgEvent, OrgOutput, PeerIdentity};
use org_sync_proto::{Codec, Digest, Epoch, OrgId, Position, SyncFrame};

use super::{deterministic_uuid_v7, SnapshotFileBatch, StaticPeer};

pub(crate) fn apply_satellite_org_event_frame(
    office: &Office,
    hq_peer: &PeerIdentity,
    frame: SyncFrame,
) -> Result<bool, String> {
    let SyncFrame::OrgEvent {
        org_id,
        epoch,
        seq,
        previous_hex,
        digest_hex,
        codec_name,
        codec_version,
        payload_b64,
    } = frame
    else {
        return Err(format!("expected OrgEvent frame, got {frame:?}"));
    };
    let payload_bytes = STANDARD
        .decode(&payload_b64)
        .map_err(|_| "payload b64".to_owned())?;
    let ev = OrgEvent {
        org_id: OrgId::new(&org_id).map_err(|_| "org_id".to_owned())?,
        position: Position {
            epoch: Epoch::new(epoch).map_err(|_| "epoch".to_owned())?,
            seq,
        },
        previous: Digest::parse_hex(&previous_hex).map_err(|_| "prev".to_owned())?,
        digest: Digest::parse_hex(&digest_hex).map_err(|_| "digest".to_owned())?,
        codec: Codec::new(&codec_name, codec_version).map_err(|_| "codec".to_owned())?,
        payload: Arc::from(payload_bytes),
    };
    let cur = office
        .replication()
        .org_checkpoint()
        .map_err(|e| e.to_string())?;
    if let Some(files) = verify_org_event_before_apply(&cur, hq_peer, &ev)? {
        let filtered = office
            .filter_self_echoed_org_event_files(files)
            .map_err(|e| format!("filter self-echoed org event failed: {e}"))?;
        if !filtered.is_empty() {
            let op_id = deterministic_uuid_v7(&format!(
                "org-event:{}:{}:{}:{}",
                hq_peer.office_id.as_str(),
                ev.org_id.as_str(),
                ev.position.epoch.get(),
                ev.position.seq
            ));
            office
                .apply_replicated_state_files(&op_id, filtered)
                .map_err(|e| format!("apply replicated delta failed: {e}"))?;
        }
    }
    let peer = StaticPeer(hq_peer.clone());
    match apply_org_event(office.replication().as_ref(), &peer, ev)
        .map_err(|e| format!("apply_org_event failed: {e}"))?
    {
        OrgOutput::Ack {
            duplicate: false, ..
        } => Ok(true),
        OrgOutput::Ack {
            duplicate: true, ..
        } => Ok(false),
        OrgOutput::Gap { .. } => Err("org event gap detected; resync required".into()),
        OrgOutput::SnapshotRequired => Err("snapshot required before applying org events".into()),
        other => Err(format!("unexpected org apply outcome: {other:?}")),
    }
}

pub(crate) fn collect_pending_work_paths_checked(
    office: &Office,
) -> Result<BTreeSet<String>, String> {
    let mut keep = BTreeSet::new();
    let outbox = office
        .replication()
        .load_work_outbox()
        .map_err(|e| format!("failed loading local work outbox: {e}"))?;
    for change in outbox.pending() {
        let delta = serde_json::from_slice::<OrgDeltaPayload>(&change.payload)
            .map_err(|e| format!("malformed pending work delta in outbox: {e}"))?;
        for (path, _) in delta.files {
            keep.insert(path);
        }
    }
    Ok(keep)
}

fn verify_org_event_before_apply(
    cur: &org_sync::organization::OrgCheckpoint,
    hq_peer: &PeerIdentity,
    ev: &OrgEvent,
) -> Result<Option<SnapshotFileBatch>, String> {
    if ev.org_id != cur.org_id || hq_peer.org_id != cur.org_id {
        return Err("org event organization mismatch".into());
    }
    if ev.position.epoch < cur.position.epoch {
        return Err("stale org event epoch".into());
    }
    if ev.position.epoch == cur.position.epoch && ev.position.seq <= cur.position.seq {
        return Ok(None);
    }
    if ev.position.epoch != cur.position.epoch
        || ev.position.seq != cur.position.seq.saturating_add(1)
        || ev.previous != cur.digest
    {
        return Err("org event gap or predecessor mismatch".into());
    }
    let expected_digest = org_sync_proto::event_digest_v1(
        &ev.org_id,
        ev.position,
        ev.previous,
        &ev.codec,
        &ev.payload,
    )
    .map_err(|_| "invalid org event digest parameters".to_owned())?;
    if ev.digest != expected_digest {
        return Err("org event digest verification failed".into());
    }
    if ev.codec.id() == "org.native.changes" && ev.codec.version() == 1 {
        let delta: OrgDeltaPayload = serde_json::from_slice(&ev.payload)
            .map_err(|e| format!("malformed org.native.changes payload: {e}"))?;
        let files = delta.decode_files().map_err(|e| e.to_string())?;
        return Ok(Some(files));
    }
    Ok(None)
}
