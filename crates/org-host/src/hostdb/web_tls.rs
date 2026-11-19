//! The browser listener's TLS pair in the registry (`crate::web_tls`): one row, the chain and
//! who made the pair; the key is in the host's secrets under `tls.<pair_fp>`, never here.
//!
//! A new registry creates the table with the rest of its schema; a registry created before X7
//! gains it at open, as `browser_sessions` was added (`sessions::ensure_schema`): the table is
//! created only when it is missing, and a present one must match its DDL exactly, or the open
//! is refused with `Outdated`. `SCHEMA_VERSION` is unchanged. The `tls_pair` table of earlier
//! registries was never written and is not read.
//!
//! `tls.set` is journaled here too (X-02 §6.3): its stored results are `completed` and the one
//! stored refusal, `tls_key_mismatch` ([`TlsStored`]), which the shared journal type does not
//! carry.
use org_host_control::ops::TlsRefusal;
use org_host_control::HostOpResult;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};

use super::clock::{advance_clock, now};
use super::journal::{id_time, immediate, maintain, stored_row, Attempt, DAY};
use super::{Error, HostDb};
use crate::web_tls::Source;

pub(super) const TABLE_DDL: &str = r#"
CREATE TABLE web_tls_pair (
 id INTEGER PRIMARY KEY CHECK (id = 1),
 source TEXT NOT NULL CHECK (source IN ('made','supplied')),
 cert_pem TEXT NOT NULL CHECK (length(cert_pem) BETWEEN 1 AND 65536),
 pair_fp BLOB NOT NULL CHECK (length(pair_fp) = 32),
 set_at_ms INTEGER NOT NULL
) STRICT"#;

/// Creates the table in a new registry's own creation transaction.
pub(super) fn create_schema(tx: &Transaction<'_>) -> Result<(), Error> {
    tx.execute_batch(TABLE_DDL)?;
    Ok(())
}

/// Read-only when the table is present and matches; otherwise one IMMEDIATE transaction
/// rechecks and creates it. A mismatch refuses the open with `Outdated`.
pub(super) fn ensure_schema(db: &mut Connection) -> Result<(), Error> {
    let present = |db: &Connection| -> Result<bool, Error> {
        Ok(db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='web_tls_pair')",
            [],
            |r| r.get(0),
        )?)
    };
    let require = |db: &Connection| {
        super::require_schema(db, TABLE_DDL).map_err(|_| {
            crate::host_log::log(super::sessions::OUTDATED);
            Error::Outdated
        })
    };
    if present(db)? {
        return require(db);
    }
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if !present(&tx)? {
        tx.execute_batch(TABLE_DDL)?;
    }
    require(&tx)?;
    tx.commit()?;
    Ok(())
}

/// The stored pair, as the registry holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StoredPair {
    pub(crate) source: Source,
    /// The chain, in the normal form `web_tls::check` returns.
    pub(crate) cert_pem: String,
    /// Names the key in the host's secrets (`tls.<hex>`).
    pub(crate) pair_fp: [u8; 32],
    pub(crate) set_at_ms: i64,
}

/// `tls.set`'s journaled result: `completed` (`{}`), or `rejected` with the one refusal that is
/// stored, `tls_key_mismatch`.
pub(crate) type TlsStored = HostOpResult<serde_json::Value, TlsRefusal>;

/// A `tls.set` journal lookup.
pub(crate) enum TlsLookup {
    Vacant,
    Replay(TlsStored),
}

fn write_pair(tx: &Transaction<'_>, pair: &StoredPair) -> Result<(), Error> {
    tx.execute(
        "INSERT INTO web_tls_pair(id,source,cert_pem,pair_fp,set_at_ms) VALUES (1,?1,?2,?3,?4)
         ON CONFLICT(id) DO UPDATE SET source=excluded.source, cert_pem=excluded.cert_pem,
         pair_fp=excluded.pair_fp, set_at_ms=excluded.set_at_ms",
        (
            pair.source.as_str(),
            &pair.cert_pem,
            &pair.pair_fp[..],
            pair.set_at_ms,
        ),
    )?;
    Ok(())
}

fn tls_lookup(db: &Connection, op: &Attempt<'_>, now: i64) -> Result<TlsLookup, Error> {
    let Some((state, result)) = stored_row(db, op, now)? else {
        return Ok(TlsLookup::Vacant);
    };
    let stored: TlsStored = serde_json::from_str(&result).map_err(|_| Error::Unsafe)?;
    match (&*state, &stored) {
        ("completed", TlsStored::Completed { .. })
        | (
            "rejected",
            TlsStored::Rejected {
                refusal: TlsRefusal::TlsKeyMismatch,
            },
        ) => Ok(TlsLookup::Replay(stored)),
        _ => Err(Error::Unsafe),
    }
}

impl HostDb {
    /// The stored pair, if any.
    pub(crate) fn web_tls_pair(&self) -> Result<Option<StoredPair>, Error> {
        let db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let row = db
            .query_row(
                "SELECT source,cert_pem,pair_fp,set_at_ms FROM web_tls_pair WHERE id=1",
                [],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Vec<u8>>(2)?,
                        r.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((source, cert_pem, pair_fp, set_at_ms)) = row else {
            return Ok(None);
        };
        Ok(Some(StoredPair {
            source: Source::parse(&source).ok_or(Error::Unsafe)?,
            cert_pem,
            pair_fp: pair_fp.try_into().map_err(|_| Error::Unsafe)?,
            set_at_ms,
        }))
    }

    /// Replaces the stored pair (the host's start, or its own renewal).
    pub(crate) fn set_web_tls_pair(&self, pair: &StoredPair) -> Result<(), Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        write_pair(&tx, pair)?;
        tx.commit()?;
        Ok(())
    }

    /// `tls.set`'s journal lookup: a stored result of `op` replays.
    pub(crate) fn tls_set_lookup(
        &self,
        op: &Attempt<'_>,
        wall_ms: i64,
    ) -> Result<TlsLookup, Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = immediate(&mut db)?;
        let now = now(&tx, wall_ms)?;
        let decision = tls_lookup(&tx, op, now);
        advance_clock(&tx, now)?;
        tx.commit()?;
        decision
    }

    /// Journals `tls.set`'s `stored` result and, with a completed one, installs `pair`, in one
    /// transaction (`None`). A result another request stored meanwhile is returned instead
    /// (`Some`), and nothing is written.
    pub(crate) fn tls_set_write(
        &self,
        op: &Attempt<'_>,
        wall_ms: i64,
        stored: &TlsStored,
        pair: Option<&StoredPair>,
    ) -> Result<Option<TlsStored>, Error> {
        let valid = matches!(
            (stored, pair),
            (TlsStored::Completed { .. }, Some(_))
                | (
                    TlsStored::Rejected {
                        refusal: TlsRefusal::TlsKeyMismatch
                    },
                    None
                )
        );
        if !valid || op.kind != "tls.set" {
            return Err(Error::Unsafe);
        }
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = immediate(&mut db)?;
        let now = now(&tx, wall_ms)?;
        match tls_lookup(&tx, op, now) {
            Ok(TlsLookup::Vacant) => {}
            Ok(TlsLookup::Replay(existing)) => {
                advance_clock(&tx, now)?;
                tx.commit()?;
                return Ok(Some(existing));
            }
            Err(error) => {
                advance_clock(&tx, now)?;
                tx.commit()?;
                return Err(error);
            }
        }
        maintain(&tx, now)?;
        let state = match stored {
            TlsStored::Completed { .. } => "completed",
            _ => "rejected",
        };
        if let Some(pair) = pair {
            write_pair(&tx, pair)?;
        }
        let result = serde_json::to_string(stored).map_err(|_| Error::Unsafe)?;
        let expires = id_time(op.id)? + 30 * DAY;
        tx.execute(
            "INSERT INTO op_journal VALUES (?1,?2,?3,?4,?5,?6,?7)",
            (
                op.id,
                op.kind,
                &op.fingerprint,
                op.account,
                state,
                result,
                expires,
            ),
        )?;
        tx.commit()?;
        Ok(None)
    }
}
