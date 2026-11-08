//! Writing a plan's [`PlannedDir`]s and [`PlannedFile`]s before the spawn, never through a
//! symlink, exactly as `org_runtime::PlannedFile` describes:
//!
//! - each location is opened by the path the office gave, with `O_DIRECTORY | O_NOFOLLOW`;
//! - a relative path is walked one component at a time with `openat(O_DIRECTORY |
//!   O_NOFOLLOW)`: a symlink (or a non-directory) anywhere on the way fails the launch;
//! - a missing directory of a [`PlannedDir`] is made with `mkdirat` (`0700`); an existing one
//!   is kept as it is;
//! - a file's contents go to a new temporary file in its directory (`O_CREAT | O_EXCL |
//!   O_NOFOLLOW`, `0600`), which is renamed over the target, so a symlink or hard link at the
//!   target is replaced, never written through.
//!
//! A [`PlannedFile`] with `git_exclude` gets an anchored line in the repository's shared
//! exclude file (`work.git/info/exclude`), which the caller names in [`PlanTargets`] and which
//! is rewritten the same no-follow way.

use std::ffi::CString;
use std::fs::{File, Permissions};
use std::io::{self, Read, Write};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Mutex;

use org_runtime::{FileLocation, LaunchPlan, PlannedDir, PlannedFile, RelPath, SpecPath};

use crate::error::{Error, Result};
use crate::sys;

/// Where a plan's files go, besides the workspace (the plan's own `cwd`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanTargets {
    /// [`FileLocation::RuntimeDir`]: `org_runtime::LaunchContext::runtime_dir`, by its real
    /// path.
    pub runtime_dir: SpecPath,
    /// The repository's shared exclude file (`<work.git>/info/exclude`), by its real path, for
    /// a workspace file with `git_exclude`. `None` when the session has no repository; a plan
    /// that asks for an exclude is then refused.
    pub git_exclude_file: Option<SpecPath>,
    /// The folder every [`org_runtime::CleanupPath`] of the plan must lie inside (e.g.
    /// `/private/tmp/cc-socks` for Claude), by its real path. Required when the plan has
    /// cleanup paths. (Interim: the port's `CleanupPath` will carry its own root.)
    pub cleanup_root: Option<SpecPath>,
}

/// The largest exclude file read back (it's rewritten whole).
const MAX_EXCLUDE_BYTES: u64 = 1 << 20;

/// Serializes read-modify-write of exclude files: sessions of one office share `work.git`.
static EXCLUDE_LOCK: Mutex<()> = Mutex::new(());

/// Creates the plan's directories, then writes its files, then adds its git excludes.
pub(crate) fn write_plan(plan: &LaunchPlan, targets: &PlanTargets) -> Result<()> {
    for dir in &plan.dirs {
        make_dir(plan, targets, dir)?;
    }
    for file in &plan.files {
        write_file(plan, targets, file)?;
    }
    let excludes: Vec<&RelPath> = plan
        .files
        .iter()
        .filter(|f| f.git_exclude)
        .map(|f| {
            if f.location == FileLocation::Workspace {
                Ok(&f.rel_path)
            } else {
                Err(Error::invalid(format!(
                    "`{}` asks for a git exclude but isn't in the workspace",
                    f.rel_path.as_str()
                )))
            }
        })
        .collect::<Result<_>>()?;
    if !excludes.is_empty() {
        let Some(exclude_file) = &targets.git_exclude_file else {
            return Err(Error::invalid(
                "the plan asks for git excludes, but the session names no exclude file",
            ));
        };
        add_excludes(exclude_file, &excludes)?;
    }
    Ok(())
}

fn location_path<'a>(
    plan: &'a LaunchPlan,
    targets: &'a PlanTargets,
    location: FileLocation,
) -> &'a SpecPath {
    match location {
        FileLocation::RuntimeDir => &targets.runtime_dir,
        FileLocation::Workspace => &plan.cwd,
    }
}

fn cstring(path: &Path, bytes: &[u8]) -> Result<CString> {
    CString::new(bytes).map_err(|_| Error::Refused {
        path: path.to_path_buf(),
        why: "it holds a NUL byte",
    })
}

/// Opens the location itself (`O_DIRECTORY | O_NOFOLLOW`).
fn open_location(path: &Path) -> Result<OwnedFd> {
    let c = cstring(path, path.as_os_str().as_bytes())?;
    sys::open_dir_nofollow(&c).map_err(|e| refusal_or_io("open the location", path, e))
}

/// A symlink (`ELOOP`) or a non-directory (`ENOTDIR`) on the way is a refusal; anything else
/// is an I/O error.
fn refusal_or_io(op: &'static str, path: &Path, e: io::Error) -> Error {
    match e.raw_os_error() {
        Some(libc::ELOOP) | Some(libc::ENOTDIR) => Error::Refused {
            path: path.to_path_buf(),
            why: "a symlink or a non-directory is on the way",
        },
        _ => Error::io_at(op, path, e),
    }
}

/// Opens one component under `dir` as a directory, without following a symlink.
fn step(dir: BorrowedFd<'_>, name: &str, full: &Path) -> Result<OwnedFd> {
    let c = cstring(full, name.as_bytes())?;
    sys::openat_dir_nofollow(dir, &c).map_err(|e| refusal_or_io("open a directory", full, e))
}

fn make_dir(plan: &LaunchPlan, targets: &PlanTargets, dir: &PlannedDir) -> Result<()> {
    let base = location_path(plan, targets, dir.location);
    let full = base.to_path_buf().join(dir.rel_path.as_str());
    let mut cur = open_location(&base.to_path_buf())?;
    for name in dir.rel_path.as_str().split('/') {
        let c = cstring(&full, name.as_bytes())?;
        let created = match sys::mkdirat(cur.as_fd(), &c, 0o700) {
            Ok(()) => true,
            Err(e) if e.raw_os_error() == Some(libc::EEXIST) => false,
            Err(e) => return Err(Error::io_at("create a directory", &full, e)),
        };
        let next = step(cur.as_fd(), name, &full)?;
        if created {
            // `mkdirat` applies the umask; make the mode exactly 0700.
            File::from(
                next.try_clone()
                    .map_err(|e| Error::io_at("dup", &full, e))?,
            )
            .set_permissions(Permissions::from_mode(0o700))
            .map_err(|e| Error::io_at("chmod a directory", &full, e))?;
        }
        cur = next;
    }
    Ok(())
}

/// Opens the directory that holds `rel`'s last component (every component before it must
/// exist), returning it with the leaf name.
fn open_parent<'r>(base: &Path, rel: &'r str, full: &Path) -> Result<(OwnedFd, &'r str)> {
    let mut cur = open_location(base)?;
    let mut parts: Vec<&str> = rel.split('/').collect();
    let leaf = parts.pop().unwrap_or(rel);
    for name in parts {
        cur = step(cur.as_fd(), name, full)?;
    }
    Ok((cur, leaf))
}

fn write_file(plan: &LaunchPlan, targets: &PlanTargets, file: &PlannedFile) -> Result<()> {
    let base = location_path(plan, targets, file.location).to_path_buf();
    let full = base.join(file.rel_path.as_str());
    let (dir, leaf) = open_parent(&base, file.rel_path.as_str(), &full)?;
    replace_file(dir.as_fd(), leaf, &file.contents, 0o600, &full)
}

/// Writes `contents` to a new temporary file in `dir` and renames it over `leaf`.
fn replace_file(
    dir: BorrowedFd<'_>,
    leaf: &str,
    contents: &[u8],
    mode: u32,
    full: &Path,
) -> Result<()> {
    let leaf_c = cstring(full, leaf.as_bytes())?;
    let tmp_name = format!(".org-supervise-{:016x}.tmp", sys::random_u64());
    let tmp_c = cstring(full, tmp_name.as_bytes())?;
    let fd = sys::openat_create_excl(dir, &tmp_c, 0o600)
        .map_err(|e| Error::io_at("create a temporary file", full, e))?;
    let mut f = File::from(fd);
    let written = f
        .set_permissions(Permissions::from_mode(mode))
        .and_then(|()| f.write_all(contents))
        .map_err(|e| Error::io_at("write", full, e))
        .and_then(|()| {
            sys::renameat(dir, &tmp_c, &leaf_c)
                .map_err(|e| Error::io_at("rename into place", full, e))
        });
    if written.is_err() {
        let _ = sys::unlinkat(dir, &tmp_c);
    }
    written
}

/// Escapes a workspace path as one anchored gitignore pattern that matches only that path.
fn exclude_line(rel: &RelPath) -> String {
    let mut line = String::from("/");
    let s = rel.as_str();
    let trailing_spaces = s.len() - s.trim_end_matches(' ').len();
    let body = &s[..s.len() - trailing_spaces];
    for c in body.chars() {
        if matches!(c, '\\' | '*' | '?' | '[' | ']') {
            line.push('\\');
        }
        line.push(c);
    }
    for _ in 0..trailing_spaces {
        line.push_str("\\ ");
    }
    line
}

fn add_excludes(exclude_file: &SpecPath, rels: &[&RelPath]) -> Result<()> {
    let full = exclude_file.to_path_buf();
    let parent = full
        .parent()
        .filter(|p| *p != Path::new("/"))
        .ok_or_else(|| Error::invalid("the exclude file has no parent directory"))?
        .to_path_buf();
    let leaf = full
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::invalid("the exclude file has no name"))?;
    let _guard = EXCLUDE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let dir = open_location(&parent)?;
    let leaf_c = cstring(&full, leaf.as_bytes())?;
    let mut text = String::new();
    match sys::openat_read_nofollow(dir.as_fd(), &leaf_c) {
        Ok(fd) => {
            let f = File::from(fd);
            let meta = f.metadata().map_err(|e| Error::io_at("stat", &full, e))?;
            if !meta.is_file() {
                return Err(Error::Refused {
                    path: full,
                    why: "it isn't a regular file",
                });
            }
            f.take(MAX_EXCLUDE_BYTES + 1)
                .read_to_string(&mut text)
                .map_err(|e| Error::io_at("read", &full, e))?;
            if text.len() as u64 > MAX_EXCLUDE_BYTES {
                return Err(Error::Refused {
                    path: full,
                    why: "it's too large",
                });
            }
        }
        Err(e) if e.raw_os_error() == Some(libc::ENOENT) => {}
        Err(e) => return Err(refusal_or_io("open", &full, e)),
    }
    let missing: Vec<String> = rels
        .iter()
        .map(|r| exclude_line(r))
        .filter(|line| !text.lines().any(|l| l == line))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    for line in missing {
        text.push_str(&line);
        text.push('\n');
    }
    replace_file(dir.as_fd(), leaf, text.as_bytes(), 0o644, &full)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclude_lines_are_anchored_and_escaped() {
        let r = |s: &str| RelPath::try_from(s).unwrap();
        assert_eq!(exclude_line(&r("CLAUDE.md")), "/CLAUDE.md");
        assert_eq!(exclude_line(&r("a/b c")), "/a/b c");
        assert_eq!(exclude_line(&r("a*[x]?\\")), "/a\\*\\[x\\]\\?\\\\");
        assert_eq!(exclude_line(&r("t  ")), "/t\\ \\ ");
        assert_eq!(exclude_line(&r("!x")), "/!x");
        assert_eq!(exclude_line(&r("#x")), "/#x");
    }
}
