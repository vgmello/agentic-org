use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use crate::link::FIRST_FRAME_DEADLINE;
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

#[test]
fn staged_import_guard_unlinks_abandoned_directory_symlink_without_deleting_target() {
    let tmp = tempfile::tempdir().unwrap();
    let import_root = tmp.path().join("snapshot-import");
    create_private_dir(&import_root).unwrap();

    let victim_dir = tmp.path().join("victim-dir");
    create_private_dir(&victim_dir).unwrap();
    let sentinel = victim_dir.join("precious.txt");
    fs::write(&sentinel, b"must-survive").unwrap();

    let sym = import_root.join("abandoned-symlink");
    org_platform::fs::create_symlink(&victim_dir, &sym).unwrap();
    assert!(fs::symlink_metadata(&sym).is_ok());

    let guard = StagedImportGuard::open_reconciled(&import_root, "fresh-run").unwrap();
    assert!(
        fs::symlink_metadata(&sym).is_err(),
        "abandoned symlink entry must be unlinked"
    );
    assert!(
        victim_dir.is_dir(),
        "symlink target directory must NEVER be deleted"
    );
    assert_eq!(fs::read(&sentinel).unwrap(), b"must-survive");
    drop(guard);
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
fn a_team_homed_record_is_its_kind_and_id_wherever_its_team_stands() {
    use super::moves::team_homed_record;
    let (top, b, person, agent) = (uid(0x11), uid(0x31), uid(0x51), uid(0x52));
    let (hire, doc, task) = (uid(0x53), uid(0x54), uid(0x61));
    let b_folder = team_folder(&[0x11, 0x31]);
    let d_folder = team_folder(&[0x11, 0x33]);
    let key = |kind: &'static str, id: &str| Some((kind, id.to_owned()));
    for (path, record) in [
        (format!("state/teams/{top}/team.yaml"), key("team", &top)),
        (format!("{b_folder}/team.yaml"), key("team", &b)),
        (
            format!("{b_folder}/people/{person}/profile.md"),
            key("employee", &person),
        ),
        (
            format!("{b_folder}/agents/{agent}/profile.md"),
            key("employee", &agent),
        ),
        (
            format!("{b_folder}/agents/{agent}/execution.md"),
            key("agent_execution", &agent),
        ),
        (format!("{b_folder}/hires/{hire}.yaml"), key("hire", &hire)),
        (
            format!("{b_folder}/processes/{doc}.md"),
            key("process", &doc),
        ),
        (
            format!("{b_folder}/standards/{doc}.md"),
            key("standard", &doc),
        ),
    ] {
        assert_eq!(team_homed_record(&path), record, "{path}");
    }
    // An employee is one record wherever its home team stands (`MovePerson`, `MoveAgent`).
    for (at_b, at_d) in [
        (
            format!("{b_folder}/people/{person}/profile.md"),
            format!("{d_folder}/people/{person}/profile.md"),
        ),
        (
            format!("{b_folder}/agents/{agent}/execution.md"),
            format!("{d_folder}/agents/{agent}/execution.md"),
        ),
    ] {
        assert_eq!(team_homed_record(&at_b), team_homed_record(&at_d));
    }
    // A task folder file moves with its task, never as a record of its team's folder; and a
    // record not homed in a team is never one.
    for file in ["task.md", "checklist.yaml", "operations.yaml"] {
        let path = format!("{b_folder}/work/tasks/{task}/{file}");
        assert_eq!(team_homed_record(&path), None, "{path}");
    }
    for path in [
        "state/organization.yaml".to_owned(),
        format!("state/projects/{task}/project.yaml"),
        format!("state/processes/{doc}.md"),
        format!("state/projects/{task}/standards/{doc}.md"),
        format!("messaging/conversations/{task}/conversation.json"),
        format!("state/teams/{top}"),
        format!("state/teams/{top}/teams/{b}"),
    ] {
        assert_eq!(team_homed_record(&path), None, "{path}");
    }
}

/// Malformed and adversarial paths: none is a canonical durable path of a team-homed record.
fn adversarial_paths() -> Vec<String> {
    let (top, b, person, agent, hire) = (uid(0x11), uid(0x31), uid(0x51), uid(0x52), uid(0x53));
    let b_folder = team_folder(&[0x11, 0x31]);
    vec![
        "state/teams/../x/team.yaml".to_owned(),
        format!("state/teams/{top}/../{b}/team.yaml"),
        format!("state/teams/{top}/teams/../teams/{b}/team.yaml"),
        "state/teams/./team.yaml".to_owned(),
        format!("state/teams/{top}/./team.yaml"),
        format!("state/teams/evil/teams/{b}/team.yaml"),
        format!("state/teams/{b}/work/tasks/not-a-uuid/task.md"),
        format!("state/teams/{b}/team.yaml/"),
        format!("{b_folder}/people/{person}/profile.md/"),
        format!("state/teams/{top}/"),
        "state/teams//team.yaml".to_owned(),
        format!("state/teams/{top}//teams/{b}/team.yaml"),
        format!("/state/teams/{b}/team.yaml"),
        format!("state/teams/{}/team.yaml", uid(0xab).to_uppercase()),
        format!("state/teams/{b}/team.yaml\0"),
        "teams/x/team.yaml".to_owned(),
        format!("{b_folder}/people/not-a-uuid/profile.md"),
        format!("{b_folder}/robots/{person}/profile.md"),
        format!("{b_folder}/people/{person}/execution.md"),
        format!("{b_folder}/people/{person}/notes/profile.md"),
        format!("{b_folder}/agents/{agent}/execution.yaml"),
        format!("{b_folder}/hires/{hire}.md"),
        format!("{b_folder}/hires/not-a-uuid.yaml"),
        format!("{b_folder}/processes/not-a-uuid.md"),
        format!("{b_folder}/standards/{hire}.yaml"),
    ]
}

#[test]
fn a_team_homed_record_refuses_every_non_canonical_path() {
    use super::moves::team_homed_record;
    for path in adversarial_paths() {
        assert_eq!(team_homed_record(&path), None, "{path}");
    }
}

#[test]
fn a_non_canonical_local_path_is_never_paired_with_a_write() {
    use super::moves::superseded_paths;
    let (top, person, agent, hire) = (uid(0x11), uid(0x51), uid(0x52), uid(0x53));
    let new_b = team_folder(&[0x11, 0x33, 0x31]);
    // Every record an adversarial path names something like is written by the snapshot.
    let incoming: BTreeSet<String> = [
        format!("state/teams/{top}/team.yaml"),
        format!("{new_b}/team.yaml"),
        format!("{new_b}/people/{person}/profile.md"),
        format!("{new_b}/agents/{agent}/execution.md"),
        format!("{new_b}/hires/{hire}.yaml"),
        format!("state/teams/{}/team.yaml", uid(0x33)),
    ]
    .into_iter()
    .collect();
    let local = adversarial_paths();
    let superseded = superseded_paths(&local, &incoming, |_| true);
    assert!(superseded.is_empty(), "{superseded:?}");
}

#[test]
fn a_moved_employees_old_paths_are_named_wherever_its_home_team_went() {
    use super::moves::superseded_paths;
    let (person, agent, hire, process, standard) =
        (uid(0x51), uid(0x52), uid(0x53), uid(0x54), uid(0x55));
    // B moved under D (`MoveTeam`); the person moved from B to the top team (`MovePerson`) and
    // the agent from C to B (`MoveAgent`), in the same snapshot.
    let (top, old_b, old_c) = (
        team_folder(&[0x11]),
        team_folder(&[0x11, 0x31]),
        team_folder(&[0x11, 0x31, 0x32]),
    );
    let (new_b, new_c) = (
        team_folder(&[0x11, 0x33, 0x31]),
        team_folder(&[0x11, 0x33, 0x31, 0x32]),
    );
    let old_person = format!("{old_b}/people/{person}/profile.md");
    let local: Vec<String> = vec![
        old_person.clone(),
        format!("{old_c}/agents/{agent}/profile.md"),
        format!("{old_c}/agents/{agent}/execution.md"),
        format!("{old_c}/hires/{hire}.yaml"),
        format!("{old_b}/processes/{process}.md"),
        format!("{old_c}/standards/{standard}.md"),
        // Duplicate local entries name one removal each.
        old_person.clone(),
        format!("{old_c}/agents/{agent}/execution.md"),
    ];
    let incoming: BTreeSet<String> = [
        format!("{top}/people/{person}/profile.md"),
        format!("{new_b}/agents/{agent}/profile.md"),
        format!("{new_b}/agents/{agent}/execution.md"),
        format!("{new_c}/hires/{hire}.yaml"),
        format!("{new_b}/processes/{process}.md"),
        format!("{new_c}/standards/{standard}.md"),
    ]
    .into_iter()
    .collect();
    let superseded = superseded_paths(&local, &incoming, |_| true);
    let expected: BTreeMap<String, Vec<String>> = [
        (format!("{top}/people/{person}/profile.md"), old_person),
        (
            format!("{new_b}/agents/{agent}/profile.md"),
            format!("{old_c}/agents/{agent}/profile.md"),
        ),
        (
            format!("{new_b}/agents/{agent}/execution.md"),
            format!("{old_c}/agents/{agent}/execution.md"),
        ),
        (
            format!("{new_c}/hires/{hire}.yaml"),
            format!("{old_c}/hires/{hire}.yaml"),
        ),
        (
            format!("{new_b}/processes/{process}.md"),
            format!("{old_b}/processes/{process}.md"),
        ),
        (
            format!("{new_c}/standards/{standard}.md"),
            format!("{old_c}/standards/{standard}.md"),
        ),
    ]
    .into_iter()
    .map(|(new, old)| (new, vec![old]))
    .collect();
    assert_eq!(superseded, expected);
    // No batch is given one path twice.
    let named: Vec<&String> = superseded.values().flatten().collect();
    let unique: BTreeSet<&String> = named.iter().copied().collect();
    assert_eq!(named.len(), unique.len(), "{named:?}");
}

#[test]
fn a_person_or_an_agent_moved_alone_names_its_old_paths() {
    use super::moves::superseded_paths;
    let (person, agent, hire) = (uid(0x51), uid(0x52), uid(0x53));
    let (b, c, d) = (
        team_folder(&[0x11, 0x31]),
        team_folder(&[0x11, 0x31, 0x32]),
        team_folder(&[0x11, 0x33]),
    );
    // `MovePerson` (C to D) and `MoveAgent` (B to D) with no team moved: the agent's hire
    // request stays in B, where the snapshot writes it again.
    let local: Vec<String> = vec![
        format!("{c}/people/{person}/profile.md"),
        format!("{b}/agents/{agent}/profile.md"),
        format!("{b}/agents/{agent}/execution.md"),
        format!("{b}/hires/{hire}.yaml"),
    ];
    let incoming: BTreeSet<String> = [
        format!("{d}/people/{person}/profile.md"),
        format!("{d}/agents/{agent}/profile.md"),
        format!("{d}/agents/{agent}/execution.md"),
        format!("{b}/hires/{hire}.yaml"),
    ]
    .into_iter()
    .collect();
    let expected: BTreeMap<String, Vec<String>> = [
        (
            format!("{d}/people/{person}/profile.md"),
            format!("{c}/people/{person}/profile.md"),
        ),
        (
            format!("{d}/agents/{agent}/profile.md"),
            format!("{b}/agents/{agent}/profile.md"),
        ),
        (
            format!("{d}/agents/{agent}/execution.md"),
            format!("{b}/agents/{agent}/execution.md"),
        ),
    ]
    .into_iter()
    .map(|(new, old)| (new, vec![old]))
    .collect();
    assert_eq!(superseded_paths(&local, &incoming, |_| true), expected);
}

#[test]
fn a_record_the_snapshot_writes_at_two_paths_is_left_to_reconciliation() {
    use super::moves::superseded_paths;
    let person = uid(0x51);
    let local = vec![format!(
        "{}/people/{person}/profile.md",
        team_folder(&[0x11, 0x31])
    )];
    let incoming: BTreeSet<String> = [&[0x11u64, 0x33][..], &[0x11, 0x34]]
        .into_iter()
        .map(|chain| format!("{}/people/{person}/profile.md", team_folder(chain)))
        .collect();
    assert!(superseded_paths(&local, &incoming, |_| true).is_empty());
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
