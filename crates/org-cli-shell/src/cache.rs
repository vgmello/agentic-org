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

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::ShellError;
use crate::escape::{is_unsafe_char, sanitize};

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
        fs::DirBuilder::new()
            .recursive(true)
            .create(&dir)
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

    /// The `kind` list of `partition`: empty when missing, unreadable, too large or corrupt.
    /// Takes no lock.
    pub fn load(&self, partition: &[&str], kind: &str) -> Vec<CachedCandidate> {
        let path = self
            .partition_dir(partition)
            .join(format!("{}.json", partition_key(kind)));
        read_list(&path).unwrap_or_default()
    }
}

/// Opens (creating) and exclusively locks `path`, retrying for at most [`LOCK_WAIT`]. The
/// lock is released when the file is dropped.
fn lock(path: &Path) -> Result<File, ShellError> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|e| ShellError::io("opening the completion cache lock", e))?;
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

fn read_list(path: &Path) -> Option<Vec<CachedCandidate>> {
    if !fs::metadata(path).ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(MAX_LIST_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_LIST_BYTES {
        return None;
    }
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
