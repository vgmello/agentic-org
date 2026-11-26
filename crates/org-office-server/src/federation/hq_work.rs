//! HQ handling of satellite work changes: digest verification, validation, and acknowledgement.

use std::net::TcpStream;

use crate::federation_stream::HqPeerRegistry;
use crate::org_feed::FeedWake;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use org_office::OrgDeltaPayload;
use org_sync::work::Origin;
use org_sync_proto::{
    work_ack_proof_digest_v1, work_record_digest_v1, work_source_digest_v1, Codec, Digest, Epoch,
    OfficeId, OriginIncarnation, Position, SourceSeq, SyncFrame,
};
use rustls::{ServerConnection, StreamOwned};

use super::hq_peer::{write_sync_frame_if_peer_active, HqSessionPeer};
use super::{deterministic_uuid_v7, SnapshotFileBatch};

pub(super) struct WorkChangeInput {
    pub(super) org_id: String,
    pub(super) origin_office: String,
    pub(super) incarnation: String,
    pub(super) seq: u64,
    pub(super) record_id: String,
    pub(super) generation: u64,
    pub(super) version: u64,
    pub(super) digest_hex: String,
    pub(super) record_digest_hex: String,
    pub(super) codec_name: String,
    pub(super) codec_version: u32,
    pub(super) payload_b64: String,
    pub(super) required_org_epoch: u64,
    pub(super) required_org_seq: u64,
    pub(super) created_ms: u64,
}

fn verify_work_change_digests(
    sat_id: &OfficeId,
    inc: &OriginIncarnation,
    src_seq: SourceSeq,
    input: &WorkChangeInput,
    payload: &[u8],
) -> bool {
    let Ok(expected_digest) = Digest::parse_hex(&input.digest_hex) else {
        return false;
    };
    let Ok(expected_rec_digest) = Digest::parse_hex(&input.record_digest_hex) else {
        return false;
    };
    let Ok(codec) = Codec::new(&input.codec_name, input.codec_version) else {
        return false;
    };
    let Ok(req_epoch) = Epoch::new(input.required_org_epoch.max(1)) else {
        return false;
    };
    let Ok(actual_rec) = work_record_digest_v1(
        &input.record_id,
        input.generation,
        input.version,
        &codec,
        payload,
    ) else {
        return false;
    };
    if actual_rec != expected_rec_digest {
        return false;
    }
    let req_org = Position {
        epoch: req_epoch,
        seq: input.required_org_seq,
    };
    let actual_src =
        work_source_digest_v1(sat_id, inc, src_seq, actual_rec, req_org, input.created_ms);
    actual_src == expected_digest
}

struct ValidatedWorkChange {
    inc: OriginIncarnation,
    src_seq: SourceSeq,
    files: SnapshotFileBatch,
}

enum WorkChangeValidation {
    Valid(ValidatedWorkChange),
    ReplyError(&'static str, &'static str),
    CloseSession,
}

fn validate_hq_work_change(
    peer: &HqSessionPeer<'_>,
    input: &WorkChangeInput,
) -> WorkChangeValidation {
    if input.origin_office != peer.sat_id.as_str() || input.org_id != peer.org_id.as_str() {
        return WorkChangeValidation::ReplyError(
            "not_authorized",
            "work change origin does not match authenticated satellite",
        );
    }
    let (Ok(inc), Ok(src_seq), Ok(payload)) = (
        OriginIncarnation::new(&input.incarnation),
        SourceSeq::new(input.seq),
        STANDARD.decode(&input.payload_b64),
    ) else {
        return WorkChangeValidation::CloseSession;
    };
    if payload.len() > org_sync::organization::MAX_PAYLOAD_BYTES {
        return WorkChangeValidation::ReplyError(
            "invalid_request",
            "work change payload exceeds org-sync event limit",
        );
    }
    if !verify_work_change_digests(peer.sat_id, &inc, src_seq, input, &payload) {
        return WorkChangeValidation::ReplyError(
            "invalid_request",
            "work change record/source digest verification failed",
        );
    }
    let Ok(cp) = peer.office.replication().org_checkpoint() else {
        return WorkChangeValidation::CloseSession;
    };
    if input.required_org_epoch > cp.position.epoch.get()
        || (input.required_org_epoch == cp.position.epoch.get()
            && input.required_org_seq > cp.position.seq)
    {
        return WorkChangeValidation::ReplyError(
            "conflict",
            "work change depends on unreached HQ organization position",
        );
    }
    let Ok(delta) = serde_json::from_slice::<OrgDeltaPayload>(&payload) else {
        return WorkChangeValidation::ReplyError(
            "invalid_request",
            "work change payload must be a valid OrgDeltaPayload",
        );
    };
    let Ok(files) = delta.decode_files() else {
        return WorkChangeValidation::ReplyError(
            "invalid_request",
            "work change delta contains invalid paths or base64",
        );
    };
    WorkChangeValidation::Valid(ValidatedWorkChange {
        inc,
        src_seq,
        files,
    })
}

pub(super) fn handle_hq_work_change(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    peer: &HqSessionPeer<'_>,
    input: WorkChangeInput,
    peers: &HqPeerRegistry,
    feed_wake: Option<&FeedWake>,
) -> bool {
    let ValidatedWorkChange {
        inc,
        src_seq,
        files,
    } = match validate_hq_work_change(peer, &input) {
        WorkChangeValidation::Valid(v) => v,
        WorkChangeValidation::ReplyError(code, message) => {
            return write_sync_frame_if_peer_active(
                tls,
                peer,
                &SyncFrame::Error {
                    code: code.into(),
                    message: message.into(),
                },
            );
        }
        WorkChangeValidation::CloseSession => return false,
    };
    let origin = Origin {
        office: peer.sat_id.clone(),
        incarnation: inc.clone(),
    };
    let op_id = deterministic_uuid_v7(&format!(
        "work:{}:{}:{}:{}",
        peer.org_id.as_str(),
        peer.sat_id.as_str(),
        inc.as_str(),
        input.seq
    ));
    let ingest = match org_office::run_if_peer_active(
        peer.office,
        peer.sat_id.as_str(),
        Some(peer.cert_serial_hex),
        || {
            peer.office.ingest_validated_satellite_work(
                &origin,
                src_seq,
                &input.record_id,
                &op_id,
                files,
            )
        },
    ) {
        Ok(Some(res)) => res,
        _ => return false,
    };
    let prev_hw = match ingest {
        Ok(hw) => hw,
        Err(err) => {
            return write_sync_frame_if_peer_active(
                tls,
                peer,
                &SyncFrame::Error {
                    code: "not_authorized".into(),
                    message: err.to_string(),
                },
            );
        }
    };
    let prev = prev_hw.min(input.seq.saturating_sub(1));
    let ack = org_sync_proto::WorkAck {
        origin_office_id: peer.sat_id.clone(),
        incarnation: inc,
        from_exclusive: prev,
        through_seq: input.seq,
        rejected: Vec::new(),
    };
    let Ok(proof) = work_ack_proof_digest_v1(&ack) else {
        return false;
    };
    // Wake all connected satellites and local HQ SSE subscribers immediately so the newly
    // ingested work delta pushes out without waiting for an idle poll (`TASK-140`).
    peers.notify_all();
    if let Some(wake) = feed_wake {
        wake.notify();
    }
    write_sync_frame_if_peer_active(
        tls,
        peer,
        &SyncFrame::WorkAck {
            origin_office: input.origin_office,
            incarnation: input.incarnation,
            seq: input.seq,
            proof_hex: proof.to_hex(),
        },
    )
}
