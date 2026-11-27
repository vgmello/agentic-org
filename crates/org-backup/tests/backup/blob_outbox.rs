//! A satellite's attachment outbox (`TASK-98.6`): `local/offices/<office-id>/blob-outbox/` holds
//! the only copy of shared files HQ has not acknowledged yet, so a backup carries it (unlike the
//! disposable `attachment-staging/`) and a restore brings it back byte for byte. Being a
//! companion store, it never trips the office store's scan for stray entries. The pulled-blob
//! cache lives in the runtime root, which no backup carries.

use org_backup::{restore_in_place, verify, Limits, RestoreRequest};

use crate::round_trip::{take_backup, verify_request};
use crate::support::Org;

const SHA: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0";
const ENTRY: &str = "019a0000-0000-7000-8000-0000000000f1";

#[test]
fn an_unacknowledged_outbox_file_is_carried_restored_and_never_a_stray() {
    let org = Org::seeded();
    let local = org.durable().join("local/offices").join(&org.office_id);
    let outbox = local.join(org_office::companion_paths::BLOB_OUTBOX_DIR_NAME);
    let blob = outbox
        .join("sha256")
        .join(&SHA[0..2])
        .join(&SHA[2..4])
        .join(SHA);
    let entry = outbox.join("entries").join(format!("{ENTRY}.yaml"));
    std::fs::create_dir_all(blob.parent().unwrap()).unwrap();
    std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
    std::fs::write(&blob, b"bytes HQ has not acked").unwrap();
    std::fs::write(&entry, b"schema_version: 1\n").unwrap();
    std::fs::create_dir_all(local.join("attachment-staging/x")).unwrap();
    std::fs::write(local.join("attachment-staging/x/data"), b"disposable").unwrap();
    let cache = org
        .runtime()
        .join(&org.office_id)
        .join(org_office::attachments::cache::BLOB_CACHE_DIR);
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("pulled"), b"cached from HQ").unwrap();
    // A companion store, not a stray: the office opens over it.
    org.open().close().unwrap();

    let out = org.root.join("outbox.tar.gz");
    let report = take_backup(&org, &out, None);
    let files: Vec<&str> = report
        .manifest
        .files
        .iter()
        .map(|f| f.path.as_str())
        .collect();
    let prefix = format!("local/offices/{}/blob-outbox/", org.office_id);
    assert!(
        files.contains(&format!("{prefix}sha256/{}/{}/{SHA}", &SHA[0..2], &SHA[2..4]).as_str()),
        "{files:?}"
    );
    assert!(files.contains(&format!("{prefix}entries/{ENTRY}.yaml").as_str()));
    assert!(!files.iter().any(|p| p.contains("attachment-staging")));
    assert!(!files.iter().any(|p| p.contains("blob-cache")));
    verify(&verify_request(&out, None, None)).expect("it verifies");

    std::fs::remove_dir_all(org.durable()).unwrap();
    restore_in_place(&RestoreRequest {
        archive: &out,
        passphrase: None,
        office_home: &org.office_home,
        org: &org.org_id,
        office: None,
        limits: Limits::default(),
    })
    .expect("it restores");
    assert_eq!(std::fs::read(&blob).unwrap(), b"bytes HQ has not acked");
    assert_eq!(std::fs::read(&entry).unwrap(), b"schema_version: 1\n");
    org.open().close().unwrap();
}
