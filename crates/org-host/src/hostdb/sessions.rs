//! SQLite adapter for the browser-session contract (`crate::sessions`).
//!
//! The table is created at open when missing (instances created before B3 keep
//! working). Creation and the column check share one IMMEDIATE transaction: a
//! present table whose columns differ rolls everything back and refuses the
//! open, so the schema is never half-created. `SCHEMA_VERSION` is unchanged (D4).
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};

use super::{clock, Error, HostDb};
use crate::sessions::{
    is_live, Admission, BrowserSessions, Ended, NewSession, Origin, Scheme, SessionDigest,
    SessionRecord, StoreError, SESSION_LIMIT,
};

/// The `prev_*` columns hold the previous cookie/proof during the rotation
/// grace window (used from B7).
const TABLE_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS browser_sessions (
 id TEXT PRIMARY KEY CHECK (length(id) = 36),
 account_id TEXT NOT NULL REFERENCES accounts(account_id) ON DELETE CASCADE,
 created_ms INTEGER NOT NULL,
 last_activity_ms INTEGER NOT NULL CHECK (last_activity_ms >= created_ms),
 cookie_digest BLOB NOT NULL UNIQUE CHECK (length(cookie_digest) = 32),
 proof_digest BLOB NOT NULL CHECK (length(proof_digest) = 32),
 prev_cookie_digest BLOB UNIQUE CHECK (length(prev_cookie_digest) = 32),
 prev_proof_digest BLOB CHECK (length(prev_proof_digest) = 32),
 prev_valid_until_ms INTEGER,
 scheme TEXT NOT NULL CHECK (scheme IN ('http','https')),
 host TEXT NOT NULL CHECK (length(host) BETWEEN 1 AND 255),
 port INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
 instance_id TEXT NOT NULL,
 CHECK ((prev_cookie_digest IS NULL) = (prev_proof_digest IS NULL)),
 CHECK ((prev_cookie_digest IS NULL) = (prev_valid_until_ms IS NULL))
) STRICT, WITHOUT ROWID;
"#;
const INDEX_DDL: &str = r#"
CREATE INDEX IF NOT EXISTS browser_sessions_account ON browser_sessions(account_id);
CREATE INDEX IF NOT EXISTS browser_sessions_activity ON browser_sessions(last_activity_ms);
"#;
/// `(name, type, not null, primary key position)` as `PRAGMA table_info` reports them.
const COLUMNS: &[(&str, &str, bool, i64)] = &[
    ("id", "TEXT", true, 1),
    ("account_id", "TEXT", true, 0),
    ("created_ms", "INTEGER", true, 0),
    ("last_activity_ms", "INTEGER", true, 0),
    ("cookie_digest", "BLOB", true, 0),
    ("proof_digest", "BLOB", true, 0),
    ("prev_cookie_digest", "BLOB", false, 0),
    ("prev_proof_digest", "BLOB", false, 0),
    ("prev_valid_until_ms", "INTEGER", false, 0),
    ("scheme", "TEXT", true, 0),
    ("host", "TEXT", true, 0),
    ("port", "INTEGER", true, 0),
    ("instance_id", "TEXT", true, 0),
];

/// What the operator is told when `ensure_schema` refuses.
pub(crate) const OUTDATED: &str =
    "host database from an earlier development build; run `agentic-org setup` on a new instance";

/// Creates the session table and its indexes if missing, in one IMMEDIATE
/// transaction. A present table with other columns refuses with `Outdated`
/// and nothing is created.
pub(super) fn ensure_schema(db: &mut Connection) -> Result<(), Error> {
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute_batch(TABLE_DDL)?;
    let actual = tx
        .prepare("SELECT name, type, \"notnull\", pk FROM pragma_table_info('browser_sessions') ORDER BY cid")?
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, bool>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let matches = actual.len() == COLUMNS.len()
        && actual
            .iter()
            .zip(COLUMNS)
            .all(|(a, e)| a.0 == e.0 && a.1.eq_ignore_ascii_case(e.1) && a.2 == e.2 && a.3 == e.3);
    if !matches {
        // Dropping `tx` rolls back.
        crate::host_log::log(OUTDATED);
        return Err(Error::Outdated);
    }
    tx.execute_batch(INDEX_DDL)?;
    tx.commit()?;
    Ok(())
}

fn unavailable<T>(_: T) -> StoreError {
    StoreError::Unavailable
}

/// Deletes the sessions `filter` selects (bound to `params`) and returns them sorted by ID.
fn delete_where(
    tx: &Transaction<'_>,
    filter: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<Ended>, StoreError> {
    let mut gone = tx
        .prepare(&format!(
            "DELETE FROM browser_sessions WHERE {filter} RETURNING id, account_id"
        ))
        .map_err(unavailable)?
        .query_map(params, |r| {
            Ok(Ended {
                id: r.get(0)?,
                account_id: r.get(1)?,
            })
        })
        .map_err(unavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(unavailable)?;
    gone.sort();
    Ok(gone)
}

/// Expired sessions: last active at or before the bound `?1`, see [`expiry_bound`].
const EXPIRED: &str = "last_activity_ms <= ?1";
/// The latest last-activity time that is expired at `now` (`is_live` negated).
fn expiry_bound(now: i64) -> i64 {
    now.saturating_sub(crate::sessions::IDLE_MS)
}

impl HostDb {
    /// Runs `body` in one IMMEDIATE transaction at the registry's monotone time
    /// (at least `wall_ms`), committing the advanced clock with it.
    fn session_tx<T>(
        &self,
        wall_ms: i64,
        body: impl FnOnce(&Transaction<'_>, i64) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let mut db = self.connection.lock().map_err(unavailable)?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(unavailable)?;
        let now = clock::now(&tx, wall_ms).map_err(unavailable)?;
        let value = body(&tx, now)?;
        clock::advance_clock(&tx, now).map_err(unavailable)?;
        tx.commit().map_err(unavailable)?;
        Ok(value)
    }
}

impl BrowserSessions for HostDb {
    fn create(&self, new: &NewSession, now_ms: i64) -> Result<Admission, StoreError> {
        self.session_tx(now_ms, |tx, now| {
            let reaped = delete_where(tx, EXPIRED, [expiry_bound(now)])?;
            let live: i64 = tx
                .query_row("SELECT count(*) FROM browser_sessions", [], |r| r.get(0))
                .map_err(unavailable)?;
            if usize::try_from(live).map_err(unavailable)? >= SESSION_LIMIT {
                return Ok(Admission::Full { reaped });
            }
            tx.execute(
                "INSERT INTO browser_sessions (id, account_id, created_ms, last_activity_ms, cookie_digest, proof_digest, scheme, host, port, instance_id) VALUES (?1,?2,?3,?3,?4,?5,?6,?7,?8,?9)",
                (
                    &new.id,
                    &new.account_id,
                    now,
                    &new.cookie.0[..],
                    &new.proof.0[..],
                    new.origin.scheme().as_str(),
                    new.origin.host(),
                    new.origin.port(),
                    &self.instance_id,
                ),
            )
            .map_err(unavailable)?;
            Ok(Admission::Created { reaped })
        })
    }
    fn find(
        &self,
        cookie: &SessionDigest,
        proof: &SessionDigest,
        origin: &Origin,
        now_ms: i64,
    ) -> Result<Option<SessionRecord>, StoreError> {
        let db = self.connection.lock().map_err(unavailable)?;
        let now = clock::now(&db, now_ms).map_err(unavailable)?;
        type Row = (String, String, i64, i64, Vec<u8>, String, String, u16);
        let row: Option<Row> = db
            .query_row(
                "SELECT id, account_id, created_ms, last_activity_ms, proof_digest, scheme, host, port FROM browser_sessions WHERE cookie_digest=?1 AND instance_id=?2",
                (&cookie.0[..], &self.instance_id),
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                    ))
                },
            )
            .optional()
            .map_err(unavailable)?;
        let Some((id, account_id, created_ms, last_activity_ms, stored_proof, scheme, host, port)) =
            row
        else {
            return Ok(None);
        };
        let stored_proof = SessionDigest(stored_proof.try_into().map_err(unavailable)?);
        let scheme = Scheme::parse(&scheme).ok_or(StoreError::Unavailable)?;
        let stored_origin = Origin::new(scheme, &host, port).map_err(unavailable)?;
        let valid =
            stored_proof.ct_eq(proof) && stored_origin == *origin && is_live(last_activity_ms, now);
        Ok(valid.then_some(SessionRecord {
            id,
            account_id,
            created_ms,
            last_activity_ms,
            origin: stored_origin,
        }))
    }
    fn touch(&self, activity: &[(&str, i64)]) -> Result<(), StoreError> {
        let mut db = self.connection.lock().map_err(unavailable)?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(unavailable)?;
        for (id, ms) in activity {
            tx.execute(
                "UPDATE browser_sessions SET last_activity_ms=max(last_activity_ms, ?2) WHERE id=?1",
                (id, ms),
            )
            .map_err(unavailable)?;
        }
        tx.commit().map_err(unavailable)
    }
    fn end(&self, id: &str) -> Result<Option<Ended>, StoreError> {
        let mut db = self.connection.lock().map_err(unavailable)?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(unavailable)?;
        let gone = delete_where(&tx, "id=?1", [id])?;
        tx.commit().map_err(unavailable)?;
        Ok(gone.into_iter().next())
    }
    fn end_account(
        &self,
        account_id: &str,
        except: Option<&str>,
    ) -> Result<Vec<Ended>, StoreError> {
        let mut db = self.connection.lock().map_err(unavailable)?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(unavailable)?;
        let gone = delete_where(&tx, "account_id=?1 AND id IS NOT ?2", (account_id, except))?;
        tx.commit().map_err(unavailable)?;
        Ok(gone)
    }
    fn reap(&self, now_ms: i64) -> Result<Vec<Ended>, StoreError> {
        self.session_tx(now_ms, |tx, now| {
            delete_where(tx, EXPIRED, [expiry_bound(now)])
        })
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
impl HostDb {
    /// Tests only: skips fsync so suites that fill `SESSION_LIMIT` run quickly.
    /// Durability is irrelevant to the contract cases that use it.
    pub(crate) fn relax_durability_for_tests(&self) {
        let db = self.connection.lock().unwrap();
        db.execute_batch("PRAGMA synchronous=OFF; PRAGMA fullfsync=OFF;")
            .unwrap();
    }
}
