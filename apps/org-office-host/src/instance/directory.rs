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
        })
    }

    fn walk(path: &Path, create_final: bool) -> Result<File, Error> {
        let mut directory = File::open("/")?;
        #[cfg(target_os = "linux")]
        require_case_sensitive(&directory, Path::new("/"))?;
        #[cfg(target_os = "linux")]
        let mut traversed = PathBuf::from("/");
        let parts: Vec<_> = path
            .components()
            .filter_map(|c| match c {
                Component::Normal(name) => Some(name),
                _ => None,
            })
            .collect();
        for (index, name) in parts.iter().enumerate() {
            let name = cstring(name)?;
            if create_final && index + 1 == parts.len() {
                // SAFETY: the parent fd and NUL-terminated component are valid.
                let result = unsafe { libc::mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o700) };
                if result != 0 {
                    let error = io::Error::last_os_error();
                    if error.kind() != io::ErrorKind::AlreadyExists {
                        return Err(error.into());
                    }
                }
                if result == 0 {
                    directory.sync_all()?;
                }
            }
            // SAFETY: each component is opened relative to a retained parent fd.
            directory = from_fd(unsafe {
                libc::openat(
                    directory.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            })?;
            #[cfg(target_os = "linux")]
            {
                traversed.push(parts[index]);
                require_case_sensitive(&directory, &traversed)?;
            }
        }
        Ok(directory)
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
    pub(super) fn identity(&self) -> Identity {
        self.identity
    }

    pub(super) fn duplicate_unlocked(&self) -> Result<Self, Error> {
        if self.locked {
            return Err(Error::Invalid("cannot duplicate a root lock owner"));
        }
        self.verify()?;
        Self::from_file(self.file.try_clone()?, self.path.clone())
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
        let child = Self::from_file(file, self.path.join(name))?;
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

    #[cfg(target_os = "linux")]
    pub(super) fn require_disk(&self) -> Result<(), Error> {
        // Linux UAPI include/uapi/linux/magic.h defines ramfs/tmpfs/hugetlbfs.
        // Fail closed on known memory filesystems; setup supplies the explicit disk base.
        let mut stats = std::mem::MaybeUninit::<libc::statfs>::uninit();
        // SAFETY: fstatfs initializes stats on success for our valid directory fd.
        if unsafe { libc::fstatfs(self.file.as_raw_fd(), stats.as_mut_ptr()) } != 0 {
            return Err(Error::FilesystemProbe {
                path: self.path.clone(),
                probe: "fstatfs",
                source: io::Error::last_os_error(),
            });
        }
        // SAFETY: fstatfs succeeded.
        let kind = unsafe { stats.assume_init() }.f_type as u32 as u64;
        if matches!(kind, 0x01021994 | 0x858458f6 | 0x958458f6) {
            return Err(Error::Unsafe("user temp base must be disk-backed"));
        }
        Ok(())
    }

    pub(super) fn verify(&self) -> Result<(), Error> {
        let current = Self::open(&self.path, false)?;
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
        #[cfg(target_os = "macos")]
        let identity = Identity {
            device: current.st_dev as u64,
            inode: current.st_ino,
        };
        #[cfg(target_os = "linux")]
        let identity = Identity {
            device: current.st_dev,
            inode: current.st_ino,
        };
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

    pub(super) fn read(&self, name: &str, limit: usize) -> Result<Option<Vec<u8>>, Error> {
        let file = match self.open_file(name, libc::O_RDONLY) {
            Err(Error::Io(error)) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            other => other?,
        };
        self.read_from(name, file, limit).map(Some)
    }

    fn read_from(&self, name: &str, mut file: File, limit: usize) -> Result<Vec<u8>, Error> {
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take((limit + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            return Err(Error::Unsafe("control file exceeds its size limit"));
        }
        self.verify_file(name, &file)?;
        Ok(bytes)
    }

    pub(super) fn publish(&self, name: &str, bytes: &[u8], replace: bool) -> Result<(), Error> {
        let to = leaf(name)?;
        self.verify()?;
        let temporary = format!(".instance-{}.tmp", crate::ids::generate()?);
        let mut file = self.open_file(&temporary, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL)?;
        let result = (|| {
            file.write_all(bytes)?;
            file.sync_all()?;
            self.verify()?;
            self.verify_file(&temporary, &file)?;
            let from = leaf(&temporary)?;
            let fd = self.file.as_raw_fd();
            // SAFETY: both names are module-owned components relative to our retained fd.
            let rc = unsafe {
                if replace {
                    libc::renameat(fd, from.as_ptr(), fd, to.as_ptr())
                } else {
                    rename_exclusive(fd, &from, &to)
                }
            };
            if rc != 0 {
                return Err(io::Error::last_os_error().into());
            }
            self.file.sync_all()?;
            self.verify_file(name, &file)?;
            self.verify()
        })();
        let temporary = cstring(std::ffi::OsStr::new(&temporary))?;
        // SAFETY: removes only our own temporary name; after publication it is absent.
        unsafe { libc::unlinkat(self.file.as_raw_fd(), temporary.as_ptr(), 0) };
        result
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
            set_errno(0);
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

#[cfg(target_os = "linux")]
fn require_case_sensitive(file: &File, path: &Path) -> Result<(), Error> {
    let mut stats = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: the retained directory descriptor and writable output are valid.
    if unsafe { libc::fstatfs(file.as_raw_fd(), stats.as_mut_ptr()) } != 0 {
        return Err(Error::FilesystemProbe {
            path: path.to_owned(),
            probe: "fstatfs",
            source: io::Error::last_os_error(),
        });
    }
    // SAFETY: fstatfs initialized stats on success.
    let kind = unsafe { stats.assume_init() }.f_type as u32 as u64;
    if kind == 0x5846_5342 {
        check_xfs_geometry(path, xfs_geometry_flags(file))?;
    }
    check_linux_directory(path, kind, || {
        let mut flags: libc::c_int = 0;
        // SAFETY: FS_IOC_GETFLAGS writes to the valid flags pointer for this directory.
        if unsafe { libc::ioctl(file.as_raw_fd(), libc::FS_IOC_GETFLAGS, &mut flags) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(flags)
    })
}

#[cfg(target_os = "linux")]
fn check_linux_directory(
    path: &Path,
    kind: u64,
    flags: impl FnOnce() -> io::Result<libc::c_int>,
) -> Result<(), Error> {
    // Never infer sensitivity from an unknown filesystem's lack of an ioctl.
    if !matches!(
        kind,
        0xEF53 | 0x5846_5342 | 0x9123_683E | 0xF2F5_2010 | 0x794C_7630
    ) {
        return Err(Error::UnsupportedFilesystem {
            path: path.to_owned(),
            kind,
        });
    }
    let flags = match flags() {
        Err(error) if matches!(error.raw_os_error(), Some(libc::ENOTTY | libc::EOPNOTSUPP)) => 0,
        result => result.map_err(|source| Error::FilesystemProbe {
            path: path.to_owned(),
            probe: "FS_IOC_GETFLAGS",
            source,
        })?,
    };
    reject_casefold(path, flags)
}

#[cfg(target_os = "linux")]
fn reject_casefold(path: &Path, flags: libc::c_int) -> Result<(), Error> {
    // Linux UAPI include/uapi/linux/fs.h: FS_CASEFOLD_FL.
    if flags & 0x4000_0000 != 0 {
        return Err(Error::CaseInsensitive(path.to_owned()));
    }
    Ok(())
}

// Linux fs/xfs/libxfs/xfs_fs.h, xfs_fsop_geom. The v5 UAPI is 256 bytes;
// newer geometry fields occupy the reserved tail, which this adapter never reads.
#[cfg(target_os = "linux")]
#[repr(C)]
struct XfsGeometry {
    dimensions: [u32; 8],
    blocks: [u64; 4],
    uuid: [u8; 16],
    stripe: [u32; 2],
    version: i32,
    flags: u32,
    sectors: [u32; 4],
    health: [u32; 2],
    reserved: [u64; 17],
}

#[cfg(target_os = "linux")]
const _: () = {
    assert!(std::mem::size_of::<XfsGeometry>() == 256);
    assert!(std::mem::offset_of!(XfsGeometry, flags) == 92);
};

#[cfg(target_os = "linux")]
fn xfs_geometry_flags(file: &File) -> io::Result<u32> {
    let mut geometry = std::mem::MaybeUninit::<XfsGeometry>::zeroed();
    let request = libc::_IOR::<XfsGeometry>(u32::from(b'X'), 126);
    // SAFETY: request encodes this UAPI buffer's size; the descriptor and buffer are live.
    if unsafe { libc::ioctl(file.as_raw_fd(), request, geometry.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful FSGEOMETRY initialized the zeroed UAPI output.
    Ok(unsafe { geometry.assume_init() }.flags)
}

#[cfg(target_os = "linux")]
fn check_xfs_geometry(path: &Path, flags: io::Result<u32>) -> Result<(), Error> {
    let flags = flags.map_err(|source| Error::FilesystemProbe {
        path: path.to_owned(),
        probe: "XFS_IOC_FSGEOMETRY",
        source,
    })?;
    // XFS_FSOP_GEOM_FLAGS_DIRV2CI: legacy ASCII case-insensitive lookups.
    if flags & (1 << 12) != 0 {
        return Err(Error::CaseInsensitive(path.to_owned()));
    }
    Ok(())
}

fn lock(file: &File, path: &Path) -> Result<(), Error> {
    match file.try_lock() {
        Ok(()) => Ok(()),
        Err(TryLockError::WouldBlock) => Err(Error::Busy(path.to_owned())),
        Err(TryLockError::Error(error)) => Err(error.into()),
    }
}

#[cfg(target_os = "macos")]
unsafe fn rename_exclusive(fd: i32, from: &CStr, to: &CStr) -> i32 {
    // SAFETY: forwarded validated relative names and retained parent fd.
    unsafe { libc::renameatx_np(fd, from.as_ptr(), fd, to.as_ptr(), libc::RENAME_EXCL) }
}

#[cfg(target_os = "linux")]
unsafe fn rename_exclusive(fd: i32, from: &CStr, to: &CStr) -> i32 {
    // SAFETY: forwarded validated relative names and retained parent fd.
    unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            fd,
            from.as_ptr(),
            fd,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        ) as i32
    }
}

fn set_errno(value: i32) {
    // SAFETY: libc returns this thread's errno pointer.
    unsafe {
        #[cfg(target_os = "macos")]
        {
            *libc::__error() = value;
        }
        #[cfg(target_os = "linux")]
        {
            *libc::__errno_location() = value;
        }
    }
}

pub(super) fn paths_overlap(a: &Path, b: &Path, unavailable: bool) -> bool {
    if a.starts_with(b) || b.starts_with(a) {
        return true;
    }
    #[cfg(target_os = "macos")]
    if unavailable {
        // Compare components, not string prefixes: APFS/HFS+ also equate canonical
        // Unicode spellings. No localized, diacritic-ignoring or width-ignoring flags.
        // Existing registered targets retain their proven distinct inode ownership
        // on case-sensitive volumes; this conservative comparison is for new claims.
        return a.components().zip(b.components()).all(|(left, right)| {
            match (left, right) {
                (std::path::Component::Normal(left), std::path::Component::Normal(right)) => {
                    use core_foundation::base::{CFComparisonResult, TCFType};
                    use core_foundation::string::{
                        kCFCompareCaseInsensitive, kCFCompareNonliteral, CFString, CFStringCompare,
                    };
                    let left = CFString::new(&left.to_string_lossy());
                    let right = CFString::new(&right.to_string_lossy());
                    // SAFETY: both owned strings remain live throughout this comparison.
                    unsafe {
                        CFStringCompare(
                            left.as_concrete_TypeRef(),
                            right.as_concrete_TypeRef(),
                            kCFCompareCaseInsensitive | kCFCompareNonliteral,
                        ) == CFComparisonResult::EqualTo
                    }
                }
                (left, right) => left == right,
            }
        });
    }
    let _ = unavailable;
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn checked_reads_refuse_control_entries_replaced_after_open() {
        use std::os::unix::fs::symlink;
        let temporary = tempfile::tempdir().unwrap();
        std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let root = Directory::open(&temporary.path().canonicalize().unwrap(), false).unwrap();
        for replacement in ["symlink", "hardlink", "different-inode"] {
            let directory = root.child(replacement, true).unwrap();
            directory.publish("control", b"original", false).unwrap();
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

    #[cfg(target_os = "linux")]
    #[test]
    fn disk_validation_refuses_a_real_memory_filesystem() {
        let temporary = tempfile::tempdir_in("/dev/shm").unwrap();
        std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = temporary.path().canonicalize().unwrap();
        let directory = Directory::from_file(File::open(&path).unwrap(), path).unwrap();
        assert!(matches!(
            directory.require_disk(),
            Err(Error::Unsafe("user temp base must be disk-backed"))
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn casefold_refusal_names_the_unsupported_component() {
        let path = Path::new("/disk/folded-parent");
        for flags in [0x4000_0000, 0x4008_0000] {
            let error = check_linux_directory(path, 0xEF53, || Ok(flags)).unwrap_err();
            assert!(matches!(&error, Error::CaseInsensitive(component) if component == path));
            let diagnostic = error.to_string();
            assert!(diagnostic.contains("/disk/folded-parent"));
            assert!(diagnostic.contains("case-insensitive directories aren't supported on Linux"));
        }
        check_linux_directory(path, 0xEF53, || Ok(0)).unwrap();
        check_linux_directory(path, 0xEF53, || Ok(0x0008_0000)).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_filesystem_support_is_explicit_and_ioctl_errors_fail_closed() {
        let path = Path::new("/disk/instance");
        for kind in [0xEF53, 0x5846_5342, 0x9123_683E, 0xF2F5_2010, 0x794C_7630] {
            check_linux_directory(path, kind, || Ok(0)).unwrap();
            for errno in [libc::ENOTTY, libc::EOPNOTSUPP] {
                check_linux_directory(path, kind, || Err(io::Error::from_raw_os_error(errno)))
                    .unwrap();
            }
            let error = check_linux_directory(path, kind, || {
                Err(io::Error::from_raw_os_error(libc::EACCES))
            })
            .unwrap_err();
            assert!(
                matches!(&error, Error::FilesystemProbe { path: reported, probe: "FS_IOC_GETFLAGS", source } if reported == path && source.raw_os_error() == Some(libc::EACCES))
            );
            assert!(error.to_string().contains("/disk/instance"));
            assert!(error.to_string().contains("FS_IOC_GETFLAGS"));
        }
        for kind in [
            0,
            0x4d44,
            0x2011_BAB0,
            0xff53_4d42,
            0x6969,
            0x6573_5546,
            0x5346_544e,
            0x0102_1994,
            0x2FC1_2FC1,
            u64::MAX,
        ] {
            let error = check_linux_directory(path, kind, || {
                panic!("unsupported type must not query flags")
            })
            .unwrap_err();
            assert!(
                matches!(&error, Error::UnsupportedFilesystem { path: reported, kind: reported_kind } if reported == path && *reported_kind == kind)
            );
            assert!(error.to_string().contains(&format!("type {kind:#x}")));
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn xfs_requires_successful_geometry_without_ascii_casefolding() {
        let path = Path::new("/disk/xfs");
        for flags in [1 << 12, (1 << 12) | (1 << 15)] {
            assert!(
                matches!(check_xfs_geometry(path, Ok(flags)), Err(Error::CaseInsensitive(component)) if component == path)
            );
        }
        check_xfs_geometry(path, Ok(0)).unwrap();
        check_xfs_geometry(path, Ok(1 << 15)).unwrap();
        for errno in [libc::ENOTTY, libc::EOPNOTSUPP, libc::EACCES, libc::EIO] {
            let error =
                check_xfs_geometry(path, Err(io::Error::from_raw_os_error(errno))).unwrap_err();
            assert!(
                matches!(&error, Error::FilesystemProbe { path: reported, probe: "XFS_IOC_FSGEOMETRY", source } if reported == path && source.raw_os_error() == Some(errno))
            );
            assert!(error.to_string().contains("/disk/xfs"));
        }
    }

    #[test]
    fn concurrent_lock_file_creation_never_loses_the_existing_entry() {
        let temporary = tempfile::tempdir().unwrap();
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
        let temporary = tempfile::tempdir().unwrap();
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
        let temporary = tempfile::tempdir().unwrap();
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
        let temporary = tempfile::tempdir().unwrap();
        std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = temporary.path().canonicalize().unwrap();
        let directory = Directory::open(&path, false).unwrap();
        let held = directory.lock_file("registry.lock").unwrap();
        std::fs::rename(path.join("registry.lock"), path.join("original")).unwrap();
        symlink(path.join("original"), path.join("registry.lock")).unwrap();
        assert!(directory.verify_file("registry.lock", &held).is_err());
    }
}
