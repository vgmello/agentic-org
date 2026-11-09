//! Office fallback files only: bounded, nonblocking reads, quarantine, and plaintext cleanup.
//! This module knows no message contract or directory. The mailbox decides delivery.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::safe_read::{
    office_file_policy, open_regular_nofollow, read_office_spool_file, SpoolFile,
};
use crate::{generate_ulid, is_valid_ulid};

pub const MAX_SPOOLED_BYTES: usize = 512 * 1024;
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
    linked: HashMap<(u64, u64), u8>,
}

impl Drain {
    /// Drain a pass of `spool/new`. On success, delete plaintext immediately; on refusal,
    /// quarantine without following links. Transient failures stay in new for the next pass.
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
        let spool = office.join("spool");
        for path in [&spool, &spool.join("new"), &spool.join("dead")] {
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
        lock.try_lock().map_err(|error| match error {
            fs::TryLockError::WouldBlock => io::Error::from(io::ErrorKind::WouldBlock),
            fs::TryLockError::Error(error) => error,
        })?;
        let previous_links = std::mem::take(&mut self.linked);
        let new = spool.join("new");
        let dead = spool.join("dead");
        purge(&dead)?;
        for entry in fs::read_dir(&new)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name();
            let id = name
                .to_str()
                .and_then(|name| name.strip_suffix(".json"))
                .filter(|id| is_valid_ulid(id));
            let meta = match fs::symlink_metadata(&path) {
                Ok(meta) => meta,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
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
                    let attempts = previous_links.get(&key).copied().unwrap_or(0) + 1;
                    if attempts < 3 {
                        self.linked.insert(key, attempts);
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
            // Never remove a replacement left while we read or committed the original.
            let unchanged = fs::symlink_metadata(&path)
                .is_ok_and(|now| now.dev() == meta.dev() && now.ino() == meta.ino());
            if !unchanged {
                continue;
            }
            match result {
                Delivery::Accepted => fs::remove_file(&path)?,
                Delivery::Refused(reason) => {
                    // New quarantine time, independent of untrusted file mtime. Unique names
                    // avoid replacing an earlier dead letter, including invalid filenames.
                    fs::rename(&path, dead.join(format!("{}.json", generate_ulid())))?;
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

fn purge(dead: &Path) -> io::Result<()> {
    for entry in fs::read_dir(dead)? {
        let entry = entry?;
        let name = entry.file_name();
        // Quarantine names are generated here, never taken from the input.
        let Some(ms) = name
            .to_str()
            .and_then(|name| name.strip_suffix(".json"))
            .and_then(timestamp_ms)
        else {
            continue;
        };
        let time = SystemTime::UNIX_EPOCH + Duration::from_millis(ms);
        if SystemTime::now()
            .duration_since(time)
            .is_ok_and(|age| age >= DEAD_LIFETIME)
        {
            let path = entry.path();
            if fs::symlink_metadata(&path)?.is_dir() {
                fs::remove_dir_all(path)?;
            } else {
                fs::remove_file(path)?;
            }
        }
    }
    Ok(())
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
        let error = Drain::default()
            .drain(dir.path(), |_, _| panic!("busy"), |_| {})
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
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
