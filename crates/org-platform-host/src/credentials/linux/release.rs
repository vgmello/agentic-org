//! GNU/Linux: the release directory's live probes, and the order of its checks.
//!
//! Every decision is a pure function of `rules.rs`; this file opens the directories,
//! asks the kernel and the account database for the facts, and stops at the first
//! rule that is broken, naming the directory that broke it. The contract, the layout
//! and the list of checks are in `release.rs`.
//!
//! The walk holds one descriptor at a time. A directory above the release directory
//! is opened with `O_PATH`: the service user needs to pass through it, not to list it
//! (a home directory's parent may be searchable only), and `fstat` and `openat` work
//! on such a descriptor. Each is judged before anything below it is opened, so an
//! unsafe directory is never walked through.

use std::ffi::{CStr, CString, OsStr, OsString};
use std::fs::{File, TryLockError};
use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::account;
use crate::credentials::rules::{self, Ancestor, Identity};
use crate::credentials::{
    directory, CredentialError, ReleaseDirectory, ReleaseItems, RootRefusal, RootRule,
};

/// The lock file's name. An item store's directory has the syntax of an item name,
/// which never starts with `.`, so none can be the lock.
const LOCK: &CStr = c".lock";

/// The most directories a walk up to `/` passes before it gives up: more than any
/// path the kernel resolves can have.
const DEEPEST: usize = 4096;

/// One open release directory (see `release.rs`). The directory's lock is held by the
/// shared [`Root`], which this handle and every store opened from it keep alive.
pub(in crate::credentials) struct ReleaseRoot(Arc<Root>);

/// The checked release directory and its lock.
pub(super) struct Root {
    /// The release directory, held since it was checked.
    directory: File,
    /// Its path: `/` and the single names that were walked, so it holds no link.
    path: PathBuf,
    /// The lock file, locked exclusively.
    lock: File,
}

impl Drop for Root {
    fn drop(&mut self) {
        // A descriptor a forked child inherited may outlive this owner; closing
        // alone would then not release the lock. Drop cannot report a failure, and
        // the file still closes afterwards.
        let _ = self.lock.unlock();
    }
}

pub(in crate::credentials) fn release_directory(
    location: &ReleaseDirectory<'_>,
) -> Result<ReleaseRoot, CredentialError> {
    // Before anything is opened.
    let names = rules::components(location.path).map_err(|rule| refused(rule, location.path))?;
    let start = Path::new("/");
    let root = open_absolute(start).map_err(|error| refused(component_rule(&error), start))?;
    open(Walk { start, root, names }, location)
}

/// The release directory at `location`, with the walk started at the directory `base`
/// instead of at `/`: for tests, whose temporary directories are below a directory
/// that is writable by others. `base` and everything below it is opened and judged
/// exactly as in a host's walk; the directories above `base` are not. The path must be
/// `base` or below it, spelled with `base` as its prefix.
#[cfg(any(test, feature = "test-support"))]
pub(in crate::credentials) fn release_directory_below(
    base: &Path,
    location: &ReleaseDirectory<'_>,
) -> Result<ReleaseRoot, CredentialError> {
    let below = location
        .path
        .strip_prefix(base)
        .map_err(|_| refused(RootRule::Relative, location.path))?;
    // The same syntax rule, on the part of the path that is walked.
    let names = rules::names(below).map_err(|rule| refused(rule, location.path))?;
    let root = open_absolute(base).map_err(|error| refused(component_rule(&error), base))?;
    open(
        Walk {
            start: base,
            root,
            names,
        },
        location,
    )
}

/// A walk to a release directory: the directory it starts at, and the single names
/// from there. A host's walk starts at `/`; that is the only start `release_directory`
/// makes. The start itself is judged like every directory below it.
struct Walk<'a> {
    /// The path of the directory the walk starts at.
    start: &'a Path,
    /// That directory, open.
    root: OwnedFd,
    /// The names from there down to the release directory, which is the last.
    names: Vec<&'a OsStr>,
}

fn open(walk: Walk<'_>, location: &ReleaseDirectory<'_>) -> Result<ReleaseRoot, CredentialError> {
    // SAFETY: no arguments and no side effects; it cannot fail.
    let service_user = unsafe { libc::geteuid() };
    let mut private_group = PrivateGroup::NotAsked;
    let mut path = walk.start.to_path_buf();
    let mut current = walk.root;
    let directory = match walk.names.split_last() {
        Some((store, above)) => {
            for name in above {
                judge_ancestor(current.as_fd(), &path, service_user, &mut private_group)?;
                path.push(name);
                current = open_at(current.as_fd(), name, libc::O_PATH)
                    .map_err(|error| refused(component_rule(&error), &path))?;
            }
            judge_ancestor(current.as_fd(), &path, service_user, &mut private_group)?;
            path.push(store);
            open_at(current.as_fd(), store, libc::O_RDONLY)
        }
        // The walk's start is the release directory itself: nothing is above it.
        None => open_at(current.as_fd(), OsStr::new("."), libc::O_RDONLY),
    }
    .map_err(|error| refused(component_rule(&error), &path))?;
    let directory = File::from(directory);
    let refuse = |rule| refused(rule, &path);
    let unavailable = |error: io::Error| refused(RootRule::Unavailable(error.kind()), &path);

    let stat = status(directory.as_fd()).map_err(unavailable)?;
    rules::store(stat.st_uid, stat.st_mode, service_user).map_err(refuse)?;
    rules::filesystem(filesystem_kind(directory.as_fd()).map_err(unavailable)?).map_err(refuse)?;
    let store_chain = chain(directory.as_fd()).map_err(unavailable)?;
    let data_chain = chain(location.data_root)
        .map_err(|error| refused(RootRule::DataRoot(error.kind()), &path))?;
    rules::disjoint(&store_chain, &data_chain).map_err(refuse)?;
    if location.mount_point_required {
        let (attributes, reported) = mount_attributes(directory.as_fd()).map_err(unavailable)?;
        rules::mount_point(attributes, reported).map_err(refuse)?;
    }
    if location.expect_items {
        let items = count_items(directory.as_fd(), service_user).map_err(unavailable)?;
        rules::items_present(true, items).map_err(refuse)?;
    }
    // The first thing that is made, after every check of the directory has passed.
    let lock = open_lock(directory.as_fd()).map_err(|error| match error.raw_os_error() {
        // A link, a directory or a device node at the lock's name is no lock file.
        Some(libc::ELOOP | libc::EISDIR | libc::ENXIO) => refuse(RootRule::LockFile),
        _ => unavailable(error),
    })?;
    let lock_is = lock.metadata().map_err(unavailable)?;
    rules::lock_file(
        lock_is.is_file(),
        lock_is.uid(),
        lock_is.mode(),
        lock_is.nlink(),
        service_user,
    )
    .map_err(refuse)?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Err(refuse(RootRule::InUse)),
        Err(TryLockError::Error(error)) => return Err(unavailable(error)),
    }
    Ok(ReleaseRoot(Arc::new(Root {
        directory,
        path,
        lock,
    })))
}

impl ReleaseRoot {
    pub(in crate::credentials) fn store(
        &self,
        items: &ReleaseItems<'_>,
    ) -> Result<super::Store, CredentialError> {
        // The engine opens the item store's directory relative to this descriptor,
        // without following a link, and checks it as it checks every directory.
        let anchor = self.0.directory.try_clone()?;
        let store = directory::Store::below(
            anchor,
            self.0.path.clone(),
            &[items.directory],
            true,
            items.sharing,
            items.suffix,
        )?;
        Ok(super::Store {
            directory: store,
            _root: Some(Arc::clone(&self.0)),
        })
    }
}

fn refused(rule: RootRule, path: &Path) -> CredentialError {
    CredentialError::Root(RootRefusal {
        rule,
        path: path.to_owned(),
    })
}

/// The rule a component that could not be opened broke: one that is not a directory,
/// a symbolic link among them (`O_NOFOLLOW` answers `ELOOP`, with `O_PATH` and
/// `O_DIRECTORY` together `ENOTDIR`), or one that is unavailable.
fn component_rule(error: &io::Error) -> RootRule {
    match error.raw_os_error() {
        Some(libc::ELOOP | libc::ENOTDIR) => RootRule::NotADirectory,
        _ => RootRule::Unavailable(error.kind()),
    }
}

/// The service user's private group, asked for at most once per open and only when a
/// directory's rule depends on it.
enum PrivateGroup {
    NotAsked,
    Known(Option<libc::gid_t>),
}

/// Judges the directory `ancestor`, at `path`, as a directory above the release
/// directory.
fn judge_ancestor(
    ancestor: BorrowedFd<'_>,
    path: &Path,
    service_user: libc::uid_t,
    private_group: &mut PrivateGroup,
) -> Result<(), CredentialError> {
    let stat =
        status(ancestor).map_err(|error| refused(RootRule::Unavailable(error.kind()), path))?;
    let group = if rules::needs_private_group(stat.st_uid, stat.st_mode, service_user) {
        if let PrivateGroup::NotAsked = private_group {
            // An account database that cannot be read vouches for no group.
            *private_group =
                PrivateGroup::Known(account::private_group_of(service_user).ok().flatten());
        }
        match private_group {
            PrivateGroup::Known(group) => *group,
            PrivateGroup::NotAsked => None,
        }
    } else {
        None
    };
    rules::ancestor(&Ancestor {
        owner: stat.st_uid,
        group: stat.st_gid,
        mode: stat.st_mode,
        service_user,
        private_group: group,
    })
    .map_err(|rule| refused(rule, path))
}

fn owned(fd: libc::c_int) -> io::Result<OwnedFd> {
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a new descriptor that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn c_name(name: &OsStr) -> io::Result<CString> {
    CString::new(name.as_bytes()).map_err(|_| io::ErrorKind::InvalidInput.into())
}

/// Opens the directory at the absolute path `path` for passing through. Only the
/// start of a walk is opened by path.
fn open_absolute(path: &Path) -> io::Result<OwnedFd> {
    let path = c_name(path.as_os_str())?;
    let flags = libc::O_PATH | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    // SAFETY: a NUL-terminated path that outlives the call, which keeps nothing.
    owned(unsafe { libc::open(path.as_ptr(), flags) })
}

/// Opens the directory `name` in `parent`, never following a link: `access` is
/// `O_PATH` to pass through it or `O_RDONLY` to read it.
fn open_at(parent: BorrowedFd<'_>, name: &OsStr, access: libc::c_int) -> io::Result<OwnedFd> {
    let name = c_name(name)?;
    let flags = access | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    // SAFETY: a valid directory descriptor and a NUL-terminated name that outlives
    // the call, which keeps nothing.
    owned(unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) })
}

fn status(fd: BorrowedFd<'_>) -> io::Result<libc::stat> {
    // SAFETY: an all-zero `stat` is a valid value for the call to overwrite.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: a valid descriptor and a valid out-pointer.
    if unsafe { libc::fstat(fd.as_raw_fd(), &mut stat) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(stat)
}

/// The type number of the filesystem `fd` is on, as `fstatfs` gives it.
fn filesystem_kind(fd: BorrowedFd<'_>) -> io::Result<u64> {
    // SAFETY: an all-zero `statfs` is a valid value for the call to overwrite.
    let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: a valid descriptor and a valid out-pointer.
    if unsafe { libc::fstatfs(fd.as_raw_fd(), &mut stat) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // The number is 32 bits wide; where the field is a signed 32-bit word, a number
    // with the top bit set arrives sign-extended.
    Ok(stat.f_type as u64 & 0xffff_ffff)
}

fn identity(stat: &libc::stat) -> Identity {
    (stat.st_dev, stat.st_ino)
}

/// The directory `fd` and every directory above it up to `/`, each identified through
/// a held descriptor: `..` is opened relative to the one before, until a directory is
/// its own parent.
fn chain(fd: BorrowedFd<'_>) -> io::Result<Vec<Identity>> {
    let first = status(fd)?;
    if first.st_mode & libc::S_IFMT != libc::S_IFDIR {
        return Err(io::ErrorKind::NotADirectory.into());
    }
    let mut chain = vec![identity(&first)];
    let mut above = open_at(fd, OsStr::new(".."), libc::O_PATH)?;
    loop {
        let next = identity(&status(above.as_fd())?);
        if chain.last() == Some(&next) {
            return Ok(chain);
        }
        if chain.len() >= DEEPEST {
            return Err(io::ErrorKind::InvalidData.into());
        }
        chain.push(next);
        above = open_at(above.as_fd(), OsStr::new(".."), libc::O_PATH)?;
    }
}

/// What `statx` says of the directory `fd`: its attributes, and the mask of the
/// attributes this kernel reports.
fn mount_attributes(fd: BorrowedFd<'_>) -> io::Result<(u64, u64)> {
    // SAFETY: an all-zero `statx` is a valid value for the call to overwrite.
    let mut stat: libc::statx = unsafe { std::mem::zeroed() };
    // SAFETY: a valid descriptor, an empty NUL-terminated path with `AT_EMPTY_PATH`
    // (the descriptor itself is examined), and a valid out-pointer.
    let status = unsafe {
        libc::statx(
            fd.as_raw_fd(),
            c"".as_ptr(),
            libc::AT_EMPTY_PATH,
            libc::STATX_TYPE,
            &mut stat,
        )
    };
    if status != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((stat.stx_attributes, stat.stx_attributes_mask))
}

/// How many items the release directory `root` holds, for the empty-store rule: the
/// regular files that count as items ([`rules::counts_as_item`]) in the directories
/// directly below `root` that could be an item store, which are the service user's
/// with mode `0700`. Nothing else is looked into, and nothing is followed.
fn count_items(root: BorrowedFd<'_>, service_user: libc::uid_t) -> io::Result<usize> {
    let mut items = 0;
    for name in entries(root)? {
        let directory = match open_at(root, &name, libc::O_RDONLY) {
            Ok(directory) => directory,
            // Not a directory, a link, or a directory this user may not read (a
            // volume's `lost+found`): no item store.
            Err(error)
                if matches!(
                    error.raw_os_error(),
                    Some(libc::ENOTDIR | libc::ELOOP | libc::EACCES | libc::ENOENT)
                ) =>
            {
                continue
            }
            Err(error) => return Err(error),
        };
        let stat = status(directory.as_fd())?;
        if rules::store(stat.st_uid, stat.st_mode, service_user).is_err() {
            continue;
        }
        for entry in entries(directory.as_fd())? {
            if rules::counts_as_item(entry.as_bytes()) && is_regular(directory.as_fd(), &entry)? {
                items += 1;
            }
        }
    }
    Ok(items)
}

/// Whether the entry `name` of `directory` is a regular file. A link is not followed;
/// an entry that is gone again is no file.
fn is_regular(directory: BorrowedFd<'_>, name: &OsStr) -> io::Result<bool> {
    let name = c_name(name)?;
    // SAFETY: an all-zero `stat` is a valid value for the call to overwrite.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: a valid directory descriptor, a NUL-terminated name that outlives the
    // call, and a valid out-pointer.
    let status = unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            name.as_ptr(),
            &mut stat,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if status != 0 {
        let error = io::Error::last_os_error();
        return if error.kind() == io::ErrorKind::NotFound {
            Ok(false)
        } else {
            Err(error)
        };
    }
    Ok(stat.st_mode & libc::S_IFMT == libc::S_IFREG)
}

/// Every name in `directory` but `.` and `..`, read through a descriptor of its own,
/// so that the held one's position is never moved.
fn entries(directory: BorrowedFd<'_>) -> io::Result<Vec<OsString>> {
    let fresh = open_at(directory, OsStr::new("."), libc::O_RDONLY)?;
    // SAFETY: a valid directory descriptor. On success the stream owns it; on failure
    // `fresh` still does and closes it.
    let stream = unsafe { libc::fdopendir(fresh.as_raw_fd()) };
    if stream.is_null() {
        return Err(io::Error::last_os_error());
    }
    std::mem::forget(fresh);
    let stream = Stream(stream);
    let mut names = Vec::new();
    loop {
        // `readdir` reports the end of the directory and a failure the same way.
        org_platform::fs::set_errno(0)?;
        // SAFETY: `stream` owns a live directory stream that no other thread reads.
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(0) {
                return Err(error);
            }
            return Ok(names);
        }
        // SAFETY: `readdir`'s NUL-terminated name stays valid until the next call.
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if name != b"." && name != b".." {
            names.push(OsString::from_vec(name.to_vec()));
        }
    }
}

struct Stream(*mut libc::DIR);

impl Drop for Stream {
    fn drop(&mut self) {
        // SAFETY: this guard exclusively owns the successful `fdopendir` result.
        unsafe {
            libc::closedir(self.0);
        }
    }
}

/// Opens the lock file in the release directory, making it (mode `0600`) if it is
/// missing, and never following a link. `O_NONBLOCK`: a FIFO at the name must not
/// block the host; the lock rule then refuses it.
fn open_lock(directory: BorrowedFd<'_>) -> io::Result<File> {
    let flags =
        libc::O_RDWR | libc::O_CREAT | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
    // SAFETY: a valid directory descriptor and a NUL-terminated name; the mode is
    // read because `O_CREAT` is set.
    let fd = unsafe { libc::openat(directory.as_raw_fd(), LOCK.as_ptr(), flags, 0o600) };
    owned(fd).map(File::from)
}

#[cfg(test)]
mod tests;
