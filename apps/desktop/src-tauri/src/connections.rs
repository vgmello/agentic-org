//! The offices and organizations this app remembers, so the next launch opens straight into the
//! shell. This is the app's own state (one small JSON file in its data directory), not the
//! organization's data: that lives with the office server.

use org_proto::{OfficeInfo, Organization};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    pub office: OfficeInfo,
    pub organization: Organization,
}

/// Just the version, read before the rest of the file.
#[derive(Deserialize)]
struct Version {
    version: u32,
}

#[derive(Serialize, Deserialize)]
struct File {
    version: u32,
    /// Oldest first: the most recently remembered connection is last.
    connections: Vec<Connection>,
}

enum Read {
    Valid(Vec<Connection>),
    Corrupt,
}

pub struct ConnectionStore {
    path: PathBuf,
    /// Serializes `remember` and the set-aside of a corrupt file within this process, so two
    /// concurrent calls can't lose an update. Two app instances are not guarded against each
    /// other: the app is assumed to run once (a single-instance guard comes with the tray work).
    write: Mutex<()>,
}

impl ConnectionStore {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            path: dir.join("connections.json"),
            write: Mutex::new(()),
        }
    }

    /// What is remembered, oldest first. A missing file means nothing is remembered yet. A file
    /// that doesn't parse is set aside as `connections.json.corrupt-<time>` so the app still
    /// starts and the owner can recover it. Any other failure (permissions, I/O) is an error:
    /// treating it as "empty" would let the next `remember` overwrite the real file.
    pub fn list(&self) -> io::Result<Vec<Connection>> {
        match self.read()? {
            Read::Valid(connections) => Ok(connections),
            // Setting a file aside changes it, so do that under the write lock, and look again
            // first: another caller may already have replaced the corrupt file with a good one.
            Read::Corrupt => {
                let _guard = self.write.lock().unwrap_or_else(|e| e.into_inner());
                self.list_locked()
            }
        }
    }

    /// `list` for a caller that already holds the write lock.
    fn list_locked(&self) -> io::Result<Vec<Connection>> {
        match self.read()? {
            Read::Valid(connections) => Ok(connections),
            Read::Corrupt => {
                let stamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis());
                let aside = self.path.with_extension(format!("json.corrupt-{stamp}"));
                match fs::rename(&self.path, aside) {
                    // Another process set it aside first: the result is the same.
                    Ok(()) => Ok(Vec::new()),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
                    Err(e) => Err(e),
                }
            }
        }
    }

    /// Read the file without changing anything.
    fn read(&self) -> io::Result<Read> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Read::Valid(Vec::new())),
            Err(e) => return Err(e),
        };
        // Check the version first, so a file written by a newer app (whose connections may have a
        // different shape) is refused, not mistaken for a corrupt one and set aside.
        match serde_json::from_slice::<Version>(&bytes) {
            Ok(Version { version }) if version != FORMAT_VERSION => Err(io::Error::other(format!(
                "connections.json has version {version}, this app reads version {FORMAT_VERSION}"
            ))),
            Ok(_) => match serde_json::from_slice::<File>(&bytes) {
                Ok(file) => Ok(Read::Valid(file.connections)),
                Err(_) => Ok(Read::Corrupt),
            },
            Err(_) => Ok(Read::Corrupt),
        }
    }

    /// Remember a connection, replacing an earlier one for the same office and organization.
    /// Written to a temp file first and renamed into place, so a crash can't leave half a file.
    pub fn remember(&self, connection: Connection) -> io::Result<()> {
        let _guard = self.write.lock().unwrap_or_else(|e| e.into_inner());
        let mut connections = self.list_locked()?;
        connections.retain(|c| {
            !(c.office.id == connection.office.id
                && c.organization.id == connection.organization.id)
        });
        connections.push(connection);
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let file = File {
            version: FORMAT_VERSION,
            connections,
        };
        let tmp = self.path.with_extension("json.tmp");
        fs::write(
            &tmp,
            serde_json::to_vec_pretty(&file).map_err(io::Error::other)?,
        )?;
        fs::rename(&tmp, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn temp_dir() -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "agentic-org-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn connection(org_id: &str, org_name: &str) -> Connection {
        Connection {
            office: OfficeInfo {
                id: "office-local".into(),
                name: "This Mac".into(),
                host: "This Mac".into(),
            },
            organization: Organization {
                id: org_id.into(),
                name: org_name.into(),
                created_at: "1970-01-01T00:00:00.000Z".into(),
            },
        }
    }

    #[test]
    fn empty_until_something_is_remembered() {
        let store = ConnectionStore::new(temp_dir());
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn remembers_across_instances_newest_last_and_replaces_the_same_organization() {
        let dir = temp_dir();
        ConnectionStore::new(dir.clone())
            .remember(connection("org-1", "Acme"))
            .unwrap();
        ConnectionStore::new(dir.clone())
            .remember(connection("org-2", "Other"))
            .unwrap();
        ConnectionStore::new(dir.clone())
            .remember(connection("org-1", "Acme Inc"))
            .unwrap();
        let list = ConnectionStore::new(dir.clone()).list().unwrap();
        let ids: Vec<_> = list.iter().map(|c| c.organization.id.as_str()).collect();
        assert_eq!(ids, ["org-2", "org-1"]);
        assert_eq!(list[1].organization.name, "Acme Inc");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_corrupt_file_is_set_aside_and_never_overwritten() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("connections.json"), b"{ not json").unwrap();
        let store = ConnectionStore::new(dir.clone());
        assert!(store.list().unwrap().is_empty());
        let aside = |dir: &PathBuf| {
            fs::read_dir(dir)
                .unwrap()
                .filter(|e| {
                    e.as_ref()
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .contains(".corrupt-")
                })
                .count()
        };
        assert_eq!(aside(&dir), 1);
        store.remember(connection("org-1", "Acme")).unwrap();
        assert_eq!(store.list().unwrap().len(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_unreadable_file_is_an_error_not_an_empty_list() {
        // A directory where the file should be: reading it fails with something other than NotFound.
        let dir = temp_dir();
        fs::create_dir_all(dir.join("connections.json")).unwrap();
        let store = ConnectionStore::new(dir.clone());
        assert!(store.list().is_err());
        assert!(store.remember(connection("org-1", "Acme")).is_err());
        assert!(dir.join("connections.json").is_dir());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_newer_file_version_is_refused_not_replaced() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        let newer = br#"{"version":99,"connections":[]}"#;
        fs::write(dir.join("connections.json"), newer).unwrap();
        let store = ConnectionStore::new(dir.clone());
        assert!(store.list().is_err());
        assert!(store.remember(connection("org-1", "Acme")).is_err());
        assert_eq!(fs::read(dir.join("connections.json")).unwrap(), newer);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_good_file_written_after_a_corrupt_read_is_not_set_aside() {
        // Reader A has seen the corrupt file; before it takes the lock, `remember` sets the
        // corrupt file aside and writes a good one. A's second look must keep the good file.
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("connections.json"), b"{ not json").unwrap();
        let store = ConnectionStore::new(dir.clone());
        assert!(matches!(store.read().unwrap(), Read::Corrupt));
        store.remember(connection("org-1", "Acme")).unwrap();

        let _guard = store.write.lock().unwrap();
        let seen = store.list_locked().unwrap();
        assert_eq!(seen.len(), 1);
        let aside = fs::read_dir(&dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains(".corrupt-")
            })
            .count();
        assert_eq!(aside, 1, "only the original corrupt file is set aside");
        assert!(dir.join("connections.json").is_file());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_future_format_is_refused_even_if_its_shape_differs() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        let future = br#"{"version":2,"connections":[{"office":"o","organization":"x"}]}"#;
        fs::write(dir.join("connections.json"), future).unwrap();
        let store = ConnectionStore::new(dir.clone());
        assert!(store.list().is_err());
        assert!(store.remember(connection("org-1", "Acme")).is_err());
        assert_eq!(fs::read(dir.join("connections.json")).unwrap(), future);
        assert!(!fs::read_dir(&dir).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".corrupt-")));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn concurrent_reads_of_a_corrupt_file_set_it_aside_once_and_succeed() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("connections.json"), b"{ not json").unwrap();
        let store = std::sync::Arc::new(ConnectionStore::new(dir.clone()));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let store = store.clone();
                std::thread::spawn(move || store.list().unwrap())
            })
            .collect();
        for h in handles {
            assert!(h.join().unwrap().is_empty());
        }
        let aside = fs::read_dir(&dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains(".corrupt-")
            })
            .count();
        assert_eq!(aside, 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn concurrent_remembers_all_land() {
        let dir = temp_dir();
        let store = std::sync::Arc::new(ConnectionStore::new(dir.clone()));
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let store = store.clone();
                std::thread::spawn(move || {
                    store
                        .remember(connection(&format!("org-{i}"), "Acme"))
                        .unwrap()
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(store.list().unwrap().len(), 8);
        fs::remove_dir_all(dir).unwrap();
    }
}
