use std::path::Path;

use org_backup::{
    backup, restore_in_place, verify, BackupRequest, Limits, LiveJournal, Passphrase,
    RestoreRequest, VerifyRequest,
};
use org_office::backup::{rebuild_disposable, DisposableRebuild};

use crate::support::{copy_tree, tree, Org};

pub fn passphrase() -> Passphrase {
    Passphrase::new(b"correct horse battery staple\n".to_vec()).unwrap()
}

pub fn take_backup(
    org: &Org,
    out: &Path,
    passphrase: Option<&Passphrase>,
) -> org_backup::BackupReport {
    backup(&BackupRequest {
        office_home: &org.office_home,
        org: &org.org_id,
        office: None,
        out,
        passphrase,
    })
    .expect("a backup")
}

pub fn verify_request<'a>(
    archive: &'a Path,
    passphrase: Option<&'a Passphrase>,
    to: Option<&'a Path>,
) -> VerifyRequest<'a> {
    VerifyRequest {
        archive,
        passphrase,
        to,
        work_dir: None,
        limits: Limits::default(),
    }
}

/// Every row of the projection rebuilt from a copy of `durable`.
fn rebuilt_rows(durable: &Path, org: &Org) -> Vec<String> {
    let work = tempfile::tempdir().unwrap();
    let copy = work.path().canonicalize().unwrap().join("durable");
    copy_tree(durable, &copy);
    let runtime = copy.with_file_name("runtime");
    std::fs::create_dir(&runtime).unwrap();
    let spec = DisposableRebuild {
        durable_root: copy,
        runtime_root: runtime,
        identity: org.custody.identity.clone(),
        pinned: None,
        is_hq: true,
    };
    rebuild_disposable(&spec, |store| Ok(store.dump()))
        .unwrap()
        .1
}

fn round_trip(encrypted: bool) {
    let org = Org::seeded();
    let before = {
        let office = org.open();
        let answers = org.queries(&office);
        office.close().unwrap();
        answers
    };
    let original_rows = rebuilt_rows(org.durable(), &org);
    let out = org.root.join("org.backup.tar.gz");
    let key = passphrase();
    let pass = encrypted.then_some(&key);
    let report = take_backup(&org, &out, pass);
    assert_eq!(report.encrypted, encrypted);
    assert_eq!(report.manifest.org_id, org.org_id);
    assert_eq!(report.manifest.office_id, org.office_id);
    assert!(report.manifest.journal.head_ordinal > 0);
    let mode = std::fs::metadata(&out).unwrap();
    assert_eq!(
        std::os::unix::fs::PermissionsExt::mode(&mode.permissions()) & 0o777,
        0o600
    );
    let paths: Vec<&str> = report
        .manifest
        .files
        .iter()
        .map(|f| f.path.as_str())
        .collect();
    assert!(
        paths.contains(&"private/task-notes.db"),
        "the notes copy is missing from {paths:?}"
    );
    for tree_name in [
        "messaging/",
        "budgets/",
        "accounting/",
        "recovery/",
        "state/",
        "local/",
    ] {
        assert!(
            paths.iter().any(|p| p.starts_with(tree_name)),
            "no {tree_name} in {paths:?}"
        );
    }
    assert!(!paths
        .iter()
        .any(|p| p.ends_with("-wal") || p.ends_with("-shm")));

    // Verify, extracting a pristine copy beside it.
    let side = org.root.join("verified");
    let verified = verify(&verify_request(&out, pass, Some(&side))).expect("it verifies");
    assert_eq!(verified.encrypted, encrypted);
    assert!(verified.private_notes_checked);
    assert_eq!(verified.manifest, report.manifest);
    assert_eq!(
        verified.rebuilt.head_ordinal,
        report.manifest.journal.head_ordinal
    );
    assert!(verified.rebuilt.stopped_cleanly);
    assert_eq!(verified.rebuilt.offices, 1);
    assert!(verified.rebuilt.employees >= 3, "{:?}", verified.rebuilt);
    let extracted = tree(&side.join("durable"));
    for file in &report.manifest.files {
        assert!(
            extracted.contains_key(file.path.as_str()),
            "{}",
            file.path.as_str()
        );
    }
    assert_eq!(extracted.len(), report.manifest.files.len());
    // The projection rebuilt from the restored copy is the original's, row for row.
    assert_eq!(rebuilt_rows(&side.join("durable"), &org), original_rows);

    // Lose the durable root, restore it in place, and the office answers as before.
    std::fs::remove_dir_all(org.durable()).unwrap();
    let restored = restore_in_place(&RestoreRequest {
        archive: &out,
        passphrase: pass,
        office_home: &org.office_home,
        org: &org.org_id,
        office: None,
        limits: Limits::default(),
    })
    .expect("it restores");
    assert_eq!(restored.live_journal, LiveJournal::Missing);
    assert_eq!(restored.previous_durable, None);
    let office = org.open();
    assert_eq!(org.queries(&office), before);
    office.close().unwrap();
}

#[test]
fn a_plain_backup_verifies_rebuilds_and_restores_everything_identically() {
    round_trip(false);
}

#[test]
fn an_encrypted_backup_verifies_rebuilds_and_restores_everything_identically() {
    round_trip(true);
}

#[test]
fn a_restore_over_the_same_head_keeps_the_previous_root_and_rebuilds_the_projections() {
    let org = Org::seeded();
    let out = org.root.join("same.tar.gz");
    take_backup(&org, &out, None);
    let runtime_office = org.runtime().join(&org.office_id);
    assert!(runtime_office.join("office.db").exists());
    let restored = restore_in_place(&RestoreRequest {
        archive: &out,
        passphrase: None,
        office_home: &org.office_home,
        org: &org.org_id,
        office: None,
        limits: Limits::default(),
    })
    .expect("it restores");
    assert!(matches!(restored.live_journal, LiveJournal::Head { .. }));
    let previous = restored
        .previous_durable
        .expect("the previous root is kept");
    assert!(previous.join("state").is_dir());
    assert!(!runtime_office.join("office.db").exists());
    assert!(!runtime_office.join("protected").exists());
    let office = org.open();
    office.close().unwrap();
}

#[test]
fn disposable_entries_are_left_out_and_members_are_deterministic() {
    let org = Org::seeded();
    let local = org.durable().join("local/offices").join(&org.office_id);
    for (dir, file) in [
        ("enroll-tmp", "scratch"),
        (
            &format!("agents/{}/runtime", org.agent) as &str,
            "socket-like",
        ),
        (&format!("agents/{}/tmp", org.agent), "temp"),
    ] {
        std::fs::create_dir_all(local.join(dir)).unwrap();
        std::fs::write(local.join(dir).join(file), b"ephemeral").unwrap();
    }
    std::fs::create_dir_all(local.join(format!("agents/{}/kept", org.agent))).unwrap();
    std::fs::write(org.runtime().join("cache.bin"), b"disposable").unwrap();
    let first = org.root.join("one.tar.gz");
    let second = org.root.join("two.tar.gz");
    let one = take_backup(&org, &first, None);
    let two = take_backup(&org, &second, None);
    let paths: Vec<&str> = one
        .manifest
        .directories
        .iter()
        .map(|d| d.as_str())
        .chain(one.manifest.files.iter().map(|f| f.path.as_str()))
        .collect();
    assert!(!paths.iter().any(|p| p.contains("enroll-tmp")), "{paths:?}");
    assert!(!paths
        .iter()
        .any(|p| p.ends_with("/runtime") || p.contains("/runtime/")));
    assert!(!paths
        .iter()
        .any(|p| p.ends_with("/tmp") || p.contains("/tmp/")));
    assert!(paths.iter().any(|p| p.ends_with("/kept")));
    assert!(!paths.iter().any(|p| p.contains("cache.bin")));
    assert_eq!(one.manifest.files, two.manifest.files);
    assert_eq!(one.manifest.directories, two.manifest.directories);
    // Past the manifest member, the two tar streams are byte for byte the same.
    let body = |path: &Path| {
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(
            &mut flate2::read::GzDecoder::new(std::fs::File::open(path).unwrap()),
            &mut bytes,
        )
        .unwrap();
        let manifest = u64::from_str_radix(
            std::str::from_utf8(&bytes[124..135])
                .unwrap()
                .trim_matches(char::from(0)),
            8,
        )
        .unwrap() as usize;
        bytes[512 + manifest.div_ceil(512) * 512..].to_vec()
    };
    assert_eq!(body(&first), body(&second));
}

#[test]
fn a_path_too_long_for_ustar_round_trips_through_its_extended_header() {
    let org = Org::seeded();
    let deep = org
        .durable()
        .join("local/offices")
        .join(&org.office_id)
        .join("agents")
        .join(&org.agent)
        .join("kept")
        .join("a".repeat(100))
        .join("b".repeat(100));
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::write(deep.join("c".repeat(90)), b"far down").unwrap();
    let out = org.root.join("long.tar.gz");
    let report = take_backup(&org, &out, None);
    let long = report
        .manifest
        .files
        .iter()
        .find(|f| f.path.as_str().ends_with(&"c".repeat(90)))
        .expect("the deep file is listed");
    assert!(long.path.member().len() > 255);
    let side = org.root.join("side");
    verify(&verify_request(&out, None, Some(&side))).expect("it verifies");
    let extracted = long.path.under(&side.join("durable"));
    assert_eq!(std::fs::read(extracted).unwrap(), b"far down");
}
