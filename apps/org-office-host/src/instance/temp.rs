//! Temp names belong to their allocator, never to caller-supplied office IDs.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use super::directory::Directory;
use super::Error;

const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

pub(super) fn valid(handle: &str) -> bool {
    handle.len() == 3 && handle.bytes().all(|b| ALPHABET.contains(&b))
}

pub(super) fn validate_base(base: &Directory) -> Result<(), Error> {
    // A private directory is required, never the OS's shared temporary directory.
    if ["/tmp", "/private/tmp", "/var/tmp"]
        .iter()
        .any(|path| base.path() == Path::new(path))
    {
        return Err(Error::Unsafe(
            "user temp base must not be a shared temporary directory",
        ));
    }
    #[cfg(target_os = "linux")]
    base.require_disk()?;
    check_budget(&environment_base(base.path()))
}

fn environment_base(base: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    if let Ok(suffix) = base.strip_prefix("/private/var") {
        // The session environment uses DARWIN_USER_TEMP_DIR's /var spelling.
        // Registry/denies retain the canonical /private/var spelling.
        return Path::new("/var").join(suffix);
    }
    base.to_owned()
}

fn check_budget(environment_path: &Path) -> Result<(), Error> {
    if environment_path
        .join("ao/aaa/aaa/aaaaa/aaa")
        .as_os_str()
        .len()
        > 71
    {
        return Err(Error::Invalid(
            "user temp base exceeds the session's 71-byte TMPDIR budget",
        ));
    }
    Ok(())
}

pub(super) fn allocate<'a>(
    parent: &Directory,
    recorded: impl Iterator<Item = &'a str>,
) -> Result<(String, Directory), Error> {
    let used: BTreeSet<_> = recorded.collect();
    let mut entropy = File::open("/dev/urandom")?;
    claim(parent, &used, || {
        let mut bytes = [0u8; 3];
        entropy.read_exact(&mut bytes)?;
        Ok(bytes)
    })
}

fn claim(
    parent: &Directory,
    used: &BTreeSet<&str>,
    mut draw: impl FnMut() -> std::io::Result<[u8; 3]>,
) -> Result<(String, Directory), Error> {
    for _ in 0..128 {
        let bytes = draw()?;
        let handle: String = bytes
            .iter()
            .map(|b| ALPHABET[(b & 31) as usize] as char)
            .collect();
        if !used.contains(handle.as_str()) {
            if let Some(directory) = parent.claim_child(&handle)? {
                return Ok((handle, directory));
            }
        }
    }
    Err(Error::Conflict(
        "could not claim an unused temp handle; repair or retry",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn session_budget_accepts_exactly_71_bytes_and_refuses_72() {
        assert!(check_budget(Path::new(&format!("/{}", "a".repeat(49)))).is_ok());
        assert!(check_budget(Path::new(&format!("/{}", "a".repeat(50)))).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn darwin_environment_uses_var_without_changing_registry_paths() {
        let canonical = Path::new("/private/var/folders/example/T");
        assert_eq!(
            environment_base(canonical),
            Path::new("/var/folders/example/T")
        );
        assert_eq!(canonical, Path::new("/private/var/folders/example/T"));
        assert_eq!(
            environment_base(Path::new("/private/various/base")),
            Path::new("/private/various/base")
        );
    }
    fn private_temp() -> tempfile::TempDir {
        let temporary = tempfile::tempdir().unwrap();
        std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        temporary
    }

    #[test]
    fn recorded_and_on_disk_collisions_both_draw_again() {
        let temporary = private_temp();
        let path = temporary.path().canonicalize().unwrap();
        let parent = Directory::open(&path, false).unwrap();
        let existing = parent.claim_child("bbb").unwrap().unwrap();
        let inode = existing.identity();
        let mut draws = [[0; 3], [1; 3], [2; 3]].into_iter();
        let (handle, claimed) = claim(&parent, &BTreeSet::from(["aaa"]), || {
            Ok(draws.next().expect("three draws suffice"))
        })
        .unwrap();
        assert_eq!(handle, "ccc");
        assert_eq!(claimed.path(), path.join("ccc"));
        assert!(
            !path.join("aaa").exists(),
            "recorded handle is not reused even without its folder"
        );
        assert_eq!(
            Directory::open(&path.join("bbb"), false)
                .unwrap()
                .identity(),
            inode
        );
    }

    #[test]
    fn entropy_failure_or_exhaustion_never_falls_back() {
        let temporary = private_temp();
        let parent = Directory::open(&temporary.path().canonicalize().unwrap(), false).unwrap();
        let failed = claim(&parent, &BTreeSet::new(), || {
            Err(std::io::Error::other("synthetic entropy outage"))
        });
        assert!(matches!(failed, Err(Error::Io(_))));
        assert!(parent.is_empty().unwrap());
        assert!(matches!(
            claim(&parent, &BTreeSet::from(["aaa"]), || Ok([0; 3])),
            Err(Error::Conflict(_))
        ));
        assert!(parent.is_empty().unwrap());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_private_memory_backed_temp_base_is_refused() {
        let temporary = tempfile::tempdir_in("/dev/shm").unwrap();
        std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let base = Directory::open(&temporary.path().canonicalize().unwrap(), false).unwrap();
        assert!(matches!(
            validate_base(&base),
            Err(Error::Unsafe("user temp base must be disk-backed"))
        ));
    }
}
