//! Persistent login throttling. Callers normalize/bound keys and authorize resets.
use super::clock::{advance_clock, now};
use super::{Error, HostDb};
use crate::hostdb::lockout::{Outcome, Socket, State};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};

struct Counter {
    failures: i64,
    until: i64,
    last: i64,
}
impl Counter {
    fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            failures: row.get(0)?,
            until: row.get(1)?,
            last: row.get(2)?,
        })
    }
    fn valid(&self) -> bool {
        (0..=63).contains(&self.failures) && self.last >= 0 && self.until >= self.last
    }
}

fn audit(socket: &str, key: &str) {
    let mut hash = Sha256::new();
    hash.update(socket.as_bytes());
    hash.update([0]);
    hash.update(key.as_bytes());
    crate::host_log::log(&format!(
        "invalid login lockout key {}",
        super::hex(&hash.finalize()[..8])
    ));
}
fn load(db: &Connection, socket: &str, key: &str) -> Result<Option<Counter>, Error> {
    let result = db
        .query_row(
            "SELECT failures,locked_until_ms,last_failure_ms FROM login_lockout WHERE socket=?1 AND login_key=?2",
            (socket, key), Counter::read,
        )
        .optional();
    match result {
        Ok(Some(row)) if !row.valid() => {
            audit(socket, key);
            Err(Error::Unsafe)
        }
        Ok(row) => Ok(row),
        Err(error) => {
            audit(socket, key);
            Err(error.into())
        }
    }
}

impl HostDb {
    /// Read only. Storage errors and malformed rows remain errors, never an unlocked answer.
    pub(crate) fn login_lockout(
        &self,
        socket: Socket,
        key: &str,
        wall_ms: u64,
    ) -> Result<State, Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = db.transaction()?;
        let now = now(&tx, i64::try_from(wall_ms).map_err(|_| Error::Unsafe)?)?;
        let row = load(&tx, socket.as_str(), key)?;
        let until_ms = row.map_or(now, |r| r.until);
        Ok(State {
            now_ms: now,
            locked: now < until_ms,
            until_ms,
        })
    }
    /// Counter and clock commit together. Locked attempts change neither. No password work here.
    pub(crate) fn record_login(
        &self,
        socket: Socket,
        key: &str,
        wall_ms: u64,
        outcome: Outcome,
    ) -> Result<State, Error> {
        self.record_login_capped(
            socket,
            key,
            wall_ms,
            outcome,
            crate::accounts::store::LOGIN_DELAY_CAP_MS,
        )
    }
    /// `record_login` with the delay capped at `cap_ms` (the per-address web key, Revision 3).
    pub(crate) fn record_login_capped(
        &self,
        socket: Socket,
        key: &str,
        wall_ms: u64,
        outcome: Outcome,
        cap_ms: i64,
    ) -> Result<State, Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = now(&tx, i64::try_from(wall_ms).map_err(|_| Error::Unsafe)?)?;
        let socket = socket.as_str();
        let row = load(&tx, socket, key)?;
        if let Some(until_ms) = row.as_ref().map(|r| r.until).filter(|u| now < *u) {
            if outcome != Outcome::Reset {
                return Ok(State {
                    now_ms: now,
                    locked: true,
                    until_ms,
                });
            }
        }
        let mut until = now;
        if outcome == Outcome::Failure {
            let failures = row
                .map_or(0, |r| r.failures)
                .checked_add(1)
                .ok_or(Error::Unsafe)?
                .min(63);
            let delay = crate::accounts::store::backoff_ms(failures, cap_ms);
            until = now.checked_add(delay).ok_or(Error::Unsafe)?;
            tx.execute(
                "INSERT INTO login_lockout (socket,login_key,failures,locked_until_ms,last_failure_ms)
                 VALUES (?1,?2,?3,?4,?5) ON CONFLICT(socket,login_key) DO UPDATE SET
                 failures=excluded.failures,locked_until_ms=excluded.locked_until_ms,last_failure_ms=excluded.last_failure_ms",
                (socket, key, failures, until, now),
            )?;
        } else {
            tx.execute(
                "DELETE FROM login_lockout WHERE socket=?1 AND login_key=?2",
                (socket, key),
            )?;
        }
        advance_clock(&tx, now)?;
        tx.commit()?;
        Ok(State {
            now_ms: now,
            locked: now < until,
            until_ms: until,
        })
    }
    /// A sweep retains corrupt rows for diagnosis and logs only their audit hash.
    pub(crate) fn prune_login_lockouts(&self, wall_ms: u64) -> Result<usize, Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = now(&tx, i64::try_from(wall_ms).map_err(|_| Error::Unsafe)?)?;
        let mut expired = Vec::new();
        {
            let mut stmt = tx.prepare("SELECT failures,locked_until_ms,last_failure_ms,socket,login_key FROM login_lockout")?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let socket: String = row.get(3)?;
                let key: String = row.get(4)?;
                let counter = match Counter::read(row) {
                    Ok(counter)
                        if counter.valid() && matches!(socket.as_str(), "control" | "local") =>
                    {
                        counter
                    }
                    _ => {
                        audit(&socket, &key);
                        continue;
                    }
                };
                let age = if counter.failures < 6 {
                    1_800_000
                } else {
                    86_400_000
                };
                if now.checked_sub(counter.last).ok_or(Error::Unsafe)? >= age {
                    expired.push((socket, key));
                }
            }
        }
        let mut removed = 0;
        for (socket, key) in expired {
            removed += tx.execute(
                "DELETE FROM login_lockout WHERE socket=?1 AND login_key=?2",
                (socket, key),
            )?;
        }
        advance_clock(&tx, now)?;
        tx.commit()?;
        Ok(removed)
    }
}
