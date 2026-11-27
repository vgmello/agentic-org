use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use crate::federation_link::FIRST_FRAME_DEADLINE;
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

/// A stable ID for the `n`th team, employee or task of a test.
fn uid(n: u64) -> String {
    format!("01900000-0000-7000-8000-{n:012x}")
}

/// The folder of the team chain `chain`.
fn team_folder(chain: &[u64]) -> String {
    let mut folder = String::from("state");
    for team in chain {
        folder.push_str(&format!("/teams/{}", uid(*team)));
    }
    folder
}

#[test]
fn a_team_homed_record_is_its_team_and_its_path_below_the_teams_folder() {
    use super::moves::team_homed_record;
    let (top, b, person, task) = (uid(0x11), uid(0x31), uid(0x51), uid(0x61));
    let b_folder = team_folder(&[0x11, 0x31]);
    for (path, record) in [
        (
            format!("state/teams/{top}/team.yaml"),
            Some((top.as_str(), "team.yaml")),
        ),
        (
            format!("{b_folder}/team.yaml"),
            Some((b.as_str(), "team.yaml")),
        ),
        (
            format!("{b_folder}/people/{person}/profile.md"),
            Some((b.as_str(), &*format!("people/{person}/profile.md"))),
        ),
        (
            format!("{b_folder}/hires/{person}.yaml"),
            Some((b.as_str(), &*format!("hires/{person}.yaml"))),
        ),
    ] {
        assert_eq!(team_homed_record(&path), record, "{path}");
    }
    // A task folder file moves with its task, never as a record of its team's folder.
    for file in ["task.md", "checklist.yaml", "operations.yaml"] {
        let path = format!("{b_folder}/work/tasks/{task}/{file}");
        assert_eq!(team_homed_record(&path), None, "{path}");
    }
    for path in [
        "state/organization.yaml".to_owned(),
        format!("state/projects/{task}/project.yaml"),
        format!("messaging/conversations/{task}/conversation.json"),
        format!("state/teams/{top}"),
        format!("state/teams/{top}/"),
        "state/teams//team.yaml".to_owned(),
        format!("state/teams/{top}/teams/{b}"),
        "teams/x/team.yaml".to_owned(),
    ] {
        assert_eq!(team_homed_record(&path), None, "{path}");
    }
}

#[test]
fn a_moved_records_old_paths_are_named_by_the_path_the_snapshot_writes() {
    use super::moves::superseded_paths;
    let (person, gone, task) = (uid(0x51), uid(0x52), uid(0x61));
    // Before HQ moved B under D: B, C below it, a profile homed in B and a task of B.
    let old_b = team_folder(&[0x11, 0x31]);
    let old_c = team_folder(&[0x11, 0x31, 0x32]);
    let new_b = team_folder(&[0x11, 0x33, 0x31]);
    let new_c = team_folder(&[0x11, 0x33, 0x31, 0x32]);
    let top = team_folder(&[0x11]);
    let stale_copy = format!("{}/team.yaml", team_folder(&[0x11, 0x34, 0x31]));
    let protected = format!("{old_b}/hires/{person}.yaml");
    let local: Vec<String> = vec![
        format!("{top}/team.yaml"),
        format!("{old_b}/team.yaml"),
        format!("{old_c}/team.yaml"),
        format!("{old_b}/people/{person}/profile.md"),
        protected.clone(),
        format!("{old_b}/people/{gone}/profile.md"),
        format!("{old_b}/work/tasks/{task}/task.md"),
        stale_copy.clone(),
    ];
    let incoming: BTreeSet<String> = [
        format!("{top}/team.yaml"),
        format!("{new_b}/team.yaml"),
        format!("{new_c}/team.yaml"),
        format!("{new_b}/people/{person}/profile.md"),
        format!("{new_b}/hires/{person}.yaml"),
        format!("{new_b}/work/tasks/{task}/task.md"),
    ]
    .into_iter()
    .collect();
    let superseded = superseded_paths(&local, &incoming, |path| path != protected);
    let expected: BTreeMap<String, Vec<String>> = [
        (
            format!("{new_b}/team.yaml"),
            vec![format!("{old_b}/team.yaml"), stale_copy],
        ),
        (
            format!("{new_c}/team.yaml"),
            vec![format!("{old_c}/team.yaml")],
        ),
        (
            format!("{new_b}/people/{person}/profile.md"),
            vec![format!("{old_b}/people/{person}/profile.md")],
        ),
    ]
    .into_iter()
    .collect();
    // Unchanged paths, a record HQ no longer has (left to reconciliation), a file
    // reconciliation keeps and a task folder file are never named.
    assert_eq!(superseded, expected);

    // A path the snapshot writes itself is never superseded, even when the same record is
    // written at another chain too.
    let mut both = incoming.clone();
    both.insert(format!("{old_b}/team.yaml"));
    let superseded = superseded_paths(&local, &both, |_| true);
    assert!(superseded.values().flatten().all(|old| !both.contains(old)));
}
