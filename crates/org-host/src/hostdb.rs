//! Private registry persistence. Callers must hold host.lock and serialize bootstrap.
//! No production caller creates a registry until 2b.3 accepts bootstrap.

pub(crate) mod accounts;
mod clock;
mod first_account;
mod verifier;
pub(crate) use first_account::FirstAccount;
pub(crate) mod journal;
pub(crate) mod lockout;
pub(crate) mod provisioning;
pub(crate) mod recovery;
mod schema;
#[cfg(test)]
mod tests;

use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::host_secrets::{HostSecrets, Name, Value};
use rusqlite::{config::DbConfig, Connection, OpenFlags, TransactionBehavior};

const DB: &str = "host-registry.db";
const TMP: &str = "host-registry.db.tmp";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Error {
    InvalidId,
    Expired,
    ClockSkew,
    OperationReused,
    Conflict,
    Unsafe,
    Exists,
    Empty,
    Version,
    Io(io::ErrorKind),
    Sql(rusqlite::ErrorCode),
    Secret(crate::host_secrets::Error),
}
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e.kind())
    }
}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sql(
            e.sqlite_error_code()
                .unwrap_or(rusqlite::ErrorCode::Unknown),
        )
    }
}
impl From<crate::host_secrets::Error> for Error {
    fn from(e: crate::host_secrets::Error) -> Self {
        Self::Secret(e)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BootstrapRecord {
    pub(crate) operation_id: String,
    pub(crate) hq_operation_id: String,
    pub(crate) fingerprint: String,
    pub(crate) account: String,
    pub(crate) finished: bool,
}
pub(crate) struct TlsPair {
    pub(crate) cert_pem: String,
    pub(crate) pair_fp: [u8; 32],
    pub(crate) set_at_ms: i64,
    pub(crate) key: Value,
}
/// Already validated by bootstrap, including TLS parsing/matching/coverage.
pub(crate) struct HostDbInit {
    pub(crate) instance_id: String,
    pub(crate) clock_high_ms: i64,
    pub(crate) bootstrap: BootstrapRecord,
    pub(crate) first_account: FirstAccount,
    pub(crate) tls_pair: Option<TlsPair>,
}
pub(crate) struct HostDb {
    connection: Mutex<Connection>,
}
fn cleanup_uncommitted_registry(home: &Path) -> Result<(), Error> {
    for suffix in ["-wal", "-shm", "-journal"] {
        let path = home.join(format!("{DB}{suffix}"));
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        if !metadata.is_file() || metadata.uid() != crate::home::euid() {
            return Err(Error::Unsafe);
        }
        fs::remove_file(path)?;
        crate::host_log::log(&format!("removed orphan host registry sidecar {suffix}"));
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
    pub(crate) fn bootstrap_record(&self) -> Result<BootstrapRecord, Error> {
        let db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let value: String =
            db.query_row("SELECT value FROM meta WHERE key='bootstrap'", [], |r| {
                r.get(0)
            })?;
        serde_json::from_str(&value).map_err(|_| Error::Unsafe)
    }
    /// Exit is a completion only after this bootstrap's exact HQ reservation
    /// linked. Interrupted/abandoned setup retains its original replay route.
    pub(crate) fn finish_bootstrap(&self) -> Result<(), Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let value: String =
            tx.query_row("SELECT value FROM meta WHERE key='bootstrap'", [], |r| {
                r.get(0)
            })?;
        let mut record: BootstrapRecord =
            serde_json::from_str(&value).map_err(|_| Error::Unsafe)?;
        let linked: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM selected_provisioning WHERE operation_id=?1 AND account=?2 AND linked=1)", (&record.hq_operation_id, &record.account), |r|r.get(0))?;
        if linked && !record.finished {
            record.finished = true;
            tx.execute(
                "UPDATE meta SET value=?1 WHERE key='bootstrap'",
                [serde_json::to_string(&record).map_err(|_| Error::Unsafe)?],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    /// Protected provisioning only. Recheck administrator authority inside the
    /// transaction; a legacy/member profile cannot establish organization ownership.
    pub(crate) fn establish_organization_owner(
        &self,
        administrator: &accounts::Account,
        org: &str,
        member: &str,
    ) -> Result<(), Error> {
        if !org_office_launch::is_storage_id(org) || !org_proto::is_valid_member_id(member) {
            return Err(Error::Unsafe);
        }
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = administrator.account_id();
        let admin: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM accounts WHERE account_id=?1 AND admin=1 AND revoked_at_ms IS NULL)", [id], |r|r.get(0))?;
        if !admin {
            return Err(Error::Unsafe);
        }
        tx.execute(
            "INSERT INTO org_owners VALUES(?1,?2,?3) ON CONFLICT(org_id) DO NOTHING",
            (org, id, member),
        )?;
        tx.execute(
            "INSERT INTO account_links VALUES(?1,?2,?3) ON CONFLICT(account_id,org_id) DO NOTHING",
            (id, org, member),
        )?;
        let valid: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM org_owners o JOIN account_links l ON l.account_id=o.account AND l.org_id=o.org_id AND l.member_id=o.owner_member_id WHERE o.org_id=?1 AND o.account=?2 AND o.owner_member_id=?3)", (org,id,member), |r|r.get(0))?;
        if !valid {
            return Err(Error::Conflict);
        }
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn verify_selected_owner(
        &self,
        org: &str,
        account: &str,
        member: &str,
    ) -> Result<(), Error> {
        let db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let valid: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM org_owners o JOIN accounts a ON a.account_id=o.account JOIN account_links l ON l.account_id=a.account_id AND l.org_id=o.org_id WHERE o.org_id=?1 AND o.account=?2 AND o.owner_member_id=?3 AND l.member_id=o.owner_member_id AND a.revoked_at_ms IS NULL)", (org,account,member), |r|r.get(0))?;
        if valid {
            Ok(())
        } else {
            Err(Error::Unsafe)
        }
    }
    pub(crate) fn verify_instance(&self, instance: &str) -> Result<(), Error> {
        let db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let recorded: String =
            db.query_row("SELECT value FROM meta WHERE key='instance_id'", [], |r| {
                r.get(0)
            })?;
        if recorded != instance {
            return Err(Error::Unsafe);
        }
        Ok(())
    }

    pub(crate) fn create(
        home: &Path,
        secrets: &dyn HostSecrets,
        init: &HostDbInit,
    ) -> Result<Self, Error> {
        if init.first_account.account_id() != init.bootstrap.account {
            return Err(Error::Unsafe);
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
        if let Some(tls) = &init.tls_pair {
            secrets.put(
                &Name::parse(&format!("tls.{}", hex(&tls.pair_fp)))?,
                &tls.key,
            )?;
        }
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
        db.pragma_update(None, "journal_mode", "DELETE")?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(schema::DDL)?;
        tx.execute_batch(provisioning::DDL)?;
        tx.execute_batch(recovery::DDL)?;
        tx.pragma_update(None, "user_version", 3)?;
        let bootstrap = serde_json::to_string(&init.bootstrap).map_err(|_| Error::Unsafe)?;
        for (name, value) in [
            ("instance_id", init.instance_id.clone()),
            ("clock_high_ms", init.clock_high_ms.to_string()),
            ("bootstrap", bootstrap),
        ] {
            tx.execute("INSERT INTO meta(key,value) VALUES (?1,?2)", (name, value))?;
        }
        #[cfg(test)]
        tests::first_account::before_insert(&tx)?;
        tx.execute(
            "INSERT INTO accounts VALUES (?1,?2,?3,?4,1,?5,?5,NULL)",
            (
                init.first_account.account_id(),
                init.first_account.login_key(),
                init.first_account.login(),
                init.first_account.verifier().expose(),
                init.clock_high_ms,
            ),
        )?;
        if let Some(tls) = &init.tls_pair {
            tx.execute(
                "INSERT INTO tls_pair VALUES (1,?1,?2,?3)",
                (&tls.cert_pem, &tls.pair_fp, tls.set_at_ms),
            )?;
        }
        checkpoint(7)?; // A failure before COMMIT must roll back DDL and metadata together.
        tx.commit()?;
        enter_wal(&db)?;
        db.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, false)?;
        #[cfg(test)]
        assert!(!db.db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE)?);
        db.close().map_err(|(_, e)| Error::from(e))?;
        checkpoint(4)?;
        for name in ["host-registry.db.tmp-wal", "host-registry.db.tmp-shm"] {
            absent(&home.join(name), Error::Unsafe)?;
        }
        checked_file(&path, false)?
            .ok_or(Error::Empty)?
            .sync_all()?;
        checkpoint(9)?; // Immediately before publishing the registry and its first account.
        install(&path, &home.join(DB))?;
        checkpoint(10)?; // Process death after rename must reopen exactly one first admin.
        File::open(&home)?.sync_all()?;
        checkpoint(5)?;
        let opened = Self::open(&home, secrets)?;
        checkpoint(6)?;
        Ok(opened)
    }
    /// Opens the plain-SQLite registry. `_custody` is not consulted: the registry
    /// has no database key (ADR-0016). The parameter remains only until the
    /// service caller drops it with the launch-contract cleanup.
    pub(crate) fn open(home: &Path, _custody: &dyn HostSecrets) -> Result<Self, Error> {
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
        let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version == 1 || version == 2 {
            require_legacy_schema(&db)?;
            if version == 2 {
                require_schema(&db, provisioning::DDL)?;
            }
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            if version == 1 {
                tx.execute_batch(provisioning::DDL)?;
            }
            tx.execute_batch(recovery::DDL)?;
            tx.pragma_update(None, "user_version", 3)?;
            tx.commit()?;
        } else if version != 3 {
            return Err(Error::Version);
        }
        require_legacy_schema(&db)?;
        require_schema(&db, provisioning::DDL)?;
        require_schema(&db, recovery::DDL)?;
        recovery::verify_all(&db)?;
        enter_wal(&db)?;
        db.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, false)?;
        Ok(Self {
            connection: Mutex::new(db),
        })
    }
}

/// Only the known registry schema may be upgraded. A version number alone is
/// not evidence that an unknown/corrupt database is this host's registry. Check
/// before any migration write, retaining all legacy rows and unknown state.
fn require_legacy_schema(db: &Connection) -> Result<(), Error> {
    require_schema(db, schema::DDL)
}
fn require_schema(db: &Connection, source: &str) -> Result<(), Error> {
    for ddl in source.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let words: Vec<_> = ddl.split_whitespace().collect();
        let kind = match words.get(1).copied() {
            Some("TABLE") => "table",
            Some("INDEX") => "index",
            _ => return Err(Error::Unsafe),
        };
        let name = words.get(2).ok_or(Error::Unsafe)?;
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

fn absent(path: &Path, present: Error) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(present),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
fn enter_wal(db: &Connection) -> Result<(), Error> {
    let mode: String = db.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get(0))?;
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
    let resolved = home.canonicalize()?;
    let m = fs::symlink_metadata(home)?;
    if !m.is_dir() || m.uid() != crate::home::euid() || m.mode() & 0o7777 != 0o700 {
        return Err(Error::Unsafe);
    }
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
    if !m.is_file() || m.uid() != crate::home::euid() || m.nlink() != 1 {
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
        Err(Error::Sql(rusqlite::ErrorCode::NotADatabase))
    }
}
fn connect(path: &Path) -> Result<Connection, Error> {
    sqlite_ready()?;
    check_database_header(path)?;
    #[allow(clippy::disallowed_methods)] // The sole durable opener crosses the fence.
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    db.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)?;
    db.pragma_update(None, "busy_timeout", 5000)?;
    db.query_row("SELECT count(*) FROM sqlite_schema", [], |r| {
        r.get::<_, i64>(0)
    })?;
    db.execute_batch("PRAGMA synchronous=FULL; PRAGMA fullfsync=ON; PRAGMA checkpoint_fullfsync=ON; PRAGMA foreign_keys=ON;")?;
    Ok(db)
}
/// Completes SQLite library initialization once before any opener (BUG-100 fence,
/// kept while the workspace's SQLite build still needs it; removed in A1.7).
fn sqlite_ready() -> Result<(), Error> {
    static READY: std::sync::OnceLock<i32> = std::sync::OnceLock::new();
    // SAFETY: sqlite3_initialize is thread-safe and idempotent.
    let code = *READY.get_or_init(|| unsafe { rusqlite::ffi::sqlite3_initialize() });
    if code != rusqlite::ffi::SQLITE_OK {
        return Err(Error::Sql(rusqlite::ffi::Error::new(code).code));
    }
    Ok(())
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
