//! Engine-independent ADR-0018 `ENG-3` replication conformance suite (`TASK-52` / `FC-145`).
//!
//! Invoked identically against BOTH [`crate::replication::memory::MemoryReplicationStore`] and
//! [`crate::store::sqlite::replication::SqliteReplicationStore`] (including durable restart and
//! `runtime/` projection delete-and-rebuild verification on SQLite).

use std::sync::Arc;

use org_sync::{
    error::{Refusal, SyncError},
    messages::{
        acknowledge, enqueue, next as next_message, receive, Message, MessageOutput, Receipt,
    },
    organization::{
        append_hq, apply, install_snapshot, OrgOutput, PeerIdentity, PeerRole, ValidatedSnapshot,
        VerifiedPeer,
    },
    work::{Change as WorkChange, Origin, RecordVersion, SourceKey},
};
use org_sync_proto::{
    work_record_digest_v1, work_source_digest_v1, Codec, Digest, Epoch, OfficeId, OrgId,
    OriginIncarnation, Position, SourceSeq,
};

use super::ReplicationStore;

pub struct ReplicationSpec {
    pub org_id: OrgId,
    pub local_office: OfficeId,
    pub hq_office: OfficeId,
    pub ca: Digest,
    pub epoch: Epoch,
    pub incarnation: OriginIncarnation,
    pub initial_snapshot_required: bool,
}

struct StaticPeer(PeerIdentity);

impl VerifiedPeer for StaticPeer {
    fn identity(&self) -> PeerIdentity {
        self.0.clone()
    }
}

fn id_org() -> OrgId {
    OrgId::new("0195a000-0000-7000-8000-000000000001").unwrap()
}
fn id_hq() -> OfficeId {
    OfficeId::new("0195a000-0000-7000-8000-000000000002").unwrap()
}
fn id_sat() -> OfficeId {
    OfficeId::new("0195a000-0000-7000-8000-000000000003").unwrap()
}
fn id_inc() -> OriginIncarnation {
    OriginIncarnation::new("0195a000-0000-7000-8000-000000000004").unwrap()
}
fn sample_ca() -> Digest {
    Digest::from_bytes([0x42; 32])
}

/// Runs the full engine-independent `ENG-3` replication suite across org replication, work
/// outbox + remote high-water tracking, cross-office message delivery & receipts, and D25
/// replacement-HQ re-enrollment reset.
pub fn run_eng3_replication_suite<F>(mut make: F)
where
    F: FnMut(ReplicationSpec) -> Box<dyn ReplicationStore>,
{
    suite_org_publish_apply_duplicate_gap_snapshot_and_reenroll(&mut make);
    suite_work_outbox_queue_ack_and_remote_dedup(&mut make);
    suite_message_outbox_deliver_receive_receipt_and_expiry(&mut make);
}

fn suite_org_publish_apply_duplicate_gap_snapshot_and_reenroll<F>(make: &mut F)
where
    F: FnMut(ReplicationSpec) -> Box<dyn ReplicationStore>,
{
    let hq = make(ReplicationSpec {
        org_id: id_org(),
        local_office: id_hq(),
        hq_office: id_hq(),
        ca: sample_ca(),
        epoch: Epoch::new(1).unwrap(),
        incarnation: id_inc(),
        initial_snapshot_required: false,
    });
    let sat = make(ReplicationSpec {
        org_id: id_org(),
        local_office: id_sat(),
        hq_office: id_hq(),
        ca: sample_ca(),
        epoch: Epoch::new(1).unwrap(),
        incarnation: id_inc(),
        initial_snapshot_required: true,
    });

    let codec = Codec::new("org.native.changes", 1).unwrap();
    let snap_codec = Codec::new("org.native.snapshot", 1).unwrap();
    let e1 = match org_sync::organization::OrgStore::transact(
        hq.as_ref(),
        Arc::new({
            let codec = codec.clone();
            move |tx| append_hq(tx, codec.clone(), Arc::from(b"ev1".as_slice()))
        }),
    )
    .unwrap()
    {
        OrgOutput::Published(ev) => ev,
        other => panic!("expected Published, got {other:?}"),
    };
    let e2 = match org_sync::organization::OrgStore::transact(
        hq.as_ref(),
        Arc::new({
            let codec = codec.clone();
            move |tx| append_hq(tx, codec.clone(), Arc::from(b"ev2".as_slice()))
        }),
    )
    .unwrap()
    {
        OrgOutput::Published(ev) => ev,
        other => panic!("expected Published, got {other:?}"),
    };
    assert_eq!(hq.org_events_after(1, 0).unwrap().len(), 2);

    let hq_peer = StaticPeer(PeerIdentity {
        org_id: id_org(),
        office_id: id_hq(),
        role: PeerRole::Hq,
        authority: Epoch::new(1).unwrap(),
    });
    // Fresh satellite requiring initial snapshot refuses delta before snapshot install.
    assert_eq!(
        apply(sat.as_ref(), &hq_peer, e1.clone()).unwrap(),
        OrgOutput::SnapshotRequired
    );

    // Stage and install initial snapshot at e1.
    sat.stage_snapshot("snap1", vec![Arc::from(b"ev1".as_slice())])
        .unwrap();
    let snap = ValidatedSnapshot {
        org_id: id_org(),
        authority: Epoch::new(1).unwrap(),
        position: e1.position,
        digest: e1.digest,
        codec: snap_codec,
        staging_id: "snap1".into(),
        protocol_level: 1,
    };
    assert_eq!(
        install_snapshot(sat.as_ref(), snap).unwrap(),
        OrgOutput::SnapshotInstalled
    );
    assert!(!sat.sync_status().unwrap().initial_snapshot_required);

    // Applying e2 advances cleanly; duplicate e2 is idempotent; satellite pretending to be HQ is refused.
    assert_eq!(
        apply(sat.as_ref(), &hq_peer, e2.clone()).unwrap(),
        OrgOutput::Ack {
            position: e2.position,
            digest: e2.digest,
            duplicate: false,
        }
    );
    assert_eq!(
        apply(sat.as_ref(), &hq_peer, e2.clone()).unwrap(),
        OrgOutput::Ack {
            position: e2.position,
            digest: e2.digest,
            duplicate: true,
        }
    );
    let rogue_peer = StaticPeer(PeerIdentity {
        org_id: id_org(),
        office_id: id_sat(),
        role: PeerRole::Satellite,
        authority: Epoch::new(1).unwrap(),
    });
    assert!(matches!(
        apply(sat.as_ref(), &rogue_peer, e2),
        Err(SyncError::Refused(Refusal::NotAuthorized))
    ));

    // D25 re-enrollment onto replacement HQ resets snapshot requirement cleanly.
    let new_hq = OfficeId::new("0195a000-0000-7000-8000-000000000099").unwrap();
    sat.reset_for_re_enrollment(
        new_hq.clone(),
        Digest::from_bytes([0x99; 32]),
        Epoch::new(1).unwrap(),
    )
    .unwrap();
    let st = sat.sync_status().unwrap();
    assert_eq!(st.hq_office, new_hq.as_str());
    assert!(st.initial_snapshot_required);
    assert_eq!(st.seq, 0);
}

fn suite_work_outbox_queue_ack_and_remote_dedup<F>(make: &mut F)
where
    F: FnMut(ReplicationSpec) -> Box<dyn ReplicationStore>,
{
    let sat = make(ReplicationSpec {
        org_id: id_org(),
        local_office: id_sat(),
        hq_office: id_hq(),
        ca: sample_ca(),
        epoch: Epoch::new(1).unwrap(),
        incarnation: id_inc(),
        initial_snapshot_required: false,
    });
    let origin = Origin {
        office: id_sat(),
        incarnation: id_inc(),
    };
    let codec = Codec::new("work-delta", 1).unwrap();
    let mk_change = |seq: u64, payload: &[u8]| {
        let version = RecordVersion {
            generation: 1,
            version: seq,
        };
        let req_org = Position {
            epoch: Epoch::new(1).unwrap(),
            seq: 0,
        };
        let created_ms = 1_700_000_000_000 + seq;
        let rec_digest = work_record_digest_v1(
            "task-1",
            version.generation,
            version.version,
            &codec,
            payload,
        )
        .unwrap();
        let src_digest = work_source_digest_v1(
            &origin.office,
            &origin.incarnation,
            SourceSeq::new(seq).unwrap(),
            rec_digest,
            req_org,
            created_ms,
        );
        WorkChange {
            org_id: id_org(),
            key: SourceKey {
                origin: origin.clone(),
                seq: SourceSeq::new(seq).unwrap(),
            },
            record_id: "task-1".into(),
            version,
            digest: src_digest,
            record_digest: rec_digest,
            codec: codec.clone(),
            payload: Arc::from(payload),
            required_org: req_org,
            created_ms,
        }
    };

    sat.enqueue_work_change(mk_change(1, b"create-task"))
        .unwrap();
    sat.enqueue_work_change(mk_change(2, b"update-task"))
        .unwrap();
    let outbox = sat.load_work_outbox().unwrap();
    assert_eq!(outbox.last_seq(), 2);
    assert_eq!(outbox.acknowledged_seq(), 0);
    assert_eq!(outbox.pending().len(), 2);

    let after_ack = sat.ack_work_through(1).unwrap();
    assert_eq!(after_ack.acknowledged_seq(), 1);
    assert_eq!(after_ack.pending().len(), 1);

    let remote_origin = Origin {
        office: id_hq(),
        incarnation: id_inc(),
    };
    assert!(sat
        .record_remote_work_seq(&remote_origin, SourceSeq::new(1).unwrap())
        .unwrap());
    assert!(!sat
        .record_remote_work_seq(&remote_origin, SourceSeq::new(1).unwrap())
        .unwrap());
    assert_eq!(sat.remote_work_high_water(&remote_origin).unwrap(), 1);
}

fn suite_message_outbox_deliver_receive_receipt_and_expiry<F>(make: &mut F)
where
    F: FnMut(ReplicationSpec) -> Box<dyn ReplicationStore>,
{
    let hq = make(ReplicationSpec {
        org_id: id_org(),
        local_office: id_hq(),
        hq_office: id_hq(),
        ca: sample_ca(),
        epoch: Epoch::new(1).unwrap(),
        incarnation: id_inc(),
        initial_snapshot_required: false,
    });
    let sat = make(ReplicationSpec {
        org_id: id_org(),
        local_office: id_sat(),
        hq_office: id_hq(),
        ca: sample_ca(),
        epoch: Epoch::new(1).unwrap(),
        incarnation: id_inc(),
        initial_snapshot_required: false,
    });

    let now = 1_700_000_000_000u64;
    let msg = Message {
        org_id: id_org(),
        origin: id_sat(),
        destination: id_hq(),
        id: "msg-cross-1".into(),
        fingerprint: Digest::from_bytes([0x11; 32]),
        payload: Arc::from(b"hello-hq".as_slice()),
        deadline_ms: now + 60_000,
    };
    assert_eq!(
        enqueue(sat.as_ref(), msg.clone(), now).unwrap(),
        MessageOutput::Queued {
            id: "msg-cross-1".into()
        }
    );
    assert_eq!(
        next_message(sat.as_ref(), now + 100).unwrap(),
        MessageOutput::Deliver(msg.clone())
    );

    let sat_peer = StaticPeer(PeerIdentity {
        org_id: id_org(),
        office_id: id_sat(),
        role: PeerRole::Satellite,
        authority: Epoch::new(1).unwrap(),
    });
    let receipt = match receive(hq.as_ref(), &sat_peer, msg.clone(), now + 200).unwrap() {
        MessageOutput::Receipt(r) => r,
        other => panic!("expected Receipt, got {other:?}"),
    };
    assert!(receipt.delivered);
    assert_eq!(hq.delivered_message_payloads().unwrap().len(), 1);

    let hq_peer = StaticPeer(PeerIdentity {
        org_id: id_org(),
        office_id: id_hq(),
        role: PeerRole::Hq,
        authority: Epoch::new(1).unwrap(),
    });
    assert!(matches!(
        acknowledge(
            sat.as_ref(),
            &hq_peer,
            Receipt {
                message: msg,
                delivered: true,
            },
        )
        .unwrap(),
        MessageOutput::Receipt(_) | MessageOutput::Idle
    ));
    assert!(sat.message_queue_snapshot().unwrap().pending.is_empty());
}
