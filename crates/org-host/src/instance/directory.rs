//! Descriptor-relative control-file operations. Never follow an instance-owned symlink.

use org_platform::posix;
use std::ffi::{CStr, CString};
use std::fs::{File, TryLockError};
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

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

/// A directory `name` of `parent`, never through a symbolic link in its last component.
fn open_directory_at(parent: &File, name: &CStr) -> io::Result<File> {
    let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    posix::open_at(parent, name, flags, 0).map(File::from)
}

fn effective_user() -> u32 {
    posix::effective_uid()
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
            // Each component is relative to a retained parent descriptor.
            let open = || open_directory_at(&directory, &name);
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
                    // A retained parent and a validated NUL-terminated component.
                    match posix::make_directory_at(&directory, &name, 0o700) {
                        Ok(()) => directory.sync_all()?,
                        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                        Err(error) => return Err(error.into()),
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
            // The verified parent and checked leaf.
            match posix::make_directory_at(&self.file, &component, 0o700) {
                Ok(()) => self.file.sync_all()?,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
        // Only a directory relative to the verified retained parent.
        let file = open_directory_at(&self.file, &component)?;
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
        // The retained parent and a module-owned component.
        match posix::make_directory_at(&self.file, &component, 0o700) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Ok(None),
            Err(error) => return Err(error.into()),
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

    /// [`Self::lock_file`] for a lock that every holder keeps only for a moment: while
    /// another process holds it, it is tried again until `wait` has passed, and only a
    /// holder that keeps it longer is reported as [`Error::Busy`].
    pub(super) fn lock_file_within(&self, name: &str, wait: Duration) -> Result<FileLock, Error> {
        const RETRY: Duration = Duration::from_millis(10);
        let deadline = Instant::now() + wait;
        loop {
            match self.lock_file(name) {
                Err(Error::Busy(_)) if Instant::now() < deadline => {
                    std::thread::sleep(
                        RETRY.min(deadline.saturating_duration_since(Instant::now())),
                    );
                }
                result => return result,
            }
        }
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
        let current = posix::status_at(&self.file, &name, libc::AT_SYMLINK_NOFOLLOW)?;
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
        // The descriptor is retained and the name is a single, module-owned component.
        let flags = flags | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
        let file = File::from(posix::open_at(&self.file, &name, flags, 0o600)?);
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
            before_commit();
            if let Some(previous) = previous {
                self.verify_file(name, &previous.file)?;
            }
            if previous.is_some() {
                // Both names are module-owned components relative to our retained directory.
                posix::rename_at(&self.file, &from, &self.file, &to)?;
            } else {
                org_platform::fs::rename_noreplace_at(&self.file, &from, &self.file, &to)?;
            }
            self.file.sync_all()?;
            self.verify_file(name, &file)?;
            self.verify()
        })();
        let temporary = cstring(std::ffi::OsStr::new(&temporary))?;
        // Removes only our own temporary name; after publication it is absent.
        let _ = posix::remove_at(&self.file, &temporary, 0);
        result?;
        Ok(ControlFile {
            file,
            bytes: bytes.to_vec(),
        })
    }

    pub(super) fn is_empty(&self) -> Result<bool, Error> {
        // A separately opened description avoids sharing a directory iteration offset.
        match posix::Entries::of(&self.file)?.next() {
            None => Ok(true),
            Some(Ok(_)) => Ok(false),
            Some(Err(error)) => Err(error.into()),
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
pub struct RootProbe {
    root: File,
    made: Option<(File, CString)>,
}

impl RootProbe {
    pub fn open(path: &Path) -> Result<Self, Error> {
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
        // A retained parent descriptor and a validated single component.
        let open = || open_directory_at(&parent, &name);
        let (root, made) = match open() {
            Ok(root) => (root, false),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // As a reservation would: the filesystem it would be made on is judged first.
                check_path_component(&parent, &parent_path, true)?;
                posix::make_directory_at(&parent, &name, 0o700)?;
                match open() {
                    Ok(root) => (root, true),
                    Err(error) => {
                        // The folder this call just made.
                        let _ = posix::remove_at(&parent, &name, libc::AT_REMOVEDIR);
                        return Err(error.into());
                    }
                }
            }
            Err(error) => return Err(error.into()),
        };
        // Built before the root's own check, so that a refusal there still removes what
        // this made.
        let probe = Self {
            root,
            made: made.then_some((parent, name)),
        };
        check_path_component(&probe.root, &path, true)?;
        Ok(probe)
    }

    pub fn descriptor(&self) -> std::os::fd::BorrowedFd<'_> {
        std::os::fd::AsFd::as_fd(&self.root)
    }
}

impl Drop for RootProbe {
    fn drop(&mut self) {
        let Some((parent, name)) = &self.made else {
            return;
        };
        let ours = posix::status_at(parent, name, libc::AT_SYMLINK_NOFOLLOW).is_ok_and(|found| {
            matches!(
                (Identity::of(&self.root), org_platform::fs::device_and_inode(&found)),
                (Ok(ours), Ok((device, inode))) if ours.device == device && ours.inode == inode
            )
        });
        if ours {
            // A folder that is no longer empty stays.
            let _ = posix::remove_at(parent, name, libc::AT_REMOVEDIR);
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
mod tests;
