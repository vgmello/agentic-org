//! Private registry persistence. Callers must hold host.lock and serialize bootstrap.
//! No production caller creates a registry until 2b.3 accepts bootstrap.

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
    Unsafe,
    Exists,
    Empty,
    KeyMissing,
    KeyLength,
    NoCipher,
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
    pub(crate) tls_pair: Option<TlsPair>,
}
pub(crate) struct HostDb {
    connection: Mutex<Connection>,
}
impl HostDb {
    pub(crate) fn create(
        home: &Path,
        secrets: &dyn HostSecrets,
        init: &HostDbInit,
    ) -> Result<Self, Error> {
        let home = checked_home(home)?;
        absent(&home.join(DB), Error::Exists)?;
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
        // A .put.* temp left when put's own cleanup failed is invisible to list/delete;
        // DevSecrets::open removes it on the next open (Rowan's accepted residual).
        for name in secrets.list()? {
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
        let generated = crate::secrets::generate_db_key()?;
        let key = Value::new(generated.expose_bytes().to_vec())?;
        drop(generated);
        secrets.put(&Name::parse("registry")?, &key)?;
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
        let mut db = connect(&path, &key)?;
        db.pragma_update(None, "journal_mode", "DELETE")?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(schema::DDL)?;
        tx.pragma_update(None, "user_version", 1)?;
        let bootstrap = serde_json::to_string(&init.bootstrap).map_err(|_| Error::Unsafe)?;
        for (name, value) in [
            ("instance_id", init.instance_id.clone()),
            ("clock_high_ms", init.clock_high_ms.to_string()),
            ("bootstrap", bootstrap),
        ] {
            tx.execute("INSERT INTO meta(key,value) VALUES (?1,?2)", (name, value))?;
        }
        if let Some(tls) = &init.tls_pair {
            tx.execute(
                "INSERT INTO tls_pair VALUES (1,?1,?2,?3)",
                (&tls.cert_pem, &tls.pair_fp, tls.set_at_ms),
            )?;
        }
        checkpoint(7)?; // A failure before COMMIT must roll back DDL and metadata together.
        tx.commit()?;
        let mode: String = db.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get(0))?;
        require_wal(&mode)?;
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
        install(&path, &home.join(DB))?;
        File::open(&home)?.sync_all()?;
        checkpoint(5)?;
        let opened = Self::open(&home, secrets)?;
        checkpoint(6)?;
        Ok(opened)
    }
    pub(crate) fn open(home: &Path, secrets: &dyn HostSecrets) -> Result<Self, Error> {
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
        let key = secrets
            .load(&Name::parse("registry")?)?
            .ok_or(Error::KeyMissing)?;
        let db = connect(&path, &key)?;
        let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version != 1 {
            return Err(Error::Version);
        }
        let mode: String = db.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get(0))?;
        require_wal(&mode)?;
        db.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, false)?;
        Ok(Self {
            connection: Mutex::new(db),
        })
    }
}

fn absent(path: &Path, present: Error) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(present),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
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
fn connect(path: &Path, key: &Value) -> Result<Connection, Error> {
    if key.expose().len() != 32 {
        return Err(Error::KeyLength);
    }
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    db.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)?;
    let mut pragma = Vec::with_capacity(83);
    pragma.extend_from_slice(b"PRAGMA key = \"x'");
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for b in key.expose() {
        pragma.extend_from_slice(&[HEX[(b >> 4) as usize], HEX[(b & 15) as usize]]);
    }
    pragma.extend_from_slice(b"'\";");
    // Wipe this owned copy on every exit; SQLite's own statement allocation is outside
    // this best-effort guarantee (as in org-office's raw-key connection).
    let pragma = Value::new(pragma)?;
    db.execute_batch(std::str::from_utf8(pragma.expose()).expect("ASCII key pragma"))?;
    drop(pragma);
    db.pragma_update(None, "busy_timeout", 5000)?;
    let cipher: String = db
        .query_row("PRAGMA cipher_version", [], |r| r.get(0))
        .unwrap_or_default();
    if cipher.is_empty() {
        return Err(Error::NoCipher);
    }
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
    // SAFETY: both paths are live NUL-terminated strings; exclusive rename never overwrites.
    #[cfg(target_os = "macos")]
    let rc = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
    #[cfg(target_os = "linux")]
    let rc = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}
fn checkpoint(step: u8) -> Result<(), Error> {
    #[cfg(test)]
    tests::checkpoint(step)?;
    let _ = step;
    Ok(())
}
