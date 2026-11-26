use std::fs::OpenOptions;

use org_backup::{backup, restore_in_place, BackupError, BackupRequest, Limits, RestoreRequest};
use serde_json::json;

use crate::round_trip::take_backup;
use crate::support::Org;

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
    std::os::unix::fs::symlink(&secret, org.durable().join("state/planted")).unwrap();
    let out = org.root.join("linked.tar.gz");
    let refused = backup_of(&org, &out).unwrap_err();
    assert!(matches!(refused, BackupError::Refused(_)), "{refused}");
    assert!(
        refused.to_string().contains("a link at state/planted"),
        "{refused}"
    );
    assert!(!out.exists());
    let leftovers: Vec<_> = std::fs::read_dir(&org.root)
        .unwrap()
        .filter_map(|e| e.unwrap().file_name().into_string().ok())
        .filter(|n| n.contains(".part-"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
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
