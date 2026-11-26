//! Tests of the contract that need a real adapter (macOS or GNU/Linux).

use std::ffi::CString;
use std::fs::{self, File};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use super::testing::full_sync_counts;
use super::{
    check_instance_root, device_and_inode, full_sync, rename_noreplace, rename_noreplace_at,
    require_disk, set_errno, set_owner_only_dir,
};

fn c_path(path: &Path) -> CString {
    CString::new(path.as_os_str().as_bytes()).unwrap()
}

#[test]
fn a_full_sync_counts_each_step_that_succeeded() {
    let directory = tempfile::tempdir().unwrap();
    let file = File::create(directory.path().join("file")).unwrap();
    let before = full_sync_counts();
    full_sync(&file).unwrap();
    full_sync(&file).unwrap();
    let after = full_sync_counts();
    assert_eq!(after.fsync - before.fsync, 2);
    // A device flush follows every fsync, or the system has none.
    let device = after.device_flush - before.device_flush;
    assert!(device == 0 || device == 2, "{device}");
}

/// The fixed fact a consumer's test relies on is what a real flush does here: each
/// `full_sync` adds exactly that many device flushes, and one `fsync`.
#[test]
fn the_device_flush_fact_is_what_one_full_sync_counts() {
    let directory = tempfile::tempdir().unwrap();
    let file = std::fs::File::create(directory.path().join("file")).unwrap();
    let per_sync = super::testing::device_flushes_per_full_sync().unwrap();
    assert!(per_sync <= 1);
    for _ in 0..3 {
        let before = full_sync_counts();
        full_sync(&file).unwrap();
        let after = full_sync_counts();
        assert_eq!(after.fsync - before.fsync, 1);
        assert_eq!(after.device_flush - before.device_flush, per_sync);
    }
}

#[test]
fn a_full_sync_that_fails_is_an_error_and_counts_nothing() {
    // A pipe cannot be flushed.
    let (reader, _writer) = io::pipe().unwrap();
    let file = File::from(std::os::fd::OwnedFd::from(reader));
    let before = full_sync_counts();
    assert!(full_sync(&file).is_err());
    assert_eq!(full_sync_counts(), before);
}

#[test]
fn a_rename_by_descriptor_moves_a_new_name_and_never_replaces_an_existing_one() {
    let directory = tempfile::tempdir().unwrap();
    let other = directory.path().join("other");
    fs::create_dir(&other).unwrap();
    fs::write(directory.path().join("staged"), b"new").unwrap();
    let from = File::open(directory.path()).unwrap();
    let to = File::open(&other).unwrap();
    rename_noreplace_at(&from, c"staged", &to, c"placed").unwrap();
    assert_eq!(fs::read(other.join("placed")).unwrap(), b"new");
    assert!(!directory.path().join("staged").exists());

    fs::write(directory.path().join("second"), b"second").unwrap();
    let error = rename_noreplace_at(&from, c"second", &to, c"placed").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read(other.join("placed")).unwrap(), b"new");
    assert_eq!(
        fs::read(directory.path().join("second")).unwrap(),
        b"second"
    );

    let error = rename_noreplace_at(&from, c"missing", &to, c"fresh").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert!(!other.join("fresh").exists());
}

#[test]
fn a_rename_by_path_moves_a_new_name_and_never_replaces_an_existing_one() {
    let directory = tempfile::tempdir().unwrap();
    let (staged, placed) = (
        directory.path().join("staged"),
        directory.path().join("placed"),
    );
    fs::write(&staged, b"new").unwrap();
    rename_noreplace(&c_path(&staged), &c_path(&placed)).unwrap();
    assert_eq!(fs::read(&placed).unwrap(), b"new");
    assert!(!staged.exists());

    fs::write(&staged, b"second").unwrap();
    let error = rename_noreplace(&c_path(&staged), &c_path(&placed)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read(&placed).unwrap(), b"new");
    assert_eq!(fs::read(&staged).unwrap(), b"second");
}

#[test]
fn errno_is_set_for_this_thread() {
    set_errno(libc::ENOENT).unwrap();
    assert_eq!(
        io::Error::last_os_error().raw_os_error(),
        Some(libc::ENOENT)
    );
    set_errno(0).unwrap();
    assert_eq!(io::Error::last_os_error().raw_os_error(), Some(0));
}

#[test]
fn a_stat_identity_is_the_one_std_reports() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("file");
    fs::write(&path, b"x").unwrap();
    let held = File::open(directory.path()).unwrap();
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: a live directory descriptor, a NUL-terminated name and writable output.
    let rc = unsafe {
        libc::fstatat(
            held.as_raw_fd(),
            c"file".as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    assert_eq!(rc, 0);
    // SAFETY: fstatat initialized the output.
    let stat = unsafe { stat.assume_init() };
    let metadata = fs::symlink_metadata(&path).unwrap();
    assert_eq!(
        device_and_inode(&stat).unwrap(),
        (metadata.dev(), metadata.ino())
    );
}

#[test]
fn a_directory_that_is_not_a_root_is_only_checked_for_case_folding() {
    // The system's temporary directory may be a memory filesystem, which no root may
    // be on; as an ancestor it passes unless it folds case.
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().canonicalize().unwrap();
    check_instance_root(&File::open(&path).unwrap(), &path, false).unwrap();
}

#[test]
fn a_directory_on_disk_passes_the_disk_check() {
    let base = crate::dirs::testing::disk_backed_temp().unwrap();
    fs::create_dir_all(&base).unwrap();
    let directory = tempfile::tempdir_in(&base).unwrap();
    let path = directory.path().canonicalize().unwrap();
    require_disk(&File::open(&path).unwrap(), &path).unwrap();
}

#[test]
fn owner_only_dir_tightens_mode_to_0700() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let child = directory.path().join("private");
    fs::create_dir(&child).unwrap();
    fs::set_permissions(&child, fs::Permissions::from_mode(0o755)).unwrap();
    set_owner_only_dir(&child).unwrap();
    let mode = fs::symlink_metadata(&child).unwrap().mode() & 0o7777;
    assert_eq!(mode, 0o700);

    // Special bits are cleared and a trailing `/` or `/.` still names the real directory.
    fs::set_permissions(&child, fs::Permissions::from_mode(0o2775)).unwrap();
    set_owner_only_dir(&directory.path().join("private/.")).unwrap();
    assert_eq!(fs::symlink_metadata(&child).unwrap().mode() & 0o7777, 0o700);
}

#[test]
fn owner_only_dir_refuses_symlinks_without_touching_their_target() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("victim");
    fs::create_dir(&target).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
    let link = directory.path().join("link");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let dangling = directory.path().join("dangling");
    std::os::unix::fs::symlink(directory.path().join("missing"), &dangling).unwrap();
    let looping = directory.path().join("loop");
    std::os::unix::fs::symlink(&looping, &looping).unwrap();

    // `lstat("link/")` would resolve the link; the trailing slash must not let it through.
    let link_slash = {
        let mut raw = link.as_os_str().to_owned();
        raw.push("/");
        std::path::PathBuf::from(raw)
    };
    for refused in [&link, &link_slash, &link.join("."), &dangling, &looping] {
        assert!(
            set_owner_only_dir(refused).is_err(),
            "{} must be refused",
            refused.display()
        );
    }
    assert_eq!(
        fs::symlink_metadata(&target).unwrap().mode() & 0o7777,
        0o755,
        "the symlink target's mode must never change"
    );
}

#[test]
fn owner_only_dir_refuses_files_fifos_and_missing_paths() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("file");
    fs::write(&file, b"x").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    let fifo = directory.path().join("fifo");
    let c_fifo = c_path(&fifo);
    // SAFETY: `c_fifo` is a NUL-terminated path that outlives the call.
    assert_eq!(unsafe { libc::mkfifo(c_fifo.as_ptr(), 0o644) }, 0);
    let fifo_mode = fs::symlink_metadata(&fifo).unwrap().mode();

    // A FIFO must fail fast (`O_NONBLOCK`) rather than block the open.
    for refused in [&file, &fifo, &directory.path().join("missing")] {
        assert!(
            set_owner_only_dir(refused).is_err(),
            "{}",
            refused.display()
        );
    }
    assert_eq!(fs::symlink_metadata(&file).unwrap().mode() & 0o7777, 0o644);
    assert_eq!(fs::symlink_metadata(&fifo).unwrap().mode(), fifo_mode);
}

#[test]
fn owner_only_dir_refuses_a_directory_owned_by_another_account() {
    if super::effective_uid().unwrap() == 0 {
        // Root owns `/`; never risk tightening it.
        return;
    }
    let root = Path::new("/");
    let before = fs::symlink_metadata(root).unwrap().mode();
    let err = set_owner_only_dir(root).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied, "{err}");
    assert_eq!(fs::symlink_metadata(root).unwrap().mode(), before);
}

#[test]
fn owner_only_ancestors_and_files_enforce_uid_mode_and_no_symlinks() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("office");
    let host_dir = root.join("git-credentials/hosts/github.com");
    super::create_owner_only_dir_all(&host_dir).unwrap();
    let uid = super::effective_uid().unwrap();
    super::verify_owner_only_ancestors(&host_dir, &root, uid).unwrap();

    // Loose intermediate ancestor fails closed.
    let mid = root.join("git-credentials");
    fs::set_permissions(&mid, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(super::verify_owner_only_ancestors(&host_dir, &root, uid).is_err());
    set_owner_only_dir(&mid).unwrap();

    // Wrong UID fails closed.
    assert!(super::verify_owner_only_ancestors(&host_dir, &root, uid.wrapping_add(1)).is_err());

    // Owner-only file write + exec check + symlink refusal.
    let helper = host_dir.join("https-helper");
    super::write_owner_only_file(&helper, b"#!/bin/sh\nexit 0\n").unwrap();
    assert!(super::verify_owner_only_file(&helper, uid, false).is_ok());
    assert!(super::verify_owner_only_file(&helper, uid, true).is_err());
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(super::verify_owner_only_file(&helper, uid, true).is_ok());
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o4700)).unwrap();
    assert!(super::verify_owner_only_file(&helper, uid, true).is_err());

    let link = host_dir.join("link-helper");
    super::create_symlink(&helper, &link).unwrap();
    assert!(super::verify_owner_only_file(&link, uid, false).is_err());
    assert!(super::write_owner_only_file(&link, b"nope").is_err());
    assert!(super::open_nofollow_ro(&link).is_err());

    // `write_owner_only_file` tightens looser existing permissions to 0600, and `open_nofollow_ro`
    // opens regular files without blocking or following symlinks.
    let secret = host_dir.join("token");
    fs::write(&secret, b"old").unwrap();
    fs::set_permissions(&secret, fs::Permissions::from_mode(0o644)).unwrap();
    super::write_owner_only_file(&secret, b"fresh").unwrap();
    assert_eq!(
        fs::symlink_metadata(&secret).unwrap().mode() & 0o7777,
        0o600
    );
    assert!(super::open_nofollow_ro(&secret).is_ok());
    assert!(super::open_nofollow_ro(&host_dir).is_err());

    // `..` traversal outside `stop_at`, mismatched `stop_at`, and trailing `/` or `/.` on
    // symlinked directories MUST all fail closed (`TASK-167`, `TASK-168`).
    let outside = directory.path().join("outside-0700");
    super::create_owner_only_dir_all(&outside).unwrap();
    let escape = host_dir.join("../../../../outside-0700");
    assert!(super::verify_owner_only_ancestors(&escape, &root, uid).is_err());
    assert!(super::verify_owner_only_ancestors(&host_dir, &outside, uid).is_err());
    let dir_link = root.join("dir-link");
    super::create_symlink(&outside, &dir_link).unwrap();
    let dir_link_slash = std::path::PathBuf::from(format!("{}/", dir_link.display()));
    let dir_link_dot = std::path::PathBuf::from(format!("{}/.", dir_link.display()));
    for evasive in [&dir_link, &dir_link_slash, &dir_link_dot] {
        assert!(super::verify_owner_only_dir(evasive, uid).is_err());
        assert!(super::verify_owner_only_ancestors(evasive, &root, uid).is_err());
        assert!(super::create_owner_only_dir_all(evasive).is_err());
    }
}

mod owner_only_dir {
    use std::ffi::OsStr;
    use std::fs;
    use std::io;
    use std::os::unix::fs::PermissionsExt;

    use super::super::{effective_uid, OwnerOnlyDir, SymlinkOutcome};

    fn owner_only(path: &std::path::Path) {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }

    #[test]
    fn opening_refuses_a_symlink_a_foreign_mode_and_a_foreign_owner() {
        let uid = effective_uid().unwrap();
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        fs::create_dir(&home).unwrap();
        owner_only(&home);
        OwnerOnlyDir::open(&home, uid).expect("a real owner-only directory opens");

        let link = root.path().join("link");
        std::os::unix::fs::symlink(&home, &link).unwrap();
        assert!(
            OwnerOnlyDir::open(&link, uid).is_err(),
            "a symlink is refused"
        );

        fs::set_permissions(&home, fs::Permissions::from_mode(0o755)).unwrap();
        let wide = OwnerOnlyDir::open(&home, uid).unwrap_err();
        assert_eq!(wide.kind(), io::ErrorKind::PermissionDenied);
        owner_only(&home);
        let foreign = OwnerOnlyDir::open(&home, uid.wrapping_add(1)).unwrap_err();
        assert_eq!(foreign.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn a_child_is_created_owner_only_and_a_symlinked_child_is_never_followed() {
        let uid = effective_uid().unwrap();
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        fs::create_dir(&home).unwrap();
        owner_only(&home);
        let dir = OwnerOnlyDir::open(&home, uid).unwrap();
        dir.open_or_create_child(OsStr::new(".cargo"), uid).unwrap();
        let mode = fs::metadata(home.join(".cargo"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o7777 & !0o700, 0, "created owner-only: {mode:o}");
        dir.open_or_create_child(OsStr::new(".cargo"), uid)
            .expect("an existing owner-only child reopens");
        fs::set_permissions(home.join(".cargo"), fs::Permissions::from_mode(0o755)).unwrap();
        dir.open_or_create_child(OsStr::new(".cargo"), uid)
            .expect("a widened child of the same account is tightened, not refused");
        let mode = fs::metadata(home.join(".cargo"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o7777,
            0o700,
            "tightened through the descriptor: {mode:o}"
        );

        let outside = root.path().join("outside");
        fs::create_dir(&outside).unwrap();
        owner_only(&outside);
        fs::remove_dir(home.join(".cargo")).unwrap();
        std::os::unix::fs::symlink(&outside, home.join(".cargo")).unwrap();
        assert!(
            dir.open_or_create_child(OsStr::new(".cargo"), uid).is_err(),
            "a symlinked child is refused rather than followed"
        );
        for bad in ["", ".", "..", "a/b"] {
            let err = dir.open_or_create_child(OsStr::new(bad), uid).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{bad:?}");
        }
    }

    #[test]
    fn symlinks_are_created_kept_replaced_and_other_entries_are_left_alone() {
        let uid = effective_uid().unwrap();
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        fs::create_dir(&home).unwrap();
        owner_only(&home);
        let target_a = root.path().join("a");
        let target_b = root.path().join("b");
        fs::create_dir(&target_a).unwrap();
        fs::create_dir(&target_b).unwrap();
        let dir = OwnerOnlyDir::open(&home, uid).unwrap();
        let name = OsStr::new("registry");

        assert_eq!(
            dir.ensure_symlink(name, &target_a).unwrap(),
            SymlinkOutcome::Created
        );
        assert_eq!(
            dir.ensure_symlink(name, &target_a).unwrap(),
            SymlinkOutcome::AlreadyLinked
        );
        assert_eq!(
            dir.ensure_symlink(name, &target_b).unwrap(),
            SymlinkOutcome::Created
        );
        assert_eq!(fs::read_link(home.join("registry")).unwrap(), target_b);

        fs::create_dir(home.join("git")).unwrap();
        assert_eq!(
            dir.ensure_symlink(OsStr::new("git"), &target_a).unwrap(),
            SymlinkOutcome::RealDirectory
        );
        fs::write(home.join("file"), b"x").unwrap();
        let err = dir
            .ensure_symlink(OsStr::new("file"), &target_a)
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(home.join("file")).unwrap(), b"x");

        assert!(dir.remove_symlink(name).unwrap());
        assert!(!dir.remove_symlink(name).unwrap(), "already gone");
        assert!(!dir.remove_symlink(OsStr::new("git")).unwrap());
        assert!(
            home.join("git").is_dir(),
            "a real directory is never removed"
        );
    }
}
