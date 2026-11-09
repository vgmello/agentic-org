use std::fs::{self, File, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::directory::{self, RuntimeView, ThisOffice};
use crate::error::OfficeError;
use crate::key::Key;
use crate::model::{NewOffice, OfficeInfo};
use crate::office_file::{self, is_plain_id};
use crate::private_fs::{absolute, private_dir, private_file, tighten_dir};
use crate::rules::OrgCommand;
use org_proto::directory::{WhoParams, WhoResult};
use org_proto::org::{CommandAccepted, OrgChangedData, OrgSnapshot, Position};
use org_proto::ApiError;

use crate::store::org as org_db;
use crate::store::{messages, status};
use crate::store::{Opening, Schema, Store};
use crate::time;
use org_proto::OrganizationName;

/// How long `create`, `open` and `discard` wait for the folder's lock: a slow-exiting old
/// server and its replacement never overlap.
pub const LOCK_WAIT: Duration = Duration::from_secs(10);

const LOCK_FILE: &str = "office.lock";
const ORG_DB: &str = "org.db";
const OFFICE_DB: &str = "office.db";
const WORKSPACE: &str = "workspace";

/// Everything an office folder can hold besides its lock; `create` refuses a folder with any
/// of it, and a failed `create` removes what it made.
const OFFICE_ENTRIES: [&str; 11] = [
    office_file::FILE_NAME,
    "office.json.tmp",
    WORKSPACE,
    // A database's log and index go before its main file: a main file that is gone while a
    // non-empty log remains is a state nothing may look into (see `discard`).
    "org.db-wal",
    "org.db-shm",
    "org.db-journal",
    "org.db",
    "office.db-wal",
    "office.db-shm",
    "office.db-journal",
    "office.db",
];

/// `office.db`: its identity (`meta`), from version 2 the message tables and from version 3 the
/// members' status lines. An older file gets the tables it lacks when it is opened.
pub(crate) const OFFICE_SCHEMA: Schema = Schema {
    version: 3,
    create: |tx| {
        tx.execute_batch("CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;")?;
        messages::create_tables(tx)?;
        status::create_table(tx)
    },
    upgrade: |tx, from| {
        if from < 2 {
            messages::create_tables(tx)?;
        }
        if from < 3 {
            status::create_table(tx)?;
        }
        Ok(())
    },
};

/// A counter and a condition variable: the commit side bumps it, followers wait on it.
#[derive(Default)]
struct ChangeSignal {
    count: std::sync::Mutex<u64>,
    changed: std::sync::Condvar,
}

impl ChangeSignal {
    fn token(&self) -> u64 {
        *self.count.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn bump(&self) {
        *self.count.lock().unwrap_or_else(|p| p.into_inner()) += 1;
        self.changed.notify_all();
    }

    fn wait(&self, seen: u64, timeout: Duration) -> u64 {
        let guard = self.count.lock().unwrap_or_else(|p| p.into_inner());
        let (guard, _) = self
            .changed
            .wait_timeout_while(guard, timeout, |count| *count == seen)
            .unwrap_or_else(|p| p.into_inner());
        *guard
    }
}

/// An office: ONE folder, `<home>/offices/<office-id>/`, served by one `org-office-server`.
///
/// ```text
/// office.lock    exclusive flock, taken before anything else is looked at (single owner)
/// org.db         SQLCipher: the organization's data (HQ's original, or a satellite's copy)
/// office.db      SQLCipher, the same key: the office's own data
/// workspace/     the office's files
/// office.json    plaintext, no secrets, written LAST by `create`: its presence is the
///                "this office is finished" marker the host reads
/// ```
///
/// The key is the host's: it is given to `create`, `open` and `discard`, and never made, stored or
/// deleted here (see [`Key`]). Safe to share across threads. The office owns the rules (the lock, the folder, the order things
/// are made in); `Store` owns the database engine.
pub struct Office {
    folder: PathBuf,
    info: OfficeInfo,
    /// Counts committed organization commands, to wake whoever follows the log.
    changes: ChangeSignal,
    // Databases close (and checkpoint) before the lock is released: fields drop in order.
    org: Store,
    local: Store,
    _lock: File,
}

impl std::fmt::Debug for Office {
    /// What `office.json` records; never a key or a database.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Office")
            .field("folder", &self.folder)
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

impl Office {
    /// Creates a new office in `folder` (`<home>/offices/<office-id>`, whose name must be the
    /// office id) and opens it with `key`, **which the host made and holds** (ADR-0002, key
    /// custody): the office never makes, stores or deletes a key. The host saves the key before
    /// it asks for the create, so a crash can't leave data without its key.
    ///
    /// The order is: lock the folder; look at it once (anything of an office already there is
    /// `OfficeExists`; nothing is ever reused or replaced); make both databases; make
    /// `workspace/`; commit the organization; write `office.json` last. A create that fails
    /// removes what it made, so the id can be created again. A crash part-way leaves a folder
    /// without `office.json`, an unfinished create for `discard`.
    pub fn create(
        folder: impl Into<PathBuf>,
        new: &NewOffice,
        key: Key,
    ) -> Result<Self, OfficeError> {
        Self::create_waiting(folder, new, key, LOCK_WAIT)
    }

    /// [`Office::create`] with a chosen lock wait.
    pub fn create_waiting(
        folder: impl Into<PathBuf>,
        new: &NewOffice,
        key: Key,
        lock_wait: Duration,
    ) -> Result<Self, OfficeError> {
        let info = OfficeInfo {
            office_id: new.office_id.clone(),
            office_name: new.office_name.trim().to_owned(),
            role: new.role,
            org_id: new.org_id.clone(),
            org_name: new.org_name.trim().to_owned(),
            created_at: time::now_rfc3339(),
        };
        // The same checks `office.json` will make, before anything is touched. A bad name is
        // `InvalidName` (the host answers `invalid_name`), a bad id is `Invalid`.
        for name in [&info.office_name, &info.org_name] {
            OrganizationName::parse(name).map_err(OfficeError::InvalidName)?;
        }
        if !is_plain_id(&info.office_id) || !is_plain_id(&info.org_id) {
            return Err(OfficeError::Invalid("ids are 1 to 64 letters and digits"));
        }
        let folder = plain_folder(folder.into())?;
        check_named_by(&folder, &info.office_id)?;
        check_not_a_link(&folder)?;
        private_dir(&folder).map_err(OfficeError::storage)?;
        // The folder may already exist (e.g. the host made it): owner-only regardless.
        tighten_dir(&folder).map_err(OfficeError::storage)?;
        let lock = lock_folder(&folder, lock_wait)?;

        // Under the lock, look once.
        for entry in OFFICE_ENTRIES {
            if exists(&folder.join(entry))? {
                return Err(OfficeError::OfficeExists);
            }
        }

        match build(&folder, &key, &info) {
            Ok((org, local)) => Ok(Self {
                folder,
                info,
                org,
                local,
                changes: ChangeSignal::default(),
                _lock: lock,
            }),
            Err(error) => {
                // Still under the lock. The databases closed when `build` failed. Every entry
                // removed is one this create made (none existed before, checked above); the
                // lock FILE stays, since another process may be waiting on it.
                let _ = remove_entries(&folder);
                drop(lock);
                Err(error)
            }
        }
    }

    /// Opens the finished office in `folder` with `key` (the host's). **Never makes a database**
    /// (it takes the folder's lock, which makes `office.lock` if there is none, and nothing
    /// else): no `office.json` is `NotFound`; one that names another office is `Invalid`; a key
    /// that does not open the databases is `KeyMissing` (fatal, never retried).
    pub fn open(folder: impl Into<PathBuf>, key: Key) -> Result<Self, OfficeError> {
        Self::open_waiting(folder, key, LOCK_WAIT)
    }

    /// [`Office::open`] with a chosen lock wait.
    pub fn open_waiting(
        folder: impl Into<PathBuf>,
        key: Key,
        lock_wait: Duration,
    ) -> Result<Self, OfficeError> {
        let folder = plain_folder(folder.into())?;
        check_not_a_link(&folder)?;
        match fs::metadata(&folder) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => return Err(OfficeError::Invalid("the office folder is not a folder")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(OfficeError::NotFound)
            }
            Err(e) => return Err(OfficeError::storage(e)),
        }
        // Lock first, then read what the folder says, once.
        let lock = lock_folder(&folder, lock_wait)?;
        let info = office_file::read(&folder)?.ok_or(OfficeError::NotFound)?;
        check_named_by(&folder, &info.office_id)?;

        let org = Store::open(
            &folder.join(ORG_DB),
            &key,
            Opening::Existing,
            &org_db::SCHEMA,
        )?;
        let local = Store::open(
            &folder.join(OFFICE_DB),
            &key,
            Opening::Existing,
            &OFFICE_SCHEMA,
        )?;
        let same = meta(&org, "office_id")? == Some(info.office_id.clone())
            && meta(&org, "organization_id")? == Some(info.org_id.clone())
            && meta(&local, "office_id")? == Some(info.office_id.clone());
        if !same {
            return Err(OfficeError::Invalid(
                "the databases belong to another office",
            ));
        }
        Ok(Self {
            folder,
            info,
            org,
            local,
            changes: ChangeSignal::default(),
            _lock: lock,
        })
    }

    /// Removes an UNFINISHED create (`org-office-server --discard`): the folder a failed create
    /// may have left. The key is the host's and is not touched here. When the folder exists it
    /// takes its lock (a missing folder takes none and makes none), and **refuses** anything
    /// that could be a real office, so one is never destroyed automatically:
    /// - a folder with `office.json`, or whose `org.db` holds a committed organization, is
    ///   `OfficeExists`;
    /// - databases it cannot check (no `key` was given, the key does not open them, SQLCipher
    ///   fails) are an error (`Storage`, or `KeyMissing` for a key that does not open them), and
    ///   nothing is deleted. With no `key`, only a folder with NO database file is removed.
    ///
    /// A refused discard removes nothing an office made (it may leave `office.lock`, and checking a
    /// database may leave an empty log and index file beside it, which SQLite ignores). A folder
    /// that is not there is fine; one that is a link, or holds anything an office does not make,
    /// is `Invalid`.
    pub fn discard(
        folder: impl Into<PathBuf>,
        office_id: &str,
        key: Option<&Key>,
    ) -> Result<(), OfficeError> {
        Self::discard_waiting(folder, office_id, key, LOCK_WAIT)
    }

    /// [`Office::discard`] with a chosen lock wait.
    pub fn discard_waiting(
        folder: impl Into<PathBuf>,
        office_id: &str,
        key: Option<&Key>,
        lock_wait: Duration,
    ) -> Result<(), OfficeError> {
        if !is_plain_id(office_id) {
            return Err(OfficeError::Invalid(
                "an office id is 1 to 64 letters and digits",
            ));
        }
        let folder = plain_folder(folder.into())?;
        check_named_by(&folder, office_id)?;
        // No folder means no unfinished create to remove: take no lock and make nothing (the
        // host serializes creates and discards of one office id).
        check_not_a_link(&folder)?;
        if !exists(&folder)? {
            return Ok(());
        }
        let _lock = lock_folder(&folder, lock_wait)?;

        if exists(&folder.join(office_file::FILE_NAME))? {
            return Err(OfficeError::OfficeExists);
        }
        let org_db = folder.join(ORG_DB);
        let mut any_database = false;
        for name in [ORG_DB, "org.db-wal", OFFICE_DB, "office.db-wal"] {
            any_database |= holds_data(&folder.join(name))?;
        }
        if any_database {
            // Anything we cannot check is refused: a real office must never be destroyed.
            let key = key.ok_or_else(|| {
                OfficeError::storage(std::io::Error::other(
                    "cannot check the databases: no key was given",
                ))
            })?;
            if !holds_data(&org_db)? && holds_data(&folder.join("org.db-wal"))? {
                // SQLite deletes a log that sits beside an empty main file on the first read.
                return Err(OfficeError::storage(std::io::Error::other(
                    "org.db is empty but its log is not: not looking",
                )));
            }
            if holds_data(&org_db)? {
                // "Committed" means the log has an entry (the genesis): the organization is
                // whole. Anything less is an unfinished create.
                let committed = Store::inspect(&org_db, key, org_db::is_committed)?;
                if committed {
                    return Err(OfficeError::OfficeExists);
                }
            }
            // office.db is checked the same way, so a key that opens nothing never has more
            // power to delete than no key: the key must open every database that holds data.
            let office_db = folder.join(OFFICE_DB);
            if !holds_data(&office_db)? && holds_data(&folder.join("office.db-wal"))? {
                return Err(OfficeError::storage(std::io::Error::other(
                    "office.db is empty but its log is not: not looking",
                )));
            }
            if holds_data(&office_db)? {
                Store::inspect(&office_db, key, |_| Ok(()))?;
            }
        }
        // Nothing here is a finished office. Empty the folder (all but the lock file), then
        // remove the lock file and the folder, so the lock is held until the end.
        empty_folder_but_the_lock(&folder, &mut |_| {})?;
        remove_the_empty_folder(&folder)
    }

    /// The organization's structure as of the last log entry, from one consistent read.
    pub fn org_snapshot(&self) -> Result<OrgSnapshot, OfficeError> {
        self.org.read(org_db::snapshot)
    }

    /// `who`: the directory cards matching `params`, from one consistent read of the organization
    /// and the runtime data (`runtime`: presence, status lines). `Err(ApiError)` inside the `Ok`
    /// is the caller's mistake (see [`crate::directory::who`]); the outer error is the storage's.
    pub fn who(
        &self,
        params: &WhoParams,
        runtime: &dyn RuntimeView,
    ) -> Result<Result<WhoResult, ApiError>, OfficeError> {
        let snapshot = self.org_snapshot()?;
        let office = ThisOffice {
            id: &self.info.office_id,
            name: &self.info.office_name,
        };
        Ok(directory::who(
            &snapshot,
            office,
            &time::now_rfc3339(),
            runtime,
            params,
        ))
    }

    /// Runs one organization command on this office (HQ): the rules decide, and the changes
    /// are logged and applied in one transaction. A retry with the same `command_id` gets the
    /// same answer. `Err(ApiError)` inside the `Ok` is the rules' refusal; the outer error is
    /// the storage's.
    pub fn submit_org(
        &self,
        command: OrgCommand,
    ) -> Result<Result<CommandAccepted, ApiError>, OfficeError> {
        // The clock is read in the writer, so the log's times follow its order.
        let answer = self
            .org
            .write(move |db| Ok((org_db::submit(db, time::now_ms(), &command)?, None)))?;
        // After the commit: whoever follows the log reads it again (a retry of an old command
        // changes nothing, and the extra wake-up is harmless).
        self.changes.bump();
        Ok(answer)
    }

    /// The position of the last log entry, from one consistent read.
    pub fn org_head(&self) -> Result<Position, OfficeError> {
        self.org.read(org_db::head)
    }

    /// Whether a subscriber that holds the organization as of `after` can be sent what follows
    /// (`false`: it must take a fresh snapshot). See `subscribe { from_position }`.
    pub fn org_can_resume_from(&self, after: Position) -> Result<bool, OfficeError> {
        self.org.read(|db| org_db::can_resume_from(db, after))
    }

    /// Up to `limit` committed commands after `after`, oldest first: the data of the
    /// `org.changed` events.
    pub fn org_changes_after(
        &self,
        after: Position,
        limit: usize,
    ) -> Result<Vec<OrgChangedData>, OfficeError> {
        self.org.read(|db| org_db::changes_after(db, after, limit))
    }

    /// A number that changes whenever a command has been committed. Read it BEFORE reading the
    /// log, then [`Office::wait_for_org_change`] with it: a commit in between is not missed.
    pub fn org_change_token(&self) -> u64 {
        self.changes.token()
    }

    /// Wakes everything waiting in [`Office::wait_for_org_change`] without a commit, for a live
    /// event (a message, a status line) that followers should push now. They read the log and
    /// their live events again; nothing in the log changed.
    pub fn wake_followers(&self) {
        self.changes.bump();
    }

    /// Waits until the token is no longer `seen` (a command was committed) or `timeout` passes,
    /// and returns the current token.
    pub fn wait_for_org_change(&self, seen: u64, timeout: Duration) -> u64 {
        self.changes.wait(seen, timeout)
    }

    /// `office.db`, where the office's own data lives (messages).
    pub(crate) fn local_store(&self) -> &Store {
        &self.local
    }

    /// `org.db`, the organization's data.
    pub(crate) fn org_store(&self) -> &Store {
        &self.org
    }

    /// Test seam: replaces the organization's members with `members`, as an organization applied
    /// up to `position` (the members are written raw, as a damaged or foreign organization could
    /// hold them).
    #[cfg(test)]
    pub(crate) fn seed_organization_for_test(
        &self,
        members: &[org_proto::Member],
        position: Position,
    ) {
        let members = members.to_vec();
        self.org
            .write(move |db| {
                org_db::seed_for_test(db, &members, position)?;
                Ok(((), None))
            })
            .expect("the organization could not be seeded");
    }

    /// What `office.json` records about this office.
    pub fn info(&self) -> &OfficeInfo {
        &self.info
    }

    /// The office folder.
    pub fn folder(&self) -> &Path {
        &self.folder
    }
}

/// Makes the two databases, the workspace and `office.json` in `folder`, in that order, with the
/// organization's own commit last before `office.json`: from that commit on, `discard` refuses
/// the folder, so the window in which a crash leaves something it refuses is as small as it can
/// be.
fn build(folder: &Path, key: &Key, info: &OfficeInfo) -> Result<(Store, Store), OfficeError> {
    let org = Store::open(&folder.join(ORG_DB), key, Opening::New, &org_db::SCHEMA)?;
    let local = Store::open(&folder.join(OFFICE_DB), key, Opening::New, &OFFICE_SCHEMA)?;
    failpoint(folder, "databases")?;
    put_meta(&org, &[("office_id", &info.office_id)])?;
    put_meta(&local, &[("office_id", &info.office_id)])?;
    private_dir(&folder.join(WORKSPACE)).map_err(OfficeError::storage)?;
    failpoint(folder, "workspace")?;
    let owner_id = crate::ulid::generate().map_err(OfficeError::storage)?;
    let genesis = org_db::Genesis {
        org_id: info.org_id.clone(),
        org_name: info.org_name.clone(),
        created_at: info.created_at.clone(),
        owner_id,
    };
    org.write(move |db| org_db::genesis(db, &genesis).map(|_| ((), None)))?;
    failpoint(folder, "organization")?;
    failpoint(folder, "office_json")?;
    office_file::write(folder, info)?;
    Ok((org, local))
}

fn put_meta(store: &Store, rows: &[(&str, &str)]) -> Result<(), OfficeError> {
    let rows: Vec<(String, String)> = rows
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    store.write(move |db| {
        for (key, value) in &rows {
            db.execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2)",
                (key, value),
            )
            .map_err(OfficeError::storage)?;
        }
        Ok(((), None))
    })
}

fn meta(store: &Store, key: &'static str) -> Result<Option<String>, OfficeError> {
    store.meta(key)
}

/// The office folder must be named by the office id, so an id never points outside `offices/`.
fn check_named_by(folder: &Path, office_id: &str) -> Result<(), OfficeError> {
    if folder.file_name().and_then(|name| name.to_str()) == Some(office_id) {
        Ok(())
    } else {
        Err(OfficeError::Invalid(
            "the folder is not named by the office id",
        ))
    }
}

/// Takes the folder's exclusive lock, waiting up to `wait` for another process to let go.
/// The lock file is close-on-exec (Rust opens every file that way), so a child never keeps
/// the lock after the server dies.
fn lock_folder(folder: &Path, wait: Duration) -> Result<File, OfficeError> {
    lock_folder_with(folder, wait, &|| {})
}

/// [`lock_folder`], calling `between` after the lock file is opened and before it is locked
/// (a test seam for the window in which another process can unlink the file).
fn lock_folder_with(
    folder: &Path,
    wait: Duration,
    between: &dyn Fn(),
) -> Result<File, OfficeError> {
    let path = folder.join(LOCK_FILE);
    let give_up_at = Instant::now() + wait;
    loop {
        let lock = match private_file(&path) {
            Ok(lock) => lock,
            // The folder was removed under us (a discard): there is nothing to lock.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(OfficeError::NotFound)
            }
            Err(e) => return Err(OfficeError::storage(e)),
        };
        between();
        match lock.try_lock() {
            // A lock on a file that was unlinked while we waited (a discard removed the
            // folder) locks nothing: only a lock on the file that is still there counts.
            Ok(()) if is_the_file_at(&lock, &path) => return Ok(lock),
            Ok(()) => {}
            Err(TryLockError::WouldBlock) if Instant::now() < give_up_at => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(TryLockError::WouldBlock) => return Err(OfficeError::InUse),
            Err(TryLockError::Error(e)) => return Err(OfficeError::storage(e)),
        }
        if Instant::now() >= give_up_at {
            return Err(OfficeError::InUse);
        }
    }
}

/// Whether `file` is the file now at `path` (same device and inode).
fn is_the_file_at(file: &File, path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (file.metadata(), fs::metadata(path)) {
        (Ok(open), Ok(named)) => open.dev() == named.dev() && open.ino() == named.ino(),
        _ => false,
    }
}

/// Removes the lock file and then the folder, which must be empty by now. Not recursive: if a
/// create of the same id slipped in while the lock file was unlinked, the folder is no longer
/// empty and is left to it.
fn remove_the_empty_folder(folder: &Path) -> Result<(), OfficeError> {
    match fs::remove_file(folder.join(LOCK_FILE)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(OfficeError::storage(e)),
    }
    match fs::remove_dir(folder) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        // Something started a create here; it is not ours to remove.
        Err(e) if e.kind() == std::io::ErrorKind::DirectoryNotEmpty => Ok(()),
        Err(e) => Err(OfficeError::storage(e)),
    }
}

/// Removes what an office makes from `folder`, all but the lock file. **Refuses** (and removes
/// nothing) a folder that holds anything else, so a wrong path whose last name happens to be an
/// id can never have its own files deleted. `removing` is told each name just before it goes.
fn empty_folder_but_the_lock(
    folder: &Path,
    removing: &mut dyn FnMut(&str),
) -> Result<(), OfficeError> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(folder).map_err(OfficeError::storage)? {
        let entry = entry.map_err(OfficeError::storage)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == LOCK_FILE {
            continue;
        }
        if !OFFICE_ENTRIES.contains(&name.as_str()) {
            return Err(OfficeError::Invalid(
                "the folder holds something an office does not make",
            ));
        }
        entries.push((name, entry));
    }
    // A failure or a crash part-way must leave a state a retry can finish: a database's main
    // file never goes before its log.
    entries.sort_by_key(|(name, _)| removal_rank(name));
    for (name, entry) in entries {
        removing(&name);
        let path = entry.path();
        let removed = if entry.file_type().map_err(OfficeError::storage)?.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        removed.map_err(OfficeError::storage)?;
    }
    Ok(())
}

/// The folder as an absolute path with no trailing separator or `.` components, so the link
/// check looks at the folder itself: `link/` would resolve the link and look like a folder.
fn plain_folder(folder: PathBuf) -> Result<PathBuf, OfficeError> {
    let absolute = absolute(folder).map_err(OfficeError::storage)?;
    Ok(absolute.components().collect())
}

/// Refuses a folder path that is a link (or anything but a directory): an office folder is
/// never reached through one, so nothing is created, locked or removed through a planted link.
fn check_not_a_link(folder: &Path) -> Result<(), OfficeError> {
    match fs::symlink_metadata(folder) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(OfficeError::Invalid(
            "the office folder is not a plain folder",
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(OfficeError::storage(e)),
    }
}

/// The order entries are removed in: everything else, then a database's log and index files,
/// then the main database files.
fn removal_rank(name: &str) -> u8 {
    if name == ORG_DB || name == OFFICE_DB {
        2
    } else if name.starts_with("org.db-") || name.starts_with("office.db-") {
        1
    } else {
        0
    }
}

/// Removal of what a failed `create` made (never the lock file); an error means some is left.
fn remove_entries(folder: &Path) -> std::io::Result<()> {
    #[cfg(test)]
    ON_STEP.with(|hook| {
        if let Some(hook) = hook.borrow().as_ref() {
            hook(folder, "cleanup");
        }
    });
    #[cfg(test)]
    if FAIL_CLEANUP.with(std::cell::Cell::get) {
        return Err(std::io::Error::other("injected cleanup failure"));
    }
    // In order, stopping at the first failure: a database's main file never goes before its
    // log, so what is left can still be discarded.
    for entry in OFFICE_ENTRIES {
        #[cfg(test)]
        if FAIL_REMOVING.with(std::cell::Cell::get) == Some(entry) {
            return Err(std::io::Error::other("injected removal failure"));
        }
        let path = folder.join(entry);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(&path)?,
            Ok(_) => fs::remove_file(&path)?,
            Err(_) => {}
        }
    }
    Ok(())
}

/// Whether anything is at `path` (a dangling link counts). Anything but "not found" is an
/// error, so a create never mistakes an unreadable entry for a missing one.
fn exists(path: &Path) -> Result<bool, OfficeError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(OfficeError::storage(e)),
    }
}

/// Whether `path` exists and is not empty. Anything but "not found" is an error: an
/// unreadable database must never be mistaken for no database.
fn holds_data(path: &Path) -> Result<bool, OfficeError> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.len() > 0),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(OfficeError::storage(e)),
    }
}

#[cfg(test)]
type StepHook = Box<dyn Fn(&Path, &str)>;

#[cfg(test)]
thread_local! {
    /// Makes `build` fail at the named step, to test what a half-done create leaves behind.
    static FAIL_AT: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
    /// Makes `remove_entries` fail when it reaches the named entry.
    static FAIL_REMOVING: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
    /// Makes `remove_entries` fail without removing anything.
    static FAIL_CLEANUP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Called at every step `build` reaches.
    static ON_STEP: std::cell::RefCell<Option<StepHook>> = const { std::cell::RefCell::new(None) };
    /// Each step `build` reached, and whether `office.json` existed by then.
    static REACHED: std::cell::RefCell<Vec<(&'static str, bool)>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
fn failpoint(folder: &Path, step: &'static str) -> Result<(), OfficeError> {
    let finished = folder.join(office_file::FILE_NAME).exists();
    REACHED.with(|reached| reached.borrow_mut().push((step, finished)));
    ON_STEP.with(|hook| {
        if let Some(hook) = hook.borrow().as_ref() {
            hook(folder, step);
        }
    });
    if FAIL_AT.with(std::cell::Cell::get) == Some(step) {
        return Err(OfficeError::storage(std::io::Error::other(
            "injected failure",
        )));
    }
    Ok(())
}

#[cfg(not(test))]
fn failpoint(_folder: &Path, _step: &'static str) -> Result<(), OfficeError> {
    Ok(())
}

#[cfg(test)]
mod tests;
