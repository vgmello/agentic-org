use std::fs;

use crate::federation::FIRST_FRAME_DEADLINE;
use org_sync_proto::SyncFrame;

use super::staging::StagedImportGuard;
use super::*;

#[test]
fn budget_enforces_concurrency_and_staged_bytes_and_releases_on_drop() {
    let budget = HqSnapshotTransferBudget::with_limits(2, 1000);
    let mut p1 = budget.try_begin().expect("first permit");
    let mut p2 = budget.try_begin().expect("second permit");
    assert!(budget.try_begin().is_none(), "third permit must fail");

    assert!(p1.try_reserve_bytes(600));
    assert!(p2.try_reserve_bytes(400));
    assert!(!p2.try_reserve_bytes(1), "byte cap must reject overflow");

    drop(p1);
    let mut p3 = budget.try_begin().expect("released concurrency slot");
    assert!(p3.try_reserve_bytes(600), "released bytes available again");
    assert!(!p3.try_reserve_bytes(1));
}

#[test]
fn legacy_request_without_supports_paging_deserializes_false() {
    let legacy_json = r#"{"type":"org_snapshot_request","after_epoch":1,"after_seq":0}"#;
    let frame: SyncFrame = serde_json::from_str(legacy_json).unwrap();
    assert!(matches!(
        frame,
        SyncFrame::OrgSnapshotRequest {
            supports_paging: false,
            page_index: 0,
            snapshot_id: None,
            ..
        }
    ));
}

#[test]
fn shared_page_and_chunk_bounds_match_identically() {
    assert_eq!(
        (MAX_SNAPSHOT_PAGE_CHUNKS as usize) * SNAPSHOT_CHUNK_RAW_BYTES,
        SNAPSHOT_PAGE_MAX_BYTES
    );
    assert!(SNAPSHOT_PREPARE_KEEPALIVE_INTERVAL < FIRST_FRAME_DEADLINE);
    assert!(SNAPSHOT_PREPARE_DEADLINE > FIRST_FRAME_DEADLINE);
}

#[test]
fn staged_import_guard_reconciles_abandoned_dirs_without_removing_active_siblings() {
    let tmp = tempfile::tempdir().unwrap();
    let import_root = tmp.path().join("snapshot-import");
    let abandoned = import_root.join("abandoned-from-crash");
    create_private_dir(&abandoned).unwrap();
    write_private_bytes(&abandoned.join("page-000000.json"), b"[]").unwrap();

    let active_a = StagedImportGuard::open_reconciled(&import_root, "run-a").unwrap();
    assert!(
        !abandoned.exists(),
        "abandoned prior import directory must be purged on next import"
    );
    assert!(active_a.path().exists());

    // Opening a second import while `active_a` is live must NOT remove `active_a`.
    let active_b = StagedImportGuard::open_reconciled(&import_root, "run-b").unwrap();
    assert!(active_a.path().exists());
    assert!(active_b.path().exists());

    let path_a = active_a.path().to_path_buf();
    let path_b = active_b.path().to_path_buf();
    drop(active_a);
    drop(active_b);
    assert!(!path_a.exists());
    assert!(!path_b.exists());
}

#[test]
fn staged_import_guard_fails_closed_when_abandoned_cleanup_fails() {
    use std::os::unix::fs::PermissionsExt as _;
    let tmp = tempfile::tempdir().unwrap();
    let import_root = tmp.path().join("snapshot-import");
    let stuck = import_root.join("stuck-abandoned");
    create_private_dir(&stuck).unwrap();
    write_private_bytes(&stuck.join("page-000000.json"), b"[]").unwrap();
    // Make the abandoned child directory non-writable so `remove_dir_all` fails.
    fs::set_permissions(&stuck, fs::Permissions::from_mode(0o500)).unwrap();

    let err = StagedImportGuard::open_reconciled(&import_root, "next-run")
        .err()
        .expect("failed abandoned staging cleanup must fail closed");
    assert!(
        err.contains("failed reconciling abandoned snapshot staging"),
        "unexpected error: {err}"
    );

    // Restore mode so tempdir cleanup succeeds.
    fs::set_permissions(&stuck, fs::Permissions::from_mode(0o700)).unwrap();
}
