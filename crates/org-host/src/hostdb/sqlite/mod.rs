//! SQLite adapter for the host registry ([`HostStore`], ADR-0018).
//!
//! Callers must hold `host.lock` and the instance lock. This module (and its submodules)
//! is the only place in `org-host` that imports `rusqlite` or holds SQL.

pub(crate) mod accounts;
mod administrator;
mod clock;
pub(crate) mod journal;
pub(crate) mod links;
pub(crate) mod lockout;
pub(crate) mod provisioning;
pub(crate) mod recovery;
mod schema;
pub(crate) mod sessions;
#[cfg(test)]
mod tests;
pub(crate) mod web_tls;

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use org_office_launch::recovery::PublicRecord;
use rusqlite::{config::DbConfig, Connection, ErrorCode, OpenFlags, TransactionBehavior};

use super::{Error, HostDbInit, HostStore, StoreFault};
use crate::accounts::identity::{AccountId, LoginKey};
use crate::host_secrets::HostSecrets;
use crate::links::{Issued, LinkKind, Subject};

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

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Store(e.sqlite_error_code().map_or(StoreFault::Other, fault_of))
    }
}

pub(crate) struct HostDb {
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
    /// Protected provisioning only. Recheck administrator authority inside the
    /// transaction; a legacy/member profile cannot establish organization ownership.
    pub(crate) fn establish_organization_owner(
        &self,
        administrator: &super::accounts::Account,
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
        if !crate::instance::registry::valid_instance_id(&init.instance_id) {
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
        db.pragma_update(None, "journal_mode", "DELETE")?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(schema::DDL)?;
        tx.execute_batch(provisioning::DDL)?;
        tx.execute_batch(recovery::DDL)?;
        tx.execute_batch(links::DDL)?;
        sessions::create_schema(&tx)?;
        web_tls::create_schema(&tx)?;
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        for (name, value) in [
            ("instance_id", init.instance_id.clone()),
            ("clock_high_ms", init.clock_high_ms.to_string()),
        ] {
            tx.execute("INSERT INTO meta(key,value) VALUES (?1,?2)", (name, value))?;
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
        if version != SCHEMA_VERSION {
            return Err(Error::Version);
        }
        for ddl in [schema::DDL, provisioning::DDL, recovery::DDL, links::DDL] {
            require_schema(&db, ddl)?;
        }
        recovery::verify_all(&db)?;
        let instance_id: String =
            db.query_row("SELECT value FROM meta WHERE key='instance_id'", [], |r| {
                r.get(0)
            })?;
        // Every digest is bound to this ID, so a tampered value is refused rather
        // than used.
        if !crate::instance::registry::valid_instance_id(&instance_id) {
            return Err(Error::InvalidId);
        }
        sessions::ensure_schema(&mut db)?;
        web_tls::ensure_schema(&mut db)?;
        enter_wal(&db)?;
        db.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, false)?;
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
    pub(crate) fn seed_account(
        &self,
        account_id: &str,
        login: &str,
        verifier: &super::journal::Verifier,
        admin: bool,
        at_ms: i64,
    ) -> Result<(), Error> {
        use crate::accounts::identity::{AccountId, LoginKey};
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
        )?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn seed_link_for_test(
        &self,
        account_id: &str,
        org_id: &str,
        member_id: &str,
    ) -> Result<(), Error> {
        let db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        db.execute(
            "INSERT INTO account_links VALUES (?1,?2,?3)",
            (account_id, org_id, member_id),
        )?;
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
        )?;
        db.execute(
            "INSERT INTO account_links VALUES (?1,?2,?3) ON CONFLICT(account_id,org_id) DO NOTHING",
            (account_id, org_id, member_id),
        )?;
        Ok(())
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
        Err(Error::Store(StoreFault::Corrupt))
    }
}
fn connect(path: &Path) -> Result<Connection, Error> {
    check_database_header(path)?;
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

impl HostStore for HostDb {
    fn verify_instance(&self, instance: &str) -> Result<(), Error> {
        HostDb::verify_instance(self, instance)
    }
    fn establish_organization_owner(
        &self,
        administrator: &super::accounts::Account,
        org: &str,
        member: &str,
    ) -> Result<(), Error> {
        HostDb::establish_organization_owner(self, administrator, org, member)
    }
    fn verify_selected_owner(&self, org: &str, account: &str, member: &str) -> Result<(), Error> {
        HostDb::verify_selected_owner(self, org, account, member)
    }
    fn has_administrator(&self) -> Result<bool, Error> {
        HostDb::administrator_exists(self)
    }
    fn account_credentials(
        &self,
        login_key: &LoginKey,
    ) -> Result<Option<super::accounts::Credentials>, Error> {
        HostDb::account_credentials(self, login_key)
    }
    fn account_authority(
        &self,
        account_id: &AccountId,
    ) -> Result<Option<super::accounts::Account>, Error> {
        HostDb::account_authority(self, account_id)
    }
    fn issue_unless_administrator(
        &self,
        kind: LinkKind,
        subject: &Subject,
        now_ms: i64,
    ) -> Result<Option<Issued>, crate::links::Error> {
        HostDb::issue_unless_administrator(self, kind, subject, now_ms)
    }
    fn login_lockout(
        &self,
        socket: super::lockout::Socket,
        key: &str,
        wall_ms: u64,
    ) -> Result<super::lockout::State, Error> {
        HostDb::login_lockout(self, socket, key, wall_ms)
    }
    fn record_login(
        &self,
        socket: super::lockout::Socket,
        key: &str,
        wall_ms: u64,
        outcome: super::lockout::Outcome,
    ) -> Result<super::lockout::State, Error> {
        HostDb::record_login(self, socket, key, wall_ms, outcome)
    }
    fn record_login_capped(
        &self,
        socket: super::lockout::Socket,
        key: &str,
        wall_ms: u64,
        outcome: super::lockout::Outcome,
        cap_ms: i64,
    ) -> Result<super::lockout::State, Error> {
        HostDb::record_login_capped(self, socket, key, wall_ms, outcome, cap_ms)
    }
    fn prune_login_lockouts(&self, wall_ms: u64) -> Result<usize, Error> {
        HostDb::prune_login_lockouts(self, wall_ms)
    }
    fn provisioning_taken(
        &self,
        name: &str,
        slug: &str,
    ) -> Result<Option<super::provisioning::Taken>, Error> {
        HostDb::provisioning_taken(self, name, slug)
    }
    fn provisioning_known(&self, operation: &str) -> Result<bool, Error> {
        HostDb::provisioning_known(self, operation)
    }
    fn provisioning_linked_to(
        &self,
        org: &str,
    ) -> Result<Option<super::provisioning::Reservation>, Error> {
        HostDb::provisioning_linked_to(self, org)
    }
    fn provisioning_reserved(&self) -> Result<Vec<super::provisioning::Reservation>, Error> {
        HostDb::provisioning_reserved(self)
    }
    fn provisioning_of(
        &self,
        account: &str,
    ) -> Result<Vec<super::provisioning::Reservation>, Error> {
        HostDb::provisioning_of(self, account)
    }
    fn provisioning_lookup(
        &self,
        account: &str,
        operation: &str,
    ) -> Result<Option<super::provisioning::Reservation>, Error> {
        HostDb::provisioning_lookup(self, account, operation)
    }
    fn provisioning_offices(&self) -> Result<BTreeSet<String>, Error> {
        HostDb::provisioning_offices(self)
    }
    fn provisioning(
        &self,
        account: &super::accounts::Account,
        operation: &str,
        fingerprint: &[u8; 32],
        wall: i64,
    ) -> Result<Option<super::provisioning::Reservation>, Error> {
        HostDb::provisioning(self, account, operation, fingerprint, wall)
    }
    fn reserve_provisioning(
        &self,
        account: &super::accounts::Account,
        frozen: &super::provisioning::Reservation,
    ) -> Result<super::provisioning::Reservation, Error> {
        HostDb::reserve_provisioning(self, account, frozen)
    }
    fn provision_key_stored(
        &self,
        r: &super::provisioning::Reservation,
        fingerprint: &str,
    ) -> Result<(), Error> {
        HostDb::provision_key_stored(self, r, fingerprint)
    }
    fn provision_linked(&self, r: &super::provisioning::Reservation) -> Result<(), Error> {
        HostDb::provision_linked(self, r)
    }
    fn recovery_state(&self, operation: &str) -> Result<Option<super::recovery::State>, Error> {
        HostDb::recovery_state(self, operation)
    }
    fn recovery_observed(&self, operation: &str) -> Result<super::recovery::Ledger, Error> {
        HostDb::recovery_observed(self, operation)
    }
    fn recovery_source(&self, org: &str) -> Result<super::provisioning::Reservation, Error> {
        HostDb::recovery_source(self, org)
    }
    fn recovery_status(
        &self,
        expected: &super::recovery::Ledger,
        after: i64,
    ) -> Result<org_host_control::recovery::StatusResult, Error> {
        HostDb::recovery_status(self, expected, after)
    }
    fn recovery_prepare(&self, public: &PublicRecord) -> Result<super::recovery::Ledger, Error> {
        HostDb::recovery_prepare(self, public)
    }
    fn recovery_delivered(
        &self,
        expected: &super::recovery::Ledger,
        account: &str,
        redeliver: bool,
        wall: i64,
    ) -> Result<super::recovery::Ledger, Error> {
        HostDb::recovery_delivered(self, expected, account, redeliver, wall)
    }
    fn recovery_ack_intent(
        &self,
        expected: &super::recovery::Ledger,
        account: &str,
        wall: i64,
    ) -> Result<super::recovery::Ledger, Error> {
        HostDb::recovery_ack_intent(self, expected, account, wall)
    }
    fn recovery_reconcile(&self, custody: &dyn HostSecrets) -> Result<(), Error> {
        HostDb::recovery_reconcile(self, custody)
    }
    fn web_tls_pair(&self) -> Result<Option<super::web_tls::StoredPair>, Error> {
        HostDb::web_tls_pair(self)
    }
    fn set_web_tls_pair(&self, pair: &super::web_tls::StoredPair) -> Result<(), Error> {
        HostDb::set_web_tls_pair(self, pair)
    }
    fn tls_set_lookup(
        &self,
        op: &super::journal::Attempt<'_>,
        wall_ms: i64,
    ) -> Result<super::web_tls::TlsLookup, Error> {
        HostDb::tls_set_lookup(self, op, wall_ms)
    }
    fn tls_set_write(
        &self,
        op: &super::journal::Attempt<'_>,
        wall_ms: i64,
        stored: &super::web_tls::TlsStored,
        pair: Option<&super::web_tls::StoredPair>,
    ) -> Result<Option<super::web_tls::TlsStored>, Error> {
        HostDb::tls_set_write(self, op, wall_ms, stored, pair)
    }
}

/// Test-only helper kept inside the SQLite adapter so external tests never import `rusqlite`
/// or write SQL directly (ADR-0018 ENG-1).
#[cfg(test)]
pub(crate) fn corrupt_provisioning_prepared_for_test(registry_path: &Path) {
    let raw = Connection::open(registry_path).unwrap();
    raw.execute_batch("PRAGMA ignore_check_constraints=ON;")
        .unwrap();
    assert_eq!(
        raw.execute("UPDATE selected_provisioning SET prepared='{'", [])
            .unwrap(),
        1
    );
}

/// Test-only helper kept inside the SQLite adapter so process tests read `linked` on disk
/// without importing `rusqlite` or SQL outside `hostdb::sqlite` (ADR-0018 ENG-1).
#[cfg(test)]
pub(crate) fn provisioning_linked_on_disk_for_test(registry_path: &Path, operation: &str) -> bool {
    Connection::open(registry_path)
        .unwrap()
        .query_row(
            "SELECT linked FROM selected_provisioning WHERE operation_id=?1",
            [operation],
            |r| r.get(0),
        )
        .expect("the reservation")
}
