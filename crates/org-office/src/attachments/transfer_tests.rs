//! A satellite's outbox and blob cache (`TASK-98.6`): the outbox keeps every shared file until
//! it is released and never takes a private one; the cache is owner-only, evicts the least
//! recently used blob first under its cap, and never touches the outbox.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use org_proto::attachments::{ArtifactStatus, AttachmentTargetKind};
use org_store::Id;

use super::blobs::ContentHash;
use super::cache::{BlobCache, BLOB_CACHE_DIR};
use super::outbox::BlobOutbox;
use super::records::AttachmentRecord;

const OFFICE: &str = "0195a000-0000-7000-8000-00000000f001";
const UPLOADER: &str = "0195a000-0000-7000-8000-00000000f002";

fn record(id: &str, kind: AttachmentTargetKind, bytes: &[u8]) -> AttachmentRecord {
    AttachmentRecord {
        id: Id::parse(id).unwrap(),
        target_kind: kind,
        target_id: (kind == AttachmentTargetKind::UserPrivate)
            .then(|| Id::parse(UPLOADER).unwrap()),
        content_sha256: ContentHash::of(bytes),
        mime_type: "text/plain".into(),
        byte_count: bytes.len() as u64,
        filename: "notes.txt".into(),
        title: "Notes".into(),
        description: None,
        artifact_status: ArtifactStatus::default(),
        width: None,
        height: None,
        page_count: None,
        uploaded_by_employee_id: Id::parse(UPLOADER).unwrap(),
        created_at: "2026-11-26T10:00:00.000Z".into(),
        updated_at: "2026-11-26T10:00:00.000Z".into(),
    }
}

fn id(n: u8) -> String {
    format!("0195a000-0000-7000-8000-0000000001{n:02x}")
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn blob(len: usize, seed: u8) -> Vec<u8> {
    (0..len).map(|i| (i as u8).wrapping_add(seed)).collect()
}

#[test]
fn the_outbox_keeps_each_shared_file_until_it_is_released_and_never_takes_a_private_one() {
    let dir = tempfile::tempdir().unwrap();
    let durable = dir.path().join("durable");
    std::fs::create_dir_all(&durable).unwrap();
    let outbox = BlobOutbox::new(&durable, OFFICE);
    assert!(outbox
        .root()
        .ends_with(format!("local/offices/{OFFICE}/blob-outbox")));
    assert!(outbox.pending().unwrap().is_empty());

    let bytes = b"shared notes written offline".to_vec();
    let first = record(&id(1), AttachmentTargetKind::Organization, &bytes);
    let second = record(&id(2), AttachmentTargetKind::Organization, &bytes);
    outbox.enqueue(&second, &bytes).unwrap();
    outbox.enqueue(&first, &bytes).unwrap();
    let pending = outbox.pending().unwrap();
    assert_eq!(
        pending
            .iter()
            .map(|e| e.record.id.clone())
            .collect::<Vec<_>>(),
        [first.id.clone(), second.id.clone()],
        "oldest attachment first"
    );
    assert_eq!(pending[0].record, first);
    assert_eq!(
        outbox
            .blobs()
            .read_all(&first.content_sha256, 1024)
            .unwrap()
            .unwrap(),
        bytes
    );
    let blob_file = super::blobs::blob_path(outbox.root(), &first.content_sha256);
    assert_eq!(mode(&blob_file), 0o600);
    assert_eq!(mode(blob_file.parent().unwrap()), 0o700);
    assert_eq!(
        mode(&outbox.root().join(format!("entries/{}.yaml", first.id))),
        0o600
    );

    // Releasing one binding keeps the bytes the other still names.
    assert!(!outbox.release(&first.id).unwrap());
    assert!(!outbox.is_pending(&first.id));
    assert!(blob_file.is_file());
    assert!(outbox.release(&second.id).unwrap());
    assert!(!blob_file.exists());
    assert!(outbox.pending().unwrap().is_empty());
    assert!(
        !outbox.release(&second.id).unwrap(),
        "a second release is a no-op"
    );

    // A private file, or bytes that are not the record's, never enter.
    let private = record(&id(3), AttachmentTargetKind::UserPrivate, &bytes);
    assert!(outbox.enqueue(&private, &bytes).is_err());
    let lying = record(&id(4), AttachmentTargetKind::Organization, b"other bytes");
    assert!(outbox.enqueue(&lying, &bytes).is_err());
    assert!(outbox.pending().unwrap().is_empty());
    assert_eq!(outbox.blobs().count().unwrap(), 0);
}

#[test]
fn an_unreadable_outbox_entry_is_skipped_and_never_removed() {
    let dir = tempfile::tempdir().unwrap();
    let durable = dir.path().join("durable");
    std::fs::create_dir_all(&durable).unwrap();
    let outbox = BlobOutbox::new(&durable, OFFICE);
    let bytes = b"kept".to_vec();
    let good = record(&id(5), AttachmentTargetKind::Organization, &bytes);
    outbox.enqueue(&good, &bytes).unwrap();
    let entries = outbox.root().join("entries");
    // A link where an entry should be, garbage, and an entry filed under another ID.
    let outside = dir.path().join("outside.yaml");
    std::fs::write(&outside, super::records::encode(&good)).unwrap();
    std::os::unix::fs::symlink(&outside, entries.join(format!("{}.yaml", id(6)))).unwrap();
    std::fs::write(entries.join(format!("{}.yaml", id(7))), b"not: [yaml").unwrap();
    std::fs::copy(
        entries.join(format!("{}.yaml", good.id)),
        entries.join(format!("{}.yaml", id(8))),
    )
    .unwrap();
    let pending = outbox.pending().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].record.id, good.id);
    for n in [6, 7, 8] {
        let path = entries.join(format!("{}.yaml", id(n)));
        assert!(path.symlink_metadata().is_ok(), "{n} stays");
    }
    assert!(outside.is_file(), "a link is never followed");
}

#[test]
fn the_cache_evicts_least_recently_used_first_and_never_touches_the_outbox_or_hq_blobs() {
    let dir = tempfile::tempdir().unwrap();
    let durable = dir.path().join("durable");
    let runtime = dir.path().join("runtime");
    std::fs::create_dir_all(&durable).unwrap();
    std::fs::create_dir_all(&runtime).unwrap();
    // An un-acked file in the outbox and a blob in the shared store, both larger than the cap.
    let outbox = BlobOutbox::new(&durable, OFFICE);
    let unacked = blob(8 * 1024, 9);
    let pending = record(&id(9), AttachmentTargetKind::Organization, &unacked);
    outbox.enqueue(&pending, &unacked).unwrap();
    let shared = super::blobs::FsBlobStore::new(&durable, durable.join("blobs"));
    let hq_hash = super::blobs::BlobStore::put(&shared, &blob(8 * 1024, 10)).unwrap();

    let cache = BlobCache::new(&runtime, OFFICE);
    assert_eq!(cache.root(), runtime.join(OFFICE).join(BLOB_CACHE_DIR));
    let cap = 3 * 1024;
    let [a, b, c, d] = [1u8, 2, 3, 4].map(|seed| blob(1024, seed));
    let hash = |bytes: &[u8]| ContentHash::of(bytes);
    for bytes in [&a, &b, &c] {
        assert!(cache.insert(&hash(bytes), bytes, cap).unwrap().is_empty());
    }
    // Reading A makes it the most recently used: B is now the oldest.
    assert_eq!(
        cache.read_range(&hash(&a), 0, 10).unwrap().unwrap(),
        a[..10]
    );
    assert_eq!(cache.insert(&hash(&d), &d, cap).unwrap(), [hash(&b)]);
    let order: Vec<ContentHash> = cache
        .entries()
        .unwrap()
        .into_iter()
        .map(|e| e.hash)
        .collect();
    assert_eq!(order, [hash(&c), hash(&a), hash(&d)]);
    // A bigger blob pushes out the two oldest, and the order survives reopening the cache.
    let e = blob(2 * 1024, 5);
    assert_eq!(
        cache.insert(&hash(&e), &e, cap).unwrap(),
        [hash(&c), hash(&a)]
    );
    let reopened = BlobCache::new(&runtime, OFFICE);
    let order: Vec<ContentHash> = reopened
        .entries()
        .unwrap()
        .into_iter()
        .map(|e| e.hash)
        .collect();
    assert_eq!(order, [hash(&d), hash(&e)]);
    assert!(reopened.read_all(&hash(&b), 4096).unwrap().is_none());

    // Owner-only files and folders under the runtime's protected folder.
    let file = super::blobs::blob_path(cache.root(), &hash(&e));
    assert_eq!(mode(&file), 0o600);
    assert_eq!(mode(file.parent().unwrap()), 0o700);
    assert_eq!(mode(cache.root()), 0o700);

    // A cap below everything empties the cache; the outbox and HQ's store keep every byte.
    let trimmed = cache.trim(0).unwrap();
    assert_eq!(trimmed.len(), 2);
    assert_eq!(cache.count().unwrap(), 0);
    assert_eq!(
        outbox
            .blobs()
            .read_all(&pending.content_sha256, 1 << 20)
            .unwrap()
            .unwrap(),
        unacked
    );
    assert_eq!(outbox.pending().unwrap().len(), 1);
    assert!(super::blobs::BlobStore::size(&shared, &hq_hash)
        .unwrap()
        .is_some());
}

#[test]
fn a_blob_alone_over_the_cap_is_kept_by_its_own_insert_and_bad_bytes_never_enter() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = dir.path().join("runtime");
    std::fs::create_dir_all(&runtime).unwrap();
    let cache = BlobCache::new(&runtime, OFFICE);
    let big = blob(4096, 1);
    let small = blob(100, 2);
    cache
        .insert(&ContentHash::of(&small), &small, 1024)
        .unwrap();
    assert_eq!(
        cache.insert(&ContentHash::of(&big), &big, 1024).unwrap(),
        [ContentHash::of(&small)]
    );
    assert_eq!(cache.count().unwrap(), 1);
    assert!(cache
        .insert(&ContentHash::of(b"claimed"), &small, 1024)
        .is_err());
    assert_eq!(cache.count().unwrap(), 1);
}

#[test]
fn a_link_or_stray_file_in_the_cache_is_removed_and_never_followed() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = dir.path().join("runtime");
    std::fs::create_dir_all(&runtime).unwrap();
    let cache = BlobCache::new(&runtime, OFFICE);
    let kept = blob(64, 3);
    cache
        .insert(&ContentHash::of(&kept), &kept, 1 << 20)
        .unwrap();
    let target = dir.path().join("secret.txt");
    std::fs::write(&target, b"not the cache's").unwrap();
    let planted = ContentHash::of(b"planted");
    let link = super::blobs::blob_path(cache.root(), &planted);
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert!(
        cache.read_range(&planted, 0, 10).is_err(),
        "a link is never read through"
    );
    let stray = cache.root().join("sha256/stray.tmp");
    std::fs::write(&stray, b"half written").unwrap();
    let listed = cache.entries().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].hash, ContentHash::of(&kept));
    assert!(link.symlink_metadata().is_err(), "the link is gone");
    assert!(!stray.exists());
    assert_eq!(std::fs::read(&target).unwrap(), b"not the cache's");
}
