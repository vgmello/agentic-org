use super::*;
use std::os::unix::fs::{symlink, PermissionsExt};

fn sample_pin(id: &str, seq: u64) -> ReleasePin {
    ReleasePin {
        release_id: id.into(),
        manifest_sha256: format!("{seq:064x}"),
        sequence: seq,
    }
}

#[test]
fn persists_pending_checkpoint_confirm_and_quarantine_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().canonicalize().unwrap().join("updater");
    let store = UpdateStateStore::open(&state_dir).unwrap();
    let mode = fs::symlink_metadata(&state_dir)
        .unwrap()
        .permissions()
        .mode()
        & 0o7777;
    assert_eq!(mode, 0o700);

    let v1 = sample_pin("v0.1.0", 1);
    let v2 = sample_pin("v0.2.0", 2);

    let pending = store
        .record_pending(
            Some(v1.clone()),
            v2.clone(),
            Some("https://example.com/pointer.v1.json".into()),
        )
        .unwrap();
    assert_eq!(
        pending.pending_health_check.as_ref().unwrap().target_pin,
        v2
    );

    let rolled = store
        .record_rollback_and_quarantine(
            &v2,
            Some(&v1),
            "post-restart health failed with Authorization: Bearer secretToken12345",
        )
        .unwrap();
    assert!(rolled.pending_health_check.is_none());
    assert_eq!(rolled.confirmed, Some(v1.clone()));
    let q = rolled.quarantined_entry("v0.2.0").unwrap();
    assert!(!q.reason.contains("secretToken12345"), "{}", q.reason);
    assert!(q.reason.contains("[REDACTED]"), "{}", q.reason);

    let reloaded = UpdateStateStore::read_readonly(&state_dir).unwrap();
    assert_eq!(reloaded, rolled);

    let v3 = sample_pin("v0.3.0", 3);
    let confirmed = store.confirm_healthy(&v3).unwrap();
    assert_eq!(confirmed.confirmed, Some(v3));
    assert!(confirmed.pending_health_check.is_none());
    assert!(confirmed.quarantined_entry("v0.2.0").is_some());
}

#[test]
fn refuses_symlink_state_dir_symlink_file_and_corrupt_json() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let real = root.join("real");
    fs::create_dir(&real).unwrap();
    let linked_dir = root.join("linked");
    symlink(&real, &linked_dir).unwrap();
    assert!(matches!(
        UpdateStateStore::open(&linked_dir),
        Err(Error::UnsafeInput)
    ));
    // Trailing slash on symlinked dir must also be refused.
    assert!(matches!(
        UpdateStateStore::open(Path::new(&format!("{}/", linked_dir.display()))),
        Err(Error::UnsafeInput)
    ));

    let state_dir = root.join("state");
    let store = UpdateStateStore::open(&state_dir).unwrap();
    let outside = root.join("outside.json");
    fs::write(&outside, b"{}").unwrap();
    symlink(&outside, state_dir.join(STATE_FILE_NAME)).unwrap();
    assert!(matches!(store.load(), Err(Error::UnsafeInput)));
    fs::remove_file(state_dir.join(STATE_FILE_NAME)).unwrap();

    fs::write(
        state_dir.join(STATE_FILE_NAME),
        br#"{"version":1,"extra":true}"#,
    )
    .unwrap();
    assert!(matches!(store.load(), Err(Error::InvalidRelease)));
}
