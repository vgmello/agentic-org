//! Descriptor-relative control-file operations. Never follow an instance-owned symlink.

use std::ffi::{CStr, CString};
use std::fs::{File, TryLockError};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use super::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Identity {
    device: u64,
    inode: u64,
}

impl Identity {
    fn of(file: &File) -> io::Result<Self> {
        let metadata = file.metadata()?;
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
}

pub(super) struct Directory {
    file: File,
    path: PathBuf,
    identity: Identity,
    locked: bool,
    root_device: Option<u64>,
}

impl Drop for Directory {
    fn drop(&mut self) {
        if self.locked {
            // An inherited/duplicated fd may outlive this owner; closing alone is insufficient.
            let _ = self.file.unlock();
        }
    }
}

pub(super) struct FileLock(File);

/// Checked bytes plus the retained inode required for a later conditional update.
pub(super) struct ControlFile {
    file: File,
    bytes: Vec<u8>,
}

impl ControlFile {
    pub(super) fn same_version(&self, other: &Self) -> Result<bool, Error> {
        Ok(self.bytes == other.bytes && Identity::of(&self.file)? == Identity::of(&other.file)?)
    }
}

impl std::ops::Deref for ControlFile {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.bytes
    }
}

impl std::ops::Deref for FileLock {
    type Target = File;
    fn deref(&self) -> &File {
        &self.0
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

/// Normalize spelling only; reject traversal rather than resolving it through symlinks.
pub(super) fn absolute(path: &Path) -> Result<PathBuf, Error> {
    if !path.is_absolute() || path.as_os_str().len() > 4096 || path.to_str().is_none() {
        return Err(Error::Invalid(
            "root must be an absolute UTF-8 path of at most 4096 bytes",
        ));
    }
    let mut result = PathBuf::from("/");
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(name) => result.push(name),
            _ => return Err(Error::Invalid("root must not contain parent traversal")),
        }
    }
    if result == Path::new("/") {
        return Err(Error::Invalid(
            "the filesystem root cannot be an instance root",
        ));
    }
    Ok(result)
}

fn cstring(value: &std::ffi::OsStr) -> Result<CString, Error> {
    CString::new(value.as_bytes()).map_err(|_| Error::Invalid("path contains NUL"))
}

fn leaf(name: &str) -> Result<CString, Error> {
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(Component::Normal(part)) if part == std::ffi::OsStr::new(name))
        || components.next().is_some()
    {
        return Err(Error::Invalid(
            "control name must be one plain path component",
        ));
    }
    cstring(std::ffi::OsStr::new(name))
}

fn from_fd(fd: libc::c_int) -> io::Result<File> {
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        // SAFETY: the successful open/dup transferred this new descriptor to us.
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}

fn effective_user() -> u32 {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

impl Directory {
    pub(super) fn open(path: &Path, create: bool) -> Result<Self, Error> {
        let path = absolute(path)?;
        let file = Self::walk(&path, create)?;
        Self::from_file(file, path)
    }

    /// An instance root additionally establishes its supported filesystem and device fence.
    pub(super) fn open_root(path: &Path, create: bool) -> Result<Self, Error> {
        let path = absolute(path)?;
        let file = Self::walk_with_probe(&path, create, true, check_path_component)?;
        let mut root = Self::from_file(file, path)?;
        root.root_device = Some(root.identity.device);
        Ok(root)
    }

    fn from_file(file: File, path: PathBuf) -> Result<Self, Error> {
        let metadata = file.metadata()?;
        if metadata.uid() != effective_user() || metadata.mode() & 0o7777 != 0o700 {
            return Err(Error::Unsafe(
                "directory must be owned by this user with mode 0700",
            ));
        }
        let identity = Identity::of(&file)?;
        Ok(Self {
            file,
            path,
            identity,
            locked: false,
            root_device: None,
        })
    }

    fn walk(path: &Path, create_final: bool) -> Result<File, Error> {
        Self::walk_with_probe(path, create_final, false, check_path_component)
    }

    fn walk_with_probe(
        path: &Path,
        create_final: bool,
        root: bool,
        mut probe: impl FnMut(&File, &Path, bool) -> Result<(), Error>,
    ) -> Result<File, Error> {
        let mut directory = File::open("/")?;
        probe(&directory, Path::new("/"), false)?;
        let mut traversed = PathBuf::from("/");
        let parts: Vec<_> = path
            .components()
            .filter_map(|c| match c {
                Component::Normal(name) => Some(name),
                _ => None,
            })
            .collect();
        for (index, part) in parts.iter().enumerate() {
            let name = cstring(part)?;
            let open = || {
                // SAFETY: each component is relative to a retained parent descriptor.
                from_fd(unsafe {
                    libc::openat(
                        directory.as_raw_fd(),
                        name.as_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    )
                })
            };
            let final_part = index + 1 == parts.len();
            let child = match open() {
                Err(error)
                    if create_final && final_part && error.kind() == io::ErrorKind::NotFound =>
                {
                    // A new directory inherits its parent's filesystem. Refuse unsupported
                    // roots before creating anything; existing mount roots are checked below.
                    if root {
                        probe(&directory, &traversed, true)?;
                    }
                    // SAFETY: retained parent and validated NUL-terminated component.
                    if unsafe { libc::mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
                        let error = io::Error::last_os_error();
                        if error.kind() != io::ErrorKind::AlreadyExists {
                            return Err(error.into());
                        }
                    } else {
                        directory.sync_all()?;
                    }
                    open()?
                }
                other => other?,
            };
            directory = child;
            traversed.push(part);
            probe(&directory, &traversed, root && final_part)?;
        }
        Ok(directory)
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
    /// The held directory descriptor, for a rule that compares directories and not paths.
    pub(super) fn descriptor(&self) -> std::os::fd::BorrowedFd<'_> {
        std::os::fd::AsFd::as_fd(&self.file)
    }
    pub(super) fn identity(&self) -> Identity {
        self.identity
    }

    pub(super) fn duplicate_unlocked(&self) -> Result<Self, Error> {
        if self.locked {
            return Err(Error::Invalid("cannot duplicate a root lock owner"));
        }
        self.verify()?;
        let mut copy = Self::from_file(self.file.try_clone()?, self.path.clone())?;
        copy.root_device = self.root_device;
        Ok(copy)
    }

    pub(super) fn child(&self, name: &str, create: bool) -> Result<Self, Error> {
        self.verify()?;
        let component = leaf(name)?;
        if create {
            // SAFETY: the verified parent fd and checked leaf are valid.
            if unsafe { libc::mkdirat(self.file.as_raw_fd(), component.as_ptr(), 0o700) } != 0 {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::AlreadyExists {
                    return Err(error.into());
                }
            } else {
                self.file.sync_all()?;
            }
        }
        // SAFETY: this opens only a directory relative to the verified retained parent.
        let file = from_fd(unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                component.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        })?;
        let mut child = Self::from_file(file, self.path.join(name))?;
        self.require_device(child.identity.device)?;
        child.root_device = self.root_device;
        self.verify()?;
        child.verify()?;
        Ok(child)
    }

    /// Claim a fresh directory. Existing names, including symlinks, are never adopted.
    pub(super) fn claim_child(&self, name: &str) -> Result<Option<Self>, Error> {
        self.verify()?;
        let component = leaf(name)?;
        // SAFETY: the retained parent fd and module-owned component are valid.
        if unsafe { libc::mkdirat(self.file.as_raw_fd(), component.as_ptr(), 0o700) } != 0 {
            let error = io::Error::last_os_error();
            return if error.kind() == io::ErrorKind::AlreadyExists {
                Ok(None)
            } else {
                Err(error.into())
            };
        }
        self.file.sync_all()?;
        Ok(Some(self.child(name, false)?))
    }

    /// Fail closed on a memory filesystem where the platform can tell
    /// (`org_platform::fs::require_disk`); setup supplies the explicit disk base.
    pub(super) fn require_disk(&self) -> Result<(), Error> {
        Ok(org_platform::fs::require_disk(&self.file, &self.path)?)
    }

    fn require_device(&self, device: u64) -> Result<(), Error> {
        if self.root_device.is_some_and(|root| root != device) {
            return Err(Error::Unsafe(
                "instance descendants must remain on the root device",
            ));
        }
        Ok(())
    }

    pub(super) fn verify(&self) -> Result<(), Error> {
        let current = Self::open(&self.path, false)?;
        self.require_device(current.identity.device)?;
        if current.identity != self.identity {
            return Err(Error::Unsafe("registered directory was replaced or moved"));
        }
        Ok(())
    }

    pub(super) fn lock_root(&mut self) -> Result<(), Error> {
        lock(&self.file, &self.path)?;
        self.locked = true;
        Ok(())
    }

    pub(super) fn lock_file(&self, name: &str) -> Result<FileLock, Error> {
        // Darwin can return ENOENT for racing O_CREAT|O_NOFOLLOW opens even
        // though the other creator published the file. Claim exclusively, then
        // open an existing entry without creation; all validation still applies.
        let file = match self.open_file(name, libc::O_RDWR | libc::O_CREAT | libc::O_EXCL) {
            Err(Error::Io(error)) if error.kind() == io::ErrorKind::AlreadyExists => {
                self.open_file(name, libc::O_RDWR)?
            }
            result => result?,
        };
        lock(&file, &self.path.join(name))?;
        let held = FileLock(file);
        self.verify_file(name, &held)?;
        Ok(held)
    }

    pub(super) fn verify_file(&self, name: &str, held: &File) -> Result<(), Error> {
        let name = leaf(name)?;
        let mut current = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: the retained dirfd, valid leaf and writable stat output are live.
        if unsafe {
            libc::fstatat(
                self.file.as_raw_fd(),
                name.as_ptr(),
                current.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return Err(io::Error::last_os_error().into());
        }
        // SAFETY: fstatat initialized the output.
        let current = unsafe { current.assume_init() };
        if current.st_mode & libc::S_IFMT != libc::S_IFREG
            || current.st_uid != effective_user()
            || current.st_mode & 0o7777 != 0o600
            || current.st_nlink != 1
        {
            return Err(Error::Unsafe(
                "control entry must be regular, private, owned and singly linked",
            ));
        }
        let (device, inode) = org_platform::fs::device_and_inode(&current)?;
        let identity = Identity { device, inode };
        self.require_device(identity.device)?;
        if identity != Identity::of(held)? {
            return Err(Error::Unsafe("control file was replaced"));
        }
        self.verify()
    }

    fn open_file(&self, name: &str, flags: i32) -> Result<File, Error> {
        let name = leaf(name)?;
        // SAFETY: the fd is retained and the name is a single, module-owned component.
        let file = from_fd(unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                name.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
                0o600,
            )
        })?;
        let metadata = file.metadata()?;
        self.require_device(metadata.dev())?;
        if !metadata.is_file()
            || metadata.uid() != effective_user()
            || metadata.mode() & 0o7777 != 0o600
            || metadata.nlink() != 1
        {
            return Err(Error::Unsafe(
                "control file must be regular, private, owned and singly linked",
            ));
        }
        Ok(file)
    }

    pub(super) fn read(&self, name: &str, limit: usize) -> Result<Option<ControlFile>, Error> {
        let file = match self.open_file(name, libc::O_RDONLY) {
            Err(Error::Io(error)) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            other => other?,
        };
        self.read_from(name, file, limit).map(Some)
    }

    fn read_from(&self, name: &str, mut file: File, limit: usize) -> Result<ControlFile, Error> {
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take((limit + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            return Err(Error::Unsafe("control file exceeds its size limit"));
        }
        self.verify_file(name, &file)?;
        Ok(ControlFile { file, bytes })
    }

    pub(super) fn publish(
        &self,
        name: &str,
        bytes: &[u8],
        previous: Option<&ControlFile>,
    ) -> Result<ControlFile, Error> {
        self.publish_after_staging(name, bytes, previous, || {})
    }

    fn publish_after_staging(
        &self,
        name: &str,
        bytes: &[u8],
        previous: Option<&ControlFile>,
        before_commit: impl FnOnce(),
    ) -> Result<ControlFile, Error> {
        let to = leaf(name)?;
        self.verify()?;
        if let Some(previous) = previous {
            self.verify_file(name, &previous.file)?;
        }
        let temporary = format!(".instance-{}.tmp", crate::ids::generate()?);
        let mut file = self.open_file(&temporary, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL)?;
        let result = (|| {
            file.write_all(bytes)?;
            // This is our live regular control-file descriptor. Refuse
            // unsupported full flush rather than acknowledging a weaker setup
            // identity/operation record before protected creation.
            org_platform::fs::full_sync(&file)?;
            self.verify()?;
            self.verify_file(&temporary, &file)?;
            let from = leaf(&temporary)?;
            let fd = self.file.as_raw_fd();
            before_commit();
            if let Some(previous) = previous {
                self.verify_file(name, &previous.file)?;
            }
            if previous.is_some() {
                // SAFETY: both names are module-owned components relative to our retained fd.
                if unsafe { libc::renameat(fd, from.as_ptr(), fd, to.as_ptr()) } != 0 {
                    return Err(io::Error::last_os_error().into());
                }
            } else {
                org_platform::fs::rename_noreplace_at(&self.file, &from, &self.file, &to)?;
            }
            self.file.sync_all()?;
            self.verify_file(name, &file)?;
            self.verify()
        })();
        let temporary = cstring(std::ffi::OsStr::new(&temporary))?;
        // SAFETY: removes only our own temporary name; after publication it is absent.
        unsafe { libc::unlinkat(self.file.as_raw_fd(), temporary.as_ptr(), 0) };
        result?;
        Ok(ControlFile {
            file,
            bytes: bytes.to_vec(),
        })
    }

    pub(super) fn is_empty(&self) -> Result<bool, Error> {
        // A separately opened description avoids sharing a directory iteration offset.
        let dot = c".";
        // SAFETY: the retained directory fd and static component are valid.
        let descriptor = from_fd(unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                dot.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        })?;
        use std::os::fd::IntoRawFd;
        let fd = descriptor.into_raw_fd();
        // SAFETY: fdopendir takes ownership on success.
        let stream = unsafe { libc::fdopendir(fd) };
        if stream.is_null() {
            let error = io::Error::last_os_error();
            // SAFETY: fdopendir failed, so ownership remains ours.
            unsafe { libc::close(fd) };
            return Err(error.into());
        }
        struct Stream(*mut libc::DIR);
        impl Drop for Stream {
            fn drop(&mut self) {
                // SAFETY: this guard uniquely owns the successful fdopendir result.
                unsafe { libc::closedir(self.0) };
            }
        }
        let stream = Stream(stream);
        loop {
            org_platform::fs::set_errno(0)?;
            // SAFETY: stream is live and exclusively used by this loop.
            let entry = unsafe { libc::readdir(stream.0) };
            if entry.is_null() {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(0) {
                    return Err(error.into());
                }
                return Ok(true);
            }
            // SAFETY: the name is NUL-terminated and valid until the next readdir.
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
            if name != b"." && name != b".." {
                return Ok(false);
            }
        }
    }

    /// Compare ancestor inodes, including case aliases and mount aliases.
    pub(super) fn contains_path(&self, path: &Path) -> Result<bool, Error> {
        Self::has_ancestor(path, self.identity)
    }

    pub(super) fn has_ancestor(path: &Path, identity: Identity) -> Result<bool, Error> {
        for ancestor in path.ancestors() {
            match Self::walk(ancestor, false) {
                Ok(file) if Identity::of(&file)? == identity => return Ok(true),
                Ok(_) => {}
                Err(Error::Io(error)) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(false)
    }
}

/// An instance root opened for a question asked before the root is reserved (setup's
/// preflight compares a credential directory with it): reached by the same walk as a
/// reservation, from `/` without following any link and with the platform's rules for each
/// folder on the way; made when it is missing, and then removed again when this is dropped,
/// from its parent's descriptor and only while that name is still the folder this made.
/// Nothing else is made or removed.
pub(crate) struct RootProbe {
    root: File,
    made: Option<(File, CString)>,
}

impl RootProbe {
    pub(crate) fn open(path: &Path) -> Result<Self, Error> {
        let path = absolute(path)?;
        let (parent_path, name) = match (path.parent(), path.file_name()) {
            (Some(parent), Some(name)) => (parent.to_owned(), cstring(name)?),
            _ => {
                return Err(Error::Invalid(
                    "the filesystem root cannot be an instance root",
                ))
            }
        };
        let parent = Directory::walk_with_probe(&parent_path, false, false, check_path_component)?;
        let open = || {
            // SAFETY: a retained parent descriptor and a validated single component.
            from_fd(unsafe {
                libc::openat(
                    parent.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            })
        };
        let (root, made) = match open() {
            Ok(root) => (root, false),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // As a reservation would: the filesystem it would be made on is judged first.
                check_path_component(&parent, &parent_path, true)?;
                // SAFETY: as above.
                if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
                    return Err(io::Error::last_os_error().into());
                }
                match open() {
                    Ok(root) => (root, true),
                    Err(error) => {
                        // SAFETY: as above; the folder this call just made.
                        unsafe {
                            libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR)
                        };
                        return Err(error.into());
                    }
                }
            }
            Err(error) => return Err(error.into()),
        };
        check_path_component(&root, &path, true)?;
        Ok(Self {
            root,
            made: made.then_some((parent, name)),
        })
    }

    pub(crate) fn descriptor(&self) -> std::os::fd::BorrowedFd<'_> {
        std::os::fd::AsFd::as_fd(&self.root)
    }
}

impl Drop for RootProbe {
    fn drop(&mut self) {
        let Some((parent, name)) = &self.made else {
            return;
        };
        // SAFETY: plain data that fstatat fills in.
        let mut found: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: a retained parent descriptor, a NUL-terminated name and a writable stat.
        let named = unsafe {
            libc::fstatat(
                parent.as_raw_fd(),
                name.as_ptr(),
                &mut found,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } == 0;
        let ours = named
            && matches!(
                (Identity::of(&self.root), org_platform::fs::device_and_inode(&found)),
                (Ok(ours), Ok((device, inode))) if ours.device == device && ours.inode == inode
            );
        if ours {
            // SAFETY: as above; a folder that is no longer empty stays.
            unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) };
        }
    }
}

/// The platform's rules for one directory on the way to an instance root
/// (`org_platform::fs::check_instance_root`): on Linux no case-folding directory and a
/// supported root filesystem; none on macOS.
fn check_path_component(file: &File, path: &Path, root: bool) -> Result<(), Error> {
    Ok(org_platform::fs::check_instance_root(file, path, root)?)
}

fn lock(file: &File, path: &Path) -> Result<(), Error> {
    match file.try_lock() {
        Ok(()) => Ok(()),
        Err(TryLockError::WouldBlock) => Err(Error::Busy(path.to_owned())),
        Err(TryLockError::Error(error)) => Err(error.into()),
    }
}

pub(super) fn paths_overlap(a: &Path, b: &Path, unavailable: bool) -> bool {
    if a.starts_with(b) || b.starts_with(a) {
        return true;
    }
    // For a new claim against a root that cannot be opened, also the spellings this
    // system's filesystems may take for the same names (`org_platform::fs`; none on
    // Linux). Existing registered targets retain their proven distinct inode ownership.
    // A system that cannot say is taken to overlap.
    unavailable && org_platform_host::names::paths_may_alias(a, b).unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn updates_require_the_current_retained_control_handle() {
        let temporary = super::super::test_directory();
        std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let directory = Directory::open(&temporary.path().canonicalize().unwrap(), false).unwrap();
        let first = directory.publish("control", b"first", None).unwrap();
        let second = directory
            .publish("control", b"second", Some(&first))
            .unwrap();
        assert!(directory
            .publish("control", b"stale", Some(&first))
            .is_err());
        assert!(directory.publish("control", b"unclaimed", None).is_err());
        assert_eq!(
            std::fs::read(directory.path().join("control")).unwrap(),
            b"second"
        );
        directory
            .publish("control", b"third", Some(&second))
            .unwrap();
        assert_eq!(
            std::fs::read(directory.path().join("control")).unwrap(),
            b"third"
        );
    }

    #[test]
    fn updates_refuse_control_entries_replaced_since_the_checked_read() {
        use std::os::unix::fs::symlink;
        let temporary = super::super::test_directory();
        std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let root = Directory::open(&temporary.path().canonicalize().unwrap(), false).unwrap();
        for replacement in ["symlink", "hardlink", "different-inode"] {
            for during_staging in [false, true] {
                let directory = root
                    .child(&format!("{replacement}-{during_staging}"), true)
                    .unwrap();
                directory.publish("control", b"original", None).unwrap();
                let before = directory.read("control", 64).unwrap().unwrap();
                assert_eq!(&*before, b"original");
                let control = directory.path().join("control");
                let target = directory.path().join("target");
                let identity = std::cell::Cell::new(None);
                let replace = || {
                    std::fs::write(&target, b"replacement").unwrap();
                    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600))
                        .unwrap();
                    std::fs::rename(&control, directory.path().join("previous")).unwrap();
                    match replacement {
                        "symlink" => symlink(&target, &control).unwrap(),
                        "hardlink" => std::fs::hard_link(&target, &control).unwrap(),
                        _ => std::fs::rename(&target, &control).unwrap(),
                    }
                    identity.set(Some(std::fs::symlink_metadata(&control).unwrap().ino()));
                };
                let result = if during_staging {
                    directory.publish_after_staging("control", b"update", Some(&before), replace)
                } else {
                    replace();
                    directory.publish("control", b"update", Some(&before))
                };
                assert!(
                    result.is_err(),
                    "{replacement} was overwritten, during_staging={during_staging}"
                );
                assert_eq!(
                    Some(std::fs::symlink_metadata(&control).unwrap().ino()),
                    identity.get()
                );
                assert_eq!(std::fs::read(&control).unwrap(), b"replacement");
            }
        }
    }

    #[test]
    fn checked_reads_refuse_control_entries_replaced_after_open() {
        use std::os::unix::fs::symlink;
        let temporary = super::super::test_directory();
        std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let root = Directory::open(&temporary.path().canonicalize().unwrap(), false).unwrap();
        for replacement in ["symlink", "hardlink", "different-inode"] {
            let directory = root.child(replacement, true).unwrap();
            directory.publish("control", b"original", None).unwrap();
            let held = directory.open_file("control", libc::O_RDONLY).unwrap();
            let path = directory.path().to_owned();
            let (ready, replaced) = std::sync::mpsc::channel();
            let mutator = std::thread::spawn(move || {
                let control = path.join("control");
                let target = path.join("target");
                std::fs::write(&target, b"replacement").unwrap();
                std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
                std::fs::rename(&control, path.join("previous")).unwrap();
                match replacement {
                    "symlink" => symlink(&target, &control).unwrap(),
                    "hardlink" => std::fs::hard_link(&target, &control).unwrap(),
                    _ => std::fs::rename(&target, &control).unwrap(),
                }
                ready.send(()).unwrap();
            });
            replaced.recv().unwrap();
            assert!(
                matches!(
                    directory.read_from("control", held, 64),
                    Err(Error::Unsafe(_))
                ),
                "{replacement} was accepted after opening the original inode"
            );
            mutator.join().unwrap();
            assert_eq!(
                std::fs::read(directory.path().join("control")).unwrap(),
                b"replacement"
            );
        }
    }

    /// X5 review, fourth round, P2-6: a root opened for a question before its reservation
    /// is reached without following a link, and only a folder the probe made is removed
    /// again, and only while its name still holds that folder.
    #[test]
    fn a_root_probe_removes_only_the_folder_it_made() {
        let fixture = super::super::test_directory();
        let base = fixture.path().canonicalize().unwrap();
        let made = base.join("new");
        let probe = RootProbe::open(&made).unwrap();
        assert!(made.is_dir());
        drop(probe);
        assert!(!made.exists(), "the folder the probe made was left");
        // An existing folder stays, and so does what is in it.
        std::fs::create_dir(&made).unwrap();
        std::fs::write(made.join("x"), b"x").unwrap();
        drop(RootProbe::open(&made).unwrap());
        assert!(made.join("x").exists());
        // A folder put in the place of the one the probe made is not removed.
        let replaced = base.join("replaced");
        let probe = RootProbe::open(&replaced).unwrap();
        std::fs::rename(&replaced, base.join("moved")).unwrap();
        std::fs::create_dir(&replaced).unwrap();
        drop(probe);
        assert!(replaced.is_dir() && base.join("moved").is_dir());
        // A link on the way is never followed, and nothing is made through it.
        std::os::unix::fs::symlink(&base, base.join("link")).unwrap();
        assert!(RootProbe::open(&base.join("link/through")).is_err());
        assert!(!base.join("through").exists());
    }

    /// What each directory is checked for is the platform's rule (`org_platform::fs`, tested
    /// there); the walk's part is to check every directory from `/` in order, and only the
    /// last one as the root.
    #[test]
    fn the_walk_checks_every_directory_in_order_and_only_the_last_as_the_root() {
        let fixture = super::super::test_directory();
        let root = fixture.path().canonicalize().unwrap();
        let mut expected: Vec<_> = root.ancestors().map(Path::to_owned).collect();
        expected.reverse();
        for is_root in [false, true] {
            let mut checked = Vec::new();
            Directory::walk_with_probe(&root, false, is_root, |file, path, root| {
                assert_eq!(
                    Identity::of(file).unwrap(),
                    Identity::of(&File::open(path).unwrap()).unwrap()
                );
                checked.push((path.to_owned(), root));
                Ok(())
            })
            .unwrap();
            let paths: Vec<_> = checked.iter().map(|(path, _)| path.clone()).collect();
            assert_eq!(paths, expected);
            for (path, as_root) in &checked {
                assert_eq!(*as_root, is_root && path == &root, "{}", path.display());
            }
        }
        // A refusal of any directory on the way ends the walk with that refusal.
        for component in &expected {
            let walked = Directory::walk_with_probe(&root, false, true, |_, path, _| {
                if path == component {
                    Err(
                        org_platform::fs::FilesystemRefusal::CaseInsensitive(path.to_owned())
                            .into(),
                    )
                } else {
                    Ok(())
                }
            });
            assert!(matches!(walked,
                Err(Error::CaseInsensitive(path)) if &path == component));
        }
    }

    #[test]
    fn unsupported_new_root_is_refused_before_creation() {
        let fixture = super::super::test_directory();
        let root = fixture.path().canonicalize().unwrap().join("new");
        let result = Directory::walk_with_probe(&root, true, true, |_, path, is_root| {
            if is_root {
                Err(Error::UnsupportedFilesystem {
                    path: path.to_owned(),
                    kind: 0x0102_1994,
                })
            } else {
                Ok(())
            }
        });
        assert!(matches!(result, Err(Error::UnsupportedFilesystem { .. })));
        assert!(!root.exists());
    }

    /// The native fixture mounts a second disk below a supported instance root.
    #[test]
    fn native_instance_children_refuse_a_different_device() {
        let Some(path) = std::env::var_os("ORG_INSTANCE_NESTED_MOUNT_FIXTURE") else {
            return;
        };
        let root = Directory::open_root(Path::new(&path), false).unwrap();
        let child_path = root.path().join("nested");
        assert_ne!(
            std::fs::metadata(&child_path).unwrap().dev(),
            root.identity.device
        );
        assert!(matches!(
            root.child("nested", false),
            Err(Error::Unsafe(
                "instance descendants must remain on the root device"
            ))
        ));
        assert!(!child_path.join("untouched").exists());
    }

    /// The platform's refusals (`org_platform::fs`, where the rules are tested) keep the
    /// data and the wording this module reports.
    #[test]
    fn platform_refusals_keep_their_instance_diagnostics() {
        use org_platform::fs::FilesystemRefusal as Refusal;
        let path = Path::new("/disk/folded-parent");
        let error = Error::from(Refusal::CaseInsensitive(path.to_owned()));
        assert!(matches!(&error, Error::CaseInsensitive(component) if component == path));
        let diagnostic = error.to_string();
        assert!(diagnostic.contains("/disk/folded-parent"));
        assert!(diagnostic.contains("case-insensitive directories aren't supported on Linux"));

        let path = Path::new("/disk/instance");
        for (probe, errno) in [
            ("FS_IOC_GETFLAGS", libc::EACCES),
            ("XFS_IOC_FSGEOMETRY", libc::EIO),
            ("fstatfs", libc::EACCES),
        ] {
            let error = Error::from(Refusal::Probe {
                path: path.to_owned(),
                probe,
                source: io::Error::from_raw_os_error(errno),
            });
            assert!(
                matches!(&error, Error::FilesystemProbe { path: reported, probe: named, source } if reported == path && *named == probe && source.raw_os_error() == Some(errno))
            );
            assert!(error.to_string().contains("/disk/instance"));
            assert!(error.to_string().contains(probe));
        }
        for kind in [0, 0x0102_1994, u64::MAX] {
            let error = Error::from(Refusal::UnsupportedFilesystem {
                path: path.to_owned(),
                kind,
            });
            assert!(
                matches!(&error, Error::UnsupportedFilesystem { path: reported, kind: reported_kind } if reported == path && *reported_kind == kind)
            );
            assert!(error.to_string().contains(&format!("type {kind:#x}")));
        }
        assert!(matches!(
            Error::from(Refusal::MemoryBacked {
                path: path.to_owned(),
                kind: 0x0102_1994
            }),
            Error::Unsafe("user temp base must be disk-backed")
        ));
    }

    #[test]
    fn concurrent_lock_file_creation_never_loses_the_existing_entry() {
        let temporary = super::super::test_directory();
        std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let root = Directory::open(&temporary.path().canonicalize().unwrap(), false).unwrap();
        let directories: Vec<_> = (0..128)
            .map(|index| root.child(&index.to_string(), true).unwrap())
            .collect();
        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            let run = || {
                let mut results = Vec::new();
                for directory in &directories {
                    barrier.wait();
                    results.push(directory.lock_file("registry.lock").map(drop));
                    barrier.wait();
                }
                results
            };
            let first = scope.spawn(run);
            let second = scope.spawn(run);
            for result in first
                .join()
                .unwrap()
                .into_iter()
                .chain(second.join().unwrap())
            {
                assert!(matches!(result, Ok(()) | Err(Error::Busy(_))), "{result:?}");
            }
        });
    }

    #[test]
    fn dropping_lock_owners_unlocks_even_while_a_duplicate_fd_survives() {
        let temporary = super::super::test_directory();
        std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = temporary.path().canonicalize().unwrap();
        let mut root = Directory::open(&path, false).unwrap();
        let held = root.lock_file("registry.lock").unwrap();
        let inherited_file = held.try_clone().unwrap();
        drop(held);
        root.lock_file("registry.lock").unwrap();
        drop(inherited_file);
        root.lock_root().unwrap();
        let inherited_root = root.file.try_clone().unwrap();
        drop(root);
        Directory::open(&path, false).unwrap().lock_root().unwrap();
        drop(inherited_root);
    }

    #[test]
    fn relative_file_capabilities_cannot_escape_the_opened_directory() {
        let temporary = super::super::test_directory();
        std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let parent = Directory::open(&temporary.path().canonicalize().unwrap(), false).unwrap();
        for name in [
            "",
            ".",
            "..",
            "../escape",
            "/escape",
            "nested/file",
            "trailing/",
            "a\0b",
        ] {
            assert!(parent.claim_child(name).is_err(), "{name:?}");
            assert!(parent.read(name, 10).is_err(), "{name:?}");
        }
        assert!(parent.is_empty().unwrap());
    }

    #[test]
    fn verifying_a_held_lock_refuses_a_symlink_back_to_the_same_inode() {
        use std::os::unix::fs::symlink;
        let temporary = super::super::test_directory();
        std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = temporary.path().canonicalize().unwrap();
        let directory = Directory::open(&path, false).unwrap();
        let held = directory.lock_file("registry.lock").unwrap();
        std::fs::rename(path.join("registry.lock"), path.join("original")).unwrap();
        symlink(path.join("original"), path.join("registry.lock")).unwrap();
        assert!(directory.verify_file("registry.lock", &held).is_err());
    }
}
