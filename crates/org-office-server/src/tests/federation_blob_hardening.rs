//! Shared attachment bytes across offices, the failure modes (`TASK-98.6` pass 2), over real
//! Organization-CA mTLS against a real HQ and enrolled satellites:
//!
//! - an upload a satellite announces and then abandons gives its room in HQ's budget back on
//!   its own, and one satellite never holds two uploads at once;
//! - two satellite files of the same bytes both upload, are kept once at HQ, and drain;
//! - a snapshot from HQ never deletes a satellite's un-shipped file, which still ships;
//! - a retried `attachment.attach` enqueues nothing more, and the outbox drains to empty;
//! - the shipper stops within a read slice, and a missing or damaged file never holds up the
//!   others;
//! - a read the caller may not make never asks HQ for anything.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use org_office::attachments::blobs::ContentHash;
use org_office::attachments::records::{self, AttachmentRecord};
use org_office::attachments::remote::{BlobPull, BlobRemote, PullFailure};
use org_office::attachments::testkit::sample_png;
use org_office::Office;
use org_proto::attachments::{
    AttachmentAttachParams, AttachmentReadChunkParams, AttachmentTargetKind,
    AttachmentUploadBeginParams, AttachmentUploadChunkParams, AttachmentUploadRef, AttachmentView,
};
use org_proto::{ApiError, ErrorCode};
use org_store::Id;
use org_sync_proto::SyncFrame;

use super::a2a_messaging::wait_until;
use super::federation_blobs::{exchange, files_under, inspect, publish, read_rpc, Net};
use crate::federation::SatelliteSyncSession;
use crate::federation_stream::SatSyncWake;
use org_federation::testkit::{flush_blob_outbox, read_blob_reply, ship_blob_outbox};

/// The record satellite `sat` would ship for an organization-wide file of `owner`'s.
fn org_record(sat: &Office, owner: &str, filename: &str, bytes: &[u8]) -> AttachmentRecord {
    AttachmentRecord {
        id: Id::parse(&sat.next_uuid_v7()).unwrap(),
        target_kind: AttachmentTargetKind::Organization,
        target_id: None,
        content_sha256: ContentHash::of(bytes),
        mime_type: "text/plain".into(),
        byte_count: bytes.len() as u64,
        filename: filename.into(),
        title: "Shipped".into(),
        description: None,
        artifact_status: Default::default(),
        width: None,
        height: None,
        page_count: None,
        uploaded_by_employee_id: Id::parse(owner).unwrap(),
        created_at: "2026-11-26T10:00:00.000Z".into(),
        updated_at: "2026-11-26T10:00:00.000Z".into(),
    }
}

fn announce(org_id: &str, request_id: &str, record: &AttachmentRecord) -> SyncFrame {
    SyncFrame::BlobHasRequest {
        request_id: request_id.into(),
        org_id: org_id.into(),
        attachment_id: record.id.to_string(),
        content_sha256: record.content_sha256.to_string(),
        byte_count: record.byte_count,
        record_b64: STANDARD.encode(records::encode(record)),
    }
}

fn whole_chunk(request_id: &str, record: &AttachmentRecord, bytes: &[u8]) -> SyncFrame {
    SyncFrame::BlobChunk {
        request_id: request_id.into(),
        content_sha256: record.content_sha256.to_string(),
        offset: 0,
        total_bytes: record.byte_count,
        data_b64: STANDARD.encode(bytes),
    }
}

fn refused_with(reply: &SyncFrame, code: &str) -> bool {
    matches!(reply, SyncFrame::BlobAck { accepted: false, code: Some(c), .. } if c == code)
}

#[test]
fn an_announced_upload_that_goes_quiet_gives_its_budget_back_without_another_frame() {
    let net = Net::new(1);
    let sat = Arc::clone(net.sat(0));
    let org_id = net.hq.info().org_id.clone();
    let peers = Arc::clone(net.server.as_ref().unwrap().peers());
    peers.set_blob_upload_idle(Duration::from_millis(800));
    let bytes = b"announced, then never sent".to_vec();
    let record = org_record(&sat, &net.owner, "quiet.txt", &bytes);

    let mut first = SatelliteSyncSession::connect(Arc::clone(&sat)).unwrap();
    let reply = exchange(&mut first, &announce(&org_id, "quiet", &record), "quiet");
    assert!(
        matches!(reply, SyncFrame::BlobHasResponse { present: false, .. }),
        "{reply:?}"
    );
    assert_eq!(peers.blob_bytes_in_flight(), bytes.len() as u64);
    // The same satellite on a second session may not start another upload meanwhile.
    let mut second = SatelliteSyncSession::connect(Arc::clone(&sat)).unwrap();
    let reply = exchange(&mut second, &announce(&org_id, "second", &record), "second");
    assert!(refused_with(&reply, "unavailable"), "{reply:?}");

    // Nothing more arrives on the first session: its loop drops the upload by itself.
    wait_until(
        Duration::from_secs(10),
        "the quiet upload gives its budget back",
        || peers.blob_bytes_in_flight() == 0,
    );
    // The satellite then uploads the file whole on the other session.
    let reply = exchange(&mut second, &announce(&org_id, "again", &record), "again");
    assert!(
        matches!(reply, SyncFrame::BlobHasResponse { present: false, .. }),
        "{reply:?}"
    );
    let reply = exchange(&mut second, &whole_chunk("again", &record, &bytes), "again");
    assert!(
        matches!(reply, SyncFrame::BlobAck { accepted: true, .. }),
        "{reply:?}"
    );
    assert_eq!(peers.blob_bytes_in_flight(), 0);
    assert_eq!(
        read_rpc(&net.hq, &net.owner, record.id.as_str()).unwrap(),
        bytes
    );
    drop(first);
}

#[test]
fn two_satellite_files_of_the_same_bytes_both_upload_are_kept_once_and_drain() {
    let net = Net::new(1);
    let sat = net.sat(0);
    let owner = &net.owner;
    let bytes = sample_png(24, 16);
    let a = publish(sat, owner, "a.png", "image/png", &bytes);
    let b = publish(sat, owner, "b.png", "image/png", &bytes);
    assert_eq!(a.content_sha256, b.content_sha256);
    assert_ne!(a.id, b.id);
    assert_eq!(sat.attachment_outbox_len().unwrap(), 2);
    let outbox_blobs = sat.attachment_outbox_root().join("sha256");
    assert_eq!(files_under(&outbox_blobs), 1, "one copy waits");
    let before = net.hq.attachment_blob_counts().unwrap();

    let flush = ship_blob_outbox(sat, None, 1 << 30).unwrap();
    assert_eq!((flush.shipped, flush.refused), (2, 0));
    assert_eq!(
        net.hq.attachment_blob_counts().unwrap(),
        (before.0 + 1, before.1),
        "HQ keeps the bytes once"
    );
    assert_eq!(sat.attachment_outbox_len().unwrap(), 0);
    assert_eq!(files_under(&sat.attachment_outbox_root()), 0);
    for view in [&a, &b] {
        assert_eq!(
            inspect(&net.hq, owner, &view.id).unwrap().content_sha256,
            view.content_sha256
        );
        assert_eq!(
            read_rpc(&net.hq, owner, &view.id).unwrap().len(),
            bytes_len(view)
        );
    }
}

fn bytes_len(view: &AttachmentView) -> usize {
    usize::try_from(view.byte_count).unwrap()
}

#[test]
fn a_snapshot_from_hq_keeps_an_unshipped_satellite_file_which_the_shipper_then_delivers() {
    let net = Net::new(1);
    let sat = Arc::clone(net.sat(0));
    let owner = net.owner.clone();
    let bytes = b"written on the satellite while HQ was away".to_vec();
    let file = publish(&sat, &owner, "offline.txt", "text/plain", &bytes);
    let path = format!("state/files/{}.yaml", file.id);
    assert!(sat
        .pending_blob_outbox_state_paths()
        .unwrap()
        .contains(&path));
    assert!(net.hq.pending_blob_outbox_state_paths().unwrap().is_empty());

    // HQ's state is authoritative: a snapshot reconciles the satellite's state to it, and HQ
    // has never heard of this file.
    let mut session = SatelliteSyncSession::connect(Arc::clone(&sat)).unwrap();
    session.sync_initial_snapshot(&sat.next_uuid_v7()).unwrap();
    drop(session);
    assert_eq!(
        inspect(&sat, &owner, &file.id).unwrap().content_sha256,
        file.content_sha256,
        "the un-shipped record survives the snapshot"
    );
    assert_eq!(read_rpc(&sat, &owner, &file.id).unwrap(), bytes);
    assert_eq!(sat.attachment_outbox_pending().unwrap().len(), 1);

    // The shipper thread delivers it afterwards.
    let stop = Arc::new(AtomicBool::new(false));
    let wake = Arc::new(SatSyncWake::default());
    let shipper = crate::federation::spawn_satellite_blob_shipper(&sat, &stop, Arc::clone(&wake))
        .expect("a satellite runs a shipper");
    wait_until(Duration::from_secs(20), "the outbox drains", || {
        sat.attachment_outbox_len().unwrap() == 0
    });
    stop.store(true, Ordering::SeqCst);
    wake.notify();
    shipper.join().unwrap();
    assert_eq!(read_rpc(&net.hq, &owner, &file.id).unwrap(), bytes);
    assert!(sat.pending_blob_outbox_state_paths().unwrap().is_empty());
}

/// Uploads and commits `bytes` on `office`; the upload's ID.
fn stage(office: &Office, actor: &str, filename: &str, bytes: &[u8]) -> String {
    let begun = office
        .attachment_upload_begin(
            actor,
            AttachmentUploadBeginParams {
                filename: filename.into(),
                mime_type: "text/plain".into(),
                byte_count: bytes.len() as u64,
            },
        )
        .unwrap()
        .unwrap();
    let upload_id = begun.upload.upload_id;
    office
        .attachment_upload_chunk(
            actor,
            AttachmentUploadChunkParams {
                upload_id: upload_id.clone(),
                offset: 0,
                data_base64: STANDARD.encode(bytes),
            },
        )
        .unwrap()
        .unwrap();
    office
        .attachment_upload_commit(
            actor,
            AttachmentUploadRef {
                upload_id: upload_id.clone(),
            },
        )
        .unwrap()
        .unwrap();
    upload_id
}

fn attach(
    office: &Office,
    actor: &str,
    command_id: &str,
    upload_id: Option<&str>,
    source: Option<&str>,
) -> AttachmentView {
    office
        .attachment_attach(
            actor,
            AttachmentAttachParams {
                command_id: command_id.into(),
                upload_id: upload_id.map(str::to_owned),
                source_attachment_id: source.map(str::to_owned),
                target_kind: AttachmentTargetKind::Organization,
                target_id: None,
                title: None,
                description: None,
                artifact_status: None,
            },
        )
        .unwrap()
        .unwrap()
        .attachment
}

/// Every file of the flat folder `dir`, with its mode, to put back later.
fn saved_files(dir: &Path) -> Vec<(PathBuf, Vec<u8>, u32)> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|item| {
            let path = item.unwrap().path();
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            (path.clone(), std::fs::read(&path).unwrap(), mode)
        })
        .collect()
}

fn put_back(dir: &Path, mode: u32, files: &[(PathBuf, Vec<u8>, u32)]) {
    std::fs::create_dir(dir).unwrap();
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode)).unwrap();
    for (path, bytes, mode) in files {
        std::fs::write(path, bytes).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(*mode)).unwrap();
    }
}

#[test]
fn a_retried_attach_on_a_satellite_enqueues_nothing_more_and_the_outbox_drains_to_empty() {
    let net = Net::new(1);
    let sat = Arc::clone(net.sat(0));
    let owner = net.owner.clone();
    let bytes = b"one file, bound by retried commands".to_vec();
    let upload_id = stage(&sat, &owner, "retry.txt", &bytes);
    let staged = sat.attachment_staging_root().join(&upload_id);
    let staged_mode = std::fs::metadata(&staged).unwrap().permissions().mode();
    let staged_files = saved_files(&staged);

    let command = sat.next_uuid_v7();
    let first = attach(&sat, &owner, &command, Some(&upload_id), None);
    assert_eq!(sat.attachment_outbox_len().unwrap(), 1);
    assert!(!staged.exists());
    // A crash after the binding committed and before the staged upload went: the command is
    // retried with the upload still there. The first answer comes back; nothing is enqueued.
    put_back(&staged, staged_mode, &staged_files);
    let replay = attach(&sat, &owner, &command, Some(&upload_id), None);
    assert_eq!(replay.id, first.id);
    assert_eq!(sat.attachment_outbox_len().unwrap(), 1);
    assert!(!staged.exists(), "the replay removes the left-over upload");

    // A copy bound from the file itself, its command retried twice: one more entry only.
    let command = sat.next_uuid_v7();
    let copy = attach(&sat, &owner, &command, None, Some(&first.id));
    assert_ne!(copy.id, first.id);
    for _ in 0..2 {
        assert_eq!(
            attach(&sat, &owner, &command, None, Some(&first.id)).id,
            copy.id
        );
    }
    assert_eq!(sat.attachment_outbox_len().unwrap(), 2);

    // Both ship, and the outbox ends empty: no entry and no bytes left behind.
    let flush = ship_blob_outbox(&sat, None, 1 << 30).unwrap();
    assert_eq!((flush.shipped, flush.refused), (2, 0));
    assert_eq!(sat.attachment_outbox_len().unwrap(), 0);
    assert_eq!(files_under(&sat.attachment_outbox_root()), 0);
    for view in [&first, &copy] {
        assert_eq!(read_rpc(&net.hq, &owner, &view.id).unwrap(), bytes);
    }
}

#[test]
fn the_shipper_stops_within_a_read_slice_and_ships_nothing_once_stopped() {
    let net = Net::new(1);
    let sat = Arc::clone(net.sat(0));
    let mut session = SatelliteSyncSession::connect(Arc::clone(&sat)).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let stopper = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        flag.store(true, Ordering::SeqCst);
    });
    // HQ never answers a request it was never sent: without the stop this waits 60 s.
    let started = Instant::now();
    assert!(read_blob_reply(&mut session, "never-sent", Some(&*stop)).is_err());
    let waited = started.elapsed();
    stopper.join().unwrap();
    assert!(waited < Duration::from_secs(3), "{waited:?}");

    // Stopped before it begins, a pass ships nothing and keeps every file.
    publish(&sat, &net.owner, "kept.txt", "text/plain", b"still waiting");
    assert!(flush_blob_outbox(&mut session, 1 << 30, Some(&*stop)).is_err());
    assert_eq!(sat.attachment_outbox_len().unwrap(), 1);
    assert!(ship_blob_outbox(&sat, Some(&*stop), 1 << 30).is_err());
    assert_eq!(sat.attachment_outbox_len().unwrap(), 1);
}

/// The outbox copy of the blob `sha`.
fn outbox_blob(sat: &Office, sha: &str) -> PathBuf {
    sat.attachment_outbox_root()
        .join("sha256")
        .join(&sha[0..2])
        .join(&sha[2..4])
        .join(sha)
}

#[test]
fn a_missing_or_damaged_outbox_file_never_holds_up_the_files_after_it() {
    let net = Net::new(1);
    let sat = Arc::clone(net.sat(0));
    let owner = net.owner.clone();
    let missing = publish(
        &sat,
        &owner,
        "1.txt",
        "text/plain",
        b"the oldest, lost on disk",
    );
    let damaged = publish(
        &sat,
        &owner,
        "2.txt",
        "text/plain",
        b"the next, damaged on disk",
    );
    let intact = publish(&sat, &owner, "3.txt", "text/plain", b"the newest, intact");
    std::fs::remove_file(outbox_blob(&sat, &missing.content_sha256)).unwrap();
    let path = outbox_blob(&sat, &damaged.content_sha256);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::write(&path, b"the next, DAMAGED on disk").unwrap();

    let flush = ship_blob_outbox(&sat, None, 1 << 30).unwrap();
    assert_eq!((flush.shipped, flush.refused), (1, 2));
    assert_eq!(
        read_rpc(&net.hq, &owner, &intact.id).unwrap(),
        b"the newest, intact"
    );
    // The two stay in the outbox, never shipped and never dropped.
    let waiting: Vec<String> = sat
        .attachment_outbox_pending()
        .unwrap()
        .into_iter()
        .map(|s| s.attachment_id)
        .collect();
    assert_eq!(waiting, [missing.id.clone(), damaged.id.clone()]);
    for view in [&missing, &damaged] {
        assert_eq!(
            inspect(&net.hq, &owner, &view.id).unwrap_err().code,
            ErrorCode::NotFound
        );
    }
}

/// A [`BlobRemote`] that counts every fetch and serves none.
#[derive(Default)]
struct CountingRemote(AtomicUsize);

impl BlobRemote for CountingRemote {
    fn fetch(&self, _: &str, _: &ContentHash, _: u64) -> Result<Vec<u8>, PullFailure> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(PullFailure::Unreachable("counted".into()))
    }
}

#[test]
fn a_read_the_caller_may_not_make_never_asks_hq_for_anything() {
    let net = Net::new(1);
    let sat = Arc::clone(net.sat(0));
    let owner = net.owner.clone();
    let shared = publish(
        &net.hq,
        &owner,
        "hq.txt",
        "text/plain",
        b"only at HQ so far",
    );
    super::federation_blobs::pull_org_events(&sat);
    let remote = CountingRemote::default();
    let pull = BlobPull {
        remote: &remote,
        cache_max_bytes: 1 << 20,
    };
    let read = |actor: &str, id: &str, offset: u64, max: Option<u32>| -> ApiError {
        sat.attachment_read_chunk_with(
            actor,
            AttachmentReadChunkParams {
                attachment_id: id.into(),
                offset,
                max_bytes: max,
            },
            Some(pull),
        )
        .unwrap()
        .unwrap_err()
    };
    let stranger = sat.next_uuid_v7();
    for (actor, id, offset, max, code) in [
        // Not a member of the organization.
        (
            stranger.as_str(),
            shared.id.as_str(),
            0,
            None,
            ErrorCode::NotAuthorized,
        ),
        // No such file here (an unknown ID, a digest).
        (
            owner.as_str(),
            stranger.as_str(),
            0,
            None,
            ErrorCode::NotFound,
        ),
        (
            owner.as_str(),
            shared.content_sha256.as_str(),
            0,
            None,
            ErrorCode::NotFound,
        ),
        // A malformed read.
        (
            owner.as_str(),
            shared.id.as_str(),
            shared.byte_count + 1,
            None,
            ErrorCode::InvalidRequest,
        ),
        (
            owner.as_str(),
            shared.id.as_str(),
            0,
            Some(0),
            ErrorCode::InvalidRequest,
        ),
    ] {
        assert_eq!(read(actor, id, offset, max).code, code, "{actor} {id}");
    }
    assert_eq!(remote.0.load(Ordering::SeqCst), 0, "HQ was never asked");
    // A read the owner may make asks HQ, once.
    let unavailable = read(&owner, &shared.id, 0, None);
    assert_eq!(unavailable.code, ErrorCode::Unavailable);
    assert_eq!(remote.0.load(Ordering::SeqCst), 1);
}
