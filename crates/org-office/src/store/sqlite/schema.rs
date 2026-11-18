//! The projection's schema: the data contract's `schema.sql`, then the adapter's own
//! tables. Its digest is stored in the projection, so a projection made from other DDL is
//! recognized and replaced (there is no migration code: a projection is rebuilt).

use org_store::{EngineError, Sha256};
use rusqlite::{Connection, ErrorCode, OptionalExtension};

use super::failed;

/// The data contract's tables, exactly as the contract writes them.
const CONTRACT: &str = include_str!("../../../../../docs/design/organization-data/schema.sql");

/// The adapter's tables: nothing here is organization data.
///
/// - `store_schema`: the digest of the DDL this projection was made from.
/// - `store_cursor`: how far the journal is applied, written by every commit.
/// - `store_sources`: the hash of every source file, by path.
/// - `store_receipts`: every receipt whole, with its original reply, so a retry is
///   answered without a file read. It refers to no other table: a receipt names its actor
///   whether or not that employee's file has been projected yet.
const ADAPTER: &str = "
CREATE TABLE store_schema (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  digest TEXT NOT NULL
) STRICT;
CREATE TABLE store_cursor (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  cursor TEXT NOT NULL
) STRICT;
CREATE TABLE store_sources (
  path TEXT PRIMARY KEY,
  sha256 BLOB NOT NULL CHECK (length(sha256) = 32)
) STRICT, WITHOUT ROWID;
CREATE TABLE store_receipts (
  operation_id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  actor_kind TEXT NOT NULL CHECK (actor_kind IN ('employee', 'system')),
  actor_id TEXT,
  fingerprint BLOB NOT NULL CHECK (length(fingerprint) = 32),
  outcome TEXT NOT NULL CHECK (outcome IN ('completed', 'rejected')),
  ack TEXT NOT NULL CHECK (ack IN ('committed', 'durable')),
  ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
  at INTEGER NOT NULL CHECK (at >= 0),
  reply BLOB NOT NULL,
  CHECK ((actor_kind = 'employee') = (actor_id IS NOT NULL))
) STRICT, WITHOUT ROWID;
";

/// How the adapter writes values into its own tables: the cursor's text, a receipt's
/// columns and its reply's bytes, a source's path. The DDL does not show any of that, so
/// this number is the third part of the digest. Add one on ANY change to what the adapter
/// encodes into a column or how (`receipts.rs`, `organization.rs`): a projection written
/// the old way is then recognized and rebuilt instead of being misread.
const ADAPTER_FORMAT: u32 = 1;

/// All the DDL a projection is made from, in the order it is loaded, and the format of
/// what the adapter writes into it.
#[derive(Clone, Debug)]
pub(super) struct Schema {
    parts: Vec<&'static str>,
    format: u32,
}

impl Schema {
    /// The schema of this build.
    pub(super) fn current() -> Self {
        Self {
            parts: vec![CONTRACT, ADAPTER],
            format: ADAPTER_FORMAT,
        }
    }

    /// This schema as a build with another adapter format would have it, for tests.
    #[cfg(test)]
    pub(super) fn encoded_as(mut self, format: u32) -> Self {
        self.format = format;
        self
    }

    /// This schema with more DDL after it: another build's schema, for tests.
    #[cfg(test)]
    pub(super) fn with(mut self, ddl: &'static str) -> Self {
        self.parts.push(ddl);
        self
    }

    /// SHA-256 of the adapter format, then every DDL part preceded by its length, in
    /// lowercase hex.
    pub(super) fn digest(&self) -> String {
        let format = self.format.to_be_bytes();
        let lengths: Vec<[u8; 8]> = self
            .parts
            .iter()
            .map(|p| (p.len() as u64).to_be_bytes())
            .collect();
        let mut framed: Vec<&[u8]> = vec![b"adapter-format", &format];
        for (part, length) in self.parts.iter().zip(&lengths) {
            framed.push(length);
            framed.push(part.as_bytes());
        }
        Sha256::of_parts(&framed).to_hex()
    }

    /// Creates every table in an empty database and records the digest, in one
    /// transaction: a database without its digest row was never finished.
    pub(super) fn create(&self, db: &Connection) -> Result<(), EngineError> {
        let create = || -> rusqlite::Result<()> {
            db.execute_batch("BEGIN IMMEDIATE")?;
            for part in &self.parts {
                db.execute_batch(part)?;
            }
            db.execute(
                "INSERT INTO store_schema (id, digest) VALUES (1, ?1)",
                [self.digest()],
            )?;
            db.execute_batch("COMMIT")
        };
        create().map_err(|error| failed(db, "creating the schema", &error))
    }

    /// Whether `db` was made from exactly this schema. A database without the digest's
    /// table or row, with another digest, or that SQLite finds damaged is "no". A database
    /// that could not be read (busy, an input/output error) is an error: it may be whole.
    pub(super) fn made(&self, db: &Connection) -> Result<bool, EngineError> {
        let read = || -> rusqlite::Result<bool> {
            let table = "SELECT count(*) FROM sqlite_schema \
                         WHERE type = 'table' AND name = 'store_schema'";
            if db.query_row(table, [], |row| row.get::<_, i64>(0))? == 0 {
                return Ok(false);
            }
            // A table of that name from other DDL may lack the column or hold another
            // type: SQLite's plain error and a value of the wrong type are both "no".
            let stored = db
                .query_row("SELECT digest FROM store_schema WHERE id = 1", [], |row| {
                    Ok(row.get::<_, String>(0).ok())
                })
                .optional();
            match stored {
                Ok(digest) => Ok(digest.flatten().is_some_and(|d| d == self.digest())),
                Err(error) if error.sqlite_error_code() == Some(ErrorCode::Unknown) => Ok(false),
                Err(error) => Err(error),
            }
        };
        match read() {
            Err(error)
                if matches!(
                    error.sqlite_error_code(),
                    Some(ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt)
                ) =>
            {
                Ok(false)
            }
            other => other.map_err(|error| failed(db, "reading the projection's schema", &error)),
        }
    }
}
