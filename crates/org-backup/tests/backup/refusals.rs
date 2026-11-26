use std::fs::OpenOptions;
use std::path::Path;
use std::sync::Arc;

use org_backup::{
    backup, restore_in_place, verify, BackupError, BackupRequest, Limits, LiveJournal,
    RestoreRequest,
};
use org_office::{Custody, Office};
use org_store::testkit::FakeCustody;
use org_store::{journal_dir, Journal, JournalHead};
use serde_json::{json, Value};

use crate::round_trip::{mkfifo, take_backup, verify_request};
use crate::support::{tree, Org};

fn backup_of(org: &Org, out: &std::path::Path) -> Result<org_backup::BackupReport, BackupError> {
    backup(&BackupRequest {
        office_home: &org.office_home,
        org: &org.org_id,
        office: None,
        out,
        passphrase: None,
    })
}

fn restore_of(
    org: &Org,
    archive: &std::path::Path,
) -> Result<org_backup::RestoreReport, BackupError> {
    restore_in_place(&RestoreRequest {
        archive,
        passphrase: None,
        office_home: &org.office_home,
        org: &org.org_id,
        office: None,
        limits: Limits::default(),
    })
}

#[test]
fn a_running_office_or_host_is_never_backed_up_or_restored_over() {
    let org = Org::seeded();
    let archive = org.root.join("ok.tar.gz");
    take_backup(&org, &archive, None);
    let inode = |path: &std::path::Path| {
        std::os::unix::fs::MetadataExt::ino(&std::fs::metadata(path).unwrap())
    };
    let durable_inode = inode(org.durable());

    // The office server holds office.lock while it runs.
    let office = org.open();
    let out = org.root.join("live.tar.gz");
    let refused = backup_of(&org, &out).unwrap_err();
    assert!(matches!(refused, BackupError::Live { .. }), "{refused}");
    assert!(refused.to_string().contains("office.lock"), "{refused}");
    assert!(refused.to_string().contains("stop it first"), "{refused}");
    assert!(!out.exists());
    let refused = restore_of(&org, &archive).unwrap_err();
    assert!(matches!(refused, BackupError::Live { .. }), "{refused}");
    office.close().unwrap();
    // Nothing was swapped in or staged beside the live root.
    assert_eq!(inode(org.durable()), durable_inode);
    let base = org.durable().parent().unwrap();
    assert_eq!(std::fs::read_dir(base).unwrap().count(), 2);
    let before = crate::support::tree(org.durable());

    // The host holds host.lock while it serves the home.
    let host = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(org.office_home.join("host.lock"))
        .unwrap();
    host.try_lock().unwrap();
    let refused = backup_of(&org, &out).unwrap_err();
    assert!(refused.to_string().contains("host.lock"), "{refused}");
    let refused = restore_of(&org, &archive).unwrap_err();
    assert!(matches!(refused, BackupError::Live { .. }), "{refused}");
    host.unlock().unwrap();
    assert!(!out.exists());
    assert_eq!(crate::support::tree(org.durable()), before);
}

#[test]
fn an_archive_behind_the_offices_journal_is_refused_rather_than_resetting_custody() {
    let org = Org::seeded();
    let archive = org.root.join("older.tar.gz");
    let older = take_backup(&org, &archive, None);
    // The office accepts more work after the backup; the host confirms the newer head.
    let office = org.open();
    office
        .task_create(
            &org.owner,
            serde_json::from_value(json!({
                "command_id": office.next_uuid_v7(), "task_id": office.next_uuid_v7(),
                "team_id": office.org_snapshot().unwrap().teams[0].id,
                "title": "After the backup",
            }))
            .unwrap(),
        )
        .unwrap()
        .unwrap();
    office.close().unwrap();
    let before = crate::support::tree(org.durable());
    let refused = restore_of(&org, &archive).unwrap_err();
    let text = refused.to_string();
    assert!(matches!(refused, BackupError::Refused(_)), "{text}");
    assert!(text.contains("writer custody"), "{text}");
    assert!(
        text.contains(&format!("ordinal {}", older.manifest.journal.head_ordinal)),
        "{text}"
    );
    // Nothing changed, and no staging folder is left behind.
    assert_eq!(crate::support::tree(org.durable()), before);
    let base = org.durable().parent().unwrap();
    let names: Vec<String> = std::fs::read_dir(base)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
    org.open().close().unwrap();
}

#[test]
fn a_link_in_the_durable_root_fails_the_backup_instead_of_being_followed_or_skipped() {
    let org = Org::seeded();
    let secret = org.root.join("outside-secret");
    std::fs::write(&secret, b"not organization data").unwrap();
    let office = format!("local/offices/{}", org.office_id);
    // Only inside an agent's own home is a link left out; everywhere else it fails the backup,
    // including an agent's folder beside its home and a home that is itself a link.
    let planted = [
        "state/planted".to_owned(),
        "messaging/planted".to_owned(),
        "recovery/planted".to_owned(),
        "private/planted".to_owned(),
        format!("{office}/planted"),
        format!("{office}/agents/{}/planted", org.agent),
        format!("{office}/agents/019a0000-0000-7000-8000-0000000000c1/home"),
    ];
    let out = org.root.join("linked.tar.gz");
    for at in &planted {
        let link = org.durable().join(at);
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&secret, &link).unwrap();
        let refused = backup_of(&org, &out).unwrap_err();
        std::fs::remove_file(&link).unwrap();
        assert!(
            matches!(refused, BackupError::Refused(_)),
            "{at}: {refused}"
        );
        assert!(
            refused.to_string().contains(&format!("a link at {at}")),
            "{at}: {refused}"
        );
        assert!(!out.exists());
    }
    let fifo = org.durable().join("state/pipe");
    mkfifo(&fifo);
    let refused = backup_of(&org, &out).unwrap_err();
    assert!(
        refused.to_string().contains("a special file at state/pipe"),
        "{refused}"
    );
    std::fs::remove_file(&fifo).unwrap();
    let leftovers: Vec<_> = std::fs::read_dir(&org.root)
        .unwrap()
        .filter_map(|e| e.unwrap().file_name().into_string().ok())
        .filter(|n| n.contains(".part-"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    // With nothing planted, the same office backs up.
    backup_of(&org, &out).expect("a backup");
}

/// The office folder's record of the writer the host reserved for it.
fn intent(org: &Org) -> std::path::PathBuf {
    org.folder.join("hq-creation.v1.json")
}

/// Rewrites one field of the reservation the office folder records.
fn reserve(org: &Org, field: &str, value: &Value) {
    let path = intent(org);
    let mut recorded: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert!(recorded.get(field).is_some(), "the intent has no {field}");
    recorded[field] = value.clone();
    std::fs::write(&path, serde_json::to_vec(&recorded).unwrap()).unwrap();
}

/// The sorted names in `folder`.
fn names(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[test]
fn an_archive_bound_to_another_writer_is_refused_whether_or_not_the_live_root_survives() {
    let org = Org::seeded();
    let archive = org.root.join("bound.tar.gz");
    take_backup(&org, &archive, None);
    let original = std::fs::read(intent(&org)).unwrap();
    let other = json!("019a0000-0000-7000-8000-0000000000b1");
    let cases = [
        (
            "writer_epoch",
            json!(org.custody.identity.writer_epoch + 1),
            "another writer epoch",
        ),
        ("machine_id", other.clone(), "another machine"),
        ("instance", other.clone(), "another instance"),
        ("journal_incarnation", other, "journal incarnation"),
    ];
    let base = org.durable().parent().unwrap().to_path_buf();
    for lost in [false, true] {
        if lost {
            std::fs::rename(org.durable(), org.root.join("lost-durable")).unwrap();
        }
        let listing = names(&base);
        let before = (!lost).then(|| tree(org.durable()));
        for (field, value, why) in &cases {
            reserve(&org, field, value);
            let refused = restore_of(&org, &archive).unwrap_err();
            std::fs::write(intent(&org), &original).unwrap();
            let text = refused.to_string();
            assert!(
                matches!(refused, BackupError::Refused(_)),
                "{field}: {text}"
            );
            assert!(text.contains(why), "{field}: {text}");
            assert!(text.contains("writer custody"), "{field}: {text}");
            // Nothing was swapped in or left staged.
            assert_eq!(names(&base), listing, "{field}");
            assert_eq!(before, (!lost).then(|| tree(org.durable())), "{field}");
        }
    }
    // Under the reservation as recorded, the same archive restores over the lost root.
    let restored = restore_of(&org, &archive).expect("it restores");
    assert_eq!(restored.live_journal, LiveJournal::Missing);
    org.open().close().unwrap();
}

/// The head of the office's journal as it is on disk.
fn journal_head(org: &Org) -> JournalHead {
    let seen = Journal::inspect(&journal_dir(org.durable(), &org.custody.identity), None)
        .expect("the journal verifies");
    JournalHead {
        ordinal: seen.head.ordinal,
        sha256: seen.head.sha256,
        high_waters: seen.head.high_waters,
    }
}

/// The office accepts one new task.
fn accept_a_task(org: &Org, office: &Office, title: &str) {
    office
        .task_create(
            &org.owner,
            serde_json::from_value(json!({
                "command_id": office.next_uuid_v7(), "task_id": office.next_uuid_v7(),
                "team_id": office.org_snapshot().unwrap().teams[0].id, "title": title,
            }))
            .unwrap(),
        )
        .unwrap()
        .unwrap();
}

#[test]
fn an_archive_forked_from_the_offices_journal_at_the_same_ordinal_is_refused() {
    let org = Org::seeded();
    let fork = org.root.join("fork.tar.gz");
    take_backup(&org, &fork, None);
    let at_fork = journal_head(&org);
    let pristine = org.root.join("pristine");
    verify(&verify_request(&fork, None, Some(&pristine))).expect("it verifies");

    // One branch: the office accepts a task, and is backed up.
    let office = org.open();
    accept_a_task(&org, &office, "Branch one");
    office.close().unwrap();
    let branch = org.root.join("branch-one.tar.gz");
    let one = take_backup(&org, &branch, None);

    // The other: the root goes back to the fork point under a custody that last confirmed
    // it, and the office accepts another task at the same ordinal.
    std::fs::rename(org.durable(), org.root.join("branch-one-root")).unwrap();
    std::fs::rename(pristine.join("durable"), org.durable()).unwrap();
    org_office::backup::discard_projections(org.runtime(), &org.office_id).unwrap();
    let custody = FakeCustody::new();
    custody.hold(&org.custody.identity, at_fork);
    let office = Office::open(
        &org.folder,
        Some(org.key.clone()),
        org.custody.identity.instance_id.as_str(),
        org.roots.clone(),
        Custody {
            identity: org.custody.identity.clone(),
            authority: Arc::new(custody),
        },
    )
    .expect("the office opens at the fork point");
    accept_a_task(&org, &office, "Branch two");
    office.close().unwrap();
    let live = journal_head(&org);
    assert_eq!(live.ordinal, one.manifest.journal.head_ordinal);
    assert_ne!(live.sha256.to_hex(), one.manifest.journal.head_sha256);

    let before = tree(org.durable());
    let refused = restore_of(&org, &branch).unwrap_err();
    let text = refused.to_string();
    assert!(matches!(refused, BackupError::Refused(_)), "{text}");
    assert!(text.contains("not on the chain"), "{text}");
    assert!(
        text.contains(&format!("differ at ordinal {}", live.ordinal)),
        "{text}"
    );
    assert!(text.contains("writer custody"), "{text}");
    assert_eq!(tree(org.durable()), before);
}

#[test]
fn a_passphrase_file_that_is_not_owner_only_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pass");
    std::fs::write(&path, b"correct horse battery staple\n").unwrap();
    for mode in [0o640, 0o604, 0o4600] {
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        let set = std::fs::metadata(&path).unwrap().permissions().mode() & 0o7777;
        if set == 0o600 {
            // The file system dropped a bit this account may not set; nothing to check.
            continue;
        }
        let refused = org_backup::Passphrase::from_file(&path).unwrap_err();
        assert!(
            matches!(refused, BackupError::Usage(_)),
            "{mode:o}: {refused}"
        );
        assert!(
            refused.to_string().contains("chmod 600"),
            "{mode:o}: {refused}"
        );
        assert!(!refused.to_string().contains("horse"));
    }
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    org_backup::Passphrase::from_file(&path).expect("an owner-only passphrase file");
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    assert!(org_backup::Passphrase::from_file(&link).is_err());
}

#[test]
fn an_existing_output_an_unknown_organization_and_a_weak_passphrase_are_refused() {
    let org = Org::seeded();
    let out = org.root.join("exists.tar.gz");
    std::fs::write(&out, b"keep me").unwrap();
    assert!(matches!(backup_of(&org, &out), Err(BackupError::Usage(_))));
    assert_eq!(std::fs::read(&out).unwrap(), b"keep me");
    let other = backup(&BackupRequest {
        office_home: &org.office_home,
        org: "019a0000-0000-7000-8000-000000000999",
        office: None,
        out: &org.root.join("none.tar.gz"),
        passphrase: None,
    });
    assert!(matches!(other, Err(BackupError::NotFound(_))), "{other:?}");
    let slug = backup(&BackupRequest {
        office_home: &org.office_home,
        org: "../acme",
        office: None,
        out: &org.root.join("none.tar.gz"),
        passphrase: None,
    });
    assert!(matches!(slug, Err(BackupError::Usage(_))), "{slug:?}");
    let inside = org.durable().join("state/backup.tar.gz");
    assert!(matches!(
        backup_of(&org, &inside),
        Err(BackupError::Usage(_))
    ));
    assert!(!inside.exists());
    let weak = org_backup::Passphrase::new(b"short".to_vec()).unwrap();
    let refused = backup(&BackupRequest {
        office_home: &org.office_home,
        org: &org.org_id,
        office: None,
        out: &org.root.join("weak.tar.gz"),
        passphrase: Some(&weak),
    });
    assert!(matches!(refused, Err(BackupError::Usage(_))), "{refused:?}");
}
