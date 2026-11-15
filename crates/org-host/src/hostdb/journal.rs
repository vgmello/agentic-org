//! Registry-only journal transactions. Callers do all non-SQLite work between lookup/write.
use super::{Error, HostDb};
use org_host_control::{journal::Journaled, HostOpResult};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};

const DAY: i64 = 86_400_000;
pub(crate) type Stored = HostOpResult<serde_json::Value>;

pub(crate) struct Attempt<'a> {
    pub(crate) id: &'a str,
    pub(crate) kind: &'static str,
    pub(crate) fingerprint: [u8; 32],
    pub(crate) account: &'a str,
}
impl<'a> Attempt<'a> {
    pub(crate) fn new<J: Journaled>(op: &'a J, account: &'a str) -> Result<Self, Error> {
        Ok(Self {
            id: op.operation_id(),
            kind: J::METHOD,
            fingerprint: org_host_control::journal::fingerprint(op).map_err(|_| Error::Unsafe)?,
            account,
        })
    }
}

/// Opaque account PHC string. Only 2b.3's bounded Argon2 path may inspect it.
pub(crate) struct Verifier(String);
impl Verifier {
    pub(crate) fn new(phc: String) -> Self {
        Self(phc)
    }
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}
impl Drop for Verifier {
    fn drop(&mut self) {
        crate::secrets::zero(&mut std::mem::take(&mut self.0).into_bytes());
    }
}
pub(crate) enum Lookup {
    Vacant,
    Replay(Stored),
    NeedsVerification { verifier: Verifier, stored: Stored },
}
pub(crate) struct Effect {
    pub(crate) stored: Stored,
    /// Created account, not the acting account. Required only for a successful account.create.
    pub(crate) verifier: Option<(String, Verifier)>,
}
pub(crate) enum Write {
    Committed(Stored),
    Existing(Lookup),
}

impl HostDb {
    pub(crate) fn lookup(&self, op: &Attempt<'_>, wall_ms: i64) -> Result<Lookup, Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = immediate(&mut db)?;
        let now = now(&tx, wall_ms)?;
        let decision = lookup(&tx, op, now);
        advance_clock(&tx, now)?;
        tx.commit()?;
        decision
    }
    /// The callback may perform SQLite work only. Error rolls back effect, clock and pruning.
    pub(crate) fn write(
        &self,
        op: &Attempt<'_>,
        wall_ms: i64,
        effect: impl FnOnce(&Transaction<'_>) -> Result<Effect, Error>,
    ) -> Result<Write, Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = immediate(&mut db)?;
        let now = now(&tx, wall_ms)?;
        match lookup(&tx, op, now) {
            Ok(Lookup::Vacant) => {}
            decision => {
                advance_clock(&tx, now)?;
                tx.commit()?;
                return decision.map(Write::Existing);
            }
        }
        if op.kind == "bootstrap" {
            return Err(Error::Unsafe);
        }
        maintain(&tx, now)?;
        let Effect { stored, verifier } = effect(&tx)?;
        let state = match &stored {
            Stored::Completed { .. } => "completed",
            Stored::Rejected { refusal }
                if matches!(
                    refusal.code,
                    org_proto::ErrorCode::AlreadyExists | org_proto::ErrorCode::InvalidName
                ) =>
            {
                "rejected"
            }
            _ => return Err(Error::Unsafe),
        };
        let needs_verifier = op.kind == "account.create" && state == "completed";
        if needs_verifier != verifier.is_some() {
            return Err(Error::Unsafe);
        }
        if let Some((created, _)) = &verifier {
            if !matches!(&stored, Stored::Completed { result } if result.get("account").and_then(|a| a.as_str()) == Some(created))
            {
                return Err(Error::Unsafe);
            }
        }
        let result = serde_json::to_string(&stored).map_err(|_| Error::Unsafe)?;
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
        if let Some((created, verifier)) = verifier {
            tx.execute(
                "INSERT INTO op_verifiers VALUES (?1,?2,?3)",
                (op.id, created, verifier.expose()),
            )?;
        }
        tx.commit()?;
        Ok(Write::Committed(stored))
    }
    /// Startup calls this only after staged-create reconciliation/abandon has finished.
    pub(crate) fn prune(&self, wall_ms: i64) -> Result<usize, Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = immediate(&mut db)?;
        let removed = maintain(&tx, now(&tx, wall_ms)?)?;
        tx.commit()?;
        Ok(removed)
    }
}

/// The caller's account mutation and verifier invalidation share its IMMEDIATE transaction.
pub(crate) fn delete_verifiers(tx: &Transaction<'_>, created_account: &str) -> Result<(), Error> {
    tx.execute(
        "DELETE FROM op_verifiers WHERE created_account=?1",
        [created_account],
    )?;
    Ok(())
}
fn immediate(db: &mut Connection) -> Result<Transaction<'_>, Error> {
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    #[cfg(test)]
    tests::after_begin();
    Ok(tx)
}
fn now(db: &Connection, wall_ms: i64) -> Result<i64, Error> {
    let high: String = db.query_row(
        "SELECT value FROM meta WHERE key='clock_high_ms'",
        [],
        |r| r.get(0),
    )?;
    let high: i64 = high.parse().map_err(|_| Error::Unsafe)?;
    if wall_ms < 0 || high < 0 {
        return Err(Error::Unsafe);
    }
    Ok(wall_ms.max(high))
}
/// Commit the served maximum with the decision; unchanged clocks produce no UPDATE.
fn advance_clock(tx: &Transaction<'_>, now: i64) -> Result<(), Error> {
    tx.execute(
        "UPDATE meta SET value=?1 WHERE key='clock_high_ms' AND CAST(value AS INTEGER)<?1",
        [now],
    )?;
    Ok(())
}
fn maintain(tx: &Transaction<'_>, now: i64) -> Result<usize, Error> {
    advance_clock(tx, now)?;
    Ok(tx.execute(
        "DELETE FROM op_journal WHERE operation_id IN (
         SELECT j.operation_id FROM op_journal j WHERE j.expires_at_ms<=?1
         AND j.state<>'reserved' AND NOT EXISTS
         (SELECT 1 FROM org_create_staging s WHERE s.operation_id=j.operation_id)
         ORDER BY j.expires_at_ms,j.operation_id LIMIT 256)",
        [now],
    )?)
}
fn id_time(id: &str) -> Result<i64, Error> {
    org_proto::ulid_time_ms(id)
        .and_then(|n| i64::try_from(n).ok())
        .ok_or(Error::InvalidId)
}
fn lookup(db: &Connection, op: &Attempt<'_>, now: i64) -> Result<Lookup, Error> {
    let row = db.query_row(
        "SELECT kind,fingerprint,account,state,result,expires_at_ms FROM op_journal WHERE operation_id=?1",
        [op.id], |r| Ok((r.get::<_,String>(0)?,r.get::<_,Vec<u8>>(1)?,r.get::<_,String>(2)?,
            r.get::<_,String>(3)?,r.get::<_,Option<String>>(4)?,r.get::<_,i64>(5)?)),
    ).optional()?;
    if let Some((kind, fingerprint, account, state, result, expires)) = row {
        if expires <= now {
            return Err(Error::Expired);
        }
        if kind != op.kind || fingerprint != op.fingerprint || account != op.account {
            return Err(Error::OperationReused);
        }
        if state == "reserved" {
            return Err(Error::Conflict);
        }
        let stored: Stored =
            serde_json::from_str(&result.ok_or(Error::Unsafe)?).map_err(|_| Error::Unsafe)?;
        if !matches!(
            (&*state, &stored),
            ("completed" | "linked", Stored::Completed { .. })
                | ("rejected" | "abandoned", Stored::Rejected { .. })
        ) {
            return Err(Error::Unsafe);
        }
        if kind == "account.create" && state == "completed" {
            let verifier = db
                .query_row(
                    "SELECT verifier FROM op_verifiers WHERE operation_id=?1",
                    [op.id],
                    |r| r.get::<_, String>(0),
                )
                .optional()?
                .ok_or(Error::OperationReused)?;
            return Ok(Lookup::NeedsVerification {
                verifier: Verifier::new(verifier),
                stored,
            });
        }
        return Ok(Lookup::Replay(stored));
    }
    let issued = id_time(op.id)?;
    if issued < now.saturating_sub(29 * DAY) {
        return Err(Error::Expired);
    }
    if issued > now.saturating_add(300_000) {
        return Err(Error::ClockSkew);
    }
    Ok(Lookup::Vacant)
}

#[cfg(test)]
mod tests;
