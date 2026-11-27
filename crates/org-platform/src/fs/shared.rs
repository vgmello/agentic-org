//! POSIX filesystem helpers shared by the macOS and GNU/Linux adapters.

use std::ffi::{CStr, CString, OsStr};
use std::fs::{DirBuilder, File, OpenOptions};
use std::io::{self, Write as _};
use std::os::fd::{AsRawFd as _, FromRawFd as _};
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{
    DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _,
};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::SymlinkOutcome;

static TMP_RO_FILE_SEQ: AtomicU64 = AtomicU64::new(1);

/// Strips trailing `/` and `/.` so POSIX `lstat(2)` inspects the final path component itself
/// rather than dereferencing a symlinked directory (`TASK-167`, `TASK-168`).
pub(super) fn normalize_lstat_path(path: &Path) -> PathBuf {
    path.components().collect()
}

fn chmod_verified_owner_dir(dir: &File, mode: libc::mode_t) -> io::Result<()> {
    let meta = dir.metadata()?;
    if !meta.file_type().is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the opened entry is not a directory",
        ));
    }
    if meta.uid() != effective_uid()? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the directory is not owned by the effective account",
        ));
    }
    // SAFETY: `dir` is an open descriptor this function owns/borrows until it returns; `fchmod`
    // reads no memory and only changes the mode of the inode that descriptor refers to.
    if unsafe { libc::fchmod(dir.as_raw_fd(), mode) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Sets mode `0700` on the real directory at `path` through its own descriptor (`TASK-190`).
///
/// Trailing `/` and `/.` are stripped first so the kernel is never asked to resolve the last
/// component; `O_NOFOLLOW` then refuses a symlink there, `O_DIRECTORY` refuses anything that is
/// not a directory, and `O_NONBLOCK` keeps a FIFO swapped in at `path` from blocking the open.
/// The opened handle must be a directory owned by the effective uid, and `fchmod` changes that
/// exact inode, so a symlink or rename race can never redirect the mode change elsewhere.
pub(super) fn set_owner_only_dir(path: &Path) -> io::Result<()> {
    let dir = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(normalize_lstat_path(path))?;
    chmod_verified_owner_dir(&dir, libc::S_IRWXU)
}

/// Sets mode `0500` (owner read + search, no write) on the real directory at `path` through its
/// own descriptor, refusing symlinks and foreign-owned directories.
pub(super) fn set_owner_read_only_dir(path: &Path) -> io::Result<()> {
    let dir = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(normalize_lstat_path(path))?;
    chmod_verified_owner_dir(&dir, libc::S_IRUSR | libc::S_IXUSR)
}

pub(super) fn set_owner_read_only_dir_handle(dir: &File) -> io::Result<()> {
    chmod_verified_owner_dir(dir, libc::S_IRUSR | libc::S_IXUSR)
}

pub(super) fn create_owner_only_dir(path: &Path) -> io::Result<()> {
    DirBuilder::new().mode(0o700).create(path)
}

fn create_one_owner_only_dir(path: &Path) -> io::Result<()> {
    let norm = normalize_lstat_path(path);
    match DirBuilder::new().mode(0o700).create(&norm) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
            let meta = std::fs::symlink_metadata(&norm)?;
            if meta.file_type().is_dir() {
                return Ok(());
            }
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} exists and is not a real directory", norm.display()),
            ))
        }
        Err(err) => Err(err),
    }
}

pub(super) fn create_owner_only_dir_all(path: &Path) -> io::Result<()> {
    let norm = normalize_lstat_path(path);
    match std::fs::symlink_metadata(&norm) {
        Ok(meta) if meta.file_type().is_dir() => return Ok(()),
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} exists and is not a real directory", norm.display()),
            ));
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }
    let mut missing = Vec::new();
    let mut cursor = norm.as_path();
    while let Some(parent) = cursor.parent() {
        if let Some(name) = cursor.file_name() {
            missing.push(name.to_os_string());
        }
        if parent.as_os_str().is_empty() || parent.exists() {
            let mut current = parent.to_path_buf();
            for seg in missing.into_iter().rev() {
                current.push(seg);
                create_one_owner_only_dir(&current)?;
            }
            return Ok(());
        }
        cursor = parent;
    }
    create_one_owner_only_dir(&norm)
}

pub(super) fn write_owner_only_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not a regular file", path.display()),
        ));
    }
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    file.write_all(bytes)
}

struct TempUnlinkGuard<'a> {
    dir: &'a File,
    name: &'a CStr,
    armed: bool,
}

impl Drop for TempUnlinkGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _ = unlink_at(self.dir, self.name);
        }
    }
}

pub(super) fn write_owner_read_only_file_at(
    dir: &File,
    name: &OsStr,
    bytes: &[u8],
) -> io::Result<()> {
    let cname = entry_name(name)?;
    let pid = std::process::id();
    let mut tmp_name = None;
    let mut tmp_file = None;
    for _ in 0..32 {
        let seq = TMP_RO_FILE_SEQ.fetch_add(1, Ordering::Relaxed);
        let candidate = CString::new(format!(".ro-tmp-{pid}-{seq}"))
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid temp filename"))?;
        // SAFETY: `dir` is an open directory descriptor and `candidate` is a NUL-terminated plain
        // entry name that outlives the call; `O_CREAT | O_EXCL | O_NOFOLLOW` creates a fresh file.
        let fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                candidate.as_ptr(),
                libc::O_WRONLY
                    | libc::O_CREAT
                    | libc::O_EXCL
                    | libc::O_NOFOLLOW
                    | libc::O_CLOEXEC
                    | libc::O_NONBLOCK,
                0o600 as libc::c_uint,
            )
        };
        if fd >= 0 {
            // SAFETY: `fd` was just created by this function and nothing else owns it.
            tmp_file = Some(unsafe { File::from_raw_fd(fd) });
            tmp_name = Some(candidate);
            break;
        }
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::AlreadyExists {
            return Err(err);
        }
    }
    let (Some(tmp_cname), Some(mut file)) = (tmp_name, tmp_file) else {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate unique temporary staging filename",
        ));
    };
    let mut guard = TempUnlinkGuard {
        dir,
        name: &tmp_cname,
        armed: true,
    };
    if !file.metadata()?.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "temporary staging entry is not a regular file",
        ));
    }
    file.write_all(bytes)?;
    // SAFETY: `file` is an open descriptor owned by this function; `fchmod` tightens that exact
    // inode to `0400` before it is published to `name`.
    if unsafe { libc::fchmod(file.as_raw_fd(), libc::S_IRUSR) } != 0 {
        return Err(io::Error::last_os_error());
    }
    drop(file);
    super::rename_noreplace_at(dir, &tmp_cname, dir, &cname)?;
    guard.armed = false;
    Ok(())
}

pub(super) fn write_owner_read_only_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let norm = normalize_lstat_path(path);
    let Some(file_name) = norm.file_name() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} has no filename", norm.display()),
        ));
    };
    let parent = match norm.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let parent_dir = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(normalize_lstat_path(parent))?;
    write_owner_read_only_file_at(&parent_dir, file_name, bytes)
}

pub(super) fn open_nofollow_ro(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)?;
    if !file.metadata()?.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not a regular file", path.display()),
        ));
    }
    Ok(file)
}

pub(super) fn create_symlink(original: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(original, link)
}

pub(super) fn effective_uid() -> io::Result<u32> {
    // SAFETY: `geteuid(2)` takes no pointers, has no preconditions, and always succeeds.
    Ok(unsafe { libc::geteuid() })
}

pub(super) fn verify_owner_only_dir(path: &Path, expected_uid: u32) -> io::Result<()> {
    let norm = normalize_lstat_path(path);
    let meta = std::fs::symlink_metadata(&norm)?;
    if !meta.file_type().is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} must be a real directory (never a symlink)",
                norm.display()
            ),
        ));
    }
    if meta.uid() != expected_uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} is not owned by the expected account", norm.display()),
        ));
    }
    if (meta.permissions().mode() & 0o7077) != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} permissions are too permissive (require owner-only 0700 without special bits)",
                norm.display()
            ),
        ));
    }
    Ok(())
}

pub(super) fn verify_owner_only_ancestors(
    path: &Path,
    stop_at: &Path,
    expected_uid: u32,
) -> io::Result<()> {
    if stop_at
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} must not contain `..` components", stop_at.display()),
        ));
    }
    let norm_stop = normalize_lstat_path(stop_at);
    let norm_path = normalize_lstat_path(path);
    let rel = norm_path.strip_prefix(&norm_stop).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{} is not under {}",
                norm_path.display(),
                norm_stop.display()
            ),
        )
    })?;
    verify_owner_only_dir(&norm_stop, expected_uid)?;
    let mut current = norm_stop;
    for component in rel.components() {
        let Component::Normal(seg) = component else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "{} contains disallowed relative component under {}",
                    norm_path.display(),
                    stop_at.display()
                ),
            ));
        };
        current.push(seg);
        verify_owner_only_dir(&current, expected_uid)?;
    }
    Ok(())
}

pub(super) fn verify_owner_only_file(
    path: &Path,
    expected_uid: u32,
    require_exec: bool,
) -> io::Result<PathBuf> {
    let norm = normalize_lstat_path(path);
    let meta = std::fs::symlink_metadata(&norm)?;
    if !meta.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} must be a regular file (never a symlink)",
                norm.display()
            ),
        ));
    }
    if meta.uid() != expected_uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} is not owned by the expected account", norm.display()),
        ));
    }
    let mode = meta.permissions().mode();
    if (mode & 0o7077) != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} permissions are too permissive (require owner-only without special bits)",
                norm.display()
            ),
        ));
    }
    if require_exec && (mode & 0o100) == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} must be executable by owner", norm.display()),
        ));
    }
    norm.canonicalize()
}

/// One plain path component (no `/`, `.`, `..`, or NUL) as a C string for an `*at` call.
pub(super) fn entry_name(name: &OsStr) -> io::Result<CString> {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes == b"." || bytes == b".." || bytes.contains(&b'/') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "an entry name must be one plain path component",
        ));
    }
    CString::new(bytes).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "an entry name must not contain NUL",
        )
    })
}

/// `fstat` on an open descriptor: a real directory owned by `expected_uid`, mode `0700` or
/// narrower, with no setuid, setgid or sticky bit.
fn verify_owner_only_dir_handle(dir: &File, expected_uid: u32) -> io::Result<()> {
    let meta = dir.metadata()?;
    if !meta.file_type().is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the opened entry is not a directory",
        ));
    }
    if meta.uid() != expected_uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the directory is not owned by the expected account",
        ));
    }
    if (meta.permissions().mode() & 0o7077) != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the directory permissions are too permissive (require owner-only 0700)",
        ));
    }
    Ok(())
}

pub(super) fn open_owner_only_dir(path: &Path, expected_uid: u32) -> io::Result<File> {
    let dir = OpenOptions::new()
        .read(true)
        // `O_NONBLOCK` (defense in depth): even a FIFO swapped in at `path` can never block the
        // open; `O_DIRECTORY` then refuses it.
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(normalize_lstat_path(path))?;
    verify_owner_only_dir_handle(&dir, expected_uid)?;
    Ok(dir)
}

pub(super) fn open_owner_only_child(
    dir: &File,
    name: &OsStr,
    expected_uid: u32,
) -> io::Result<File> {
    let cname = entry_name(name)?;
    // SAFETY: `dir` is an open directory descriptor and `cname` is a NUL-terminated plain entry
    // name that outlives the call; `O_DIRECTORY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC` refuses
    // symlinks and FIFOs.
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            cname.as_ptr(),
            libc::O_RDONLY
                | libc::O_DIRECTORY
                | libc::O_NOFOLLOW
                | libc::O_CLOEXEC
                | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `fd` was just opened by this function and nothing else owns it.
    let child = unsafe { File::from_raw_fd(fd) };
    verify_owner_only_dir_handle(&child, expected_uid)?;
    Ok(child)
}

pub(super) fn open_or_create_owner_only_child(
    dir: &File,
    name: &OsStr,
    expected_uid: u32,
) -> io::Result<File> {
    let cname = entry_name(name)?;
    // SAFETY: `dir` is an open directory descriptor and `cname` is a NUL-terminated name that
    // outlives the call; `mkdirat` reads only those.
    if unsafe { libc::mkdirat(dir.as_raw_fd(), cname.as_ptr(), 0o700) } != 0 {
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::AlreadyExists {
            return Err(err);
        }
    }
    // SAFETY: as above; `openat` reads `cname` and returns a new descriptor or -1. `O_NONBLOCK`
    // (defense in depth) keeps a FIFO raced in under `name` from blocking before `O_DIRECTORY`
    // refuses it.
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            cname.as_ptr(),
            libc::O_RDONLY
                | libc::O_DIRECTORY
                | libc::O_NOFOLLOW
                | libc::O_CLOEXEC
                | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `fd` was just opened by this function and nothing else owns it.
    let child = unsafe { File::from_raw_fd(fd) };
    let meta = child.metadata()?;
    if meta.file_type().is_dir()
        && meta.uid() == expected_uid
        && (meta.permissions().mode() & 0o7077) != 0
    {
        // A child the expected account already owns, made wider by a tool that ran inside it
        // (cargo creates `.cargo` as 0755): tighten it through the descriptor, never the path.
        child.set_permissions(std::fs::Permissions::from_mode(0o700))?;
    }
    verify_owner_only_dir_handle(&child, expected_uid)?;
    Ok(child)
}

pub(super) enum EntryKind {
    Missing,
    Regular,
    Symlink,
    Directory,
    Other,
}

/// `fstatat(AT_SYMLINK_NOFOLLOW)` of one name inside `dir`.
pub(super) fn entry_kind_at(dir: &File, name: &CStr) -> io::Result<EntryKind> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `stat` is writable storage for one `stat` structure, `name` is NUL-terminated and
    // outlives the call, and the flag keeps a final symbolic link from being followed.
    let rc = unsafe {
        libc::fstatat(
            dir.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if rc != 0 {
        let err = io::Error::last_os_error();
        return if err.kind() == io::ErrorKind::NotFound {
            Ok(EntryKind::Missing)
        } else {
            Err(err)
        };
    }
    // SAFETY: `fstatat` returned success, so it initialized `stat`.
    let stat = unsafe { stat.assume_init() };
    let kind = stat.st_mode & libc::S_IFMT;
    Ok(if kind == libc::S_IFREG {
        EntryKind::Regular
    } else if kind == libc::S_IFLNK {
        EntryKind::Symlink
    } else if kind == libc::S_IFDIR {
        EntryKind::Directory
    } else {
        EntryKind::Other
    })
}

/// `readlinkat` of one symbolic link inside `dir`: its target bytes.
fn read_link_at(dir: &File, name: &CStr) -> io::Result<Vec<u8>> {
    let mut buf = vec![0u8; usize::try_from(libc::PATH_MAX).unwrap_or(4096) + 1];
    // SAFETY: `buf` is writable for `buf.len()` bytes and `readlinkat` writes at most that many;
    // `name` is NUL-terminated and outlives the call.
    let written = unsafe {
        libc::readlinkat(
            dir.as_raw_fd(),
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            buf.len(),
        )
    };
    let Ok(len) = usize::try_from(written) else {
        return Err(io::Error::last_os_error());
    };
    if len >= buf.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the symbolic link target is too long",
        ));
    }
    buf.truncate(len);
    Ok(buf)
}

/// `unlinkat` of one non-directory name inside `dir`.
pub(super) fn unlink_at(dir: &File, name: &CStr) -> io::Result<()> {
    // SAFETY: `dir` is an open directory descriptor and `name` is NUL-terminated and outlives
    // the call; flag `0` never removes a directory.
    if unsafe { libc::unlinkat(dir.as_raw_fd(), name.as_ptr(), 0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(super) fn ensure_symlink_at(
    dir: &File,
    name: &OsStr,
    target: &Path,
) -> io::Result<SymlinkOutcome> {
    let cname = entry_name(name)?;
    let ctarget = CString::new(target.as_os_str().as_bytes()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "a symbolic link target must not contain NUL",
        )
    })?;
    match entry_kind_at(dir, &cname)? {
        EntryKind::Directory => return Ok(SymlinkOutcome::RealDirectory),
        EntryKind::Regular | EntryKind::Other => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "the entry is neither a symbolic link nor a directory",
            ));
        }
        EntryKind::Symlink => {
            if read_link_at(dir, &cname)? == target.as_os_str().as_bytes() {
                return Ok(SymlinkOutcome::AlreadyLinked);
            }
            unlink_at(dir, &cname)?;
        }
        EntryKind::Missing => {}
    }
    // SAFETY: both strings are NUL-terminated and outlive the call; `dir` is an open directory
    // descriptor, so the link is created inside it and nowhere else.
    if unsafe { libc::symlinkat(ctarget.as_ptr(), dir.as_raw_fd(), cname.as_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(SymlinkOutcome::Created)
}

pub(super) fn remove_symlink_at(dir: &File, name: &OsStr) -> io::Result<bool> {
    let cname = entry_name(name)?;
    match entry_kind_at(dir, &cname)? {
        EntryKind::Symlink => {
            unlink_at(dir, &cname)?;
            Ok(true)
        }
        EntryKind::Missing | EntryKind::Regular | EntryKind::Directory | EntryKind::Other => {
            Ok(false)
        }
    }
}
