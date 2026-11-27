//! SQLite adapter for the host registry ([`HostStore`], ADR-0018).
//!
//! Callers must hold `host.lock` and the instance lock. This crate is the sole host-side
//! persistence adapter that imports `rusqlite` or executes SQL.

#![forbid(unsafe_code)]

pub(crate) mod accounts;
mod administrator;
mod clock;
mod host_store;
pub(crate) mod journal;
pub(crate) mod links;
pub(crate) mod local_auth;
pub(crate) mod lockout;
pub(crate) mod provisioning;
pub(crate) mod recovery;
mod schema;
pub(crate) mod sessions;
#[cfg(all(feature = "test-support", debug_assertions))]
pub mod test_support;
#[cfg(test)]
mod tests;
pub(crate) mod web_tls;

use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use org_host::ports::host_store::{Error, HostDbInit, HostStore, StoreFault};
use org_host::ports::{HostPorts, HostSecrets, HostStoreOpener};
use org_host::{Config, Host, HostError};
use rusqlite::{config::DbConfig, Connection, ErrorCode, OpenFlags, TransactionBehavior};

const DB: &str = "host-registry.db";
const TMP: &str = "host-registry.db.tmp";
/// The registry schema version. There is no migration from earlier registries.
const SCHEMA_VERSION: i64 = 1;

fn fault_of(code: ErrorCode) -> StoreFault {
    match code {
        ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked => StoreFault::Busy,
        ErrorCode::SystemIoFailure => StoreFault::TransientIo,
        ErrorCode::OutOfMemory => StoreFault::OutOfMemory,
        ErrorCode::ConstraintViolation => StoreFault::Constraint,
        ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase => StoreFault::Corrupt,
        _ => StoreFault::Other,
    }
}

pub(crate) fn sqlite_err(e: rusqlite::Error) -> Error {
    Error::Store(e.sqlite_error_code().map_or(StoreFault::Other, fault_of))
}

pub(crate) trait SqliteResultExt<T> {
    fn db(self) -> Result<T, Error>;
}

impl<T> SqliteResultExt<T> for Result<T, rusqlite::Error> {
    fn db(self) -> Result<T, Error> {
        self.map_err(sqlite_err)
    }
}

/// Local SQLite [`HostStoreOpener`] adapter for `org-host`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SqliteHostStoreOpener;

impl HostStoreOpener for SqliteHostStoreOpener {
    fn exists(&self, office_home: &Path) -> Result<bool, io::Error> {
        match fs::symlink_metadata(office_home.join(DB)) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn create(
        &self,
        office_home: &Path,
        custody: &dyn HostSecrets,
        init: &HostDbInit,
    ) -> Result<Arc<dyn HostStore>, Error> {
        create_host_store(office_home, custody, init)
    }

    fn open(
        &self,
        office_home: &Path,
        custody: &dyn HostSecrets,
    ) -> Result<Arc<dyn HostStore>, Error> {
        open_host_store(office_home, custody)
    }
}

/// Opens an existing local SQLite host registry as a trait object.
pub fn open_host_store(
    home: &Path,
    custody: &dyn HostSecrets,
) -> Result<Arc<dyn HostStore>, Error> {
    HostDb::open(home, custody).map(|db| Arc::new(db) as Arc<dyn HostStore>)
}

/// Creates a new local SQLite host registry as a trait object.
pub fn create_host_store(
    home: &Path,
    custody: &dyn HostSecrets,
    init: &HostDbInit,
) -> Result<Arc<dyn HostStore>, Error> {
    HostDb::create(home, custody, init).map(|db| Arc::new(db) as Arc<dyn HostStore>)
}

/// Starts the registered instance `name` (the default instance when `None`) backed by the
/// local SQLite host registry.
pub fn start_registered(config: Config, name: Option<&str>) -> Result<Host, HostError> {
    start_registered_with_ports(config, name, HostPorts::default())
}

/// Starts the registered instance `name`, defaulting to [`SqliteHostStoreOpener`] when the
/// caller supplied neither `ports.store` nor `ports.store_opener`.
pub fn start_registered_with_ports(
    config: Config,
    name: Option<&str>,
    mut ports: HostPorts,
) -> Result<Host, HostError> {
    if ports.store.is_none() && ports.store_opener.is_none() {
        ports.store_opener = Some(Arc::new(SqliteHostStoreOpener));
    }
    Host::start_registered_with_ports(config, name, ports)
}

/// Debug test-support composition entry for starting a reserved instance backed by local SQLite.
#[cfg(all(feature = "test-support", debug_assertions))]
pub fn start_instance_for_test(
    config: Config,
    locator: &Path,
    user_temp: &Path,
    name: Option<&str>,
    root: &Path,
) -> Result<Host, HostError> {
    let ports = HostPorts {
        store_opener: Some(Arc::new(SqliteHostStoreOpener)),
        ..HostPorts::default()
    };
    org_host::start_instance_for_test_with_ports(config, locator, user_temp, name, root, ports)
}

pub struct HostDb {
    connection: Mutex<Connection>,
    instance_id: String,
}
fn cleanup_uncommitted_registry(home: &Path) -> Result<(), Error> {
    for suffix in ["-wal", "-shm", "-journal"] {
        let path = home.join(format!("{DB}{suffix}"));
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        if !metadata.is_file() || metadata.uid() != org_host::home::euid() {
            return Err(Error::Unsafe);
        }
        fs::remove_file(path)?;
        org_host::host_log::log(&format!("removed orphan host registry sidecar {suffix}"));
    }
    // Interrupted creates leave only temps; no installed registry needs rollback recovery.
    for name in [
        TMP,
        "host-registry.db.tmp-journal",
        "host-registry.db.tmp-wal",
        "host-registry.db.tmp-shm",
    ] {
        match fs::remove_file(home.join(name)) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

impl HostDb {
    /// Protected provisioning only. Recheck administrator authority inside the
    /// transaction; a legacy/member profile cannot establish organization ownership.
    pub fn establish_organization_owner(
        &self,
        administrator: &org_host::ports::host_store::accounts::Account,
        org: &str,
        member: &str,
    ) -> Result<(), Error> {
        if !org_office_launch::is_storage_id(org) || !org_proto::is_valid_member_id(member) {
            return Err(Error::Unsafe);
        }
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .db()?;
        let id = administrator.account_id();
        let admin: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM accounts WHERE account_id=?1 AND admin=1 AND revoked_at_ms IS NULL)",
                [id],
                |r| r.get(0),
            )
            .db()?;
        if !admin {
            return Err(Error::Unsafe);
        }
        tx.execute(
            "INSERT INTO org_owners VALUES(?1,?2,?3) ON CONFLICT(org_id) DO NOTHING",
            (org, id, member),
        )
        .db()?;
        tx.execute(
            "INSERT INTO account_links VALUES(?1,?2,?3) ON CONFLICT(account_id,org_id) DO NOTHING",
            (id, org, member),
        )
        .db()?;
        let valid: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM org_owners o JOIN account_links l ON l.account_id=o.account AND l.org_id=o.org_id AND l.member_id=o.owner_member_id WHERE o.org_id=?1 AND o.account=?2 AND o.owner_member_id=?3)",
                (org, id, member),
                |r| r.get(0),
            )
            .db()?;
        if !valid {
            return Err(Error::Conflict);
        }
        tx.commit().db()?;
        Ok(())
    }
    pub fn verify_selected_owner(
        &self,
        org: &str,
        account: &str,
        member: &str,
    ) -> Result<(), Error> {
        let db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let valid: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM org_owners o JOIN accounts a ON a.account_id=o.account JOIN account_links l ON l.account_id=a.account_id AND l.org_id=o.org_id WHERE o.org_id=?1 AND o.account=?2 AND o.owner_member_id=?3 AND l.member_id=o.owner_member_id AND a.revoked_at_ms IS NULL)",
                (org, account, member),
                |r| r.get(0),
            )
            .db()?;
        if valid {
            Ok(())
        } else {
            Err(Error::Unsafe)
        }
    }
    pub fn verify_instance(&self, instance: &str) -> Result<(), Error> {
        let db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let recorded: String = db
            .query_row("SELECT value FROM meta WHERE key='instance_id'", [], |r| {
                r.get(0)
            })
            .db()?;
        if recorded != instance {
            return Err(Error::Unsafe);
        }
        Ok(())
    }

    pub fn create(
        home: &Path,
        secrets: &dyn HostSecrets,
        init: &HostDbInit,
    ) -> Result<Self, Error> {
        if !org_host::instance::registry::valid_instance_id(&init.instance_id) {
            return Err(Error::InvalidId);
        }
        let home = checked_home(home)?;
        absent(&home.join(DB), Error::Exists)?;
        let names = secrets.list()?;
        // Pending recovery custody can precede writer reservation. Its presence,
        // even if malformed, requires forward repair before any cleanup/reset.
        if names.iter().any(|name| {
            let name = name.as_str();
            name == "writer-device" || name.starts_with("writer.") || name.starts_with("recovery.")
        }) {
            return Err(Error::Unsafe);
        }
        cleanup_uncommitted_registry(&home)?;
        // A .put.* temp left when put's own cleanup failed is invisible to list/delete;
        // DevSecrets::open removes it on the next open (Rowan's accepted residual).
        for name in names {
            secrets.delete(&name)?;
        }
        checkpoint(1)?;
        checkpoint(2)?;
        // The registry is plain SQLite protected by the account and file permissions
        // (ADR-0016); no database key is generated or stored.
        checkpoint(3)?;
        let path = home.join(TMP);
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)?;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        drop(file); // A later close on this inode would cancel SQLite's POSIX locks.
        let mut db = connect(&path)?;
        db.pragma_update(None, "journal_mode", "DELETE").db()?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .db()?;
        tx.execute_batch(schema::DDL).db()?;
        tx.execute_batch(provisioning::DDL).db()?;
        tx.execute_batch(recovery::DDL).db()?;
        tx.execute_batch(links::DDL).db()?;
        sessions::create_schema(&tx)?;
        web_tls::create_schema(&tx)?;
        accounts::create_preferences_schema(&tx)?;
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)
            .db()?;
        for (name, value) in [
            ("instance_id", init.instance_id.clone()),
            ("clock_high_ms", init.clock_high_ms.to_string()),
        ] {
            tx.execute("INSERT INTO meta(key,value) VALUES (?1,?2)", (name, value))
                .db()?;
        }
        checkpoint(7)?; // A failure before COMMIT must roll back DDL and metadata together.
        tx.commit().db()?;
        enter_wal(&db)?;
        db.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, false)
            .db()?;
        #[cfg(test)]
        assert!(!db
            .db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE)
            .db()?);
        db.close().map_err(|(_, e)| sqlite_err(e))?;
        checkpoint(4)?;
        for name in ["host-registry.db.tmp-wal", "host-registry.db.tmp-shm"] {
            absent(&home.join(name), Error::Unsafe)?;
        }
        checked_file(&path, false)?
            .ok_or(Error::Empty)?
            .sync_all()?;
        checkpoint(9)?; // Immediately before publishing the registry.
        install(&path, &home.join(DB))?;
        checkpoint(10)?; // Process death after rename must reopen the published registry.
        File::open(&home)?.sync_all()?;
        checkpoint(5)?;
        let opened = Self::open(&home, secrets)?;
        checkpoint(6)?;
        Ok(opened)
    }
    /// Opens the plain-SQLite registry. `_custody` is not consulted: the registry
    /// has no database key (ADR-0016). The parameter remains only until the
    /// service caller drops it with the launch-contract cleanup.
    pub fn open(home: &Path, _custody: &dyn HostSecrets) -> Result<Self, Error> {
        let home = checked_home(home)?;
        absent(&home.join("host-registry.db-journal"), Error::Unsafe)?;
        let path = home.join(DB);
        checked_file(&path, false)?.ok_or(Error::Empty)?;
        for path in [
            &path,
            &home.join("host-registry.db-wal"),
            &home.join("host-registry.db-shm"),
        ] {
            if let Some(file) = checked_file(path, true)? {
                file.set_permissions(fs::Permissions::from_mode(0o600))?;
            }
        }
        checkpoint(8)?; // Test the SQLite no-follow fence after validation.
        let mut db = connect(&path)?;
        let version: i64 = db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .db()?;
        if version != SCHEMA_VERSION {
            return Err(Error::Version);
        }
        for ddl in [schema::DDL, provisioning::DDL, recovery::DDL, links::DDL] {
            require_schema(&db, ddl)?;
        }
        recovery::verify_all(&db)?;
        let instance_id: String = db
            .query_row("SELECT value FROM meta WHERE key='instance_id'", [], |r| {
                r.get(0)
            })
            .db()?;
        // Every digest is bound to this ID, so a tampered value is refused rather
        // than used.
        if !org_host::instance::registry::valid_instance_id(&instance_id) {
            return Err(Error::InvalidId);
        }
        sessions::ensure_schema(&mut db)?;
        web_tls::ensure_schema(&mut db)?;
        accounts::ensure_preferences_schema(&mut db)?;
        enter_wal(&db)?;
        db.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, false)
            .db()?;
        Ok(Self {
            connection: Mutex::new(db),
            instance_id,
        })
    }
}

/// A version number alone is not evidence that an unknown or corrupt database is
/// this host's registry: every table and index must match its source DDL.
fn require_schema(db: &Connection, source: &str) -> Result<(), Error> {
    for ddl in source.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let words: Vec<_> = ddl.split_whitespace().collect();
        let (kind, name) = match words.get(1..3) {
            Some(["TABLE", name]) => ("table", name),
            Some(["INDEX", name]) => ("index", name),
            Some(["UNIQUE", "INDEX"]) => ("index", words.get(3).ok_or(Error::Unsafe)?),
            _ => return Err(Error::Unsafe),
        };
        let actual: String = db
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE type=?1 AND name=?2",
                (kind, name),
                |r| r.get(0),
            )
            .map_err(|_| Error::Unsafe)?;
        if actual.split_whitespace().ne(ddl.split_whitespace()) {
            return Err(Error::Unsafe);
        }
    }
    Ok(())
}

#[cfg(any(test, all(feature = "test-support", debug_assertions)))]
impl HostDb {
    /// Tests only: an account as a redeemed registration link will create it (A3).
    pub fn seed_account(
        &self,
        account_id: &str,
        login: &str,
        verifier: &org_host::ports::accounts::Verifier,
        admin: bool,
        at_ms: i64,
    ) -> Result<(), Error> {
        use org_host::ports::accounts::{AccountId, LoginKey};
        let id = AccountId::parse(account_id).map_err(|_| Error::InvalidId)?;
        let key = LoginKey::new(login).map_err(|_| Error::Unsafe)?;
        let db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        db.execute(
            "INSERT INTO accounts VALUES (?1,?2,?3,?4,?5,?6,?6,NULL)",
            (
                id.as_str(),
                key.as_str(),
                login,
                verifier.expose(),
                i64::from(admin),
                at_ms,
            ),
        )
        .db()?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn seed_owner_for_test(
        &self,
        account_id: &str,
        org_id: &str,
        member_id: &str,
    ) -> Result<(), Error> {
        let db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        db.execute(
            "INSERT INTO org_owners VALUES (?1,?2,?3)",
            (org_id, account_id, member_id),
        )
        .db()?;
        db.execute(
            "INSERT INTO account_links VALUES (?1,?2,?3) ON CONFLICT(account_id,org_id) DO NOTHING",
            (account_id, org_id, member_id),
        )
        .db()?;
        Ok(())
    }

    pub fn recovery_db_unlocked_for_test(&self) -> bool {
        self.connection.try_lock().is_ok()
    }
}

fn absent(path: &Path, present: Error) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(present),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
fn enter_wal(db: &Connection) -> Result<(), Error> {
    let mode: String = db
        .pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get(0))
        .db()?;
    require_wal(&mode)
}
fn require_wal(mode: &str) -> Result<(), Error> {
    // A VFS can return its unchanged mode instead of accepting WAL.
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(Error::Unsafe);
    }
    Ok(())
}

fn checked_home(home: &Path) -> Result<PathBuf, Error> {
    if !home.is_absolute() {
        return Err(Error::Unsafe);
    }
    // Strip trailing `/` and `/.` components so POSIX `lstat` inspects the final
    // path component itself rather than dereferencing a symlinked home directory.
    let norm: PathBuf = home.components().collect();
    let m = fs::symlink_metadata(&norm)?;
    if !m.is_dir() || m.uid() != org_host::home::euid() || m.mode() & 0o7777 != 0o700 {
        return Err(Error::Unsafe);
    }
    let resolved = norm.canonicalize()?;
    Ok(resolved)
}
fn checked_file(path: &Path, empty_ok: bool) -> Result<Option<File>, Error> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let m = file.metadata()?;
    if !m.is_file() || m.uid() != org_host::home::euid() || m.nlink() != 1 {
        return Err(Error::Unsafe);
    }
    if !empty_ok && m.len() == 0 {
        return Err(Error::Empty);
    }
    Ok(Some(file))
}
/// The first 16 bytes of every SQLite database file.
const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";
/// Refuses a main file that is not empty and not a SQLite database before SQLite opens it.
/// With a WAL beside it, SQLite reads page 1 from the log and would accept a main file that
/// is not a database; a later checkpoint would then write the log into that file. An empty
/// file passes: only `create` connects to one (its fresh temporary file), and `open` refuses
/// an empty registry through `checked_file` first.
fn check_database_header(path: &Path) -> Result<(), Error> {
    use std::io::Read;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)?;
    let mut header = Vec::with_capacity(SQLITE_HEADER.len());
    file.take(SQLITE_HEADER.len() as u64)
        .read_to_end(&mut header)?;
    if header.is_empty() || header == SQLITE_HEADER {
        Ok(())
    } else {
        Err(Error::Store(StoreFault::Corrupt))
    }
}
fn connect(path: &Path) -> Result<Connection, Error> {
    check_database_header(path)?;
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .db()?;
    db.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)
        .db()?;
    db.pragma_update(None, "busy_timeout", 5000).db()?;
    db.query_row("SELECT count(*) FROM sqlite_schema", [], |r| {
        r.get::<_, i64>(0)
    })
    .db()?;
    db.execute_batch("PRAGMA synchronous=FULL; PRAGMA fullfsync=ON; PRAGMA checkpoint_fullfsync=ON; PRAGMA foreign_keys=ON;")
        .db()?;
    Ok(db)
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn install(from: &Path, to: &Path) -> Result<(), Error> {
    use std::os::unix::ffi::OsStrExt;
    let from = std::ffi::CString::new(from.as_os_str().as_bytes()).map_err(|_| Error::Unsafe)?;
    let to = std::ffi::CString::new(to.as_os_str().as_bytes()).map_err(|_| Error::Unsafe)?;
    // Exclusive rename never overwrites.
    org_platform::fs::rename_noreplace(&from, &to)?;
    Ok(())
}
fn checkpoint(step: u8) -> Result<(), Error> {
    #[cfg(test)]
    tests::checkpoint(step)?;
    let _ = step;
    Ok(())
}

#[cfg(all(feature = "test-support", debug_assertions))]
pub use test_support::{
    corrupt_provisioning_prepared_for_test, provisioning_linked_on_disk_for_test,
};
