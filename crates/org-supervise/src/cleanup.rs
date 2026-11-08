//! Removing what a runtime leaves behind when it's killed (Claude's
//! `/private/tmp/cc-socks/<pid>.sock`), from a plan's [`CleanupPath`]s.
//!
//! The rule, for each path:
//! 1. It's resolved for the child's pid, and must lie **inside the allowed root** the caller
//!    gives (e.g. `/private/tmp/cc-socks`), strictly below it.
//! 2. It's walked from `/` one directory at a time with `openat(O_DIRECTORY | O_NOFOLLOW)`: a
//!    symlink or non-directory anywhere on the way ends the cleanup for that path.
//! 3. The leaf is checked with `fstatat(AT_SYMLINK_NOFOLLOW)` and unlinked (`unlinkat`, no
//!    `AT_REMOVEDIR`) **only if it's a socket owned by this process's uid**: never a
//!    directory, a regular file, a symlink or another user's entry.
//!
//! [`crate::Child::reap`] runs it while the child is still an unreaped zombie, so its pid
//! can't have been reused by another process whose socket would sit at the same path.
//!
//! Between the check and the unlink, the entry could only be swapped by someone who can write
//! the leaf's directory; in a sticky shared folder (`/private/tmp`) that's its owner, and the
//! unlink refuses a directory on its own.

use std::ffi::CString;
use std::io;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};

use org_runtime::{CleanupPath, SpecPath};

use crate::sys;

/// What happened to one cleanup path.
#[derive(Debug)]
#[non_exhaustive]
pub enum CleanupOutcome {
    /// The socket was removed.
    Removed,
    /// Nothing was there (or a directory on the way didn't exist).
    Absent,
    /// It was left alone, for this reason (a symlink on the way, not a socket, not ours,
    /// outside the root, or a process still has the pid).
    Refused(&'static str),
    /// A system call failed.
    Failed(io::Error),
}

/// One cleanup path's outcome.
#[derive(Debug)]
pub struct CleanupReport {
    /// The resolved path.
    pub path: PathBuf,
    /// What happened.
    pub outcome: CleanupOutcome,
}

/// Removes `template`'s path for `pid` under the rule above. For a caller that knows `pid`'s
/// process is gone but isn't its parent (cleanup at office startup): it refuses while any
/// process has that pid. That check is best effort: a pid reused right after it can race.
pub fn remove_stale(template: &CleanupPath, root: &SpecPath, pid: u32) -> CleanupReport {
    let path = template.path_for(pid);
    let alive = libc::pid_t::try_from(pid)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "bad pid"))
        .and_then(sys::proc_info);
    let outcome = match alive {
        Ok(Some(_)) => CleanupOutcome::Refused("a process has that pid"),
        Ok(None) => remove_path(&path, &root.to_path_buf()),
        Err(e) => CleanupOutcome::Failed(e),
    };
    CleanupReport { path, outcome }
}

/// Removes the socket at `path` if the rule allows it.
pub(crate) fn remove_path(path: &Path, root: &Path) -> CleanupOutcome {
    if !inside(path, root) {
        return CleanupOutcome::Refused("outside the allowed root");
    }
    let mut names = Vec::new();
    for c in path.components() {
        match c {
            Component::RootDir => {}
            Component::Normal(n) => match CString::new(n.as_bytes()) {
                Ok(n) => names.push(n),
                Err(_) => return CleanupOutcome::Refused("a NUL byte in the path"),
            },
            _ => return CleanupOutcome::Refused("not a plain absolute path"),
        }
    }
    let Some(leaf) = names.pop() else {
        return CleanupOutcome::Refused("no leaf");
    };
    let root_c = CString::from(c"/");
    let mut dir: OwnedFd = match sys::open_dir_nofollow(&root_c) {
        Ok(fd) => fd,
        Err(e) => return CleanupOutcome::Failed(e),
    };
    for name in &names {
        dir = match sys::openat_dir_nofollow(dir.as_fd(), name) {
            Ok(fd) => fd,
            Err(e) => {
                return match e.raw_os_error() {
                    Some(libc::ENOENT) => CleanupOutcome::Absent,
                    Some(libc::ELOOP) | Some(libc::ENOTDIR) => {
                        CleanupOutcome::Refused("a symlink or non-directory on the way")
                    }
                    _ => CleanupOutcome::Failed(e),
                }
            }
        };
    }
    let stat = match sys::fstatat_nofollow(dir.as_fd(), &leaf) {
        Ok(st) => st,
        Err(e) if e.raw_os_error() == Some(libc::ENOENT) => return CleanupOutcome::Absent,
        Err(e) => return CleanupOutcome::Failed(e),
    };
    if !stat.is_socket {
        return CleanupOutcome::Refused("not a socket");
    }
    if stat.uid != sys::geteuid() {
        return CleanupOutcome::Refused("owned by another user");
    }
    match sys::unlinkat(dir.as_fd(), &leaf) {
        Ok(()) => CleanupOutcome::Removed,
        Err(e) if e.raw_os_error() == Some(libc::ENOENT) => CleanupOutcome::Absent,
        Err(e) => CleanupOutcome::Failed(e),
    }
}

/// Whether `path` is strictly below `root`, component by component (both absolute and
/// plain: the types guarantee it for `path`; a root that isn't is never matched).
fn inside(path: &Path, root: &Path) -> bool {
    let plain = |p: &Path| {
        p.is_absolute()
            && p.components()
                .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
    };
    plain(path) && plain(root) && path != root && path.starts_with(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inside_is_by_component_and_strict() {
        let p = Path::new;
        assert!(inside(
            p("/private/tmp/cc-socks/1.sock"),
            p("/private/tmp/cc-socks")
        ));
        assert!(!inside(
            p("/private/tmp/cc-socks"),
            p("/private/tmp/cc-socks")
        ));
        assert!(!inside(
            p("/private/tmp/cc-socksX/1"),
            p("/private/tmp/cc-socks")
        ));
        assert!(!inside(
            p("/private/tmp/1.sock"),
            p("/private/tmp/cc-socks")
        ));
        assert!(!inside(
            p("/private/tmp/cc-socks/1"),
            p("/private/tmp/../tmp/cc-socks")
        ));
        assert!(!inside(p("/private/tmp/cc-socks/1"), p("rel")));
    }
}
