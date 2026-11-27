//! HQ's custody of shared files across offices (`TASK-98.6`): a shipped file is bound only
//! once its record is allowed and its bytes verify, a retry binds nothing new, a satellite
//! fetches shared files by ID alone and never a private one, and the state snapshot every
//! satellite installs carries records, never bytes.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_proto::attachments::{ArtifactStatus, AttachmentTargetKind};
use org_proto::ErrorCode;
use org_store::Id;

use super::{attach_upload, next_uuid, read_all, upload, Fixture, ENGINES};
use crate::attachments::blobs::ContentHash;
use crate::attachments::media::{self, MediaType};
use crate::attachments::records::{self, AttachmentRecord};
use crate::attachments::testkit::sample_png;
use crate::store::open::EngineChoice;

pub(super) const SATELLITE: &str = "0195a000-0000-7000-8000-00000000aaaa";

/// The record a satellite ships for `bytes`: named for its type, with the dimensions and page
/// count the bytes decode to (none when they do not decode).
pub(super) fn shipped(
    uploader: &str,
    kind: AttachmentTargetKind,
    target: Option<&str>,
    mime: &str,
    bytes: &[u8],
) -> AttachmentRecord {
    let media = MediaType::from_mime(mime);
    let kept = media.and_then(|media| media::validate(media, bytes.to_vec()).ok());
    AttachmentRecord {
        id: Id::parse(&next_uuid()).unwrap(),
        target_kind: kind,
        target_id: target.map(|t| Id::parse(t).unwrap()),
        content_sha256: ContentHash::of(bytes),
        mime_type: mime.into(),
        byte_count: bytes.len() as u64,
        filename: format!("shipped.{}", media.map_or("bin", |m| m.extensions()[0])),
        title: "Shipped".into(),
        description: None,
        artifact_status: ArtifactStatus::default(),
        width: kept.as_ref().and_then(|k| k.width),
        height: kept.as_ref().and_then(|k| k.height),
        page_count: kept.as_ref().and_then(|k| k.page_count),
        uploaded_by_employee_id: Id::parse(uploader).unwrap(),
        created_at: "2026-11-26T10:00:00.000Z".into(),
        updated_at: "2026-11-26T10:00:00.000Z".into(),
    }
}

pub(super) fn check(
    fx: &Fixture,
    origin: &str,
    record: &AttachmentRecord,
) -> Result<crate::attachments::remote::ShippedAttachment, org_proto::ApiError> {
    fx.office
        .attachment_ingest_check(
            origin,
            record.id.as_str(),
            record.content_sha256.as_str(),
            record.byte_count,
            &records::encode(record),
        )
        .unwrap()
}

#[test]
fn hq_binds_a_shipped_file_only_once_its_bytes_verify_and_a_retry_binds_nothing_new() {
    for engine in ENGINES {
        let fx = Fixture::new(engine);
        let bytes = sample_png(16, 8);
        let record = shipped(
            &fx.person,
            AttachmentTargetKind::Project,
            Some(&fx.project),
            "image/png",
            &bytes,
        );
        let id = record.id.to_string();
        let (shared_before, private_before) = fx.office.attachment_blob_counts().unwrap();
        let checked = check(&fx, SATELLITE, &record).unwrap();
        assert!(!checked.present);
        assert_eq!(checked.content_sha256(), record.content_sha256.as_str());

        // Other bytes, or none while HQ holds none, bind nothing and keep nothing.
        let wrong = fx
            .office
            .attachment_ingest_commit(&checked, Some(b"not the png".to_vec()))
            .unwrap()
            .unwrap_err();
        assert_eq!(wrong.code, ErrorCode::InvalidRequest);
        let missing = fx
            .office
            .attachment_ingest_commit(&checked, None)
            .unwrap()
            .unwrap_err();
        assert_eq!(missing.code, ErrorCode::Conflict);
        assert_eq!(
            fx.office.attachment_blob_counts().unwrap(),
            (shared_before, private_before)
        );
        assert_eq!(
            read_all(&fx.office, &fx.person, &id, 4096)
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );

        fx.office
            .attachment_ingest_commit(&checked, Some(bytes.clone()))
            .unwrap()
            .unwrap();
        assert_eq!(read_all(&fx.office, &fx.person, &id, 4096).unwrap(), bytes);
        assert_eq!(
            fx.office.attachment_blob_counts().unwrap(),
            (shared_before + 1, private_before),
            "{engine:?}: the bytes are in HQ's shared store, never the private one"
        );

        // A retry after a lost ack: HQ holds both already and binds nothing new.
        let again = check(&fx, SATELLITE, &record).unwrap();
        assert!(again.present);
        fx.office
            .attachment_ingest_commit(&again, None)
            .unwrap()
            .unwrap();
        assert_eq!(
            fx.office.attachment_blob_counts().unwrap(),
            (shared_before + 1, private_before)
        );
        // The same ID naming other bytes is a conflict, never a rebinding.
        let mut forged = record.clone();
        let other = sample_png(8, 8);
        forged.content_sha256 = ContentHash::of(&other);
        forged.byte_count = other.len() as u64;
        assert_eq!(
            check(&fx, SATELLITE, &forged).unwrap_err().code,
            ErrorCode::Conflict
        );
    }
}

#[test]
fn hq_refuses_a_shipped_file_its_uploader_could_not_add_or_whose_bytes_lie() {
    let fx = Fixture::new(EngineChoice::Memory);
    let bytes = sample_png(8, 8);
    let refused = |record: &AttachmentRecord, origin: &str| check(&fx, origin, record).unwrap_err();

    let private = shipped(
        &fx.person,
        AttachmentTargetKind::UserPrivate,
        Some(&fx.person),
        "image/png",
        &bytes,
    );
    assert_eq!(
        refused(&private, SATELLITE).code,
        ErrorCode::NotAuthorized,
        "a private file never ships"
    );
    let outsider = shipped(
        &fx.other_person,
        AttachmentTargetKind::Project,
        Some(&fx.project),
        "image/png",
        &bytes,
    );
    assert_eq!(refused(&outsider, SATELLITE).code, ErrorCode::NotAuthorized);
    let stranger = shipped(
        &next_uuid(),
        AttachmentTargetKind::Organization,
        None,
        "image/png",
        &bytes,
    );
    assert_eq!(refused(&stranger, SATELLITE).code, ErrorCode::NotAuthorized);
    // An agent's file ships only from the office the agent is placed in.
    let agents = shipped(
        &fx.agent,
        AttachmentTargetKind::Project,
        Some(&fx.project),
        "image/png",
        &bytes,
    );
    assert_eq!(refused(&agents, SATELLITE).code, ErrorCode::NotAuthorized);
    check(&fx, &fx.office.info().office_id.clone(), &agents).unwrap();

    // The announcement must be the record's, within the type's size limit.
    let fine = shipped(
        &fx.person,
        AttachmentTargetKind::Organization,
        None,
        "image/png",
        &bytes,
    );
    let other_size = fx
        .office
        .attachment_ingest_check(
            SATELLITE,
            fine.id.as_str(),
            fine.content_sha256.as_str(),
            fine.byte_count + 1,
            &records::encode(&fine),
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(other_size.code, ErrorCode::InvalidRequest);
    let garbage = fx
        .office
        .attachment_ingest_check(SATELLITE, fine.id.as_str(), "x", 1, b"not: [a record")
        .unwrap()
        .unwrap_err();
    assert_eq!(garbage.code, ErrorCode::InvalidRequest);
    let mut huge = fine.clone();
    huge.byte_count = 11 * 1024 * 1024;
    assert_eq!(refused(&huge, SATELLITE).code, ErrorCode::TooLarge);

    // Bytes that hash right but are not what their type says are never kept.
    let text = b"plain text pretending to be a png".to_vec();
    let disguised = shipped(
        &fx.person,
        AttachmentTargetKind::Organization,
        None,
        "image/png",
        &text,
    );
    let checked = check(&fx, SATELLITE, &disguised).unwrap();
    let before = fx.office.attachment_blob_counts().unwrap();
    assert!(fx
        .office
        .attachment_ingest_commit(&checked, Some(text))
        .unwrap()
        .is_err());
    assert_eq!(fx.office.attachment_blob_counts().unwrap(), before);
}

#[test]
fn a_satellite_fetches_shared_files_by_id_alone_and_never_a_private_one() {
    for engine in ENGINES {
        let fx = Fixture::new(engine);
        let secret = b"PRIVATE-BYTES-only-the-owner-may-read".to_vec();
        let staged = upload(&fx.office, &fx.owner, "secret.txt", "text/plain", &secret).unwrap();
        let private = attach_upload(
            &fx.office,
            &fx.owner,
            &staged.upload_id,
            AttachmentTargetKind::UserPrivate,
            None,
        )
        .unwrap();
        let shared_bytes = b"shared notes for every office".to_vec();
        let staged = upload(
            &fx.office,
            &fx.person,
            "notes.txt",
            "text/plain",
            &shared_bytes,
        )
        .unwrap();
        let shared = attach_upload(
            &fx.office,
            &fx.person,
            &staged.upload_id,
            AttachmentTargetKind::Organization,
            None,
        )
        .unwrap();

        let blob = fx
            .office
            .attachment_peer_fetch(&shared.id)
            .unwrap()
            .unwrap();
        assert_eq!(blob.byte_count, shared_bytes.len() as u64);
        assert_eq!(
            fx.office
                .attachment_peer_range(&blob, 7, 5)
                .unwrap()
                .unwrap(),
            shared_bytes[7..12]
        );
        for asked in [
            private.id.as_str(),
            private.content_sha256.as_str(),
            shared.content_sha256.as_str(),
            "../../private/blobs",
            "",
        ] {
            let refused = fx.office.attachment_peer_fetch(asked).unwrap().unwrap_err();
            assert_eq!(refused.code, ErrorCode::NotFound, "{engine:?} {asked}");
        }
        // Announcing a shared file that names the private bytes: HQ's shared store is the only
        // one asked, so HQ says it does not hold them.
        let probe = shipped(
            &fx.person,
            AttachmentTargetKind::Organization,
            None,
            "text/plain",
            &secret,
        );
        assert_eq!(probe.content_sha256.as_str(), private.content_sha256);
        assert!(!check(&fx, SATELLITE, &probe).unwrap().present);
    }
}

#[test]
fn the_state_snapshot_satellites_install_carries_file_records_never_their_bytes() {
    let fx = Fixture::new(EngineChoice::Sqlite);
    let marker = b"BLOB-BYTES-MARKER-never-in-a-snapshot-0042".to_vec();
    let staged = upload(&fx.office, &fx.person, "m.txt", "text/plain", &marker).unwrap();
    let shared = attach_upload(
        &fx.office,
        &fx.person,
        &staged.upload_id,
        AttachmentTargetKind::Organization,
        None,
    )
    .unwrap();
    let staging = fx.tmp.path().join("snapshot-pages");
    crate::export_authoritative_state_snapshot_pages(&fx.office, &staging, 1 << 20, 256, |_| true)
        .unwrap();
    let mut paths = Vec::new();
    for page in std::fs::read_dir(&staging).unwrap() {
        let bytes = std::fs::read(page.unwrap().path()).unwrap();
        let Ok(entries) = serde_json::from_slice::<Vec<(String, String)>>(&bytes) else {
            continue;
        };
        for (path, content) in entries {
            let content = STANDARD.decode(content).unwrap();
            assert!(
                !content.windows(marker.len()).any(|w| w == marker),
                "{path} carries the file's bytes"
            );
            paths.push(path);
        }
    }
    assert!(
        paths.contains(&format!("state/files/{}.yaml", shared.id)),
        "{paths:?}"
    );
    assert!(paths
        .iter()
        .all(|p| !p.starts_with("blobs/") && !p.starts_with("private/") && !p.contains("blob")));
}
