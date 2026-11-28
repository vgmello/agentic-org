//! The refusing filesystem adapter: nothing is flushed, renamed, probed or compared.
//!
//! Every function returns the `fs` refusal. None of them touches the descriptor or the
//! paths it is given.

use std::ffi::CStr;
use std::fs::File;
use std::io;
use std::path::Path;

use super::FilesystemRefusal;
use crate::Refused;

fn refusal() -> Refused {
    Refused::unsupported("fs")
}

fn refused<T>() -> io::Result<T> {
    Err(refusal().into())
}

pub(super) fn full_sync(_file: &File) -> io::Result<()> {
    refused()
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn device_flushes_per_full_sync() -> Result<u64, Refused> {
    Err(refusal())
}

pub(super) fn rename_noreplace_at(
    _from_dir: &File,
    _from: &CStr,
    _to_dir: &File,
    _to: &CStr,
) -> io::Result<()> {
    refused()
}

pub(super) fn rename_noreplace(_from: &CStr, _to: &CStr) -> io::Result<()> {
    refused()
}

pub(super) fn set_errno(_value: i32) -> io::Result<()> {
    refused()
}

pub(super) fn device_and_inode(_stat: &libc::stat) -> io::Result<(u64, u64)> {
    refused()
}

pub(super) fn check_instance_root(
    _dir: &File,
    _path: &Path,
    _root: bool,
) -> Result<(), FilesystemRefusal> {
    Err(FilesystemRefusal::Unsupported(refusal()))
}

pub(super) fn require_disk(_dir: &File, _path: &Path) -> Result<(), FilesystemRefusal> {
    Err(FilesystemRefusal::Unsupported(refusal()))
}

pub(super) fn set_owner_only_dir(_path: &Path) -> io::Result<()> {
    refused()
}

pub(super) fn set_owner_read_only_dir(_path: &Path) -> io::Result<()> {
    refused()
}

pub(super) fn set_owner_read_only_dir_handle(_dir: &File) -> io::Result<()> {
    refused()
}

pub(super) fn create_owner_only_dir(_path: &Path) -> io::Result<()> {
    refused()
}

pub(super) fn create_owner_only_dir_all(_path: &Path) -> io::Result<()> {
    refused()
}

pub(super) fn write_owner_only_file(_path: &Path, _bytes: &[u8]) -> io::Result<()> {
    refused()
}

pub(super) fn write_owner_read_only_file(_path: &Path, _bytes: &[u8]) -> io::Result<()> {
    refused()
}

pub(super) fn write_owner_read_only_file_at(
    _dir: &File,
    _name: &std::ffi::OsStr,
    _bytes: &[u8],
) -> io::Result<()> {
    refused()
}

pub(super) fn open_nofollow_ro(_path: &Path) -> io::Result<File> {
    refused()
}

pub(super) fn open_regular_beneath(_root: &Path, _relative: &Path) -> io::Result<File> {
    refused()
}

pub(super) fn write_regular_beneath(
    _root: &Path,
    _relative: &Path,
    _bytes: &[u8],
) -> io::Result<()> {
    refused()
}

pub(super) fn create_symlink(_original: &Path, _link: &Path) -> io::Result<()> {
    refused()
}

pub(super) fn effective_uid() -> io::Result<u32> {
    refused()
}

pub(super) fn verify_owner_only_dir(_path: &Path, _expected_uid: u32) -> io::Result<()> {
    refused()
}

pub(super) fn verify_owner_only_ancestors(
    _path: &Path,
    _stop_at: &Path,
    _expected_uid: u32,
) -> io::Result<()> {
    refused()
}

pub(super) fn verify_owner_only_file(
    _path: &Path,
    _expected_uid: u32,
    _require_exec: bool,
) -> io::Result<std::path::PathBuf> {
    refused()
}

pub(super) fn open_owner_only_dir(_path: &Path, _expected_uid: u32) -> io::Result<File> {
    refused()
}

pub(super) fn open_owner_only_child(
    _dir: &File,
    _name: &std::ffi::OsStr,
    _expected_uid: u32,
) -> io::Result<File> {
    refused()
}

pub(super) fn open_or_create_owner_only_child(
    _dir: &File,
    _name: &std::ffi::OsStr,
    _expected_uid: u32,
) -> io::Result<File> {
    refused()
}

pub(super) fn ensure_symlink_at(
    _dir: &File,
    _name: &std::ffi::OsStr,
    _target: &Path,
) -> io::Result<super::SymlinkOutcome> {
    refused()
}

pub(super) fn remove_symlink_at(_dir: &File, _name: &std::ffi::OsStr) -> io::Result<bool> {
    refused()
}

pub(super) fn paths_case_insensitive_by_default() -> bool {
    false
}

pub(super) fn has_macos_private_root_aliases() -> bool {
    false
}

pub(super) fn is_safe_executable_file(_path: &Path) -> bool {
    false
}

pub(super) fn resolve_safe_executable_file(_path: &Path) -> Option<std::path::PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::io;
    use std::path::Path;

    use super::super::FilesystemRefusal;
    use crate::Refused;

    fn assert_refused<T>(result: io::Result<T>) {
        let Err(error) = result else {
            panic!("the refusing adapter succeeded");
        };
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        assert_eq!(
            error
                .get_ref()
                .and_then(|source| source.downcast_ref::<Refused>()),
            Some(&Refused::unsupported("fs"))
        );
    }

    #[test]
    fn nothing_is_flushed_and_no_errno_or_identity_is_given() {
        let directory = tempfile::tempdir().unwrap();
        let file = File::create(directory.path().join("file")).unwrap();
        assert_refused(super::full_sync(&file));
        assert_eq!(super::device_flushes_per_full_sync(), Err(super::refusal()));
        assert_refused(super::set_errno(0));
        // SAFETY: an all-zero `stat` is a valid value; the adapter does not read it.
        let stat: libc::stat = unsafe { std::mem::zeroed() };
        assert_refused(super::device_and_inode(&stat));
    }

    #[test]
    fn nothing_is_renamed_by_descriptor_or_by_path() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("from"), b"kept").unwrap();
        let held = File::open(directory.path()).unwrap();
        assert_refused(super::rename_noreplace_at(&held, c"from", &held, c"to"));
        let path = |name: &str| {
            std::ffi::CString::new(directory.path().join(name).to_str().unwrap()).unwrap()
        };
        assert_refused(super::rename_noreplace(&path("from"), &path("to")));
        assert_eq!(fs::read(directory.path().join("from")).unwrap(), b"kept");
        assert!(!directory.path().join("to").exists());
    }

    #[test]
    fn no_directory_passes_a_check_and_no_paths_are_compared() {
        let directory = File::open("/").unwrap();
        let unsupported = |result: Result<(), FilesystemRefusal>| {
            assert!(matches!(
                result,
                Err(FilesystemRefusal::Unsupported(refused)) if refused == Refused::unsupported("fs")
            ));
        };
        for root in [false, true] {
            unsupported(super::check_instance_root(&directory, Path::new("/"), root));
        }
        unsupported(super::require_disk(&directory, Path::new("/")));
        let dir = tempfile::tempdir().unwrap();
        assert_refused(super::set_owner_only_dir(dir.path()));
        assert_refused(super::set_owner_read_only_dir(dir.path()));
        assert_refused(super::set_owner_read_only_dir_handle(&directory));
        assert_refused(super::create_owner_only_dir(&dir.path().join("a")));
        assert_refused(super::create_owner_only_dir_all(&dir.path().join("a/b")));
        assert_refused(super::write_owner_only_file(&dir.path().join("f"), b"x"));
        assert_refused(super::write_owner_read_only_file(
            &dir.path().join("ro"),
            b"x",
        ));
        assert_refused(super::write_owner_read_only_file_at(
            &directory,
            std::ffi::OsStr::new("ro"),
            b"x",
        ));
        assert_refused(super::open_nofollow_ro(&dir.path().join("f")));
        assert_refused(super::open_regular_beneath(dir.path(), Path::new("f")));
        assert_refused(super::write_regular_beneath(
            dir.path(),
            Path::new("f"),
            b"x",
        ));
        assert_refused(super::create_symlink(dir.path(), &dir.path().join("l")));
        assert_refused(super::effective_uid());
        assert_refused(super::verify_owner_only_dir(dir.path(), 0));
        assert_refused(super::verify_owner_only_ancestors(
            dir.path(),
            dir.path(),
            0,
        ));
        assert_refused(super::verify_owner_only_file(
            &dir.path().join("f"),
            0,
            false,
        ));
        assert!(!super::paths_case_insensitive_by_default());
        assert!(!super::has_macos_private_root_aliases());
        assert!(!super::is_safe_executable_file(&dir.path().join("bin")));
        assert!(super::resolve_safe_executable_file(&dir.path().join("bin")).is_none());
    }

    #[test]
    fn no_owner_only_directory_is_opened_or_created_and_no_symlink_is_changed() {
        let dir = tempfile::tempdir().unwrap();
        let held = File::open(dir.path()).unwrap();
        assert_refused(super::open_owner_only_dir(dir.path(), 0));
        assert_refused(super::open_owner_only_child(
            &held,
            std::ffi::OsStr::new("child"),
            0,
        ));
        assert_refused(super::open_or_create_owner_only_child(
            &held,
            std::ffi::OsStr::new("child"),
            0,
        ));
        assert!(!dir.path().join("child").exists(), "nothing is created");
        assert_refused(super::ensure_symlink_at(
            &held,
            std::ffi::OsStr::new("link"),
            Path::new("/target"),
        ));
        assert!(fs::symlink_metadata(dir.path().join("link")).is_err());
        std::os::unix::fs::symlink("/target", dir.path().join("kept")).unwrap();
        assert_refused(super::remove_symlink_at(
            &held,
            std::ffi::OsStr::new("kept"),
        ));
        assert!(
            fs::symlink_metadata(dir.path().join("kept")).is_ok(),
            "an existing symlink is never removed"
        );
    }
}
