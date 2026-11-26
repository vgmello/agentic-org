//! Blob stores, the staging area and the attachment record (`TASK-98.1` AC #1).

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use org_proto::attachments::{ArtifactStatus, AttachmentTargetKind};
use org_proto::ErrorCode;
use org_store::{DurablePath, Id};

use super::blobs::{BlobStore, ContentHash, FsBlobStore, MemoryBlobStore};
use super::media::{self, MediaType};
use super::records::{self, AttachmentRecord, FileKind};
use super::staging::{NewUpload, StagingArea, MAX_STAGED_BYTES, MAX_STAGED_FILES, STAGED_TTL_MS};

const ALICE: &str = "0195a000-0000-7000-8000-00000000a11c";
const BOB: &str = "0195a000-0000-7000-8000-000000000b0b";
const OFFICE: &str = "0195a000-0000-7000-8000-0000000000f1";
const NOW: u64 = 1_800_000_000_000;

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn blob_conformance(store: &dyn BlobStore) {
    let bytes = b"the same bytes".to_vec();
    let first = store.put(&bytes).unwrap();
    let second = store.put(&bytes).unwrap();
    assert_eq!(first, second);
    assert_eq!(first, ContentHash::of(&bytes));
    assert_eq!(store.count().unwrap(), 1, "content-addressed: stored once");
    assert_eq!(store.size(&first).unwrap(), Some(bytes.len() as u64));
    assert_eq!(store.read_range(&first, 4, 4).unwrap().unwrap(), b"same");
    assert_eq!(store.read_range(&first, 100, 4).unwrap().unwrap(), b"");
    assert_eq!(store.read_all(&first, 1024).unwrap().unwrap(), bytes);
    assert!(store.read_all(&first, 3).is_err(), "a read is bounded");
    let missing = ContentHash::of(b"never stored");
    assert_eq!(store.size(&missing).unwrap(), None);
    assert_eq!(store.read_range(&missing, 0, 4).unwrap(), None);
    store.put(b"other").unwrap();
    assert_eq!(store.count().unwrap(), 2);
}

#[test]
fn both_blob_stores_are_content_addressed_write_once_and_bounded() {
    blob_conformance(&MemoryBlobStore::new());
    let dir = tempfile::tempdir().unwrap();
    let durable = dir.path().join("durable");
    std::fs::create_dir(&durable).unwrap();
    let store = FsBlobStore::new(&durable, durable.join("blobs"));
    blob_conformance(&store);
    let hash = ContentHash::of(b"the same bytes");
    let hex = hash.as_str();
    let path = durable
        .join("blobs/sha256")
        .join(&hex[0..2])
        .join(&hex[2..4])
        .join(hex);
    assert_eq!(std::fs::read(&path).unwrap(), b"the same bytes");
    assert_eq!(mode(&path), 0o600);
    for folder in [
        durable.join("blobs"),
        durable.join("blobs/sha256"),
        path.parent().unwrap().to_path_buf(),
    ] {
        assert_eq!(mode(&folder), 0o700, "{}", folder.display());
    }
    // No temporary file is left behind.
    let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(leftovers.len(), 1, "{leftovers:?}");
}

#[test]
fn a_blob_replaced_by_a_link_or_a_folder_is_refused_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    let durable = dir.path().join("durable");
    std::fs::create_dir(&durable).unwrap();
    let store = FsBlobStore::new(&durable, durable.join("blobs"));
    let hash = store.put(b"bytes").unwrap();
    let hex = hash.as_str().to_owned();
    let path = durable
        .join("blobs/sha256")
        .join(&hex[0..2])
        .join(&hex[2..4])
        .join(&hex);
    let secret = dir.path().join("secret");
    std::fs::write(&secret, b"bytes").unwrap();
    std::fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(&secret, &path).unwrap();
    assert!(store.read_range(&hash, 0, 5).is_err());
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(store.read_all(&hash, 100).is_err());
    assert_eq!(ContentHash::parse("../etc"), None);
    assert_eq!(ContentHash::parse(&"A".repeat(64)), None);
}

fn staging() -> (tempfile::TempDir, StagingArea) {
    let dir = tempfile::tempdir().unwrap();
    let durable = dir.path().join("durable");
    std::fs::create_dir(&durable).unwrap();
    let area = StagingArea::new(&durable, OFFICE);
    (dir, area)
}

fn upload_id(n: u64) -> String {
    format!("0195a000-0000-7000-8000-{n:012x}")
}

fn begin(area: &StagingArea, n: u64, by: &str, bytes: u64, now: u64) -> Result<String, ErrorCode> {
    let id = upload_id(n);
    area.begin(&NewUpload {
        upload_id: &id,
        uploaded_by: by,
        filename: "notes.txt",
        media: MediaType::PlainText,
        declared_bytes: bytes,
        now_ms: now,
    })
    .unwrap()
    .map(|_| id)
    .map_err(|e| e.code)
}

#[test]
fn a_staged_upload_is_owner_only_and_its_folder_is_private() {
    let (_dir, area) = staging();
    let id = begin(&area, 1, ALICE, 5, NOW).unwrap();
    let folder = area.root().join(&id);
    assert_eq!(mode(&folder), 0o700);
    assert_eq!(mode(&folder.join("meta.json")), 0o600);
    assert_eq!(mode(&folder.join("data")), 0o600);
    assert!(area
        .root()
        .ends_with(format!("local/offices/{OFFICE}/attachment-staging")));
    // Bob sees nothing, can append nothing, commit nothing, abort nothing.
    assert!(area.get(BOB, &id, NOW).unwrap().is_none());
    assert_eq!(
        area.append(BOB, &id, 0, b"hello", NOW)
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    let refused = area
        .commit(BOB, &id, NOW, media::validate)
        .unwrap()
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::NotFound);
    assert_eq!(
        area.abort(BOB, &id, NOW).unwrap().unwrap_err().code,
        ErrorCode::NotFound
    );
    assert!(folder.exists());
    // A path is never an upload id.
    assert!(area.get(ALICE, "../../../etc", NOW).unwrap().is_none());
    // Alice finishes it.
    area.append(ALICE, &id, 0, b"hel", NOW).unwrap().unwrap();
    // The same chunk again is answered as before; a gap or an overlap is a conflict.
    area.append(ALICE, &id, 0, b"hel", NOW).unwrap().unwrap();
    assert_eq!(
        area.append(ALICE, &id, 4, b"o", NOW)
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert_eq!(
        area.append(ALICE, &id, 0, b"HEL", NOW)
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert_eq!(
        area.append(ALICE, &id, 3, b"lo!", NOW)
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::TooLarge
    );
    // Not complete yet: commit waits.
    assert_eq!(
        area.commit(ALICE, &id, NOW, media::validate)
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    area.append(ALICE, &id, 3, b"lo", NOW).unwrap().unwrap();
    let committed = area
        .commit(ALICE, &id, NOW, media::validate)
        .unwrap()
        .unwrap();
    let kept = committed.committed.as_ref().unwrap();
    assert_eq!(kept.content_sha256, ContentHash::of(b"hello").to_string());
    assert_eq!(area.kept_bytes(&committed).unwrap(), b"hello");
    assert!(
        !folder.join("data").exists(),
        "only the validated copy is kept"
    );
    // Committing again answers the first result.
    assert_eq!(
        area.commit(ALICE, &id, NOW, media::validate)
            .unwrap()
            .unwrap(),
        committed
    );
    area.abort(ALICE, &id, NOW).unwrap().unwrap();
    assert!(!folder.exists());
}

#[test]
fn a_tampered_kept_copy_is_refused_at_attach_time() {
    let (_dir, area) = staging();
    let id = begin(&area, 2, ALICE, 5, NOW).unwrap();
    area.append(ALICE, &id, 0, b"hello", NOW).unwrap().unwrap();
    let committed = area
        .commit(ALICE, &id, NOW, media::validate)
        .unwrap()
        .unwrap();
    std::fs::write(area.root().join(&id).join("kept"), b"HELLO").unwrap();
    assert!(area.kept_bytes(&committed).is_err());
}

#[test]
fn the_staging_quota_is_fifty_files_and_200_mib_per_employee() {
    let (_dir, area) = staging();
    for n in 0..MAX_STAGED_FILES as u64 {
        begin(&area, 100 + n, ALICE, 1, NOW).unwrap();
    }
    assert_eq!(
        begin(&area, 999, ALICE, 1, NOW),
        Err(ErrorCode::RateLimited)
    );
    // Bob's quota is his own.
    begin(&area, 1_000, BOB, MAX_STAGED_BYTES - 10, NOW).unwrap();
    assert_eq!(begin(&area, 1_001, BOB, 11, NOW), Err(ErrorCode::TooLarge));
    begin(&area, 1_002, BOB, 10, NOW).unwrap();
    // Aborting frees a slot.
    area.abort(ALICE, &upload_id(100), NOW).unwrap().unwrap();
    begin(&area, 999, ALICE, 1, NOW).unwrap();
}

#[test]
fn an_upload_that_grows_past_the_uploaders_quota_when_kept_is_refused_at_commit() {
    let (_dir, area) = staging();
    // Bob holds all but 100 bytes of his 200 MiB in one declared upload.
    begin(&area, 2_000, BOB, MAX_STAGED_BYTES - 100, NOW).unwrap();
    let id = begin(&area, 2_001, BOB, 50, NOW).unwrap();
    area.append(BOB, &id, 0, &[b'a'; 50], NOW).unwrap().unwrap();
    // Its validation keeps 200 bytes (a re-encoded image can grow): past the quota.
    let grow = |media: MediaType, _raw: Vec<u8>| {
        Ok(media::ValidatedFile {
            media,
            bytes: vec![b'b'; 200],
            width: None,
            height: None,
            page_count: None,
        })
    };
    let refused = area.commit(BOB, &id, NOW, grow).unwrap().unwrap_err();
    assert_eq!(refused.code, ErrorCode::TooLarge);
    // Nothing was kept: the upload is still uncommitted, its raw bytes still there.
    let folder = area.root().join(&id);
    assert!(!folder.join("kept").exists());
    assert!(folder.join("data").exists());
    assert!(area
        .get(BOB, &id, NOW)
        .unwrap()
        .unwrap()
        .committed
        .is_none());
    // Within the quota (100 bytes is exactly what is left), the same upload commits.
    let fits = |media: MediaType, _raw: Vec<u8>| {
        Ok(media::ValidatedFile {
            media,
            bytes: vec![b'b'; 100],
            width: None,
            height: None,
            page_count: None,
        })
    };
    let kept = area.commit(BOB, &id, NOW, fits).unwrap().unwrap();
    assert_eq!(kept.committed.unwrap().byte_count, 100);
    // Another employee's uploads never count against Bob's quota.
    let alice = begin(&area, 2_002, ALICE, 5, NOW).unwrap();
    area.append(ALICE, &alice, 0, b"hello", NOW)
        .unwrap()
        .unwrap();
    area.commit(ALICE, &alice, NOW, media::validate)
        .unwrap()
        .unwrap();
}

#[test]
fn expired_uploads_are_swept_after_24_hours_and_vanish_for_their_uploader() {
    let (_dir, area) = staging();
    let old = begin(&area, 1, ALICE, 3, NOW).unwrap();
    let young = begin(&area, 2, ALICE, 3, NOW + STAGED_TTL_MS / 2).unwrap();
    // Garbage in the folder is swept too.
    std::fs::create_dir(area.root().join("stray")).unwrap();
    let expiry = NOW + STAGED_TTL_MS;
    assert!(area.get(ALICE, &old, expiry - 1).unwrap().is_some());
    assert!(
        area.get(ALICE, &old, expiry).unwrap().is_none(),
        "expired uploads are not served"
    );
    assert_eq!(area.gc(expiry).unwrap(), 2);
    assert!(!area.root().join(&old).exists());
    assert!(area.root().join(&young).exists());
    // `begin` sweeps as well, so expired uploads never count against the quota.
    for n in 0..MAX_STAGED_FILES as u64 - 1 {
        begin(&area, 10 + n, ALICE, 1, NOW + STAGED_TTL_MS / 2).unwrap();
    }
    let later = NOW + STAGED_TTL_MS / 2 + STAGED_TTL_MS;
    begin(&area, 500, ALICE, 1, later).unwrap();
    assert_eq!(std::fs::read_dir(area.root()).unwrap().count(), 1);
}

fn record(kind: AttachmentTargetKind, target: Option<&str>) -> AttachmentRecord {
    AttachmentRecord {
        id: Id::parse("0195a000-0000-7000-8000-0000000000a7").unwrap(),
        target_kind: kind,
        target_id: target.map(|t| Id::parse(t).unwrap()),
        content_sha256: ContentHash::of(b"x"),
        mime_type: "application/pdf".into(),
        byte_count: 10,
        filename: "spec v2.pdf".into(),
        title: "Spec — v2".into(),
        description: Some("line one\nline two".into()),
        artifact_status: ArtifactStatus::Review,
        width: None,
        height: None,
        page_count: Some(3),
        uploaded_by_employee_id: Id::parse(ALICE).unwrap(),
        created_at: "2026-11-26T10:00:00.000Z".into(),
        updated_at: "2026-11-26T11:00:00.000Z".into(),
    }
}

#[test]
fn records_round_trip_through_canonical_yaml_at_their_paths() {
    let project = "0195a000-0000-7000-8000-0000000000b1";
    for (kind, target, path) in [
        (
            AttachmentTargetKind::Project,
            Some(project),
            format!("state/projects/{project}/files/0195a000-0000-7000-8000-0000000000a7.yaml"),
        ),
        (
            AttachmentTargetKind::Organization,
            None,
            "state/files/0195a000-0000-7000-8000-0000000000a7.yaml".to_owned(),
        ),
    ] {
        let original = record(kind, target);
        let bytes = records::encode(&original);
        assert_eq!(records::decode(&bytes).unwrap(), original);
        assert_eq!(records::encode(&records::decode(&bytes).unwrap()), bytes);
        let at = records::record_path(&original).unwrap();
        assert_eq!(at.as_str(), path);
        assert_eq!(records::kind_of(&at).unwrap().attachment(), &original.id);
        let text = String::from_utf8(bytes).unwrap();
        assert!(!text.contains("organization_id"), "{text}");
    }
    // A private record has no shared file.
    assert!(
        records::record_path(&record(AttachmentTargetKind::UserPrivate, Some(ALICE))).is_none()
    );
    let path = |p: &str| DurablePath::parse(p).unwrap();
    assert_eq!(
        records::kind_of(&path(
            "state/files/0195a000-0000-7000-8000-0000000000a7.yaml"
        )),
        Some(FileKind::Organization {
            attachment: Id::parse("0195a000-0000-7000-8000-0000000000a7").unwrap()
        })
    );
    for other in [
        "state/files/not-an-id.yaml",
        "state/files/0195a000-0000-7000-8000-0000000000a7.json",
        "state/projects/x/files/0195a000-0000-7000-8000-0000000000a7.yaml",
        "state/projects/0195a000-0000-7000-8000-0000000000b1/project.yaml",
        "state/organization.yaml",
    ] {
        assert!(records::kind_of(&path(other)).is_none(), "{other}");
    }
}

#[test]
fn a_record_breaking_its_contract_does_not_decode() {
    let good = String::from_utf8(records::encode(&record(
        AttachmentTargetKind::Organization,
        None,
    )))
    .unwrap();
    for (from, to) in [
        ("\"review\"", "\"published\""),
        ("\"application/pdf\"", "\"text/html\""),
        ("schema_version: 1", "schema_version: 2"),
        ("\"spec v2.pdf\"", "\"../spec.pdf\""),
        ("\"Spec — v2\"", "\"\""),
        // A hand-edited file name with edge spaces is not one the office ever keeps.
        ("\"spec v2.pdf\"", "\" spec v2.pdf\""),
        ("\"spec v2.pdf\"", "\"spec v2.pdf \""),
        ("\"spec v2.pdf\"", "\"   \""),
    ] {
        let bad = good.replace(from, to);
        assert_ne!(bad, good, "{from}");
        assert!(records::decode(bad.as_bytes()).is_err(), "{from} -> {to}");
    }
    let long_title = good.replace("\"Spec — v2\"", &format!("\"{}\"", "t".repeat(161)));
    assert!(records::decode(long_title.as_bytes()).is_err());
    let extra = format!("{good}surprise: true\n");
    assert!(records::decode(extra.as_bytes()).is_err());
}
