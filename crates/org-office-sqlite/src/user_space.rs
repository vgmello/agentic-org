//! The SQLite adapter of HQ's human-private file space (`TASK-98.1`, ADR-0037): the isolated
//! `<durable-root>/private/user-space.db`, made from `user-space-schema.sql` verbatim, outside
//! `office.db`, `durable/state/`, the journal and Git. Authoritative, never a cache.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use org_proto::attachments::{ArtifactStatus, AttachmentTargetKind};
use org_proto::ApiError;
use org_store::Id;
use rusqlite::{params, Connection, ErrorCode, OptionalExtension, Row, TransactionBehavior};

use org_office::attachments::blobs::ContentHash;
use org_office::attachments::records::{AttachmentRecord, MetaPatch};
use org_office::attachments::user_space::{PrivateOperation, UserSpaceStore};
use org_office::private_fs::{
    absolute, full_sync, private_dir_synced, private_file, sync_dir, tighten_sqlite_sidecars,
};
use org_office::storage_layout as layout;
use org_office::OfficeError;

/// The private space's DDL, as the data contract writes it.
const USER_SPACE_DDL: &str =
    include_str!("../../../docs/design/organization-data/user-space-schema.sql");

const COLUMNS: &str = "id, owner_employee_id, content_sha256, mime_type, byte_count, filename, \
                       title, description, artifact_status, width, height, page_count, \
                       created_at, updated_at";

/// `private/user-space.db` behind one serialized connection.
pub struct SqliteUserSpaceStore {
    db_path: PathBuf,
    conn: Mutex<Connection>,
}

impl SqliteUserSpaceStore {
    /// Opens (or makes) HQ's `<durable-root>/private/user-space.db`, owner-only.
    pub fn open(durable_root: &Path) -> Result<Self, OfficeError> {
        let folder = layout::private_notes(durable_root);
        private_dir_synced(durable_root, &folder).map_err(OfficeError::storage)?;
        let db_path = absolute(folder.join("user-space.db")).map_err(OfficeError::storage)?;
        // Made 0600 before SQLite opens it: there is no wider creation window.
        drop(private_file(&db_path).map_err(OfficeError::storage)?);
        let conn = Connection::open(&db_path).map_err(OfficeError::storage)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             PRAGMA fullfsync = ON;
             PRAGMA checkpoint_fullfsync = ON;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )
        .map_err(OfficeError::storage)?;
        initialize(&conn)?;
        sync_dir(&folder).map_err(OfficeError::storage)?;
        tighten_sqlite_sidecars(&db_path).map_err(OfficeError::storage)?;
        flush(&db_path)?;
        Ok(Self {
            db_path,
            conn: Mutex::new(conn),
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, OfficeError> {
        self.conn
            .lock()
            .map_err(|_| OfficeError::Invalid("user space connection poisoned"))
    }

    fn committed(&self) -> Result<(), OfficeError> {
        tighten_sqlite_sidecars(&self.db_path).map_err(OfficeError::storage)?;
        flush(&self.db_path)
    }
}

fn initialize(conn: &Connection) -> Result<(), OfficeError> {
    let tables: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'user_space_files'",
            [],
            |row| row.get(0),
        )
        .map_err(OfficeError::storage)?;
    if tables == 1 {
        return Ok(());
    }
    let anything: i64 = conn
        .query_row("SELECT COUNT(*) FROM sqlite_master", [], |row| row.get(0))
        .map_err(OfficeError::storage)?;
    if anything != 0 {
        return Err(OfficeError::Invalid(
            "corrupt or partially initialized user space database",
        ));
    }
    conn.execute_batch("BEGIN IMMEDIATE;")
        .map_err(OfficeError::storage)?;
    if let Err(error) = conn.execute_batch(USER_SPACE_DDL) {
        let _ = conn.execute_batch("ROLLBACK;");
        return Err(OfficeError::storage(error));
    }
    conn.execute_batch("COMMIT;").map_err(OfficeError::storage)
}

/// `full_sync` on the WAL (when there) and the database, so an acknowledged write never
/// depends on SQLite's own fallback to a plain `fsync`.
fn flush(db_path: &Path) -> Result<(), OfficeError> {
    let mut wal = db_path.as_os_str().to_os_string();
    wal.push("-wal");
    match std::fs::File::open(PathBuf::from(wal)) {
        Ok(file) => full_sync(&file).map_err(OfficeError::storage)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(OfficeError::storage(error)),
    }
    let db = std::fs::File::open(db_path).map_err(OfficeError::storage)?;
    full_sync(&db).map_err(OfficeError::storage)
}

fn corrupt_row(column: usize) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        column,
        rusqlite::types::Type::Text,
        "a user space row breaks its contract".into(),
    )
}

fn id_at(row: &Row<'_>, column: usize) -> rusqlite::Result<Id> {
    let text: String = row.get(column)?;
    Id::parse(&text).map_err(|_| corrupt_row(column))
}

fn small(row: &Row<'_>, column: usize) -> rusqlite::Result<Option<u32>> {
    let value: Option<i64> = row.get(column)?;
    value
        .map(|v| u32::try_from(v).map_err(|_| corrupt_row(column)))
        .transpose()
}

fn record_row(row: &Row<'_>) -> rusqlite::Result<AttachmentRecord> {
    let owner = id_at(row, 1)?;
    let sha: String = row.get(2)?;
    let status: String = row.get(8)?;
    let bytes: i64 = row.get(4)?;
    Ok(AttachmentRecord {
        id: id_at(row, 0)?,
        target_kind: AttachmentTargetKind::UserPrivate,
        target_id: Some(owner.clone()),
        content_sha256: ContentHash::parse(&sha).ok_or_else(|| corrupt_row(2))?,
        mime_type: row.get(3)?,
        byte_count: u64::try_from(bytes).map_err(|_| corrupt_row(4))?,
        filename: row.get(5)?,
        title: row.get(6)?,
        description: row.get(7)?,
        artifact_status: ArtifactStatus::parse(&status).ok_or_else(|| corrupt_row(8))?,
        width: small(row, 9)?,
        height: small(row, 10)?,
        page_count: small(row, 11)?,
        uploaded_by_employee_id: owner,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

fn by_operation(
    conn: &Connection,
    org_id: &str,
    owner: &str,
    operation_id: &str,
) -> Result<Option<PrivateOperation>, OfficeError> {
    // The fingerprint is the column after the record's.
    let sql = format!(
        "SELECT {COLUMNS}, operation_fingerprint FROM user_space_files \
         WHERE organization_id = ?1 AND owner_employee_id = ?2 AND operation_id = ?3"
    );
    conn.query_row(&sql, params![org_id, owner, operation_id], |row| {
        Ok(PrivateOperation {
            record: record_row(row)?,
            fingerprint: row.get(14)?,
        })
    })
    .optional()
    .map_err(OfficeError::storage)
}

fn by_id(
    conn: &Connection,
    org_id: &str,
    id: &str,
) -> Result<Option<AttachmentRecord>, OfficeError> {
    let sql =
        format!("SELECT {COLUMNS} FROM user_space_files WHERE organization_id = ?1 AND id = ?2");
    conn.query_row(&sql, params![org_id, id], record_row)
        .optional()
        .map_err(OfficeError::storage)
}

impl UserSpaceStore for SqliteUserSpaceStore {
    fn available(&self) -> bool {
        true
    }

    fn insert(
        &self,
        org_id: &str,
        operation_id: &str,
        fingerprint: &str,
        record: &AttachmentRecord,
    ) -> Result<Result<AttachmentRecord, ApiError>, OfficeError> {
        let Some(owner) = record.target_id.as_ref() else {
            return Err(OfficeError::Invalid("a private file names its owner"));
        };
        let conn = self.lock()?;
        if let Some(earlier) = by_operation(&conn, org_id, owner.as_str(), operation_id)? {
            return Ok(earlier.replay(fingerprint));
        }
        let inserted = conn.execute(
            "INSERT INTO user_space_files (organization_id, id, owner_employee_id, operation_id, \
             operation_fingerprint, content_sha256, mime_type, byte_count, filename, title, \
             description, artifact_status, width, height, page_count, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![
                org_id,
                record.id.as_str(),
                owner.as_str(),
                operation_id,
                fingerprint,
                record.content_sha256.as_str(),
                record.mime_type,
                i64::try_from(record.byte_count).map_err(|_| OfficeError::Invalid("byte count"))?,
                record.filename,
                record.title,
                record.description,
                record.artifact_status.as_str(),
                record.width,
                record.height,
                record.page_count,
                record.created_at,
                record.updated_at,
            ],
        );
        match inserted {
            Ok(_) => {}
            Err(rusqlite::Error::SqliteFailure(error, _))
                if error.code == ErrorCode::ConstraintViolation =>
            {
                return Err(OfficeError::Invalid("a private file breaks its contract"));
            }
            Err(error) => return Err(OfficeError::storage(error)),
        }
        drop(conn);
        self.committed()?;
        Ok(Ok(record.clone()))
    }

    fn get(&self, org_id: &str, id: &str) -> Result<Option<AttachmentRecord>, OfficeError> {
        by_id(&*self.lock()?, org_id, id)
    }

    fn by_operation(
        &self,
        org_id: &str,
        owner: &str,
        operation_id: &str,
    ) -> Result<Option<PrivateOperation>, OfficeError> {
        by_operation(&*self.lock()?, org_id, owner, operation_id)
    }

    fn list(
        &self,
        org_id: &str,
        owner: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<AttachmentRecord>, OfficeError> {
        let conn = self.lock()?;
        let sql = format!(
            "SELECT {COLUMNS} FROM user_space_files \
             WHERE organization_id = ?1 AND owner_employee_id = ?2 AND id > ?3 \
             ORDER BY id LIMIT ?4"
        );
        let mut statement = conn.prepare(&sql).map_err(OfficeError::storage)?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = statement
            .query_map(
                params![org_id, owner, after.unwrap_or(""), limit],
                record_row,
            )
            .map_err(OfficeError::storage)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(OfficeError::storage)
    }

    fn update_meta(
        &self,
        org_id: &str,
        owner: &str,
        id: &str,
        patch: &MetaPatch,
    ) -> Result<Option<AttachmentRecord>, OfficeError> {
        let mut conn = self.lock()?;
        // The patch lands on the row as this transaction reads it, never on an earlier copy.
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(OfficeError::storage)?;
        let Some(mut record) = by_id(&tx, org_id, id)?
            .filter(|r| r.target_id.as_ref().is_some_and(|o| o.as_str() == owner))
        else {
            return Ok(None);
        };
        patch.apply(&mut record);
        let changed = tx
            .execute(
                "UPDATE user_space_files SET title = ?4, description = ?5, artifact_status = ?6, \
                 updated_at = ?7 WHERE organization_id = ?1 AND id = ?2 AND owner_employee_id = ?3",
                params![
                    org_id,
                    id,
                    owner,
                    record.title,
                    record.description,
                    record.artifact_status.as_str(),
                    record.updated_at,
                ],
            )
            .map_err(OfficeError::storage)?;
        if changed != 1 {
            return Err(OfficeError::Invalid(
                "a private file changed under its update",
            ));
        }
        tx.commit().map_err(OfficeError::storage)?;
        drop(conn);
        self.committed()?;
        Ok(Some(record))
    }
}
