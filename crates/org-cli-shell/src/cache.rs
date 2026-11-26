//! The completion cache: candidate names (instances, org slugs, office IDs, project keys)
//! that `<TAB>` offers without asking the host.
//!
//! A list lives in `<dir>/completion/<partition...>/<kind>.json`, every path component
//! percent-encoded by [`partition_key`] so that no value can leave the cache directory. A
//! writer holds an exclusive lock on the sibling `<kind>.lock` (waiting at most 2 s), writes a
//! temporary file in the same directory and renames it over the list, so a reader sees the
//! old list or the new one, never a torn write. Readers take no lock and never fail: a missing
//! or corrupt list is empty. Names with control or bidirectional characters are dropped on
//! the way in and out; help text is sanitized on the way out.
//!
//! The folders from `completion` down are the user's alone (`0700`, created so and tightened
//! when they exist): the names they list are not for other users to read. A writer walks them
//! one at a time through descriptors (`org_platform::fs::OwnerOnlyDir`) before it writes, so
//! a symbolic link or another account's folder from `completion` down is refused, never
//! followed. The cache directory itself is the caller's: created owner-only when it is missing
//! (and refused when it then does not open as such), refused when it is a symbolic link, not a
//! directory, another account's or writable by anyone (checked on its opened handle), left as
//! it is otherwise. The lock, the temporary file and the list are then reached
//! by path inside the walked folders (a follow-up moves them to `*at` calls on the walked
//! descriptor through `org-platform`); the lock is opened without following a link or
//! blocking on a FIFO and must be a regular file. A reader gets an empty list when the cache
//! directory or any folder from `completion` down is a symbolic link, and reads the list
//! through [`crate::files::read_regular`].

use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use org_platform::fs::{OwnerOnlyDir, O_CLOEXEC, O_NOFOLLOW, O_NONBLOCK};
use serde::{Deserialize, Serialize};

use crate::error::ShellError;
use crate::escape::{is_unsafe_char, sanitize};
use crate::files::read_regular;

/// The cache's subdirectory for completion lists.
pub const COMPLETION_DIR: &str = "completion";
/// The most candidates a list keeps.
pub const MAX_CANDIDATES: usize = 1000;
/// The longest name kept, in bytes.
pub const MAX_NAME_BYTES: usize = 256;
/// The largest list file read.
const MAX_LIST_BYTES: u64 = 1024 * 1024;
/// How long a writer waits for the lock, and how often it tries.
const LOCK_WAIT: Duration = Duration::from_secs(2);
const LOCK_RETRY: Duration = Duration::from_millis(10);

/// One completion candidate.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CachedCandidate {
    pub name: String,
    pub help: Option<String>,
}

/// The completion cache under a cache directory (see [`crate::paths`]).
#[derive(Clone, Debug)]
pub struct CompletionCache {
    dir: PathBuf,
}

impl CompletionCache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The cache directory it was made with.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn partition_dir(&self, partition: &[&str]) -> PathBuf {
        let mut dir = self.dir.join(COMPLETION_DIR);
        for part in partition {
            dir.push(partition_key(part));
        }
        dir
    }

    /// Replaces the `kind` list of `partition` (e.g. `["prod"]`, `"orgs"`) with the valid
    /// names among `candidates` (the first [`MAX_CANDIDATES`]). Directories are created as
    /// needed. Fails with [`ShellError::Io`] when the lock stays held for 2 s or a file
    /// cannot be written; callers treat the cache as best effort.
    pub fn store(
        &self,
        partition: &[&str],
        kind: &str,
        candidates: &[CachedCandidate],
    ) -> Result<(), ShellError> {
        let dir = self.partition_dir(partition);
        let kind = partition_key(kind);
        self.create_private(partition)
            .map_err(|e| ShellError::io("creating the completion cache", e))?;
        let _lock = lock(&dir.join(format!("{kind}.lock")))?;
        let kept: Vec<&CachedCandidate> = candidates
            .iter()
            .filter(|candidate| valid_name(&candidate.name))
            .take(MAX_CANDIDATES)
            .collect();
        let bytes = serde_json::to_vec(&kept)
            .map_err(|e| ShellError::io("encoding the completion cache", io::Error::other(e)))?;
        let write_error = |e| ShellError::io("writing the completion cache", e);
        let mut temporary = tempfile::NamedTempFile::new_in(&dir).map_err(write_error)?;
        temporary.write_all(&bytes).map_err(write_error)?;
        temporary
            .persist(dir.join(format!("{kind}.json")))
            .map_err(|e| write_error(e.error))?;
        Ok(())
    }

    /// Opens `<cache>/completion/<partition...>` one folder at a time through descriptors:
    /// each is created `0700` when missing, tightened to `0700` through its descriptor when it
    /// is the user's and more open, and refused when it is a symbolic link, not a directory or
    /// another account's. On a system without an adapter it fails closed (`org_platform::fs`).
    fn create_private(&self, partition: &[&str]) -> io::Result<()> {
        let uid = org_platform::fs::effective_uid()?;
        let mut dir = self.open_completion(uid)?;
        for part in partition {
            dir = dir.open_or_create_child(OsStr::new(&partition_key(part)), uid)?;
        }
        Ok(())
    }

    /// `<cache>/completion`, verified by descriptor. A missing cache directory is created
    /// owner-only and must then open as such: one that does not (raced, replaced or loosened
    /// after it was made) is refused. An existing one that is a symbolic link or not a
    /// directory is refused; one that is not the user's alone is the caller's: it is opened
    /// without following a link and refused, by its opened handle, when it is not a directory,
    /// is another account's or is writable by anyone, and otherwise left as it is,
    /// `completion` then made or tightened through its own descriptor.
    fn open_completion(&self, uid: u32) -> io::Result<OwnerOnlyDir> {
        // `components` drops a trailing `/` or `/.`, which would make `lstat` follow a link.
        let root: PathBuf = self.dir.components().collect();
        let created = match fs::symlink_metadata(&root) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                org_platform::fs::create_owner_only_dir_all(&root)?;
                true
            }
            Err(error) => return Err(error),
            Ok(metadata) if !metadata.is_dir() => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("the cache directory {} is not a directory", root.display()),
                ))
            }
            Ok(_) => false,
        };
        match OwnerOnlyDir::open(&root, uid) {
            Ok(owned) => owned.open_or_create_child(OsStr::new(COMPLETION_DIR), uid),
            // Made owner-only just now, yet not so when opened: never the caller's to keep.
            Err(error) if created => Err(error),
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                // The caller's directory, more open than owner-only. Another account, or anyone
                // at all, could swap `completion` for a link after this walk: refuse it.
                verify_caller_dir(&root, uid)?;
                let completion = root.join(COMPLETION_DIR);
                match org_platform::fs::create_owner_only_dir(&completion) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                        tighten_own_dir(&completion, uid)?;
                    }
                    Err(error) => return Err(error),
                }
                OwnerOnlyDir::open(&completion, uid)
            }
            Err(error) => Err(error),
        }
    }

    /// The `kind` list of `partition`: empty when missing, unreadable, too large or corrupt,
    /// or when the cache directory or a folder from `completion` down is a symbolic link (as
    /// [`CompletionCache::store`] refuses them). Takes no lock.
    pub fn load(&self, partition: &[&str], kind: &str) -> Vec<CachedCandidate> {
        let real_dir = |path: &Path| fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir());
        // `components` drops a trailing `/` or `/.`, which would make `lstat` follow a link.
        let mut dir: PathBuf = self.dir.components().collect();
        if !real_dir(&dir) {
            return Vec::new();
        }
        let folders = std::iter::once(COMPLETION_DIR.to_owned())
            .chain(partition.iter().map(|part| partition_key(part)));
        for folder in folders {
            dir.push(folder);
            if !real_dir(&dir) {
                return Vec::new();
            }
        }
        read_list(&dir.join(format!("{}.json", partition_key(kind)))).unwrap_or_default()
    }
}

/// The user's own directory at `path`, opened without following a symbolic link or blocking
/// on a FIFO, with its metadata read from the opened handle (`fstat`, never an `lstat` made
/// before the open). A link, another account's directory or any other entry is refused.
fn open_own_dir(path: &Path, uid: u32) -> io::Result<(File, fs::Metadata)> {
    let dir = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC)
        .open(path)?;
    let metadata = dir.metadata()?;
    if !metadata.is_dir() || metadata.uid() != uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} is not the user's own directory", path.display()),
        ));
    }
    Ok((dir, metadata))
}

/// Verifies the caller's cache directory at `path` on its opened handle ([`open_own_dir`]):
/// the user's own directory, and not writable by anyone.
fn verify_caller_dir(path: &Path, uid: u32) -> io::Result<()> {
    let (_dir, metadata) = open_own_dir(path, uid)?;
    if metadata.mode() & 0o002 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "the cache directory {} is writable by anyone",
                path.display()
            ),
        ));
    }
    Ok(())
}

/// Tightens the user's own directory at `path` ([`open_own_dir`]) to `0700` through its
/// descriptor.
fn tighten_own_dir(path: &Path, uid: u32) -> io::Result<()> {
    let (dir, metadata) = open_own_dir(path, uid)?;
    if metadata.permissions().mode() & 0o7077 != 0 {
        dir.set_permissions(fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Opens (creating, `0600`) and exclusively locks `path`, retrying for at most
/// [`LOCK_WAIT`]. The open neither follows a symbolic link nor blocks on a FIFO, and the
/// opened handle must be a regular file. The lock is released when the file is dropped.
fn lock(path: &Path) -> Result<File, ShellError> {
    let open_error = |e| ShellError::io("opening the completion cache lock", e);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC)
        .open(path)
        .map_err(open_error)?;
    if !file.metadata().map_err(open_error)?.is_file() {
        return Err(open_error(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the lock is not a regular file",
        )));
    }
    let deadline = Instant::now() + LOCK_WAIT;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                thread::sleep(LOCK_RETRY);
            }
            Err(TryLockError::WouldBlock) => {
                return Err(ShellError::io(
                    "waiting for the completion cache lock",
                    io::Error::new(io::ErrorKind::TimedOut, "another writer holds it"),
                ));
            }
            Err(TryLockError::Error(e)) => {
                return Err(ShellError::io("locking the completion cache", e));
            }
        }
    }
}

/// A list: `None` when it is missing, a symbolic link, not a regular file, larger than
/// [`MAX_LIST_BYTES`] or corrupt.
fn read_list(path: &Path) -> Option<Vec<CachedCandidate>> {
    let bytes = read_regular(path, MAX_LIST_BYTES).ok()?;
    let raw: Vec<CachedCandidate> = serde_json::from_slice(&bytes).ok()?;
    Some(
        raw.into_iter()
            .filter(|candidate| valid_name(&candidate.name))
            .take(MAX_CANDIDATES)
            .map(|candidate| CachedCandidate {
                help: candidate.help.map(|help| sanitize(&help).into_owned()),
                name: candidate.name,
            })
            .collect(),
    )
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= MAX_NAME_BYTES && !name.chars().any(is_unsafe_char)
}

/// A path component for `part`: every byte outside `[A-Za-z0-9._-]` as `%XX`, `.` and `..`
/// as `%2E` and `%2E%2E`, and the empty string as `%`. Different parts never share a key,
/// and no key is a path separator, a parent reference or a control character.
pub fn partition_key(part: &str) -> String {
    match part {
        "" => return String::from("%"),
        "." => return String::from("%2E"),
        ".." => return String::from("%2E%2E"),
        _ => {}
    }
    let mut key = String::with_capacity(part.len());
    for byte in part.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-') {
            key.push(char::from(byte));
        } else {
            key.push_str(&format!("%{byte:02X}"));
        }
    }
    key
}

/// The last value of any flag in `names` within `args` (a command line being completed), in
/// the forms `--name X`, `--name=X` and, as bash splits words at `=`, `--name = X`. A
/// following flag is not a value; the scan stops at `--`.
pub fn scan_flag(args: &[OsString], names: &[&str]) -> Option<String> {
    let mut found = None;
    let mut words = args.iter().map(|arg| arg.to_str()).peekable();
    while let Some(word) = words.next() {
        let Some(word) = word else { continue };
        if word == "--" {
            break;
        }
        if names.contains(&word) {
            if words.peek() == Some(&Some("=")) {
                words.next();
            }
            if let Some(Some(value)) = words.next_if(|next| next.is_some_and(is_value)) {
                found = Some(value.to_owned());
            }
            continue;
        }
        let attached = names.iter().find_map(|name| {
            word.strip_prefix(name)
                .and_then(|rest| rest.strip_prefix('='))
        });
        if let Some(value) = attached.filter(|value| !value.is_empty()) {
            found = Some(value.to_owned());
        }
    }
    found
}

fn is_value(word: &str) -> bool {
    !word.is_empty() && !word.starts_with('-')
}

#[cfg(test)]
mod tests;
