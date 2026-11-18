//! The projection's schema: the data contract's `schema.sql`, then the adapter's own
//! tables. Its digest is stored in the projection, so a projection made from other DDL is
//! recognized and replaced (there is no migration code: a projection is rebuilt).

use org_store::{EngineError, Sha256};
use rusqlite::{Connection, OptionalExtension};

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

/// All the DDL a projection is made from, in the order it is loaded.
#[derive(Clone, Debug)]
pub(super) struct Schema {
    parts: Vec<&'static str>,
}

impl Schema {
    /// The schema of this build.
    pub(super) fn current() -> Self {
        Self {
            parts: vec![CONTRACT, ADAPTER],
        }
    }

    /// This schema with more DDL after it: another build's schema, for tests.
    #[cfg(test)]
    pub(super) fn with(mut self, ddl: &'static str) -> Self {
        self.parts.push(ddl);
        self
    }

    /// SHA-256 of every part, each preceded by its length, in lowercase hex.
    pub(super) fn digest(&self) -> String {
        let lengths: Vec<[u8; 8]> = self
            .parts
            .iter()
            .map(|p| (p.len() as u64).to_be_bytes())
            .collect();
        let mut framed: Vec<&[u8]> = Vec::new();
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

    /// Whether `db` was made from exactly this schema. Anything unreadable is "no".
    pub(super) fn made(&self, db: &Connection) -> bool {
        let stored = db
            .query_row("SELECT digest FROM store_schema WHERE id = 1", [], |row| {
                row.get::<_, String>(0)
            })
            .optional();
        matches!(stored, Ok(Some(digest)) if digest == self.digest())
    }
}
