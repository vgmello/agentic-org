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
}
