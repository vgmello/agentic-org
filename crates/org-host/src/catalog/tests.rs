use super::*;
use std::os::unix::fs::PermissionsExt;

const T: &str = "2026-11-08T00:00:00.000Z";

fn office(id: &str, org: &str, name: &str, role: OfficeRole, at: &str) -> OfficeFile {
    OfficeFile::new(id, "HQ", role, org, name, at)
}

fn write(home: &Path, folder: &str, bytes: &[u8]) {
    let dir = home.join("offices").join(folder);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(OFFICE_FILE_NAME), bytes).unwrap();
}

#[test]
fn organizations_are_ordered_with_hq_first() {
    let mut catalog = Catalog::default();
    let (early, late) = ("2026-11-08T01:00:00.000Z", "2026-11-08T02:00:00.000Z");
    catalog.insert(office("SAT1", "ORGB", "B", OfficeRole::Satellite, late));
    catalog.insert(office("HQB", "ORGB", "B", OfficeRole::Hq, late));
    catalog.insert(office("HQA", "ORGA", "A", OfficeRole::Hq, early));
    catalog.insert(office("HQC", "ORGC", "C", OfficeRole::Hq, early));
    let orgs = catalog.organizations();
    let ids: Vec<_> = orgs.iter().map(|o| o.id).collect();
    assert_eq!(ids, ["ORGA", "ORGC", "ORGB"]);
    let b: Vec<_> = orgs[2].offices.iter().map(|o| &o.office_id[..]).collect();
    assert_eq!(b, ["HQB", "SAT1"]);
}

#[test]
fn scan_skips_bad_folders() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    fs::create_dir_all(home.join("offices")).unwrap();
    let good_id = "01GOODAAAAAAAAAAAAAAAAAAAA";
    let good = office(good_id, "ORG1", "Good", OfficeRole::Hq, T);
    write(home, good_id, &good.to_vec().unwrap());
    write(home, "BADJSON", b"{not json");
    write(
        home,
        "NEWER",
        br#"{"v":2,"office_id":"NEWER","office_name":"HQ","org_id":"O","org_name":"N","created_at":"t"}"#,
    );
    let other = office("SOMEONEELSE", "ORG2", "Other", OfficeRole::Hq, T);
    write(home, "MISMATCH", &other.to_vec().unwrap());
    let mut big = good.to_vec().unwrap();
    big.resize(MAX_OFFICE_FILE_BYTES + 10, b' ');
    write(home, "BIG", &big);
    // The same handle as the good office's, differing in case only; scanned after it.
    let clash_id = "02CLASHaaaaaaaaaaaaaaaaaaa";
    let clash = office(clash_id, "ORG3", "Clash", OfficeRole::Hq, T);
    write(home, clash_id, &clash.to_vec().unwrap());
    fs::create_dir_all(home.join("offices/UNFINISHED")).unwrap();
    fs::create_dir_all(home.join("offices/not-plain")).unwrap();
    // A symlinked office.json, and a symlinked office folder, both pointing at a valid
    // office: neither is followed.
    let linked_id = "01LINKEDBBBBBBBBBBBBBBBBBB";
    let elsewhere = home.join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    let linked = office(linked_id, "ORG5", "Linked", OfficeRole::Hq, T);
    fs::write(elsewhere.join(OFFICE_FILE_NAME), linked.to_vec().unwrap()).unwrap();
    fs::create_dir_all(home.join("offices").join(linked_id)).unwrap();
    std::os::unix::fs::symlink(
        elsewhere.join(OFFICE_FILE_NAME),
        home.join("offices").join(linked_id).join(OFFICE_FILE_NAME),
    )
    .unwrap();
    let linked_dir_id = "01LINKEDDIRCCCCCCCCCCCCCCC";
    let dir_target = home.join("dir-target");
    fs::create_dir_all(&dir_target).unwrap();
    let linked_dir = office(linked_dir_id, "ORG6", "LinkedDir", OfficeRole::Hq, T);
    fs::write(
        dir_target.join(OFFICE_FILE_NAME),
        linked_dir.to_vec().unwrap(),
    )
    .unwrap();
    std::os::unix::fs::symlink(&dir_target, home.join("offices").join(linked_dir_id)).unwrap();
    // A refused create, moved aside, with a valid office.json: not an office.
    let refused_id = "01REFUSEDAAAAAAAAAAAAAAAAA";
    let refused = office(refused_id, "ORG4", "Refused", OfficeRole::Hq, T);
    write(
        home,
        &format!("{REFUSED_DIR_NAME}/{refused_id}"),
        &refused.to_vec().unwrap(),
    );

    let catalog = Catalog::scan(home).unwrap();
    assert_eq!(catalog.offices(), [good]);
    assert_eq!(catalog.misplaced(), ["MISMATCH"]);
}

#[test]
fn a_refused_folder_is_moved_aside_without_following_symlinks() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let offices = home.join("offices");
    let refused = offices.join(REFUSED_DIR_NAME);
    // Moved, with what it holds, into an owner-only .refused/.
    write(home, "REFUSED", b"{}");
    let moved = move_refused(home, "REFUSED").unwrap();
    assert_eq!(moved, refused.join("REFUSED"));
    assert!(!offices.join("REFUSED").exists());
    assert_eq!(fs::read(moved.join(OFFICE_FILE_NAME)).unwrap(), b"{}");
    let mode = fs::metadata(&refused).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o700);
    // The same name again: moved under a new name, the first one kept.
    write(home, "REFUSED", b"{\"again\":1}");
    let again = move_refused(home, "REFUSED").unwrap();
    let name = again.file_name().unwrap().to_str().unwrap();
    assert!(name.starts_with("REFUSED-"), "{name}");
    assert!(name["REFUSED-".len()..].bytes().all(|b| b.is_ascii_digit()));
    assert_eq!(fs::read(moved.join(OFFICE_FILE_NAME)).unwrap(), b"{}");
    // No folder, or an id that isn't plain: an error.
    assert!(move_refused(home, "NOFOLDER").is_err());
    assert!(move_refused(home, "../REFUSED").is_err());
    // The folder is a symlink: it and its target stay.
    let elsewhere = home.join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, offices.join("LINKEDDIR")).unwrap();
    assert!(move_refused(home, "LINKEDDIR").is_err());
    assert!(fs::symlink_metadata(offices.join("LINKEDDIR")).is_ok());
    assert!(!refused.join("LINKEDDIR").exists());
    // .refused/ is a symlink: nothing moves through it.
    let other_home = tempfile::tempdir().unwrap();
    let other = other_home.path();
    write(other, "KEPT", b"{}");
    std::os::unix::fs::symlink(&elsewhere, other.join("offices").join(REFUSED_DIR_NAME)).unwrap();
    assert!(move_refused(other, "KEPT").is_err());
    assert!(other.join("offices/KEPT/office.json").exists());
    assert!(!elsewhere.join("KEPT").exists());
}

#[test]
fn an_office_json_that_isnt_a_regular_file_is_refused_and_left_alone() {
    use std::os::unix::ffi::OsStrExt;
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let offices = home.join("offices");
    // A directory named office.json, and a FIFO.
    let as_dir = "01DIRAAAAAAAAAAAAAAAAAAAAA";
    fs::create_dir_all(offices.join(as_dir).join(OFFICE_FILE_NAME)).unwrap();
    let as_fifo = "01FIFOBBBBBBBBBBBBBBBBBBBB";
    fs::create_dir_all(offices.join(as_fifo)).unwrap();
    let fifo = offices.join(as_fifo).join(OFFICE_FILE_NAME);
    let fifo_c = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
    // SAFETY: a valid NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) }, 0);

    for id in [as_dir, as_fifo] {
        let err = read_office_file(&offices.join(id).join(OFFICE_FILE_NAME)).unwrap_err();
        assert!(err.contains("isn't a regular file"), "{id}: {err}");
        assert!(is_stored(home, id), "{id}");
    }
    // Never listed, never moved aside, never discarded.
    let catalog = Catalog::scan(home).unwrap();
    assert!(catalog.offices().is_empty());
    assert!(catalog.misplaced().is_empty());
    assert!(unfinished_folders(home).unwrap().is_empty());
}

/// A folder that is not named by a UUIDv7 office id is never listed for a discard: a
/// server can't discard it, so a leftover plain-ID folder would start a doomed discard
/// at every host start. It is logged once per listing and kept.
#[test]
fn unfinished_folders_skip_names_that_are_not_uuidv7_office_ids() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let offices = home.join("offices");
    fs::create_dir_all(offices.join(UNFINISHED)).unwrap();
    for plain in ["PLAINID1", "PLAINID2"] {
        fs::create_dir_all(offices.join(plain)).unwrap();
    }
    let (listed, logged) = crate::host_log::capture(|| unfinished_folders(home).unwrap());
    assert_eq!(listed, [UNFINISHED]);
    assert_eq!(logged.len(), 1, "{logged:?}");
    for plain in ["PLAINID1", "PLAINID2"] {
        assert!(logged[0].contains(plain), "{logged:?}");
        assert!(offices.join(plain).is_dir());
    }
    // Nothing skipped: nothing logged.
    for plain in ["PLAINID1", "PLAINID2"] {
        fs::remove_dir(offices.join(plain)).unwrap();
    }
    let (listed, logged) = crate::host_log::capture(|| unfinished_folders(home).unwrap());
    assert_eq!(listed, [UNFINISHED]);
    assert!(logged.is_empty(), "{logged:?}");
}

const UNFINISHED: &str = "01900000-0000-7000-8000-0000000000a1";

#[test]
fn unfinished_folders_are_those_without_office_json() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let offices = home.join("offices");
    fs::create_dir_all(offices.join(UNFINISHED)).unwrap();
    fs::create_dir_all(offices.join("not-plain")).unwrap();
    fs::write(offices.join("AFILE"), b"").unwrap();
    write(home, "STORED", b"{broken");
    // Refused creates, moved aside: neither unfinished nor stored.
    fs::create_dir_all(offices.join(REFUSED_DIR_NAME).join("REFUSED")).unwrap();
    // Folders the host reserved writer custody in: its marker, or only a staging file of
    // one that a killed host left. They are the reservation's, never discarded.
    fs::create_dir_all(offices.join("MARKED")).unwrap();
    fs::write(offices.join("MARKED/selected-writer.required"), b"selected").unwrap();
    fs::create_dir_all(offices.join("STAGED")).unwrap();
    fs::write(
        offices.join("STAGED/.selected-writer-01ARZ.tmp"),
        b"selected",
    )
    .unwrap();
    assert_eq!(unfinished_folders(home).unwrap(), [UNFINISHED]);
    assert!(!is_stored(home, REFUSED_DIR_NAME));
    assert!(is_stored(home, "STORED"));
    assert!(!is_stored(home, UNFINISHED));
    assert!(!is_stored(home, "../STORED"));
    // A folder that can't be searched: its office.json can't be checked, so it is kept.
    // (Root can search it anyway, so this part needs a normal user.)
    if crate::home::euid() == 0 {
        return;
    }
    write(home, "LOCKED", b"{}");
    let locked = offices.join("LOCKED");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let can_check = fs::symlink_metadata(locked.join(OFFICE_FILE_NAME)).is_ok();
    let unfinished = unfinished_folders(home).unwrap();
    let locked_stored = is_stored(home, "LOCKED");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(
        !can_check,
        "running as root? the folder was still searchable"
    );
    assert!(locked_stored);
    assert_eq!(unfinished, [UNFINISHED]);
}

fn orphan_ids(list: &[&str]) -> Vec<String> {
    list.iter().map(|id| (*id).to_owned()).collect()
}

/// A home whose `offices/` holds a folder for each of `live`.
fn home_with(live: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(OFFICES_DIR_NAME)).unwrap();
    for id in live {
        fs::create_dir_all(dir.path().join(OFFICES_DIR_NAME).join(id)).unwrap();
    }
    dir
}

/// L1: a home that was moved, restored or re-created elsewhere makes every key look
/// orphaned: an empty (or missing) `offices/` with keys present is no answer at all.
#[test]
fn an_empty_offices_folder_with_keys_marks_nothing() {
    let ids = orphan_ids(&["01A", "01B"]);
    let empty = home_with(&[]);
    let err = orphans(empty.path(), &ids).unwrap_err();
    assert!(err.contains("offices/ is empty"), "{err}");
    let missing = tempfile::tempdir().unwrap();
    assert!(orphans(missing.path(), &ids).is_err());
    // No keys: nothing to say, and nothing refused.
    assert_eq!(orphans(empty.path(), &[]), Ok(Vec::new()));
}

/// L1 (ada): more than half of the keys orphaned at once is no answer; half or less is.
#[test]
fn more_than_half_orphaned_marks_nothing_and_half_or_less_is_answered() {
    let home = home_with(&["01A", "01B"]);
    let three = orphan_ids(&["01A", "01X", "01Y"]);
    let err = orphans(home.path(), &three).unwrap_err();
    assert!(err.contains("more than half"), "{err}");
    let half = orphan_ids(&["01A", "01X"]);
    assert_eq!(orphans(home.path(), &half), Ok(orphan_ids(&["01X"])));
    let less = orphan_ids(&["01A", "01B", "01X"]);
    assert_eq!(orphans(home.path(), &less), Ok(orphan_ids(&["01X"])));
}

/// L3b: an office folder that can't be checked is never taken for an orphan, and stops the
/// sweep.
#[test]
fn a_folder_that_cant_be_checked_stops_the_sweep() {
    let home = home_with(&["01A", "01B", "01C"]);
    let ids = orphan_ids(&["01A", "01B", "01C", "01X"]);
    let probe = |path: &Path| {
        if path.ends_with("01X") {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        } else {
            Ok(path.exists())
        }
    };
    let err = orphans_in(home.path(), &ids, probe).unwrap_err();
    assert!(
        err.contains("01X") && err.contains("can't be checked"),
        "{err}"
    );
    // And for real: a folder of offices/ that can't be looked into is an error, never "no
    // office.json" (not as root, whom permissions don't stop).
    if crate::home::euid() == 0 {
        return;
    }
    let locked = home.path().join(OFFICES_DIR_NAME).join("01C");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let swept = orphans(home.path(), &ids);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(swept.is_err(), "{swept:?}");
}

/// A folder whose `office.json` can't be looked for (its parent isn't searchable) is an
/// error, never "no office.json"; a plain file or a dangling link where a folder was
/// expected has none.
#[test]
fn an_office_json_that_cant_be_looked_for_is_an_error() {
    let home = home_with(&["01A"]);
    let offices = home.path().join(OFFICES_DIR_NAME);
    fs::write(offices.join("plain"), b"x").unwrap();
    std::os::unix::fs::symlink(offices.join("nowhere"), offices.join("dangling")).unwrap();
    assert_eq!(
        office_id_in(&offices.join("plain").join(OFFICE_FILE_NAME)).unwrap(),
        None
    );
    assert_eq!(
        office_id_in(&offices.join("dangling").join(OFFICE_FILE_NAME)).unwrap(),
        None
    );
    // Not as root, whom permissions don't stop.
    if crate::home::euid() == 0 {
        return;
    }
    fs::set_permissions(&offices, fs::Permissions::from_mode(0o600)).unwrap();
    let looked = office_id_in(&offices.join("01A").join(OFFICE_FILE_NAME));
    fs::set_permissions(&offices, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(looked.is_err(), "{looked:?}");
}

/// Hidden entries (`.refused/`, `.DS_Store`) don't make an empty `offices/` a live one.
#[test]
fn hidden_entries_dont_make_offices_non_empty() {
    let home = home_with(&[]);
    let offices = home.path().join(OFFICES_DIR_NAME);
    fs::write(offices.join(".DS_Store"), b"").unwrap();
    fs::create_dir(offices.join(REFUSED_DIR_NAME)).unwrap();
    let err = orphans(home.path(), &orphan_ids(&["01A"])).unwrap_err();
    assert!(err.contains("offices/ is empty"), "{err}");
}

#[test]
fn a_refused_name_must_be_the_id_or_the_id_with_a_timestamp() {
    const REFUSED: &str = "01REFUSEDREFUSEDREFUSEDREF";
    assert!(is_refused_as(REFUSED, REFUSED));
    assert!(is_refused_as(&format!("{REFUSED}-123"), REFUSED));
    for other in [
        format!("{REFUSED}-"),
        format!("{REFUSED}-x"),
        format!("{REFUSED}X"),
        format!("{REFUSED}-1-2"),
        REFUSED[1..].to_owned(),
    ] {
        assert!(!is_refused_as(&other, REFUSED), "{other}");
    }
}
