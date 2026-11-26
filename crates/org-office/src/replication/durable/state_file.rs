//! The durable state file `state.v1.json`: encoding the replication core into it and
//! decoding it back, and the organization events' stored form. Engine-independent: no SQL
//! here.

use std::collections::BTreeMap;
use std::sync::Arc;

use org_sync::{
    messages::Queue,
    organization::{OrgCheckpoint, OrgEvent},
};
use org_sync_proto::{Codec, Epoch, OfficeId, OrgId, Position};

use crate::error::OfficeError;
use crate::replication::{
    decode_b64, decode_hex_digest, encode_b64, memory::ReplicationCoreData, message_from_stored,
    message_to_stored, outbox_from_stored, receipt_from_stored, receipt_to_stored,
    ReplicationStateFile, StoredEventsLog, StoredOrgEvent, StoredWorkChange,
};

/// An organization event in its stored form: a line of `events.v1.ndjson`, or an entry of
/// an inline `events` list.
pub(super) fn stored_event(e: &OrgEvent) -> StoredOrgEvent {
    StoredOrgEvent {
        epoch: e.position.epoch.get(),
        seq: e.position.seq,
        previous_hex: e.previous.to_hex(),
        digest_hex: e.digest.to_hex(),
        codec_name: e.codec.id().to_owned(),
        codec_version: e.codec.version(),
        payload_b64: encode_b64(&e.payload),
    }
}

/// Encodes `data` as `state.v1.json`: its events inline when `events_log` is `None`, else
/// only the log's committed extent (`TASK-140`).
pub(super) fn encode_state_file(
    data: &ReplicationCoreData,
    events_log: Option<&StoredEventsLog>,
) -> Result<Vec<u8>, OfficeError> {
    let cp = &data.checkpoint;
    let file = ReplicationStateFile {
        version: 1,
        org_id: cp.org_id.as_str().to_owned(),
        local_office: cp.local_office.as_str().to_owned(),
        hq_office: cp.hq_office.as_str().to_owned(),
        ca_hex: cp.ca.to_hex(),
        epoch: cp.position.epoch.get(),
        seq: cp.position.seq,
        digest_hex: cp.digest.to_hex(),
        highest_epoch: cp.highest_epoch.get(),
        restore_pending: cp.restore_pending,
        pending_restore_base_hex: cp.pending_restore_base.map(|d| d.to_hex()),
        protocol_level: cp.protocol_level,
        initial_snapshot_required: data.initial_snapshot_required,
        events: match events_log {
            Some(_) => Vec::new(),
            None => data.events.values().map(stored_event).collect(),
        },
        events_log: events_log.cloned(),
        work_incarnation: data.work_outbox.origin().incarnation.as_str().to_owned(),
        work_last_seq: data.work_outbox.last_seq(),
        work_acked_seq: data.work_outbox.acknowledged_seq(),
        work_pending: data
            .work_outbox
            .pending()
            .iter()
            .map(|c| StoredWorkChange {
                origin_office: c.key.origin.office.as_str().to_owned(),
                incarnation: c.key.origin.incarnation.as_str().to_owned(),
                seq: c.key.seq.get(),
                record_id: c.record_id.clone(),
                generation: c.version.generation,
                version: c.version.version,
                digest_hex: c.digest.to_hex(),
                record_digest_hex: c.record_digest.to_hex(),
                codec_name: c.codec.id().to_owned(),
                codec_version: c.codec.version(),
                payload_b64: encode_b64(&c.payload),
                required_org_epoch: c.required_org.epoch.get(),
                required_org_seq: c.required_org.seq,
                created_ms: c.created_ms,
            })
            .collect(),
        remote_work_high_water: data.remote_work_high_water.clone(),
        message_clock_high_ms: data.message_queue.clock_high_ms,
        message_pending: data
            .message_queue
            .pending
            .iter()
            .map(message_to_stored)
            .collect(),
        message_outgoing_receipts: data
            .outgoing_receipts
            .values()
            .map(receipt_to_stored)
            .collect(),
        message_received_receipts: data
            .received_receipts
            .values()
            .map(receipt_to_stored)
            .collect(),
        message_expired: data
            .expired_messages
            .values()
            .map(message_to_stored)
            .collect(),
        pending_relays: data.pending_relays.clone(),
        last_published_journal: data.last_published_journal.clone(),
        last_published_work_journal: data.last_published_work_journal.clone(),
        last_published_message_journal: data.last_published_message_journal.clone(),
        last_published_message_journal_by_dest: data.last_published_message_journal_by_dest.clone(),
        replicated_op_ids: data.replicated_op_ids.clone(),
    };
    serde_json::to_vec_pretty(&file).map_err(OfficeError::storage)
}

/// Decodes `state.v1.json` with its organization events, `events` (inline, or read from
/// the log by [`super::events_log::EventsLog::open`]).
pub(super) fn core_from_file(
    file: &ReplicationStateFile,
    stored_events: &[StoredOrgEvent],
) -> Result<ReplicationCoreData, OfficeError> {
    if file.version != 1 {
        return Err(OfficeError::Invalid(
            "unsupported replication state version",
        ));
    }
    let org_id = OrgId::new(&file.org_id).map_err(|_| OfficeError::Invalid("org_id"))?;
    let local_office =
        OfficeId::new(&file.local_office).map_err(|_| OfficeError::Invalid("local_office"))?;
    let hq_office =
        OfficeId::new(&file.hq_office).map_err(|_| OfficeError::Invalid("hq_office"))?;
    let ca = decode_hex_digest(&file.ca_hex)?;
    let epoch = Epoch::new(file.epoch).map_err(|_| OfficeError::Invalid("epoch"))?;
    let highest_epoch =
        Epoch::new(file.highest_epoch).map_err(|_| OfficeError::Invalid("highest_epoch"))?;
    let pending_restore_base = file
        .pending_restore_base_hex
        .as_deref()
        .map(decode_hex_digest)
        .transpose()?;
    let checkpoint = OrgCheckpoint {
        org_id: org_id.clone(),
        local_office: local_office.clone(),
        hq_office,
        ca,
        position: Position {
            epoch,
            seq: file.seq,
        },
        digest: decode_hex_digest(&file.digest_hex)?,
        highest_epoch,
        restore_pending: file.restore_pending,
        pending_restore_base,
        protocol_level: file.protocol_level,
    };
    let mut events = BTreeMap::new();
    let mut applied_org = Vec::with_capacity(stored_events.len());
    for ev in stored_events {
        let payload: Arc<[u8]> = Arc::from(decode_b64(&ev.payload_b64)?);
        let event = OrgEvent {
            org_id: org_id.clone(),
            position: Position {
                epoch: Epoch::new(ev.epoch).map_err(|_| OfficeError::Invalid("event epoch"))?,
                seq: ev.seq,
            },
            previous: decode_hex_digest(&ev.previous_hex)?,
            digest: decode_hex_digest(&ev.digest_hex)?,
            codec: Codec::new(&ev.codec_name, ev.codec_version)
                .map_err(|_| OfficeError::Invalid("event codec"))?,
            payload: Arc::clone(&payload),
        };
        // Every stored event still carries its own digest: a damaged log or state file fails
        // closed instead of serving altered history (`TASK-140`).
        let expected = org_sync_proto::event_digest_v1(
            &event.org_id,
            event.position,
            event.previous,
            &event.codec,
            &event.payload,
        )
        .map_err(|_| OfficeError::Invalid("stored event digest parameters"))?;
        if expected != event.digest {
            return Err(OfficeError::Invalid("stored event digest mismatch"));
        }
        events.insert((ev.epoch, ev.seq), event);
        applied_org.push(payload);
    }
    let work_outbox = outbox_from_stored(
        &local_office,
        &file.work_incarnation,
        file.work_last_seq,
        file.work_acked_seq,
        &file.work_pending,
        &org_id,
    )?;
    let mut message_queue = Queue::new(org_id, local_office);
    message_queue.clock_high_ms = file.message_clock_high_ms;
    for sm in &file.message_pending {
        message_queue.pending.push_back(message_from_stored(sm)?);
    }
    let mut outgoing_receipts = BTreeMap::new();
    for sr in &file.message_outgoing_receipts {
        let r = receipt_from_stored(sr)?;
        outgoing_receipts.insert(r.message.id.clone(), r);
    }
    let mut received_receipts = BTreeMap::new();
    let mut delivered_messages = Vec::new();
    for sr in &file.message_received_receipts {
        let r = receipt_from_stored(sr)?;
        if r.delivered {
            delivered_messages.push(Arc::clone(&r.message.payload));
        }
        received_receipts.insert((r.message.origin.clone(), r.message.id.clone()), r);
    }
    let mut expired_messages = BTreeMap::new();
    for sm in &file.message_expired {
        let m = message_from_stored(sm)?;
        expired_messages.insert((m.origin.clone(), m.id.clone()), m);
    }
    Ok(ReplicationCoreData {
        checkpoint,
        initial_snapshot_required: file.initial_snapshot_required,
        events,
        applied_org,
        staging: BTreeMap::new(),
        supported_protocol_max: 1,
        supported_codec_max: 1,
        work_outbox,
        remote_work_high_water: file.remote_work_high_water.clone(),
        message_queue,
        outgoing_receipts,
        received_receipts,
        delivered_messages,
        expired_messages,
        pending_relays: file.pending_relays.clone(),
        last_published_journal: file.last_published_journal.clone(),
        last_published_work_journal: file.last_published_work_journal.clone(),
        last_published_message_journal: file.last_published_message_journal.clone(),
        last_published_message_journal_by_dest: file.last_published_message_journal_by_dest.clone(),
        replicated_op_ids: file.replicated_op_ids.clone(),
    })
}
