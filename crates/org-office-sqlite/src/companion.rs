//! Shared open-verify-rebuild helper for rebuildable protected SQLite companion databases
//! (`protected/budgets.db`, `protected/accounting.db`, `protected/providers.db`).
//!
//! Each rebuildable companion projection records its DDL digest in `companion_schema(id=1)`.
//! If an existing file is corrupt (`NotADatabase` / `DatabaseCorrupt` / failed `quick_check`),
//! partially initialized, or built from an older DDL digest, it is deleted (along with
//! `-wal`/`-shm`/`-journal`) and freshly created with `0600` permissions so the caller's
//! `reconcile_durable` pass can rebuild it cleanly from durable files (`TASK-97` / `ARCH-05`).

use std::path::Path;

use org_store::Sha256;
use rusqlite::{Connection, OptionalExtension};

use org_office::private_fs::{
    private_file, remove_sqlite_files, sync_dir, tighten_sqlite_sidecars,
};
use org_office::OfficeError;

const COMPANION_META_DDL: &str = "
CREATE TABLE IF NOT EXISTS companion_schema (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    digest TEXT NOT NULL
) STRICT;
";

/// Computes a deterministic SHA-256 digest over `(label, format_version, ddl)`.
pub(super) fn schema_digest(label: &str, format_version: u32, ddl: &str) -> String {
    let fmt = format_version.to_be_bytes();
    let label_len = (label.len() as u64).to_be_bytes();
    let ddl_len = (ddl.len() as u64).to_be_bytes();
    Sha256::of_parts(&[
        b"companion-sqlite",
        &fmt,
        &label_len,
        label.as_bytes(),
        &ddl_len,
        ddl.as_bytes(),
    ])
    .to_hex()
}

/// Opens a rebuildable protected companion SQLite database at `db_path` and runs `reconcile`.
///
/// If an existing DB matches the DDL digest AND passes `PRAGMA quick_check(1)`, it is reused;
/// if `reconcile` still encounters SQLite corruption on that reused DB, the file is deleted,
/// freshly recreated, and `reconcile` is retried ONCE (`TASK-97` / Finding #9).
pub(super) fn open_and_reconcile_companion_db<F>(
    db_path: &Path,
    label: &str,
    format_version: u32,
    ddl: &str,
    pragmas: &str,
    mut reconcile: F,
) -> Result<Connection, OfficeError>
where
    F: FnMut(&mut Connection) -> Result<(), OfficeError>,
{
    let expected = schema_digest(label, format_version, ddl);
    let mut reused_existing = false;
    let mut conn = if db_path.exists() {
        if let Some(c) = try_open_matching(db_path, pragmas, &expected)? {
            tighten_sqlite_sidecars(db_path).map_err(OfficeError::storage)?;
            reused_existing = true;
            c
        } else {
            remove_sqlite_files(db_path).map_err(OfficeError::storage)?;
            create_fresh_and_sync(db_path, pragmas, ddl, &expected)?
        }
    } else {
        create_fresh_and_sync(db_path, pragmas, ddl, &expected)?
    };

    match reconcile(&mut conn) {
        Ok(()) => Ok(conn),
        Err(err) if reused_existing && is_storage_corrupt_error(&err) => {
            drop(conn);
            remove_sqlite_files(db_path).map_err(OfficeError::storage)?;
            let mut fresh = create_fresh_and_sync(db_path, pragmas, ddl, &expected)?;
            reconcile(&mut fresh)?;
            Ok(fresh)
        }
        Err(err) => Err(err),
    }
}

fn create_fresh_and_sync(
    db_path: &Path,
    pragmas: &str,
    ddl: &str,
    expected_digest: &str,
) -> Result<Connection, OfficeError> {
    let conn = create_fresh_companion_db(db_path, pragmas, ddl, expected_digest)?;
    if let Some(parent) = db_path.parent() {
        sync_dir(parent).map_err(OfficeError::storage)?;
    }
    tighten_sqlite_sidecars(db_path).map_err(OfficeError::storage)?;
    Ok(conn)
}

fn is_corrupt_error(err: &rusqlite::Error) -> bool {
    matches!(
        err.sqlite_error_code(),
        Some(rusqlite::ErrorCode::NotADatabase | rusqlite::ErrorCode::DatabaseCorrupt)
    )
}

fn is_storage_corrupt_error(err: &OfficeError) -> bool {
    let mut current = std::error::Error::source(err);
    while let Some(src) = current {
        if let Some(sql_err) = src.downcast_ref::<rusqlite::Error>() {
            return is_corrupt_error(sql_err);
        }
        current = src.source();
    }
    false
}

fn try_open_matching(
    db_path: &Path,
    pragmas: &str,
    expected_digest: &str,
) -> Result<Option<Connection>, OfficeError> {
    let conn = match Connection::open(db_path) {
        Ok(c) => c,
        Err(err) if is_corrupt_error(&err) => return Ok(None),
        Err(err) => return Err(OfficeError::storage(err)),
    };
    if let Err(err) = conn.execute_batch(pragmas) {
        return if is_corrupt_error(&err) {
            Ok(None)
        } else {
            Err(OfficeError::storage(err))
        };
    }
    if !digest_matches(&conn, expected_digest)? || !quick_check_ok(&conn)? {
        return Ok(None);
    }
    Ok(Some(conn))
}

fn digest_matches(conn: &Connection, expected_digest: &str) -> Result<bool, OfficeError> {
    let has_meta: Result<i64, rusqlite::Error> = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'companion_schema'",
        [],
        |r| r.get(0),
    );
    let has_meta = match has_meta {
        Ok(n) => n,
        Err(err) if is_corrupt_error(&err) => return Ok(false),
        Err(err) => return Err(OfficeError::storage(err)),
    };
    if has_meta == 0 {
        return Ok(false);
    }
    let stored: Result<Option<String>, rusqlite::Error> = conn
        .query_row(
            "SELECT digest FROM companion_schema WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .optional();
    match stored {
        Ok(Some(d)) => Ok(d == expected_digest),
        Ok(None) => Ok(false),
        Err(err) if is_corrupt_error(&err) => Ok(false),
        Err(err) => Err(OfficeError::storage(err)),
    }
}

fn quick_check_ok(conn: &Connection) -> Result<bool, OfficeError> {
    let res: Result<String, rusqlite::Error> =
        conn.query_row("PRAGMA quick_check(1)", [], |r| r.get(0));
    match res {
        Ok(status) => Ok(status.eq_ignore_ascii_case("ok")),
        Err(err) if is_corrupt_error(&err) => Ok(false),
        Err(err) => Err(OfficeError::storage(err)),
    }
}

fn create_fresh_companion_db(
    db_path: &Path,
    pragmas: &str,
    ddl: &str,
    expected_digest: &str,
) -> Result<Connection, OfficeError> {
    // Pre-create with 0o600 before SQLite opens it (no 0o644 creation window).
    drop(private_file(db_path).map_err(OfficeError::storage)?);
    let conn = Connection::open(db_path).map_err(OfficeError::storage)?;
    conn.execute_batch(pragmas).map_err(OfficeError::storage)?;
    conn.execute_batch("BEGIN IMMEDIATE;")
        .map_err(OfficeError::storage)?;
    let init = (|| -> rusqlite::Result<()> {
        conn.execute_batch(ddl)?;
        conn.execute_batch(COMPANION_META_DDL)?;
        conn.execute(
            "INSERT OR REPLACE INTO companion_schema (id, digest) VALUES (1, ?1)",
            [expected_digest],
        )?;
        conn.execute_batch("COMMIT;")
    })();
    if let Err(err) = init {
        let _ = conn.execute_batch("ROLLBACK;");
        return Err(OfficeError::storage(err));
    }
    Ok(conn)
}
