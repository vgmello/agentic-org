//! Office-owned ciphertext scratch, swept only while the caller holds its office lock.
use std::fs::{self, DirBuilder};
use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::store::Store;
use crate::ulid;

const ROOT: &str = ".snapshot-staging";
const PREFIX: &str = "snapshot-";

pub(super) struct Scratch(PathBuf);

impl Scratch {
    pub(super) fn new(store: &Store) -> io::Result<Self> {
        let office = store.db_path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "snapshot office folder is missing",
            )
        })?;
        let root = checked_root(office, true)?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "snapshot staging folder is missing",
            )
        })?;
        let directory = root.join(format!("{PREFIX}{}", ulid::generate()?));
        DirBuilder::new().mode(0o700).create(&directory)?;
        Ok(Self(directory))
    }

    pub(super) fn database(&self) -> PathBuf {
        self.0.join("snapshot.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            warn(&error);
        }
    }
}

fn checked_root(office: &Path, create: bool) -> io::Result<Option<PathBuf>> {
    let root = office.join(ROOT);
    if create {
        match DirBuilder::new().mode(0o700).create(&root) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    let metadata = match fs::symlink_metadata(&root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !metadata.file_type().is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unsafe snapshot staging folder",
        ));
    }
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
    Ok(Some(root))
}

pub(super) fn sweep(office: &Path) -> io::Result<()> {
    let Some(root) = checked_root(office, false)? else {
        return Ok(());
    };
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let recognized = name
            .to_str()
            .and_then(|name| name.strip_prefix(PREFIX))
            .is_some_and(|id| org_proto::ulid_time_ms(id).is_some());
        if !recognized {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unrecognized snapshot staging entry",
            ));
        }
        if entry.file_type()?.is_dir() {
            fs::remove_dir_all(entry.path())?;
        } else {
            // Remove a stale link itself, without following its target.
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

pub(super) fn warn(error: &io::Error) {
    // The error may carry a private filename. Log its class, never its display/source.
    eprintln!(
        "org-office: snapshot scratch cleanup failed ({:?})",
        error.kind()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_reports_cleanup_failure_without_a_private_path() {
        const CHILD_PATH: &str = "ORG_SNAPSHOT_DROP_TEST_PATH";
        if let Some(path) = std::env::var_os(CHILD_PATH) {
            let path = PathBuf::from(path);
            assert!(!path.exists(), "cleanup fixture must be absent");
            drop(Scratch(path));
            return;
        }
        let dir = tempfile::TempDir::new().unwrap();
        let secret_path = dir.path().join("SENSITIVE-SNAPSHOT-NAME");
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "store::org::snapshot::scratch::tests::drop_reports_cleanup_failure_without_a_private_path", "--nocapture"])
            .env(CHILD_PATH, &secret_path).output().unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        let diagnostic = String::from_utf8(output.stderr).unwrap();
        assert!(diagnostic.contains("snapshot scratch cleanup failed (NotFound)"));
        assert!(!diagnostic.contains("SENSITIVE-SNAPSHOT-NAME"));
        assert!(!diagnostic.contains(&secret_path.to_string_lossy().to_string()));
    }
}
