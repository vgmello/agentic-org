//! An upload HQ holds in memory ends on its own (`TASK-98.6`): quiet for its idle limit, or
//! running past [`UPLOAD_TOTAL_MAX`] however steadily it trickles in, and its room in the
//! budget comes back either way. One satellite holds at most one upload at a time.

use std::sync::Arc;
use std::time::{Duration, Instant};

use org_office::attachments::blobs::ContentHash;
use org_office::attachments::records::{self, AttachmentRecord};
use org_office::{Office, TestOffice};
use org_proto::attachments::AttachmentTargetKind;
use org_store::Id;
use org_sync_proto::BlobAssembler;

use super::{BudgetHold, HqBlobUpload, MAX_HQ_BLOB_BYTES_IN_FLIGHT, UPLOAD_IDLE, UPLOAD_TOTAL_MAX};
use crate::federation_stream::HqPeerRegistry;

const SAT_A: &str = "0195a000-0000-7000-8000-00000000b10a";
const SAT_B: &str = "0195a000-0000-7000-8000-00000000b10b";
const BYTES: &[u8] = b"held in HQ's memory";

/// An upload of satellite `sat` that started `started_ago` and last heard `quiet_for` ago.
fn upload(
    office: &Office,
    peers: &Arc<HqPeerRegistry>,
    sat: &str,
    started_ago: Duration,
    quiet_for: Duration,
) -> HqBlobUpload {
    let record = AttachmentRecord {
        id: Id::parse(&office.next_uuid_v7()).unwrap(),
        target_kind: AttachmentTargetKind::Organization,
        target_id: None,
        content_sha256: ContentHash::of(BYTES),
        mime_type: "text/plain".into(),
        byte_count: BYTES.len() as u64,
        filename: "held.txt".into(),
        title: "Held".into(),
        description: None,
        artifact_status: Default::default(),
        width: None,
        height: None,
        page_count: None,
        uploaded_by_employee_id: Id::parse(office.owner_member_id()).unwrap(),
        created_at: "2026-11-26T10:00:00.000Z".into(),
        updated_at: "2026-11-26T10:00:00.000Z".into(),
    };
    let shipped = office
        .attachment_ingest_check(
            sat,
            record.id.as_str(),
            record.content_sha256.as_str(),
            record.byte_count,
            &records::encode(&record),
        )
        .unwrap()
        .unwrap();
    assert!(peers.try_reserve_blob_bytes(sat, record.byte_count, MAX_HQ_BLOB_BYTES_IN_FLIGHT));
    let now = Instant::now();
    let ago = |d: Duration| now.checked_sub(d).expect("the clock has run long enough");
    HqBlobUpload {
        request_id: "upload".into(),
        assembler: BlobAssembler::new(
            record.content_sha256.as_str(),
            record.byte_count,
            shipped.max_bytes(),
        )
        .unwrap(),
        shipped,
        started_at: ago(started_ago),
        last_chunk: ago(quiet_for),
        idle: UPLOAD_IDLE,
        _hold: BudgetHold {
            peers: Arc::clone(peers),
            sat_id: sat.to_owned(),
        },
    }
}

#[test]
fn a_quiet_or_endless_upload_expires_and_returns_its_budget() {
    let office = Office::for_test(TestOffice::default());
    let peers = Arc::new(HqPeerRegistry::default());
    let held = BYTES.len() as u64;

    let mut fresh = Some(upload(
        &office,
        &peers,
        SAT_A,
        Duration::ZERO,
        Duration::ZERO,
    ));
    HqBlobUpload::expire_if_idle(&mut fresh);
    assert!(fresh.is_some(), "a live upload stays");
    assert_eq!(peers.blob_bytes_in_flight(), held);
    drop(fresh);
    assert_eq!(peers.blob_bytes_in_flight(), 0);

    // No chunk for the idle limit: dropped, room returned.
    let mut quiet = Some(upload(
        &office,
        &peers,
        SAT_A,
        UPLOAD_IDLE,
        UPLOAD_IDLE + Duration::from_secs(1),
    ));
    assert_eq!(peers.blob_bytes_in_flight(), held);
    HqBlobUpload::expire_if_idle(&mut quiet);
    assert!(quiet.is_none());
    assert_eq!(peers.blob_bytes_in_flight(), 0);

    // A chunk a moment ago, but running past the total limit: a trickle is dropped too.
    let mut trickle = Some(upload(
        &office,
        &peers,
        SAT_A,
        UPLOAD_TOTAL_MAX + Duration::from_secs(1),
        Duration::from_secs(1),
    ));
    HqBlobUpload::expire_if_idle(&mut trickle);
    assert!(trickle.is_none());
    assert_eq!(peers.blob_bytes_in_flight(), 0);
    let mut empty: Option<HqBlobUpload> = None;
    HqBlobUpload::expire_if_idle(&mut empty);
    assert!(empty.is_none());
}

#[test]
fn one_satellite_holds_at_most_one_upload_at_a_time_within_the_server_budget() {
    let office = Office::for_test(TestOffice::default());
    let peers = Arc::new(HqPeerRegistry::default());
    let first = upload(&office, &peers, SAT_A, Duration::ZERO, Duration::ZERO);
    // A second upload of the same satellite (on any session) waits for the first.
    assert!(!peers.try_reserve_blob_bytes(SAT_A, 1, MAX_HQ_BLOB_BYTES_IN_FLIGHT));
    // Another satellite still gets room, but never past the server's total.
    assert!(!peers.try_reserve_blob_bytes(
        SAT_B,
        MAX_HQ_BLOB_BYTES_IN_FLIGHT,
        MAX_HQ_BLOB_BYTES_IN_FLIGHT
    ));
    let second = upload(&office, &peers, SAT_B, Duration::ZERO, Duration::ZERO);
    assert_eq!(peers.blob_bytes_in_flight(), 2 * BYTES.len() as u64);
    drop(first);
    assert_eq!(peers.blob_bytes_in_flight(), BYTES.len() as u64);
    assert!(peers.try_reserve_blob_bytes(SAT_A, 1, MAX_HQ_BLOB_BYTES_IN_FLIGHT));
    peers.release_blob_bytes(SAT_A);
    drop(second);
    assert_eq!(peers.blob_bytes_in_flight(), 0);
    // A release for a satellite holding nothing changes nothing.
    peers.release_blob_bytes(SAT_B);
    assert_eq!(peers.blob_bytes_in_flight(), 0);
}
