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
            file.sync_all()?;
            self.verify()?;
            self.verify_file(&temporary, &file)?;
            let from = leaf(&temporary)?;
            let fd = self.file.as_raw_fd();
            before_commit();
            if let Some(previous) = previous {
                self.verify_file(name, &previous.file)?;
            }
            // SAFETY: both names are module-owned components relative to our retained fd.
            let rc = unsafe {
                if previous.is_some() {
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

fn check_path_component(file: &File, path: &Path, root: bool) -> Result<(), Error> {
    #[cfg(target_os = "linux")]
    {
        check_linux_component(file, path, root, &NativeProbe)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (file, path, root);
        Ok(())
    }
}

#[cfg(target_os = "linux")]
trait LinuxProbe {
    fn flags(&self, file: &File, path: &Path) -> io::Result<libc::c_int>;
    fn filesystem(&self, file: &File, path: &Path) -> io::Result<u64>;
    fn xfs(&self, file: &File, path: &Path) -> io::Result<u32>;
}

#[cfg(target_os = "linux")]
struct NativeProbe;

#[cfg(target_os = "linux")]
impl LinuxProbe for NativeProbe {
    fn flags(&self, file: &File, _: &Path) -> io::Result<libc::c_int> {
        let mut flags = 0;
        // SAFETY: FS_IOC_GETFLAGS writes into our valid output for a retained directory.
        if unsafe { libc::ioctl(file.as_raw_fd(), libc::FS_IOC_GETFLAGS, &mut flags) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(flags)
    }
    fn filesystem(&self, file: &File, _: &Path) -> io::Result<u64> {
        let mut stats = std::mem::MaybeUninit::<libc::statfs>::uninit();
        // SAFETY: fstatfs initializes stats on success for this retained descriptor.
        if unsafe { libc::fstatfs(file.as_raw_fd(), stats.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the call succeeded.
        Ok(unsafe { stats.assume_init() }.f_type as u32 as u64)
    }
    fn xfs(&self, file: &File, _: &Path) -> io::Result<u32> {
        xfs_geometry_flags(file)
    }
}

#[cfg(target_os = "linux")]
fn check_linux_component(
    file: &File,
    path: &Path,
    root: bool,
    probe: &impl LinuxProbe,
) -> Result<(), Error> {
    // Ancestor mount types may be tmpfs/autofs/etc.; their casefold flags still matter.
    check_flags(path, probe.flags(file, path))?;
    if root {
        let kind = probe
            .filesystem(file, path)
            .map_err(|source| Error::FilesystemProbe {
                path: path.to_owned(),
                probe: "fstatfs",
                source,
            })?;
        require_root_filesystem(path, kind)?;
        if kind == 0x5846_5342 {
            check_xfs_geometry(path, probe.xfs(file, path))?;
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn require_root_filesystem(path: &Path, kind: u64) -> Result<(), Error> {
    if !matches!(
        kind,
        0xEF53 | 0x5846_5342 | 0x9123_683E | 0xF2F5_2010 | 0x794C_7630
    ) {
        return Err(Error::UnsupportedFilesystem {
            path: path.to_owned(),
            kind,
        });
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn check_flags(path: &Path, flags: io::Result<libc::c_int>) -> Result<(), Error> {
    let flags = match flags {
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

    #[cfg(target_os = "linux")]
    #[test]
    fn walked_ancestors_probe_flags_but_only_the_root_probes_filesystem_and_xfs() {
        use std::cell::RefCell;
        struct Probe<'a> {
            root: &'a Path,
            folded: Option<&'a Path>,
            kind: u64,
            xfs_flags: u32,
            flags: RefCell<Vec<PathBuf>>,
            xfs_calls: RefCell<Vec<PathBuf>>,
        }
        impl LinuxProbe for Probe<'_> {
            fn flags(&self, _: &File, path: &Path) -> io::Result<i32> {
                self.flags.borrow_mut().push(path.to_owned());
                if self.folded == Some(path) {
                    Ok(0x4000_0000)
                } else if path == self.root {
                    Ok(0)
                } else {
                    Err(io::Error::from_raw_os_error(libc::ENOTTY))
                }
            }
            fn filesystem(&self, _: &File, path: &Path) -> io::Result<u64> {
                // Any call on an ancestor (which may be unsupported tmpfs/autofs)
                // is a policy/walk wiring regression, not a classifier failure.
                assert_eq!(path, self.root);
                Ok(self.kind)
            }
            fn xfs(&self, _: &File, path: &Path) -> io::Result<u32> {
                self.xfs_calls.borrow_mut().push(path.to_owned());
                Ok(self.xfs_flags)
            }
        }
        let fixture = super::super::test_directory();
        let root = fixture.path().canonicalize().unwrap();
        let probe = |folded, kind, xfs_flags| Probe {
            root: &root,
            folded,
            kind,
            xfs_flags,
            flags: RefCell::default(),
            xfs_calls: RefCell::default(),
        };
        let walk = |probe: &Probe<'_>| {
            Directory::walk_with_probe(&root, false, true, |file, path, is_root| {
                check_linux_component(file, path, is_root, probe)
            })
        };
        let ext4 = probe(None, 0xEF53, 0);
        walk(&ext4).unwrap();
        let mut expected: Vec<_> = root.ancestors().map(Path::to_owned).collect();
        expected.reverse();
        assert_eq!(*ext4.flags.borrow(), expected);
        for component in &expected {
            assert!(matches!(walk(&probe(Some(component), 0xEF53, 0)),
                Err(Error::CaseInsensitive(path)) if &path == component));
        }
        assert!(matches!(walk(&probe(None, 0x0102_1994, 0)),
            Err(Error::UnsupportedFilesystem { path, .. }) if path == root));
        let xfs = probe(None, 0x5846_5342, 0);
        walk(&xfs).unwrap();
        assert_eq!(*xfs.xfs_calls.borrow(), vec![root.clone()]);
        assert!(matches!(walk(&probe(None, 0x5846_5342, 1 << 12)),
            Err(Error::CaseInsensitive(path)) if path == root));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn unsupported_new_root_is_refused_before_creation() {
        let fixture = super::super::test_directory();
        let root = fixture.path().canonicalize().unwrap().join("new");
        let result = Directory::walk_with_probe(&root, true, true, |_, path, is_root| {
            if is_root {
                require_root_filesystem(path, 0x0102_1994)
            } else {
                Ok(())
            }
        });
        assert!(matches!(result, Err(Error::UnsupportedFilesystem { .. })));
        assert!(!root.exists());
    }

    /// The native fixture mounts a second disk below a supported instance root.
    #[cfg(target_os = "linux")]
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
            let error = check_flags(path, Ok(flags)).unwrap_err();
            assert!(matches!(&error, Error::CaseInsensitive(component) if component == path));
            let diagnostic = error.to_string();
            assert!(diagnostic.contains("/disk/folded-parent"));
            assert!(diagnostic.contains("case-insensitive directories aren't supported on Linux"));
        }
        check_flags(path, Ok(0)).unwrap();
        check_flags(path, Ok(0x0008_0000)).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_filesystem_support_is_explicit_and_ioctl_errors_fail_closed() {
        let path = Path::new("/disk/instance");
        for kind in [0xEF53, 0x5846_5342, 0x9123_683E, 0xF2F5_2010, 0x794C_7630] {
            require_root_filesystem(path, kind).unwrap();
            check_flags(path, Ok(0)).unwrap();
            for errno in [libc::ENOTTY, libc::EOPNOTSUPP] {
                check_flags(path, Err(io::Error::from_raw_os_error(errno))).unwrap();
            }
            let error =
                check_flags(path, Err(io::Error::from_raw_os_error(libc::EACCES))).unwrap_err();
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
            let error = require_root_filesystem(path, kind).unwrap_err();
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
