//! Organization events on a satellite: pending work paths, pre-apply verification, and applying
//! HQ event frames, in verified batches (`TASK-140`).

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use org_office::{Office, OrgDeltaPayload};
use org_sync::organization::{
    apply as apply_org_event, apply_batch as apply_org_events, OrgEvent, OrgOutput, PeerIdentity,
};
use org_sync_proto::{Codec, Digest, Epoch, OrgId, Position, SyncFrame};

use super::{deterministic_uuid_v7, SnapshotFileBatch, StaticPeer};

/// Most consecutive `OrgEvent` frames applied as one batch (`TASK-140`).
pub(crate) const ORG_EVENT_BATCH_MAX: usize = 64;

/// Most replicated file bytes one batch's single files operation carries, far below
/// `org_store::MAX_ENTRY_BYTES`: merging events never builds an entry a lone event could not.
const ORG_EVENT_BATCH_FILE_BYTES: usize = 4 * 1024 * 1024;

/// Applies consecutive HQ `OrgEvent` frames in order and returns how many were new
/// (`TASK-140`). Every frame is decoded and verified first (organization, epoch, predecessor
/// chain, digest, codec) against the checkpoint the frames before it reach. Verified events
/// then apply in batches of up to [`ORG_EVENT_BATCH_MAX`]: one replicated-files operation and
/// one replication transaction per batch instead of one durable sync each, so a 420-task
/// burst costs a handful of commits. An event touching anything but task or messaging files
/// (organization structure, which the self-echo filter and HQ office ordering read) applies
/// alone, exactly as before. The checkpoint never passes files that are not applied: files
/// first, then the events. When a frame fails, the events before it are applied first and
/// the same error is returned, as one call per frame would.
pub(crate) fn apply_satellite_org_event_frames(
    office: &Office,
    hq_peer: &PeerIdentity,
    frames: Vec<SyncFrame>,
) -> Result<usize, String> {
    let mut cursor = office
        .replication()
        .org_checkpoint()
        .map_err(|e| e.to_string())?;
    let mut burst = Burst::default();
    let mut applied = 0usize;
    for frame in frames {
        let verified = decode_org_event_frame(frame).and_then(|ev| {
            let files = verify_org_event_before_apply(&cursor, hq_peer, &ev)?;
            Ok((ev, files))
        });
        let (ev, files) = match verified {
            Ok(verified) => verified,
            Err(e) => {
                // The verified events before it still apply, as one call per frame did.
                burst.apply(office, hq_peer)?;
                return Err(e);
            }
        };
        let alone = files
            .as_ref()
            .is_some_and(|files| !files.iter().all(|(path, _)| is_work_path(path)));
        let bytes = files.as_ref().map_or(0, file_bytes);
        if alone || burst.is_full(bytes) {
            applied += burst.apply(office, hq_peer)?;
        }
        let duplicate =
            ev.position.epoch == cursor.position.epoch && ev.position.seq <= cursor.position.seq;
        if !duplicate {
            cursor.position = ev.position;
            cursor.digest = ev.digest;
        }
        burst.push(ev, files);
        if alone {
            applied += burst.apply(office, hq_peer)?;
        }
    }
    applied += burst.apply(office, hq_peer)?;
    Ok(applied)
}

fn is_work_path(path: &str) -> bool {
    path.starts_with("state/work/") || path.starts_with("messaging/")
}

fn file_bytes(files: &SnapshotFileBatch) -> usize {
    files
        .iter()
        .map(|(_, content)| content.as_ref().map_or(0, Vec::len))
        .sum()
}

/// Verified events waiting to apply together, with their files merged in event order: a path
/// a later event writes again keeps only its last content, the state one-at-a-time applies
/// would leave.
#[derive(Default)]
struct Burst {
    events: Vec<OrgEvent>,
    files: Vec<Option<(String, Option<Vec<u8>>)>>,
    index: HashMap<String, usize>,
    bytes: usize,
}

impl Burst {
    fn is_full(&self, more_bytes: usize) -> bool {
        !self.events.is_empty()
            && (self.events.len() >= ORG_EVENT_BATCH_MAX
                || self.bytes.saturating_add(more_bytes) > ORG_EVENT_BATCH_FILE_BYTES)
    }

    fn push(&mut self, ev: OrgEvent, files: Option<SnapshotFileBatch>) {
        for (path, content) in files.into_iter().flatten() {
            self.bytes = self
                .bytes
                .saturating_add(content.as_ref().map_or(0, Vec::len));
            if let Some(earlier) = self.index.insert(path.clone(), self.files.len()) {
                self.files[earlier] = None;
            }
            self.files.push(Some((path, content)));
        }
        self.events.push(ev);
    }

    /// Applies the batch's files, then its events; empties the batch.
    fn apply(&mut self, office: &Office, hq_peer: &PeerIdentity) -> Result<usize, String> {
        let Burst { events, files, .. } = std::mem::take(self);
        let (Some(first), Some(last)) = (events.first(), events.last()) else {
            return Ok(0);
        };
        let files: SnapshotFileBatch = files.into_iter().flatten().collect();
        if !files.is_empty() {
            let filtered = office
                .filter_self_echoed_org_event_files(files)
                .map_err(|e| format!("filter self-echoed org event failed: {e}"))?;
            if !filtered.is_empty() {
                let (hq, org) = (hq_peer.office_id.as_str(), first.org_id.as_str());
                let name = if events.len() == 1 {
                    let (epoch, seq) = (first.position.epoch.get(), first.position.seq);
                    format!("org-event:{hq}:{org}:{epoch}:{seq}")
                } else {
                    let (from, to) = (first.position, last.position);
                    format!(
                        "org-event-batch:{hq}:{org}:{}:{}:{}:{}",
                        from.epoch.get(),
                        from.seq,
                        to.epoch.get(),
                        to.seq
                    )
                };
                office
                    .apply_replicated_state_files(&deterministic_uuid_v7(&name), filtered)
                    .map_err(|e| format!("apply replicated delta failed: {e}"))?;
            }
        }
        let peer = StaticPeer(hq_peer.clone());
        let store = office.replication();
        let outputs = match apply_org_events(store.as_ref(), &peer, events.clone()) {
            Ok(outputs) => outputs,
            // The batch committed nothing: replay one at a time so the events before the
            // failing one commit and its error is the one a lone apply reports.
            Err(_) => {
                let mut outputs = Vec::with_capacity(events.len());
                for ev in events {
                    let output = apply_org_event(store.as_ref(), &peer, ev)
                        .map_err(|e| format!("apply_org_event failed: {e}"))?;
                    let more = matches!(output, OrgOutput::Ack { .. });
                    outputs.push(output);
                    if !more {
                        break;
                    }
                }
                outputs
            }
        };
        let mut applied = 0usize;
        for output in outputs {
            match output {
                OrgOutput::Ack {
                    duplicate: false, ..
                } => applied += 1,
                OrgOutput::Ack {
                    duplicate: true, ..
                } => {}
                OrgOutput::Gap { .. } => {
                    return Err("org event gap detected; resync required".into());
                }
                OrgOutput::SnapshotRequired => {
                    return Err("snapshot required before applying org events".into());
                }
                other => return Err(format!("unexpected org apply outcome: {other:?}")),
            }
        }
        Ok(applied)
    }
}

fn decode_org_event_frame(frame: SyncFrame) -> Result<OrgEvent, String> {
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
    Ok(OrgEvent {
        org_id: OrgId::new(&org_id).map_err(|_| "org_id".to_owned())?,
        position: Position {
            epoch: Epoch::new(epoch).map_err(|_| "epoch".to_owned())?,
            seq,
        },
        previous: Digest::parse_hex(&previous_hex).map_err(|_| "prev".to_owned())?,
        digest: Digest::parse_hex(&digest_hex).map_err(|_| "digest".to_owned())?,
        codec: Codec::new(&codec_name, codec_version).map_err(|_| "codec".to_owned())?,
        payload: Arc::from(payload_bytes),
    })
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
