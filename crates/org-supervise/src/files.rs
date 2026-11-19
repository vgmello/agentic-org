//! Directory-relative, no-follow planned file installation. Never writes through a leaf.
use std::ffi::{CString, OsStr};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use crate::invalid;

fn c(name: &OsStr) -> io::Result<CString> {
    CString::new(name.as_bytes()).map_err(|_| invalid("NUL in planned path"))
}

fn directory(parent: &OwnedFd, name: &OsStr, create: bool) -> io::Result<OwnedFd> {
    let name = c(name)?;
    let mut created = false;
    if create {
        // SAFETY: valid directory descriptor and literal child name; no symlink traversal.
        if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } == 0 {
            created = true;
        } else {
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::AlreadyExists {
                return Err(e);
            }
        }
    }
    open_directory(
        parent,
        &name,
        if created {
            DirectoryUse::Created
        } else if create {
            DirectoryUse::Existing
        } else {
            DirectoryUse::Traverse
        },
    )
}

enum DirectoryUse {
    Traverse,
    Existing,
    Created,
}

// mkdir does not return an inode handle: even a pinned directory may have been substituted
// before open. Never chmod a directory. Validate the new-path inode's owner/mode instead.
fn open_directory(parent: &OwnedFd, name: &CString, usage: DirectoryUse) -> io::Result<OwnedFd> {
    // SAFETY: openat is confined to this component; O_NOFOLLOW rejects a symlink and
    // O_DIRECTORY rejects a replacement that isn't a directory.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        let error = io::Error::last_os_error();
        if matches!(usage, DirectoryUse::Created) && error.kind() == io::ErrorKind::PermissionDenied
        {
            return Err(io::Error::new(error.kind(), "new planned directory cannot be pinned securely; use a umask preserving owner permissions"));
        }
        return Err(error);
    }
    // SAFETY: newly opened descriptor, already verified as a non-symlink directory.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    if !matches!(usage, DirectoryUse::Traverse) {
        let mut info = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: owned directory descriptor and correctly sized output slot.
        if unsafe { libc::fstat(fd.as_raw_fd(), info.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful fstat initialized the complete stat record.
        let info = unsafe { info.assume_init() };
        // SAFETY: credential query only; preparation already refuses euid0.
        if info.st_uid != unsafe { libc::geteuid() } {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "planned directory must be owned by the current user",
            ));
        }
        if matches!(usage, DirectoryUse::Created) && info.st_mode & 0o7777 != 0o700 {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "new planned directory must already have mode 0700; check owner-masking umask or concurrent replacement"));
        }
    }
    Ok(fd)
}

pub(crate) fn root(path: &Path) -> io::Result<OwnedFd> {
    if !path.is_absolute() {
        return Err(invalid("planned-file root must be absolute"));
    }
    // SAFETY: constant C string names root, returning a newly owned directory fd.
    let fd = unsafe {
        libc::open(
            c"/".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: ownership from open.
    let mut current = unsafe { OwnedFd::from_raw_fd(fd) };
    for part in path.components() {
        match part {
            Component::RootDir => {}
            Component::Normal(name) => current = directory(&current, name, false)?,
            _ => return Err(invalid("planned-file root must be normalized")),
        }
    }
    Ok(current)
}

pub(crate) fn private_directory(fd: &OwnedFd) -> io::Result<()> {
    let mut info = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: an owned directory descriptor and a valid output slot.
    if unsafe { libc::fstat(fd.as_raw_fd(), info.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful fstat initialized the record; geteuid only queries credentials.
    let info = unsafe { info.assume_init() };
    if info.st_mode & libc::S_IFMT != libc::S_IFDIR
        || info.st_uid != unsafe { libc::geteuid() }
        || info.st_mode & 0o7777 != 0o700
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "cleanup parent must already be a current-user directory with mode 0700",
        ));
    }
    Ok(())
}

/// A fresh disposable directory, never an existing caller directory. Once attached to a
/// child, its owner explicitly removes it after exit; losing ownership leaves it unresolved.
pub(crate) struct PrivateDirectory {
    path: PathBuf,
    parent_path: PathBuf,
    parent: OwnedFd,
    name: CString,
    dev: u64,
    ino: u64,
    armed: bool,
    present: bool,
}

// Shared by the pinned-parent and allocated-directory checks. Keep both identity
// components explicit so cross-device refusal is testable without privileged mounts.
fn same_directory(left: (u64, u64), right: (u64, u64)) -> bool {
    left.0 == right.0 && left.1 == right.1
}

impl PrivateDirectory {
    pub(crate) fn new(parent_path: &Path) -> io::Result<Self> {
        let parent = root(parent_path)?;
        private_directory(&parent)?;
        let mut random = [0; 16];
        File::open("/dev/urandom")?.read_exact(&mut random)?;
        let name = CString::new(format!("probe-{:032x}", u128::from_ne_bytes(random))).unwrap();
        // SAFETY: pinned private parent, newly allocated literal name, no chmod of any inode.
        if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let result = (|| {
            let fd = open_directory(&parent, &name, DirectoryUse::Created)?;
            private_directory(&fd)?;
            let path = parent_path.join(OsStr::from_bytes(name.as_bytes()));
            let metadata = File::from(fd).metadata()?;
            Ok(Self {
                path,
                parent_path: parent_path.to_owned(),
                parent,
                name,
                dev: metadata.dev(),
                ino: metadata.ino(),
                armed: true,
                present: true,
            })
        })();
        // On an owner-masking umask, opening the created directory may fail. Never repair
        // permissions. The empty allocation remains for its owner to diagnose/remove.
        result
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn attach(&mut self) {
        self.armed = false;
    }

    pub(crate) fn remove(&mut self) -> io::Result<bool> {
        if !self.present {
            return Ok(false);
        }
        private_directory(&self.parent)?;
        // Reopen from / without following links; refuse a moved/replaced ancestor.
        let current_parent = root(&self.parent_path)?;
        private_directory(&current_parent)?;
        let pinned = File::from(self.parent.try_clone()?).metadata()?;
        let current = File::from(current_parent).metadata()?;
        if !same_directory((pinned.dev(), pinned.ino()), (current.dev(), current.ino())) {
            return Err(invalid("probe parent was replaced"));
        }
        let current = match open_directory(&self.parent, &self.name, DirectoryUse::Created) {
            Ok(fd) => fd,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.present = false;
                return Ok(false);
            }
            Err(error) => return Err(error),
        };
        private_directory(&current)?;
        let metadata = File::from(current).metadata()?;
        if !same_directory((metadata.dev(), metadata.ino()), (self.dev, self.ino)) {
            return Err(invalid("probe directory was replaced"));
        }
        // Rust's Unix remove_dir_all uses fd-relative traversal and protects against
        // symlink TOCTOU on our supported macOS/Linux platforms. The private parent and
        // identity checks above bound which tree is owned; deliberate same-UID mutation
        // remains outside v1's threat model, not an atomic rename/identity guarantee.
        std::fs::remove_dir_all(&self.path)?;
        self.present = false;
        Ok(true)
    }
}

impl Drop for PrivateDirectory {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.remove();
        }
    }
}

fn components(relative: &Path) -> io::Result<Vec<&OsStr>> {
    let parts = relative
        .components()
        .map(|part| match part {
            Component::Normal(name) => Ok(name),
            _ => Err(invalid("planned path must be relative and normalized")),
        })
        .collect::<io::Result<Vec<_>>>()?;
    if parts.is_empty() {
        return Err(invalid("empty planned path"));
    }
    Ok(parts)
}

pub(crate) fn create_dir(base: &Path, relative: &Path) -> io::Result<()> {
    let mut current = root(base)?;
    for name in components(relative)? {
        current = directory(&current, name, true)?;
    }
    Ok(())
}

pub(crate) fn write_file(base: &Path, relative: &Path, contents: &[u8]) -> io::Result<()> {
    let mut parent = root(base)?;
    let mut parts = components(relative)?;
    let leaf = c(parts.pop().unwrap())?;
    for name in parts {
        parent = directory(&parent, name, false)?;
    }
    let mut random = [0u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut random)?;
    let temp = CString::new(format!(
        ".org-supervise-{:032x}",
        u128::from_ne_bytes(random)
    ))
    .unwrap();
    // SAFETY: parent is held open, name is literal, O_EXCL prevents writing another file.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            temp.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: new descriptor transferred exactly once to File.
    let mut file = unsafe { File::from_raw_fd(fd) };
    let result = (|| {
        // SAFETY: adjust only the newly created inode held by our descriptor. A restrictive
        // process umask must not silently change the planned file's specified mode.
        if unsafe { libc::fchmod(file.as_raw_fd(), 0o600) } != 0 {
            return Err(io::Error::last_os_error());
        }
        file.write_all(contents)?;
        // SAFETY: both names relative to the same owned directory. Replace the leaf, never
        // follow its symlink or truncate a file with additional hardlinks.
        if unsafe {
            libc::renameat(
                parent.as_raw_fd(),
                temp.as_ptr(),
                parent.as_raw_fd(),
                leaf.as_ptr(),
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    })();
    if result.is_err() {
        // SAFETY: only the temporary file created above, in the still-owned parent.
        unsafe { libc::unlinkat(parent.as_raw_fd(), temp.as_ptr(), 0) };
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    fn private_parent(root: &Path) -> PathBuf {
        let parent = root.join("private");
        std::fs::create_dir(&parent).unwrap();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).unwrap();
        parent
    }

    #[test]
    fn private_directory_identity_refuses_a_changed_device() {
        let temp = tempfile::tempdir().unwrap();
        let metadata = temp.path().metadata().unwrap();
        let identity = (metadata.dev(), metadata.ino());
        assert!(same_directory(identity, identity));
        let changed = (identity.0.wrapping_add(1), identity.1);
        assert!(!same_directory(identity, changed));
        assert!(!same_directory(changed, identity));
    }

    #[test]
    fn private_directory_identity_refuses_a_changed_inode() {
        let temp = tempfile::tempdir().unwrap();
        let metadata = temp.path().metadata().unwrap();
        let identity = (metadata.dev(), metadata.ino());
        assert!(same_directory(identity, identity));
        let changed = (identity.0, identity.1.wrapping_add(1));
        assert!(!same_directory(identity, changed));
        assert!(!same_directory(changed, identity));
    }

    #[test]
    fn private_directory_cleanup_refuses_a_replaced_cwd_and_keeps_both_trees() {
        let temp = tempfile::tempdir().unwrap();
        let parent = private_parent(&temp.path().canonicalize().unwrap());
        let mut directory = PrivateDirectory::new(&parent).unwrap();
        directory.attach();
        let path = directory.path().to_owned();
        std::fs::write(path.join("owned"), b"original").unwrap();
        let saved = parent.join("saved");
        std::fs::rename(&path, &saved).unwrap();
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(path.join("replacement"), b"untouched").unwrap();

        let error = directory.remove().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(error.to_string(), "probe directory was replaced");
        assert_eq!(std::fs::read(saved.join("owned")).unwrap(), b"original");
        assert_eq!(
            std::fs::read(path.join("replacement")).unwrap(),
            b"untouched"
        );
        std::fs::remove_dir_all(&path).unwrap();
        std::fs::rename(saved, &path).unwrap();
        assert!(directory.remove().unwrap());
        assert!(!path.exists());
        assert!(!directory.remove().unwrap());
    }

    #[test]
    fn private_directory_cleanup_refuses_a_replaced_parent_and_keeps_both_trees() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let parent = private_parent(&root);
        let mut directory = PrivateDirectory::new(&parent).unwrap();
        directory.attach();
        let path = directory.path().to_owned();
        let leaf = path.file_name().unwrap();
        std::fs::write(path.join("owned"), b"original").unwrap();
        let saved = root.join("saved-parent");
        std::fs::rename(&parent, &saved).unwrap();
        std::fs::create_dir(&parent).unwrap();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(path.join("replacement"), b"untouched").unwrap();

        let error = directory.remove().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(error.to_string(), "probe parent was replaced");
        assert_eq!(
            std::fs::read(saved.join(leaf).join("owned")).unwrap(),
            b"original"
        );
        assert_eq!(
            std::fs::read(path.join("replacement")).unwrap(),
            b"untouched"
        );
        std::fs::remove_dir_all(&parent).unwrap();
        std::fs::rename(saved, &parent).unwrap();
        assert!(directory.remove().unwrap());
        assert!(!path.exists());
    }

    #[test]
    fn private_directory_cleanup_refuses_a_changed_recorded_device() {
        let temp = tempfile::tempdir().unwrap();
        let parent = private_parent(&temp.path().canonicalize().unwrap());
        let mut directory = PrivateDirectory::new(&parent).unwrap();
        directory.attach();
        let path = directory.path().to_owned();
        std::fs::write(path.join("owned"), b"original").unwrap();
        let device = directory.dev;
        directory.dev = device.wrapping_add(1);
        let error = directory.remove().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(error.to_string(), "probe directory was replaced");
        assert_eq!(std::fs::read(path.join("owned")).unwrap(), b"original");
        directory.dev = device;
        assert!(directory.remove().unwrap());
    }

    #[test]
    fn refuses_an_existing_directory_owned_by_another_user() {
        // Query an existing system directory directly: no mkdir/chown or privileged
        // harness needed, and no system directory is modified by this test.
        assert_eq!(std::fs::metadata("/usr").unwrap().uid(), 0);
        let parent = root(Path::new("/")).unwrap();
        let error = open_directory(
            &parent,
            &CString::new("usr").unwrap(),
            DirectoryUse::Existing,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(error.to_string().contains("owned by the current user"));
    }

    #[test]
    fn concurrent_substitution_cannot_chmod_an_unrelated_hardlink() {
        let temp = tempfile::tempdir().unwrap();
        let private = tempfile::tempdir().unwrap();
        let target = private.path().join("unchanged");
        std::fs::write(&target, b"private content").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
        let name = temp.path().join("new");
        std::fs::create_dir(&name).unwrap();
        let parent = root(&temp.path().canonicalize().unwrap()).unwrap();
        let replacement = target.clone();
        // Deterministically schedule another actor between mkdir and the pin/repair step.
        std::thread::spawn(move || {
            std::fs::remove_dir(&name).unwrap();
            std::fs::hard_link(replacement, name).unwrap();
        })
        .join()
        .unwrap();
        assert!(open_directory(
            &parent,
            &CString::new("new").unwrap(),
            DirectoryUse::Created
        )
        .is_err());
        assert_eq!(std::fs::metadata(target).unwrap().mode() & 0o777, 0o600);
    }

    #[test]
    fn concurrent_directory_substitution_keeps_existing_permissions() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("existing");
        std::fs::create_dir(&target).unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o750)).unwrap();
        let name = temp.path().join("new");
        std::fs::create_dir(&name).unwrap();
        let parent = root(&temp.path().canonicalize().unwrap()).unwrap();
        std::thread::spawn(move || {
            std::fs::remove_dir(&name).unwrap();
            std::fs::rename(target, name).unwrap();
        })
        .join()
        .unwrap();
        let result = open_directory(
            &parent,
            &CString::new("new").unwrap(),
            DirectoryUse::Created,
        );
        assert_eq!(
            std::fs::metadata(temp.path().join("new")).unwrap().mode() & 0o777,
            0o750
        );
        assert!(result.is_err());
    }
}
