//! Shared attachment bytes across offices over real Organization-CA mTLS (`TASK-98.6`,
//! ADR-0037), against a real HQ and enrolled satellites:
//!
//! - a satellite's own image and multi-chunk PDF wait in its outbox (readable offline at once),
//!   ship to HQ, verify, are acknowledged, and leave the outbox;
//! - satellite A's file reaches satellite B through HQ, pulled on demand by
//!   `attachment.read_chunk` and served from B's cache once HQ is gone;
//! - the cache evicts the least recently used pulled file under its cap while an un-acked
//!   outbox file is never lost, and a read HQ cannot serve is a retryable `unavailable`;
//! - a satellite can never get a private file's bytes, by attachment ID or by digest, and the
//!   state snapshot it installs holds no bytes.

use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use org_office::attachments::blobs::ContentHash;
use org_office::attachments::records::{self, AttachmentRecord};
use org_office::attachments::remote::{BlobPull, PullFailure};
use org_office::attachments::testkit::{sample_pdf, sample_png};
use org_office::{EngineChoice, Office, TestOffice};
use org_proto::attachments::{
    AttachmentAttachParams, AttachmentInspect, AttachmentInspectParams, AttachmentReadChunk,
    AttachmentReadChunkParams, AttachmentReadChunkResult, AttachmentTargetKind,
    AttachmentUploadBeginParams, AttachmentUploadChunkParams, AttachmentUploadRef, AttachmentView,
    ATTACHMENT_CHUNK_MAX_BYTES,
};
use org_proto::{ApiError, ErrorCode, Outcome, Request};
use org_store::Id;
use org_sync_proto::{blob_digest, SyncFrame};

use super::a2a_messaging::enroll_satellite;
use crate::attachment_calls::dispatch_attachment_rpc;
use crate::federation::{ship_blob_outbox, HqBlobRemote, HqFederationServer, SatelliteSyncSession};

mod message_files;

pub(super) struct Net {
    pub(super) hq: Arc<Office>,
    pub(super) owner: String,
    pub(super) server: Option<HqFederationServer>,
    pub(super) sats: Vec<(tempfile::TempDir, Arc<Office>)>,
}

impl Net {
    pub(super) fn new(satellites: u8) -> Self {
        let hq = Arc::new(Office::for_test_with_real_ca(TestOffice {
            engine: EngineChoice::Sqlite,
        }));
        let owner = hq.owner_member_id().to_owned();
        let server = HqFederationServer::start(
            Arc::clone(&hq),
            "127.0.0.1:0".parse().unwrap(),
            &["127.0.0.1".into()],
            None,
        )
        .unwrap();
        let addr = server.local_addr().to_string();
        let sats = (1..=satellites)
            .map(|n| enroll_satellite(&hq, &owner, &addr, &format!("Blob Sat {n}"), 40 + n))
            .collect();
        Self {
            hq,
            owner,
            server: Some(server),
            sats,
        }
    }

    pub(super) fn sat(&self, n: usize) -> &Arc<Office> {
        &self.sats[n].1
    }

    pub(super) fn stop_hq(&mut self) {
        if let Some(server) = self.server.take() {
            server.stop();
        }
    }
}

/// Uploads `bytes` in 512 KiB chunks, commits them and binds them organization-wide.
pub(super) fn publish(
    office: &Office,
    actor: &str,
    filename: &str,
    mime: &str,
    bytes: &[u8],
) -> AttachmentView {
    let begun = office
        .attachment_upload_begin(
            actor,
            AttachmentUploadBeginParams {
                filename: filename.into(),
                mime_type: mime.into(),
                byte_count: bytes.len() as u64,
            },
        )
        .unwrap()
        .unwrap();
    let upload_id = begun.upload.upload_id;
    for (n, chunk) in bytes
        .chunks(ATTACHMENT_CHUNK_MAX_BYTES as usize)
        .enumerate()
    {
        office
            .attachment_upload_chunk(
                actor,
                AttachmentUploadChunkParams {
                    upload_id: upload_id.clone(),
                    offset: (n * ATTACHMENT_CHUNK_MAX_BYTES as usize) as u64,
                    data_base64: STANDARD.encode(chunk),
                },
            )
            .unwrap()
            .unwrap();
    }
    office
        .attachment_upload_commit(
            actor,
            AttachmentUploadRef {
                upload_id: upload_id.clone(),
            },
        )
        .unwrap()
        .unwrap();
    office
        .attachment_attach(
            actor,
            AttachmentAttachParams {
                command_id: office.next_uuid_v7(),
                upload_id: Some(upload_id),
                source_attachment_id: None,
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

/// Reads a whole file through the real `attachment.read_chunk` dispatch.
pub(super) fn read_rpc(office: &Arc<Office>, member: &str, id: &str) -> Result<Vec<u8>, ApiError> {
    let mut out = Vec::new();
    loop {
        let request = Request::new::<AttachmentReadChunk>(
            7,
            &AttachmentReadChunkParams {
                attachment_id: id.into(),
                offset: out.len() as u64,
                max_bytes: None,
            },
        )
        .unwrap();
        let response = dispatch_attachment_rpc(office, Some(member), &request).unwrap();
        let chunk: AttachmentReadChunkResult = match response.outcome {
            Outcome::Result(value) => serde_json::from_value(value).unwrap(),
            Outcome::Error(error) => return Err(error),
        };
        out.extend(STANDARD.decode(chunk.data_base64).unwrap());
        if chunk.eof {
            return Ok(out);
        }
    }
}

pub(super) fn inspect(
    office: &Arc<Office>,
    member: &str,
    id: &str,
) -> Result<AttachmentView, ApiError> {
    let request = Request::new::<AttachmentInspect>(
        8,
        &AttachmentInspectParams {
            attachment_id: id.into(),
        },
    )
    .unwrap();
    match dispatch_attachment_rpc(office, Some(member), &request)
        .unwrap()
        .outcome
    {
        Outcome::Result(value) => Ok(serde_json::from_value::<
            org_proto::attachments::AttachmentResult,
        >(value)
        .unwrap()
        .attachment),
        Outcome::Error(error) => Err(error),
    }
}

pub(super) fn pull_org_events(sat: &Arc<Office>) {
    let mut session = SatelliteSyncSession::connect(Arc::clone(sat)).unwrap();
    session.pull_org_events().unwrap();
}

/// HQ's own copy of a blob, straight from `<durable>/blobs/`.
pub(super) fn hq_blob_on_disk(hq: &Office, sha: &str) -> Vec<u8> {
    let path = hq
        .durable_root()
        .join("blobs/sha256")
        .join(&sha[0..2])
        .join(&sha[2..4])
        .join(sha);
    std::fs::read(path).unwrap()
}

pub(super) fn files_under(dir: &std::path::Path) -> usize {
    let Ok(listing) = std::fs::read_dir(dir) else {
        return 0;
    };
    listing
        .flatten()
        .map(|item| {
            if item.file_type().unwrap().is_dir() {
                files_under(&item.path())
            } else {
                1
            }
        })
        .sum()
}

#[test]
fn federation_ships_a_satellite_image_and_multi_chunk_pdf_to_hq_and_drains_the_outbox() {
    let net = Net::new(1);
    let sat = net.sat(0);
    let owner = &net.owner;
    let png = publish(sat, owner, "diagram.png", "image/png", &sample_png(96, 64));
    let pdf_bytes = sample_pdf(3, 6 * 1024 * 1024);
    let pdf = publish(sat, owner, "spec.pdf", "application/pdf", &pdf_bytes);
    assert!(pdf.byte_count > 10 * u64::from(ATTACHMENT_CHUNK_MAX_BYTES));

    // Offline at once: both are readable on the satellite from its outbox, and wait there.
    assert_eq!(sat.attachment_outbox_len().unwrap(), 2);
    let png_local = read_rpc(sat, owner, &png.id).unwrap();
    assert_eq!(blob_digest(&png_local), png.content_sha256);
    assert_eq!(read_rpc(sat, owner, &pdf.id).unwrap(), pdf_bytes);
    let hq_before = net.hq.attachment_blob_counts().unwrap();

    let flush = ship_blob_outbox(sat, None, 1 << 30).unwrap();
    assert_eq!((flush.shipped, flush.refused), (2, 0));
    assert_eq!(
        sat.attachment_outbox_len().unwrap(),
        0,
        "the outbox drained"
    );
    assert_eq!(files_under(&sat.attachment_outbox_root().join("sha256")), 0);

    // HQ holds the verified bytes in `<durable>/blobs/` and has bound both records.
    let hq_after = net.hq.attachment_blob_counts().unwrap();
    assert_eq!(hq_after, (hq_before.0 + 2, hq_before.1));
    for view in [&png, &pdf] {
        let on_disk = hq_blob_on_disk(&net.hq, &view.content_sha256);
        assert_eq!(blob_digest(&on_disk), view.content_sha256);
        let at_hq = net
            .hq
            .attachment_inspect(
                owner,
                org_proto::attachments::AttachmentInspectParams {
                    attachment_id: view.id.clone(),
                },
            )
            .unwrap()
            .unwrap()
            .attachment;
        assert_eq!(at_hq.content_sha256, view.content_sha256);
        assert_eq!(at_hq.byte_count, view.byte_count);
    }
    // Still readable on the satellite (from its cache now), and nothing more to ship.
    assert_eq!(read_rpc(sat, owner, &pdf.id).unwrap(), pdf_bytes);
    assert_eq!(sat.attachment_blob_cache().count().unwrap(), 2);
    assert_eq!(
        ship_blob_outbox(sat, None, 1 << 30).unwrap(),
        crate::federation::OutboxFlush::default()
    );
}

#[test]
fn federation_carries_satellite_a_file_through_hq_to_satellite_b_on_demand_and_caches_it() {
    let mut net = Net::new(2);
    let owner = net.owner.clone();
    let (a, b) = (Arc::clone(net.sat(0)), Arc::clone(net.sat(1)));
    let bytes = sample_pdf(2, 1536 * 1024);
    let file = publish(&a, &owner, "plan.pdf", "application/pdf", &bytes);
    assert_eq!(ship_blob_outbox(&a, None, 1 << 30).unwrap().shipped, 1);

    // HQ's binding replicates to B: B knows the file, holds none of its bytes.
    pull_org_events(&b);
    assert_eq!(
        inspect(&b, &owner, &file.id).unwrap().content_sha256,
        file.content_sha256
    );
    let cache = b.attachment_blob_cache();
    assert_eq!(cache.count().unwrap(), 0);
    assert_eq!(b.attachment_blob_counts().unwrap(), (0, 0));

    // The first read pulls from HQ, verifies and caches; the second is a cache hit, served
    // with HQ gone.
    assert_eq!(read_rpc(&b, &owner, &file.id).unwrap(), bytes);
    assert_eq!(cache.count().unwrap(), 1);
    assert!(cache
        .root()
        .starts_with(b.runtime_root().join(&b.info().office_id)));
    net.stop_hq();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(read_rpc(&b, &owner, &file.id).unwrap(), bytes);
}

#[test]
fn federation_cache_evicts_pulled_files_under_its_cap_and_never_loses_an_unacked_outbox_file() {
    let mut net = Net::new(1);
    let owner = net.owner.clone();
    let sat = Arc::clone(net.sat(0));
    let files: Vec<(AttachmentView, Vec<u8>)> = (0..3u8)
        .map(|n| {
            let bytes: Vec<u8> = format!("shared file {n}\n")
                .into_bytes()
                .into_iter()
                .cycle()
                .take(2048)
                .collect();
            let view = publish(&net.hq, &owner, &format!("f{n}.txt"), "text/plain", &bytes);
            (view, bytes)
        })
        .collect();
    pull_org_events(&sat);
    // The satellite's own file, never shipped: it stays in the outbox whatever the cache does.
    let own_bytes = b"written on the satellite, not acked yet".to_vec();
    let own = publish(&sat, &owner, "own.txt", "text/plain", &own_bytes);

    let remote = HqBlobRemote::new(Arc::clone(&sat));
    let pull = BlobPull {
        remote: &remote,
        cache_max_bytes: 5 * 1024,
    };
    let read = |view: &AttachmentView| -> Result<Vec<u8>, ApiError> {
        let mut out = Vec::new();
        loop {
            let chunk = sat
                .attachment_read_chunk_with(
                    &owner,
                    AttachmentReadChunkParams {
                        attachment_id: view.id.clone(),
                        offset: out.len() as u64,
                        max_bytes: Some(1000),
                    },
                    Some(pull),
                )
                .unwrap()?;
            out.extend(STANDARD.decode(chunk.data_base64).unwrap());
            if chunk.eof {
                return Ok(out);
            }
        }
    };
    for (view, bytes) in &files {
        assert_eq!(&read(view).unwrap(), bytes);
    }
    // Three 2 KiB files under a 5 KiB cap: the first one read went.
    let cache = sat.attachment_blob_cache();
    let cached: Vec<String> = cache
        .entries()
        .unwrap()
        .into_iter()
        .map(|e| e.hash.to_string())
        .collect();
    assert_eq!(
        cached,
        [
            files[1].0.content_sha256.clone(),
            files[2].0.content_sha256.clone()
        ]
    );
    // Reading file 1 again makes file 2 the oldest; file 0 comes back from HQ and pushes it out.
    assert_eq!(read(&files[1].0).unwrap(), files[1].1);
    assert_eq!(read(&files[0].0).unwrap(), files[0].1);
    let cached: Vec<String> = cache
        .entries()
        .unwrap()
        .into_iter()
        .map(|e| e.hash.to_string())
        .collect();
    assert_eq!(
        cached,
        [
            files[1].0.content_sha256.clone(),
            files[0].0.content_sha256.clone()
        ]
    );

    // HQ gone: cached files and the un-acked outbox file still read; an evicted one is a
    // retryable `unavailable`.
    net.stop_hq();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(read(&files[0].0).unwrap(), files[0].1);
    assert_eq!(read(&own).unwrap(), own_bytes);
    let refused = read(&files[2].0).unwrap_err();
    assert_eq!(refused.code, ErrorCode::Unavailable);
    assert!(refused.retry_after_ms.is_some());
    assert_eq!(sat.attachment_outbox_len().unwrap(), 1);
    assert_eq!(
        sat.attachment_outbox_pending().unwrap()[0].attachment_id,
        own.id
    );
}

#[test]
fn federation_reads_degrade_to_a_retryable_unavailable_when_hq_is_unreachable() {
    let mut net = Net::new(1);
    let owner = net.owner.clone();
    let sat = Arc::clone(net.sat(0));
    let bytes = b"only at HQ so far".to_vec();
    let file = publish(&net.hq, &owner, "hq.txt", "text/plain", &bytes);
    pull_org_events(&sat);
    net.stop_hq();
    std::thread::sleep(Duration::from_millis(50));
    let refused = read_rpc(&sat, &owner, &file.id).unwrap_err();
    assert_eq!(refused.code, ErrorCode::Unavailable);
    assert_eq!(
        refused.retry_after_ms,
        Some(org_office::attachments::remote::PULL_RETRY_AFTER_MS)
    );
    assert_eq!(sat.attachment_blob_cache().count().unwrap(), 0);
    // The shipper with HQ gone fails, and keeps the satellite's own file.
    let own = publish(&sat, &owner, "own.txt", "text/plain", b"offline");
    assert!(ship_blob_outbox(&sat, None, 1 << 30).is_err());
    assert_eq!(read_rpc(&sat, &owner, &own.id).unwrap(), b"offline");
    assert_eq!(sat.attachment_outbox_len().unwrap(), 1);
}

/// Sends `frame` on a raw session and returns HQ's first reply naming `request_id`.
pub(super) fn exchange(
    session: &mut SatelliteSyncSession,
    frame: &SyncFrame,
    request_id: &str,
) -> SyncFrame {
    session.write_frame(frame).unwrap();
    loop {
        let reply = session
            .read_correlated_frame(Duration::from_secs(15), None)
            .unwrap()
            .expect("HQ answers");
        let named = match &reply {
            SyncFrame::BlobHasResponse { request_id: r, .. }
            | SyncFrame::BlobAck { request_id: r, .. }
            | SyncFrame::BlobChunk { request_id: r, .. } => r == request_id,
            _ => false,
        };
        if named {
            return reply;
        }
    }
}

#[test]
fn federation_never_gives_a_satellite_private_bytes_by_attachment_id_or_digest() {
    let net = Net::new(1);
    let owner = net.owner.clone();
    let sat = Arc::clone(net.sat(0));
    let secret = b"PRIVATE-the-owner's-eyes-only".to_vec();
    let staged = {
        let begun = net
            .hq
            .attachment_upload_begin(
                &owner,
                AttachmentUploadBeginParams {
                    filename: "secret.txt".into(),
                    mime_type: "text/plain".into(),
                    byte_count: secret.len() as u64,
                },
            )
            .unwrap()
            .unwrap();
        net.hq
            .attachment_upload_chunk(
                &owner,
                AttachmentUploadChunkParams {
                    upload_id: begun.upload.upload_id.clone(),
                    offset: 0,
                    data_base64: STANDARD.encode(&secret),
                },
            )
            .unwrap()
            .unwrap();
        net.hq
            .attachment_upload_commit(
                &owner,
                AttachmentUploadRef {
                    upload_id: begun.upload.upload_id.clone(),
                },
            )
            .unwrap()
            .unwrap();
        begun.upload.upload_id
    };
    let private = net
        .hq
        .attachment_attach(
            &owner,
            AttachmentAttachParams {
                command_id: net.hq.next_uuid_v7(),
                upload_id: Some(staged),
                source_attachment_id: None,
                target_kind: AttachmentTargetKind::UserPrivate,
                target_id: None,
                title: None,
                description: None,
                artifact_status: None,
            },
        )
        .unwrap()
        .unwrap()
        .attachment;
    let shared = publish(
        &net.hq,
        &owner,
        "open.txt",
        "text/plain",
        b"for every office",
    );
    pull_org_events(&sat);

    // By attachment ID or by digest, through the real pull path: refused as not found.
    let mut session = SatelliteSyncSession::connect(Arc::clone(&sat)).unwrap();
    for asked in [&private.id, &private.content_sha256] {
        match session.fetch_shared_blob(asked, &private.content_sha256, private.byte_count) {
            Err(PullFailure::Refused { code, .. }) => assert_eq!(code, "not_found", "{asked}"),
            other => panic!("{asked}: {other:?}"),
        }
    }
    // A raw fetch frame for the private ID gets no chunk, only a refusal.
    let reply = exchange(
        &mut session,
        &SyncFrame::BlobFetchRequest {
            request_id: "fetch-private".into(),
            org_id: net.hq.info().org_id.clone(),
            attachment_id: private.id.clone(),
        },
        "fetch-private",
    );
    assert!(
        matches!(&reply, SyncFrame::BlobAck { accepted: false, code: Some(c), .. } if c == "not_found"),
        "{reply:?}"
    );
    // Announcing a shared file that names the private bytes: HQ only asks its shared store.
    let probe = AttachmentRecord {
        id: Id::parse(&sat.next_uuid_v7()).unwrap(),
        target_kind: AttachmentTargetKind::Organization,
        target_id: None,
        content_sha256: ContentHash::parse(&private.content_sha256).unwrap(),
        mime_type: "text/plain".into(),
        byte_count: private.byte_count,
        filename: "probe.txt".into(),
        title: "Probe".into(),
        description: None,
        artifact_status: Default::default(),
        width: None,
        height: None,
        page_count: None,
        uploaded_by_employee_id: Id::parse(&owner).unwrap(),
        created_at: "2026-11-26T10:00:00.000Z".into(),
        updated_at: "2026-11-26T10:00:00.000Z".into(),
    };
    let announce = |request_id: &str, record: &AttachmentRecord| SyncFrame::BlobHasRequest {
        request_id: request_id.into(),
        org_id: net.hq.info().org_id.clone(),
        attachment_id: record.id.to_string(),
        content_sha256: record.content_sha256.to_string(),
        byte_count: record.byte_count,
        record_b64: STANDARD.encode(records::encode(record)),
    };
    let reply = exchange(&mut session, &announce("probe", &probe), "probe");
    assert!(
        matches!(reply, SyncFrame::BlobHasResponse { present: false, .. }),
        "{reply:?}"
    );
    // A private record never ships to HQ.
    let as_private = AttachmentRecord {
        id: Id::parse(&sat.next_uuid_v7()).unwrap(),
        target_kind: AttachmentTargetKind::UserPrivate,
        target_id: Some(Id::parse(&owner).unwrap()),
        ..probe.clone()
    };
    let reply = exchange(
        &mut session,
        &announce("private-ship", &as_private),
        "private-ship",
    );
    assert!(
        matches!(&reply, SyncFrame::BlobAck { accepted: false, code: Some(c), .. } if c == "not_authorized"),
        "{reply:?}"
    );
    // The shared file comes through, and the satellite holds no bytes of either otherwise.
    assert_eq!(
        read_rpc(&sat, &owner, &shared.id).unwrap(),
        b"for every office"
    );
    assert_eq!(
        inspect(&sat, &owner, &private.id).unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(sat.attachment_blob_counts().unwrap(), (0, 0));
    assert!(!sat.durable_root().join("private").exists());
    assert!(
        !sat.durable_root().join("blobs").exists(),
        "the snapshot carried no bytes"
    );
}

#[test]
fn federation_hq_binds_nothing_from_chunks_that_are_out_of_order_or_do_not_verify() {
    let net = Net::new(1);
    let owner = net.owner.clone();
    let sat = Arc::clone(net.sat(0));
    let bytes = b"bytes the satellite claims".to_vec();
    let record = AttachmentRecord {
        id: Id::parse(&sat.next_uuid_v7()).unwrap(),
        target_kind: AttachmentTargetKind::Organization,
        target_id: None,
        content_sha256: ContentHash::of(&bytes),
        mime_type: "text/plain".into(),
        byte_count: bytes.len() as u64,
        filename: "claim.txt".into(),
        title: "Claim".into(),
        description: None,
        artifact_status: Default::default(),
        width: None,
        height: None,
        page_count: None,
        uploaded_by_employee_id: Id::parse(&owner).unwrap(),
        created_at: "2026-11-26T10:00:00.000Z".into(),
        updated_at: "2026-11-26T10:00:00.000Z".into(),
    };
    let sha = record.content_sha256.to_string();
    let announce = |request_id: &str| SyncFrame::BlobHasRequest {
        request_id: request_id.into(),
        org_id: net.hq.info().org_id.clone(),
        attachment_id: record.id.to_string(),
        content_sha256: sha.clone(),
        byte_count: record.byte_count,
        record_b64: STANDARD.encode(records::encode(&record)),
    };
    let chunk = |request_id: &str, offset: u64, data: &[u8]| SyncFrame::BlobChunk {
        request_id: request_id.into(),
        content_sha256: sha.clone(),
        offset,
        total_bytes: record.byte_count,
        data_b64: STANDARD.encode(data),
    };
    let refused_with = |reply: &SyncFrame, code: &str| matches!(reply, SyncFrame::BlobAck { accepted: false, code: Some(c), .. } if c == code);
    let before = net.hq.attachment_blob_counts().unwrap();
    let mut session = SatelliteSyncSession::connect(Arc::clone(&sat)).unwrap();

    // Same length, other bytes: the digest does not match and nothing is kept.
    let reply = exchange(&mut session, &announce("forged"), "forged");
    assert!(matches!(
        reply,
        SyncFrame::BlobHasResponse { present: false, .. }
    ));
    let forged = vec![b'x'; bytes.len()];
    let reply = exchange(&mut session, &chunk("forged", 0, &forged), "forged");
    assert!(refused_with(&reply, "invalid_request"), "{reply:?}");
    // A chunk that skips ahead is refused; a chunk for no announced upload too.
    let reply = exchange(&mut session, &announce("skip"), "skip");
    assert!(matches!(
        reply,
        SyncFrame::BlobHasResponse { present: false, .. }
    ));
    let reply = exchange(&mut session, &chunk("skip", 4, &bytes[4..]), "skip");
    assert!(refused_with(&reply, "invalid_request"), "{reply:?}");
    let reply = exchange(&mut session, &chunk("nobody", 0, &bytes), "nobody");
    assert!(refused_with(&reply, "conflict"), "{reply:?}");
    assert_eq!(net.hq.attachment_blob_counts().unwrap(), before);
    assert_eq!(
        inspect(&net.hq, &owner, &record.id.to_string())
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    // Another organization's announcement is refused outright.
    let mut foreign = announce("foreign");
    if let SyncFrame::BlobHasRequest { org_id, .. } = &mut foreign {
        *org_id = "0195a000-0000-7000-8000-00000000dead".into();
    }
    let reply = exchange(&mut session, &foreign, "foreign");
    assert!(refused_with(&reply, "not_authorized"), "{reply:?}");

    // The honest upload of the same announcement then goes through.
    let reply = exchange(&mut session, &announce("honest"), "honest");
    assert!(matches!(
        reply,
        SyncFrame::BlobHasResponse { present: false, .. }
    ));
    let reply = exchange(&mut session, &chunk("honest", 0, &bytes), "honest");
    assert!(
        matches!(reply, SyncFrame::BlobAck { accepted: true, .. }),
        "{reply:?}"
    );
    assert_eq!(
        read_rpc(&net.hq, &owner, &record.id.to_string()).unwrap(),
        bytes
    );
}

#[test]
fn federation_shipper_thread_drains_the_outbox_on_its_own_and_stops_cleanly() {
    let net = Net::new(1);
    let owner = net.owner.clone();
    let sat = Arc::clone(net.sat(0));
    let file = publish(
        &sat,
        &owner,
        "auto.txt",
        "text/plain",
        b"shipped by the thread",
    );
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let wake = Arc::new(crate::federation_stream::SatSyncWake::default());
    let shipper = crate::federation::spawn_satellite_blob_shipper(&sat, &stop, Arc::clone(&wake))
        .expect("a satellite runs a shipper");
    super::a2a_messaging::wait_until(Duration::from_secs(20), "the outbox drains", || {
        sat.attachment_outbox_len().unwrap() == 0
    });
    assert_eq!(
        read_rpc(&net.hq, &owner, &file.id).unwrap(),
        b"shipped by the thread"
    );
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    wake.notify();
    shipper.join().unwrap();
    assert!(
        crate::federation::spawn_satellite_blob_shipper(&net.hq, &stop, wake).is_none(),
        "HQ runs no shipper"
    );
}
