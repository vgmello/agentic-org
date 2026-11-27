//! Organization events on a satellite: pending work paths, pre-apply verification, and applying
//! HQ event frames, in verified batches (`TASK-140`).

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use org_office::{Office, OrgDeltaPayload};
use org_sync::error::{Refusal, SyncError};
use org_sync::organization::{
    apply as apply_org_event, apply_batch as apply_org_events, verify_pre_apply, OrgCheckpoint,
    OrgEvent, OrgOutput, PeerIdentity,
};
use org_sync_proto::{Codec, Digest, Epoch, OrgId, Position, SyncFrame};
use sha2::{Digest as _, Sha256};

use super::{deterministic_uuid_v7, hex_lower, SnapshotFileBatch, StaticPeer};

/// Most consecutive `OrgEvent` frames applied as one batch (`TASK-140`).
pub(crate) const ORG_EVENT_BATCH_MAX: usize = 64;

/// Most replicated file bytes one batch's single files operation carries, far below
/// `org_store::MAX_ENTRY_BYTES`: merging events never builds an entry a lone event could not.
/// Counted with [`entry_bytes`], framing included.
const ORG_EVENT_BATCH_FILE_BYTES: usize = 4 * 1024 * 1024;

/// A generous bound on what one file of a files operation adds to its `org-store` entry beside
/// its path and bytes (record framing, kind, lengths and digests), so thousands of tiny files
/// cannot carry a batch past `org_store::MAX_ENTRY_BYTES` through framing alone.
const ORG_EVENT_FILE_FRAMING_BYTES: usize = 256;

/// The error a lone apply reports for an [`OrgOutput::Gap`].
const GAP: &str = "org event gap detected; resync required";
/// The error a lone apply reports for an [`OrgOutput::SnapshotRequired`].
const SNAPSHOT_REQUIRED: &str = "snapshot required before applying org events";

/// Applies consecutive HQ `OrgEvent` frames in order and returns how many were new
/// (`TASK-140`). Every frame is decoded and verified first against the checkpoint the frames
/// before it reach: `org-sync`'s own pre-apply gates (`organization::verify_pre_apply`: HQ
/// authority and epoch, payload size, digest, codec, no restore fence), no initial snapshot
/// owed, then the predecessor chain.
/// Verified events then apply in batches of up to [`ORG_EVENT_BATCH_MAX`]: one
/// replicated-files operation and one replication transaction per batch instead of one
/// durable sync each, so a 420-task burst costs a handful of commits. Three kinds of event
/// apply alone, after the batch before them, exactly as one call per frame would: one touching
/// anything but task or messaging files (organization structure, which the self-echo filter
/// and HQ office ordering read), a duplicate (only `org-sync` can tell it from a divergent
/// one) and one of any codec but `org.native.changes` v1 (no files to verify, and possibly
/// unsupported). Their outcome is therefore known before any later event's files reach
/// `org-store`. The checkpoint never passes files that are not applied: files first, then the
/// events. When a frame fails, the events before it are applied first and the same error is
/// returned, as one call per frame would.
pub fn apply_satellite_org_event_frames(
    office: &Office,
    hq_peer: &PeerIdentity,
    frames: Vec<SyncFrame>,
) -> Result<usize, String> {
    let mut cursor = office
        .replication()
        .org_checkpoint()
        .map_err(|e| e.to_string())?;
    let initial_snapshot_required = office
        .sync_status()
        .map_err(|e| e.to_string())?
        .initial_snapshot_required;
    let mut burst = Burst::default();
    let mut applied = 0usize;
    for frame in frames {
        let verified = decode_org_event_frame(frame).and_then(|ev| {
            let files =
                verify_org_event_before_apply(&cursor, initial_snapshot_required, hq_peer, &ev)?;
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
        let duplicate = is_duplicate(&cursor, &ev);
        let alone = duplicate
            || !is_native_changes(&ev.codec)
            || files
                .as_ref()
                .is_some_and(|files| !files.iter().all(|(path, _)| is_work_path(path)));
        let bytes = files.as_ref().map_or(0, file_bytes);
        if alone || burst.is_full(bytes) {
            applied += burst.apply(office, hq_peer)?;
        }
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

/// A task folder file, a task history event or a messaging file: what a satellite's own
/// work touches (`doc-77`, `doc-79`).
fn is_work_path(path: &str) -> bool {
    org_office::task_id_for_work_path(path).is_some() || path.starts_with("messaging/")
}

/// The one codec whose payload this side decodes into files before applying.
fn is_native_changes(codec: &Codec) -> bool {
    codec.id() == "org.native.changes" && codec.version() == 1
}

/// An event at or before the cursor in its epoch: `org-sync` acknowledges it as a duplicate
/// or refuses it as divergent against the retained history.
fn is_duplicate(cur: &OrgCheckpoint, ev: &OrgEvent) -> bool {
    ev.position.epoch == cur.position.epoch && ev.position.seq <= cur.position.seq
}

/// The `org-store` operation ID of the replicated files applied for the events `first..=last`:
/// named by the events AND by exactly what is applied for them (`files`, after the self-echo
/// filter). An exact retry derives the same ID and meets its idempotency receipt; a retry the
/// filter trims (a local task update already applied before a crash) is a new operation,
/// never a reuse of the old ID with other files (`StoreError::Conflict`).
pub fn replicated_files_op_id(
    hq_office: &str,
    first: &OrgEvent,
    last: &OrgEvent,
    files: &SnapshotFileBatch,
) -> String {
    let org = first.org_id.as_str();
    let files = files_sha256_hex(files);
    let name = if first.position == last.position {
        let (epoch, seq) = (first.position.epoch.get(), first.position.seq);
        format!("org-event:{hq_office}:{org}:{epoch}:{seq}:{files}")
    } else {
        let (from, to) = (first.position, last.position);
        format!(
            "org-event-batch:{hq_office}:{org}:{}:{}:{}:{}:{files}",
            from.epoch.get(),
            from.seq,
            to.epoch.get(),
            to.seq
        )
    };
    deterministic_uuid_v7(&name)
}

/// SHA-256 over `files` in order: each path, whether it is written or removed, and the
/// written bytes, every variable part length-prefixed so two batches never share an encoding.
fn files_sha256_hex(files: &SnapshotFileBatch) -> String {
    let mut hasher = Sha256::new();
    for (path, content) in files {
        hasher.update((path.len() as u64).to_le_bytes());
        hasher.update(path.as_bytes());
        match content {
            Some(bytes) => {
                hasher.update([1u8]);
                hasher.update((bytes.len() as u64).to_le_bytes());
                hasher.update(bytes);
            }
            None => hasher.update([0u8]),
        }
    }
    hex_lower(&hasher.finalize())
}

fn file_bytes(files: &SnapshotFileBatch) -> usize {
    files
        .iter()
        .map(|(path, content)| entry_bytes(path, content.as_ref()))
        .fold(0, usize::saturating_add)
}

/// What one file adds to a batch's files operation: its path, its bytes and its framing.
fn entry_bytes(path: &str, content: Option<&Vec<u8>>) -> usize {
    path.len()
        .saturating_add(ORG_EVENT_FILE_FRAMING_BYTES)
        .saturating_add(content.map_or(0, Vec::len))
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
                .saturating_add(entry_bytes(&path, content.as_ref()));
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
                let op_id =
                    replicated_files_op_id(hq_peer.office_id.as_str(), first, last, &filtered);
                office
                    .apply_replicated_state_files(&op_id, filtered)
                    .map_err(|e| format!("apply replicated delta failed: {e}"))?;
            }
        }
        let peer = StaticPeer(hq_peer.clone());
        let store = office.replication();
        let batched = if events.len() > 1 {
            match apply_org_events(store.as_ref(), &peer, events.clone()) {
                Ok(outputs) => Some(outputs),
                // A refusal aborted the whole transaction: nothing of the batch committed.
                Err(SyncError::Refused(_)) => None,
                // A storage failure may come after a durable commit: replaying the events
                // one at a time could take committed ones for duplicates. Report it.
                Err(e) => return Err(format!("apply_org_event failed: {e}")),
            }
        } else {
            None
        };
        let outputs = match batched {
            Some(outputs) => outputs,
            // A lone event, or a batch a refusal rolled back: one `apply` per event, so the
            // events before a failing one commit and its error is the one a lone apply reports.
            None => {
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
                OrgOutput::Gap { .. } => return Err(GAP.into()),
                OrgOutput::SnapshotRequired => return Err(SNAPSHOT_REQUIRED.into()),
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

/// Verifies `ev` against the checkpoint `cur` before anything of it is buffered or applied,
/// and returns the files of an `org.native.changes` v1 event. Refuses, in `organization::apply`'s
/// order and with the error a lone apply reports: what `org-sync`'s own pre-apply gates refuse
/// (`organization::verify_pre_apply`, the one implementation `apply` runs: checkpoint, HQ
/// authority and epoch, sequence, payload size, digest, codec, restore fence); an initial
/// snapshot still owed; then, for the event after the checkpoint, a gap or a broken chain.
/// A repeat of the checkpoint or an earlier event is left to `org-sync`, which alone holds the
/// retained history (`Ok(None)`).
fn verify_org_event_before_apply(
    cur: &OrgCheckpoint,
    initial_snapshot_required: bool,
    hq_peer: &PeerIdentity,
    ev: &OrgEvent,
) -> Result<Option<SnapshotFileBatch>, String> {
    verify_pre_apply(cur, hq_peer, ev).map_err(refused)?;
    if initial_snapshot_required {
        return Err(SNAPSHOT_REQUIRED.into());
    }
    if is_duplicate(cur, ev) {
        return Ok(None);
    }
    // Past the gates the event and the checkpoint share the highest epoch: `org-sync`'s order
    // for an event past the checkpoint is a gap, a broken chain (divergence) or the next one.
    if cur.position.seq.checked_add(1) != Some(ev.position.seq) {
        return Err(GAP.into());
    }
    if ev.previous != cur.digest {
        return Err(refused(Refusal::Divergence));
    }
    if is_native_changes(&ev.codec) {
        let delta: OrgDeltaPayload = serde_json::from_slice(&ev.payload)
            .map_err(|e| format!("malformed org.native.changes payload: {e}"))?;
        let files = delta.decode_files().map_err(|e| e.to_string())?;
        return Ok(Some(files));
    }
    Ok(None)
}

/// The error a lone `organization::apply` reports for `refusal`.
fn refused(refusal: Refusal) -> String {
    format!("apply_org_event failed: {}", SyncError::from(refusal))
}
