//! Office fallback files only: bounded, nonblocking reads, quarantine, and plaintext cleanup.
//! This module knows no message contract or directory. The mailbox decides delivery.
//!
//! The private 0700 spool directories are the trust boundary. Inode checks narrow a
//! same-user pathname swap race, but a replacement between the final lstat and
//! unlink/rename can still be removed or quarantined. POSIX has no inode-conditional
//! unlink/rename operation; these checks do not eliminate that residual race.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::safe_read::{
    office_file_policy, open_regular_nofollow, read_office_spool_file, SpoolFile,
};
use crate::{generate_ulid, is_valid_ulid};

pub const MAX_SPOOLED_BYTES: usize = 512 * 1024;
// Each directory gets a separate budget so a large dead/tmp backlog cannot starve new.
const PASS_ENTRIES: usize = 128;
const TMP_LIFETIME: Duration = Duration::from_secs(10 * 60);
const DEAD_LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);

/// Delivery is durable, permanently refused, or temporarily unavailable. Reasons must be
/// static diagnostics, never message contents, filenames, serde errors, or storage errors.
pub enum Delivery {
    Accepted,
    Refused(&'static str),
    Retry(&'static str),
}

/// Diagnostic fields safe to log: a validated filename ULID (or `invalid`) and fixed reason.
#[derive(Debug)]
pub struct Notice<'a> {
    pub id: &'a str,
    pub reason: &'static str,
}

/// A serial drainer. Retains at most three observations of a two-link publication
/// window; no contents are read until the publisher removes its temporary hard link.
#[derive(Default)]
pub struct Drain {
    linked: HashMap<(u64, u64), (u8, u64)>,
    pass: u64,
    scan_start: u64,
    office: PathBuf,
    new: Option<fs::ReadDir>,
    dead: Cleanup,
    tmp: Cleanup,
}

impl Drain {
    /// Drain a pass of `spool/new`. On success, delete plaintext immediately; on refusal,
    /// quarantine without following links. Transient failures stay in new until retry or a 24-hour ULID age limit.
    /// Reuse this drainer before serving clients and every five seconds thereafter.
    /// Private spool directories may be created; unsafe existing directories fail closed.
    pub fn drain(
        &mut self,
        office: &Path,
        deliver: impl FnMut(&str, &[u8]) -> Delivery,
        log: impl FnMut(Notice<'_>),
    ) -> io::Result<()> {
        // SAFETY: geteuid has no preconditions.
        self.drain_for_file_owner(office, unsafe { libc::geteuid() }, deliver, log)
    }

    // Explicit uid keeps file ownership policy testable without privileged chown.
    // Directory and lock ownership always use the actual process identity.
    fn drain_for_file_owner(
        &mut self,
        office: &Path,
        file_owner: u32,
        mut deliver: impl FnMut(&str, &[u8]) -> Delivery,
        mut log: impl FnMut(Notice<'_>),
    ) -> io::Result<()> {
        if self.office != office {
            *self = Self {
                office: office.to_owned(),
                ..Self::default()
            };
        }
        let spool = office.join("spool");
        for path in [
            &spool,
            &spool.join("new"),
            &spool.join("dead"),
            &spool.join("tmp"),
        ] {
            private_dir(path)?;
        }
        let lock = open_regular_nofollow(
            OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .mode(0o600),
            &spool.join(".meta.lock"),
        )?;
        // SAFETY: geteuid has no preconditions.
        office_file_policy(&lock.metadata()?, unsafe { libc::geteuid() })
            .map_err(io::Error::other)?;
        match lock.try_lock() {
            Ok(()) => (),
            Err(fs::TryLockError::WouldBlock) => {
                log(Notice {
                    id: "invalid",
                    reason: "spool lock busy",
                });
                return Ok(());
            }
            Err(fs::TryLockError::Error(error)) => return Err(error),
        }
        self.pass = self.pass.wrapping_add(1);
        let new = spool.join("new");
        let dead = spool.join("dead");
        cleanup(&mut self.dead, &dead, false, file_owner, &mut log)?;
        cleanup(
            &mut self.tmp,
            &spool.join("tmp"),
            true,
            file_owner,
            &mut log,
        )?;
        if self.new.is_none() {
            self.scan_start = self.pass;
        }
        for entry in batch(&mut self.new, &new)? {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    log(Notice {
                        id: "invalid",
                        reason: "directory entry failed",
                    });
                    continue;
                }
            };
            let path = entry.path();
            let name = entry.file_name();
            let id = name
                .to_str()
                .and_then(|name| name.strip_suffix(".json"))
                .filter(|id| is_valid_ulid(id));
            let meta = match fs::symlink_metadata(&path) {
                Ok(meta) => meta,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(_) => {
                    log(Notice {
                        id: id.unwrap_or("invalid"),
                        reason: "file metadata failed",
                    });
                    continue;
                }
            };
            let result = match id {
                None => Delivery::Refused("invalid filename"),
                Some(_)
                    if meta.is_file()
                        && meta.uid() == file_owner
                        && meta.mode() & 0o7777 == 0o600
                        && meta.nlink() == 2 =>
                {
                    let key = (meta.dev(), meta.ino());
                    let observation = self.linked.entry(key).or_insert((0, 0));
                    if observation.1 != self.pass {
                        observation.0 = observation.0.saturating_add(1);
                        observation.1 = self.pass;
                    }
                    if observation.0 < 3 {
                        Delivery::Retry("temporary publication hard link")
                    } else {
                        Delivery::Refused("file remains linked")
                    }
                }
                Some(id) => match read_office_spool_file(&path, MAX_SPOOLED_BYTES, file_owner) {
                    Ok(SpoolFile::Bytes(bytes)) => deliver(id, &bytes),
                    Ok(SpoolFile::Rejected(reason)) => Delivery::Refused(match reason.as_str() {
                        "not a regular file" => "not a regular file",
                        "wrong file owner" => "wrong file owner",
                        "file mode is not 0600" => "file mode is not 0600",
                        "file has multiple hard links" => "file has multiple hard links",
                        _ => "unsafe or oversized spool file",
                    }),
                    Ok(SpoolFile::Missing) => continue,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                    Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                        Delivery::Refused("unreadable file")
                    }
                    Err(_) => Delivery::Retry("file read failed"),
                },
            };
            let result = match result {
                Delivery::Retry(_) if id.is_some_and(|id| expired(id, DEAD_LIFETIME)) => {
                    Delivery::Refused("retry age exceeded")
                }
                result => result,
            };
            if !matches!(result, Delivery::Retry(_)) && meta.nlink() != 2 {
                self.linked.remove(&(meta.dev(), meta.ino()));
            }
            // Never remove a replacement observed after reading/committing the original.
            // Residual race: a same-user process can replace the pathname between this
            // lstat and unlink/rename. POSIX has no conditional inode unlink/rename;
            // the private 0700 directories are the trust boundary. This check narrows,
            // but cannot eliminate, that race (also applies to cleanup below).
            if !unchanged(&path, &meta, id.unwrap_or("invalid"), &mut log) {
                continue;
            }
            match result {
                Delivery::Accepted => {
                    if fs::remove_file(&path).is_err() {
                        log(Notice {
                            id: id.unwrap_or("invalid"),
                            reason: "plaintext unlink failed",
                        });
                    }
                }
                Delivery::Refused(reason) => {
                    // New quarantine time, independent of untrusted file mtime. Unique names
                    // avoid replacing an earlier dead letter, including invalid filenames.
                    if fs::rename(&path, dead.join(format!("{}.json", generate_ulid()))).is_err() {
                        log(Notice {
                            id: id.unwrap_or("invalid"),
                            reason: "quarantine failed",
                        });
                        continue;
                    }
                    log(Notice {
                        id: id.unwrap_or("invalid"),
                        reason,
                    });
                }
                Delivery::Retry(reason) => log(Notice {
                    id: id.unwrap_or("invalid"),
                    reason,
                }),
            }
        }
        if self.new.is_none() {
            // Forget removed inodes after a complete scan; retain observations across
            // budget boundaries so large queues cannot reset the three-visit bound.
            self.linked
                .retain(|_, observation| observation.1 >= self.scan_start);
        }
        Ok(())
    }
}

fn private_dir(path: &Path) -> io::Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => (),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e),
    }
    let meta = fs::symlink_metadata(path)?;
    // SAFETY: geteuid has no preconditions.
    let uid = unsafe { libc::geteuid() };
    if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o7777 != 0o700 {
        return Err(io::Error::other("unsafe office spool directory"));
    }
    Ok(())
}

// Retain directory iterators across polls; repeatedly taking the first N would starve
// later entries when early entries retry. At EOF reopen on the next pass.
fn batch(
    cursor: &mut Option<fs::ReadDir>,
    path: &Path,
) -> io::Result<Vec<io::Result<fs::DirEntry>>> {
    if cursor.is_none() {
        *cursor = Some(fs::read_dir(path)?);
    }
    let mut entries = Vec::new();
    for _ in 0..PASS_ENTRIES {
        match cursor.as_mut().unwrap().next() {
            Some(Err(error)) => {
                entries.push(Err(error));
                *cursor = None;
                break;
            }
            Some(Ok(entry)) => entries.push(Ok(entry)),
            None => {
                *cursor = None;
                break;
            }
        }
    }
    Ok(entries)
}

fn expired(id: &str, lifetime: Duration) -> bool {
    timestamp_ms(id).is_some_and(|ms| {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH + Duration::from_millis(ms))
            .is_ok_and(|age| age > lifetime)
    })
}

fn unchanged(
    path: &Path,
    before: &fs::Metadata,
    id: &str,
    log: &mut impl FnMut(Notice<'_>),
) -> bool {
    match fs::symlink_metadata(path) {
        Ok(now) => now.dev() == before.dev() && now.ino() == before.ino(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(_) => {
            log(Notice {
                id,
                reason: "inode recheck failed",
            });
            false
        }
    }
}

#[derive(Default)]
struct Cleanup {
    cursor: Option<fs::ReadDir>,
    // Only the current expired tree is held; depth-first cursors resume without
    // rescanning failed children or storing a whole directory in memory.
    tree: Vec<CleanupDir>,
}

struct CleanupDir {
    path: PathBuf,
    meta: fs::Metadata,
    entries: fs::ReadDir,
    id: String,
}

fn cleanup(
    state: &mut Cleanup,
    dir: &Path,
    tmp: bool,
    uid: u32,
    log: &mut impl FnMut(Notice<'_>),
) -> io::Result<()> {
    // Descendant entries, iteration failures, and directory removals all consume
    // the same budget. Never use remove_dir_all on untrusted quarantine trees.
    for _ in 0..PASS_ENTRIES {
        if !state.tree.is_empty() {
            cleanup_tree_step(&mut state.tree, log);
            continue;
        }
        if state.cursor.is_none() {
            state.cursor = Some(fs::read_dir(dir)?);
        }
        let entry = match state.cursor.as_mut().unwrap().next() {
            Some(Ok(entry)) => entry,
            Some(Err(_)) => {
                state.cursor = None;
                log(Notice {
                    id: "invalid",
                    reason: "cleanup entry failed",
                });
                break;
            }
            None => {
                state.cursor = None;
                break;
            }
        };
        let name = entry.file_name();
        let id = name
            .to_str()
            .and_then(|n| n.strip_suffix(".json"))
            .filter(|id| is_valid_ulid(id));
        let path = entry.path();
        let meta = match fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => {
                log(Notice {
                    id: id.unwrap_or("invalid"),
                    reason: "cleanup metadata failed",
                });
                continue;
            }
        };
        let eligible = if tmp {
            meta.is_file()
                && meta.uid() == uid
                && meta.nlink() == 1
                && meta
                    .modified()
                    .ok()
                    .and_then(|t| SystemTime::now().duration_since(t).ok())
                    .is_some_and(|age| age > TMP_LIFETIME)
        } else {
            id.is_some_and(|id| expired(id, DEAD_LIFETIME))
        };
        if eligible {
            cleanup_entry(&path, meta, id.unwrap_or("invalid"), &mut state.tree, log);
        }
    }
    Ok(())
}

fn cleanup_entry(
    path: &Path,
    meta: fs::Metadata,
    id: &str,
    tree: &mut Vec<CleanupDir>,
    log: &mut impl FnMut(Notice<'_>),
) {
    if !unchanged(path, &meta, id, log) {
        return;
    }
    if meta.is_dir() {
        match fs::read_dir(path) {
            Ok(entries) => tree.push(CleanupDir {
                path: path.to_owned(),
                meta,
                entries,
                id: id.to_owned(),
            }),
            Err(_) => log(Notice {
                id,
                reason: "cleanup unlink failed",
            }),
        }
    } else if fs::remove_file(path).is_err() {
        log(Notice {
            id,
            reason: "cleanup unlink failed",
        });
    }
}

fn cleanup_tree_step(tree: &mut Vec<CleanupDir>, log: &mut impl FnMut(Notice<'_>)) {
    let frame = tree.last_mut().unwrap();
    if !unchanged(&frame.path, &frame.meta, &frame.id, log) {
        tree.pop();
        return;
    }
    let entry = match frame.entries.next() {
        Some(Ok(entry)) => entry,
        Some(Err(_)) => {
            log(Notice {
                id: &frame.id,
                reason: "cleanup entry failed",
            });
            tree.pop(); // Retry this directory with a fresh cursor on a later scan.
            return;
        }
        None => {
            if fs::remove_dir(&frame.path).is_err() {
                log(Notice {
                    id: &frame.id,
                    reason: "cleanup unlink failed",
                });
            }
            tree.pop();
            return;
        }
    };
    let path = entry.path();
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return,
        Err(_) => {
            log(Notice {
                id: &frame.id,
                reason: "cleanup metadata failed",
            });
            return;
        }
    };
    let id = frame.id.clone();
    cleanup_entry(&path, meta, &id, tree, log);
}

fn timestamp_ms(id: &str) -> Option<u64> {
    if !is_valid_ulid(id) {
        return None;
    }
    id.bytes().take(10).try_fold(0u64, |time, byte| {
        let value = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ"
            .iter()
            .position(|&candidate| candidate == byte.to_ascii_uppercase())?;
        Some(time * 32 + value as u64)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use tempfile::TempDir;

    fn setup() -> TempDir {
        let dir = TempDir::new().unwrap();
        Drain::default()
            .drain(dir.path(), |_, _| panic!("empty"), |_| {})
            .unwrap();
        dir
    }
    fn place(dir: &Path, bytes: &[u8]) -> std::path::PathBuf {
        let path = dir
            .join("spool/new")
            .join(format!("{}.json", generate_ulid()));
        fs::write(&path, bytes).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        path
    }
    #[test]
    fn recheck_logs_metadata_errors_but_skips_disappearance() {
        let dir = setup();
        let parent = dir.path().join("PRIVATE_PARENT");
        fs::create_dir(&parent).unwrap();
        let path = parent.join("PRIVATE_FILE");
        fs::write(&path, b"private").unwrap();
        let meta = fs::symlink_metadata(&path).unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o000)).unwrap();
        let id = generate_ulid();
        let mut notices = Vec::new();
        let result = unchanged(&path, &meta, &id, &mut |n| {
            notices.push((n.id.to_owned(), n.reason))
        });
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(!result);
        assert_eq!(notices, [(id.clone(), "inode recheck failed")]);
        fs::remove_file(&path).unwrap();
        assert!(!unchanged(&path, &meta, &id, &mut |_| panic!(
            "disappearance is normal"
        )));
    }

    #[test]
    fn dead_tree_descendants_share_the_budget_and_resume_without_following_links() {
        let dir = setup();
        let tree = dir
            .path()
            .join("spool/dead")
            .join(format!("{}.json", crate::encode_ulid(0, 0, 1)));
        let nested = tree.join("PRIVATE_DIRECTORY");
        fs::create_dir_all(&nested).unwrap();
        let total = PASS_ENTRIES * 2 + 7;
        for n in 0..total {
            fs::write(nested.join(n.to_string()), b"private").unwrap();
        }
        let outside = dir.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("keep"), b"untouched").unwrap();
        symlink(&outside, tree.join("PRIVATE_LINK")).unwrap();
        let good = place(dir.path(), b"valid");
        let mut drain = Drain::default();
        drain
            .drain(dir.path(), |_, _| Delivery::Accepted, |_| {})
            .unwrap();
        assert!(!good.exists());
        assert!(tree.exists());
        assert!(fs::read_dir(&nested).unwrap().count() >= total - PASS_ENTRIES);
        for _ in 0..5 {
            drain
                .drain(dir.path(), |_, _| panic!("empty"), |_| {})
                .unwrap();
        }
        assert!(!tree.exists());
        assert_eq!(fs::read(outside.join("keep")).unwrap(), b"untouched");
    }

    #[test]
    fn every_retry_expires_by_ulid_age_including_link_publication() {
        let dir = setup();
        for linked in [false, true] {
            let path = dir
                .path()
                .join("spool/new")
                .join(format!("{}.json", crate::encode_ulid(0, 0, linked as u64)));
            fs::write(&path, b"private").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            if linked {
                fs::hard_link(&path, dir.path().join("alias")).unwrap();
            }
            let mut reasons = Vec::new();
            Drain::default()
                .drain(
                    dir.path(),
                    |_, _| Delivery::Retry("storage unavailable"),
                    |n| reasons.push(n.reason),
                )
                .unwrap();
            assert!(!path.exists());
            assert_eq!(reasons, ["retry age exceeded"]);
        }
    }

    #[test]
    fn replacement_during_delivery_survives_both_unlink_and_quarantine() {
        for accepted in [false, true] {
            let dir = setup();
            let path = place(dir.path(), b"original");
            Drain::default()
                .drain(
                    dir.path(),
                    |_, _| {
                        fs::rename(&path, dir.path().join("original")).unwrap();
                        fs::write(&path, b"replacement").unwrap();
                        if accepted {
                            Delivery::Accepted
                        } else {
                            Delivery::Refused("refused")
                        }
                    },
                    |_| {},
                )
                .unwrap();
            assert_eq!(fs::read(&path).unwrap(), b"replacement");
            assert_eq!(
                fs::read_dir(dir.path().join("spool/dead")).unwrap().count(),
                0
            );
        }
    }

    #[test]
    fn failed_unlink_and_quarantine_log_and_continue_the_pass() {
        for accepted in [false, true] {
            let dir = setup();
            for _ in 0..3 {
                place(dir.path(), b"private");
            }
            let new = dir.path().join("spool/new");
            let mut calls = 0;
            let mut reasons = Vec::new();
            let result = Drain::default().drain(
                dir.path(),
                |_, _| {
                    calls += 1;
                    fs::set_permissions(&new, fs::Permissions::from_mode(0o500)).unwrap();
                    if accepted {
                        Delivery::Accepted
                    } else {
                        Delivery::Refused("refused")
                    }
                },
                |n| {
                    assert!(is_valid_ulid(n.id));
                    reasons.push(n.reason);
                },
            );
            fs::set_permissions(&new, fs::Permissions::from_mode(0o700)).unwrap();
            result.unwrap();
            assert_eq!(calls, 3);
            assert_eq!(
                reasons,
                vec![
                    if accepted {
                        "plaintext unlink failed"
                    } else {
                        "quarantine failed"
                    };
                    3
                ]
            );
        }
    }

    #[test]
    fn unquarantinable_directory_does_not_prevent_valid_delivery() {
        let dir = setup();
        let bad = dir
            .path()
            .join("spool/new")
            .join(format!("{}.json", generate_ulid()));
        fs::create_dir(&bad).unwrap();
        fs::set_permissions(&bad, fs::Permissions::from_mode(0o500)).unwrap();
        let good = place(dir.path(), b"valid");
        let mut reasons = Vec::new();
        let result = Drain::default().drain(
            dir.path(),
            |_, _| Delivery::Accepted,
            |n| reasons.push(n.reason),
        );
        // macOS refuses renaming the directory without write permission on it.
        if bad.exists() {
            fs::set_permissions(&bad, fs::Permissions::from_mode(0o700)).unwrap();
        }
        for entry in fs::read_dir(dir.path().join("spool/dead")).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
            }
        }
        result.unwrap();
        assert!(!good.exists());
        #[cfg(target_os = "macos")]
        assert_eq!(reasons, ["quarantine failed"]);
        #[cfg(not(target_os = "macos"))]
        assert_eq!(reasons.len(), 1);
    }

    #[test]
    fn failed_dead_purge_is_nonfatal_and_delivery_continues() {
        let dir = setup();
        let blocked = dir
            .path()
            .join("spool/dead")
            .join(format!("{}.json", crate::encode_ulid(0, 0, 1)));
        fs::create_dir(&blocked).unwrap();
        fs::write(blocked.join("private"), b"private").unwrap();
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000)).unwrap();
        let path = place(dir.path(), b"valid");
        let mut reasons = Vec::new();
        let result = Drain::default().drain(
            dir.path(),
            |_, _| Delivery::Accepted,
            |n| reasons.push(n.reason),
        );
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o700)).unwrap();
        result.unwrap();
        assert!(!path.exists());
        assert_eq!(reasons, ["cleanup unlink failed"]);
    }

    #[test]
    fn tmp_cleanup_only_unlinks_stale_regular_owned_single_link_files() {
        let dir = setup();
        let tmp = dir.path().join("spool/tmp");
        let old = |name: &str| {
            let path = tmp.join(name);
            fs::write(&path, b"private").unwrap();
            fs::File::open(&path)
                .unwrap()
                .set_times(
                    fs::FileTimes::new()
                        .set_modified(SystemTime::now() - TMP_LIFETIME - Duration::from_secs(1)),
                )
                .unwrap();
            path
        };
        let stale = old("stale");
        let linked = old("linked");
        fs::hard_link(&linked, dir.path().join("alias")).unwrap();
        let recent = tmp.join("recent");
        fs::write(&recent, b"recent").unwrap();
        let link = tmp.join("symlink");
        symlink(&stale, &link).unwrap();
        let directory = tmp.join("directory");
        fs::create_dir(&directory).unwrap();
        let foreign = old("foreign");
        let uid = fs::metadata(&foreign).unwrap().uid();
        Drain::default()
            .drain_for_file_owner(
                dir.path(),
                uid.wrapping_add(1),
                |_, _| panic!("empty"),
                |_| {},
            )
            .unwrap();
        assert!(foreign.exists());
        Drain::default()
            .drain(dir.path(), |_, _| panic!("empty"), |_| {})
            .unwrap();
        assert!(!stale.exists());
        assert!(!foreign.exists());
        assert!(linked.exists() && recent.exists() && directory.exists());
        assert!(fs::symlink_metadata(link).unwrap().is_symlink());
    }

    #[test]
    fn pass_budget_resumes_past_retries_and_bounds_cleanup_too() {
        let dir = setup();
        let total = PASS_ENTRIES * 2 + 7;
        for n in 0..total {
            place(dir.path(), b"private");
            fs::write(
                dir.path()
                    .join("spool/dead")
                    .join(format!("{}.json", crate::encode_ulid(0, 0, n as u64))),
                b"old",
            )
            .unwrap();
            let tmp = dir.path().join("spool/tmp").join(n.to_string());
            fs::write(&tmp, b"stale").unwrap();
            fs::File::open(tmp)
                .unwrap()
                .set_times(fs::FileTimes::new().set_modified(SystemTime::UNIX_EPOCH))
                .unwrap();
        }
        let mut drain = Drain::default();
        let mut ids = std::collections::HashSet::new();
        for pass in 0..3 {
            let mut calls = 0;
            drain
                .drain(
                    dir.path(),
                    |id, _| {
                        ids.insert(id.to_owned());
                        calls += 1;
                        Delivery::Retry("temporary")
                    },
                    |_| {},
                )
                .unwrap();
            assert!(calls <= PASS_ENTRIES);
            let remaining = total.saturating_sub((pass + 1) * PASS_ENTRIES);
            for name in ["dead", "tmp"] {
                assert_eq!(
                    fs::read_dir(dir.path().join("spool").join(name))
                        .unwrap()
                        .count(),
                    remaining
                );
            }
        }
        assert_eq!(
            ids.len(),
            total,
            "later files must not starve behind retries"
        );
    }

    #[test]
    fn unsafe_files_are_quarantined_without_reading_or_logging_contents() {
        let dir = setup();
        let target = dir.path().join("outside");
        fs::write(&target, b"PRIVATE_BODY").unwrap();
        let link = dir
            .path()
            .join("spool/new")
            .join(format!("{}.json", generate_ulid()));
        symlink(&target, &link).unwrap();
        for mode in [0o644, 0o400, 0o000, 0o660] {
            let path = place(dir.path(), b"PRIVATE_BODY");
            fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
        }
        let fifo = dir
            .path()
            .join("spool/new")
            .join(format!("{}.json", generate_ulid()));
        let c = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: NUL-terminated temporary path, no aliased pointers.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        let hard = place(dir.path(), b"PRIVATE_BODY");
        fs::hard_link(&hard, dir.path().join("another-link")).unwrap();
        fs::hard_link(&hard, dir.path().join("third-link")).unwrap();
        fs::create_dir(
            dir.path()
                .join("spool/new")
                .join(format!("{}.json", generate_ulid())),
        )
        .unwrap();
        fs::write(dir.path().join("spool/new/PRIVATE_BODY"), b"PRIVATE_META").unwrap();
        place(dir.path(), &vec![b'x'; MAX_SPOOLED_BYTES + 1]);
        let mut logs = Vec::new();
        Drain::default()
            .drain(
                dir.path(),
                |_, _| panic!("unsafe file read"),
                |notice| logs.push(format!("{notice:?}")),
            )
            .unwrap();
        assert_eq!(logs.len(), 10);
        assert!(logs
            .iter()
            .all(|line| !line.contains("PRIVATE_BODY") && !line.contains("PRIVATE_META")));
        assert_eq!(
            fs::read_dir(dir.path().join("spool/new")).unwrap().count(),
            0
        );
        assert_eq!(
            fs::read_dir(dir.path().join("spool/dead")).unwrap().count(),
            10
        );
        assert_eq!(fs::read(&target).unwrap(), b"PRIVATE_BODY");
    }
    #[test]
    fn two_links_are_retried_then_delivered_or_quarantined_with_a_bound() {
        let dir = setup();
        let path = place(dir.path(), b"private");
        let staged = dir.path().join("staged");
        fs::hard_link(&path, &staged).unwrap();
        let mut drainer = Drain::default();
        for _ in 0..2 {
            drainer
                .drain(dir.path(), |_, _| panic!("linked file read"), |_| {})
                .unwrap();
            assert!(path.exists());
        }
        fs::remove_file(&staged).unwrap();
        drainer
            .drain(
                dir.path(),
                |_, bytes| {
                    assert_eq!(bytes, b"private");
                    Delivery::Accepted
                },
                |_| {},
            )
            .unwrap();
        assert!(!path.exists());

        let path = place(dir.path(), b"private");
        fs::hard_link(&path, &staged).unwrap();
        for _ in 0..3 {
            drainer
                .drain(dir.path(), |_, _| panic!("linked file read"), |_| {})
                .unwrap();
        }
        assert!(!path.exists());
        assert_eq!(
            fs::read_dir(dir.path().join("spool/dead")).unwrap().count(),
            1
        );
    }

    #[test]
    fn two_names_for_one_inode_cannot_reset_the_retry_bound() {
        let dir = setup();
        let path = place(dir.path(), b"private");
        let alias = dir
            .path()
            .join("spool/new")
            .join(format!("{}.json", generate_ulid()));
        fs::hard_link(&path, &alias).unwrap();
        let mut drainer = Drain::default();
        for _ in 0..3 {
            drainer
                .drain(dir.path(), |_, _| panic!("linked file read"), |_| {})
                .unwrap();
        }
        assert!(!path.exists());
        assert!(!alias.exists());
        assert_eq!(
            fs::read_dir(dir.path().join("spool/dead")).unwrap().count(),
            2
        );
    }

    #[test]
    fn a_busy_drain_lock_never_blocks_startup() {
        let dir = setup();
        let lock = std::fs::File::open(dir.path().join("spool/.meta.lock")).unwrap();
        lock.lock().unwrap();
        let mut reasons = Vec::new();
        Drain::default()
            .drain(
                dir.path(),
                |_, _| panic!("busy"),
                |n| reasons.push(n.reason),
            )
            .unwrap();
        assert_eq!(reasons, ["spool lock busy"]);
    }

    #[test]
    fn wrong_owner_goes_to_dead_without_reading_and_logs_only_a_reason() {
        let dir = setup();
        for linked in [false, true] {
            let path = place(dir.path(), b"PRIVATE_BODY");
            if linked {
                fs::hard_link(&path, dir.path().join("foreign-alias")).unwrap();
            }
            let wrong_uid = fs::metadata(&path).unwrap().uid().wrapping_add(1);
            let mut reasons = Vec::new();
            Drain::default()
                .drain_for_file_owner(
                    dir.path(),
                    wrong_uid,
                    |_, _| panic!("foreign file read"),
                    |notice| reasons.push(notice.reason),
                )
                .unwrap();
            assert!(!path.exists());
            assert_eq!(reasons, ["wrong file owner"]);
        }
        assert_eq!(
            fs::read_dir(dir.path().join("spool/dead")).unwrap().count(),
            2
        );
    }

    #[test]
    fn cap_boundary_and_retry_and_immediate_plaintext_deletion() {
        let dir = setup();
        let path = place(dir.path(), &vec![b'x'; MAX_SPOOLED_BYTES]);
        let mut calls = 0;
        Drain::default()
            .drain(
                dir.path(),
                |_, bytes| {
                    assert_eq!(bytes.len(), MAX_SPOOLED_BYTES);
                    calls += 1;
                    Delivery::Retry("storage unavailable")
                },
                |_| {},
            )
            .unwrap();
        assert_eq!(calls, 1);
        assert!(path.exists());
        Drain::default()
            .drain(dir.path(), |_, _| Delivery::Accepted, |_| {})
            .unwrap();
        assert!(!path.exists());
        assert_eq!(
            fs::read_dir(dir.path().join("spool/dead")).unwrap().count(),
            0
        );
    }
    #[test]
    fn unsafe_spool_directories_and_lock_symlinks_fail_closed() {
        let dir = setup();
        let new = dir.path().join("spool/new");
        fs::remove_dir(&new).unwrap();
        symlink(dir.path(), &new).unwrap();
        assert!(Drain::default()
            .drain(dir.path(), |_, _| panic!("unsafe"), |_| {})
            .is_err());
        fs::remove_file(&new).unwrap();
        fs::create_dir(&new).unwrap();
        fs::set_permissions(&new, fs::Permissions::from_mode(0o700)).unwrap();
        let lock = dir.path().join("spool/.meta.lock");
        fs::remove_file(&lock).unwrap();
        symlink(dir.path().join("outside"), &lock).unwrap();
        assert!(Drain::default()
            .drain(dir.path(), |_, _| panic!("unsafe"), |_| {})
            .is_err());
    }
    #[test]
    fn dead_retention_uses_quarantine_time_and_does_not_follow_symlinks() {
        let dir = setup();
        let dead = dir.path().join("spool/dead");
        let target = dir.path().join("outside");
        fs::write(&target, b"untouched").unwrap();
        let old = format!("{}.json", crate::encode_ulid(0, 0, 1));
        symlink(&target, dead.join(old)).unwrap();
        fs::write(dead.join(format!("{}.json", generate_ulid())), b"recent").unwrap();
        Drain::default()
            .drain(dir.path(), |_, _| panic!("empty"), |_| {})
            .unwrap();
        assert_eq!(fs::read_dir(dead).unwrap().count(), 1);
        assert!(target.exists());
    }
}
