//! Registry-only journal transactions. Callers do all non-SQLite work between lookup/write.
use super::clock::{advance_clock, now};
use super::{Error, HostDb, SqliteResultExt};
pub(crate) use org_host::ports::accounts::Verifier;
pub(super) use org_host::ports::host_store::journal::{id_time, DAY, PRUNE_BATCH};
pub(crate) use org_host::ports::host_store::journal::{
    Attempt, Effect, Lookup, PruneOutcome, Stored, Write, MAX_PRUNE_BATCHES,
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};

impl HostDb {
    pub(crate) fn lookup(&self, op: &Attempt<'_>, wall_ms: i64) -> Result<Lookup, Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = immediate(&mut db)?;
        let now = now(&tx, wall_ms)?;
        let decision = lookup(&tx, op, now);
        advance_clock(&tx, now)?;
        tx.commit().db()?;
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
                tx.commit().db()?;
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
        )
        .db()?;
        if let Some((created, verifier)) = verifier {
            tx.execute(
                "INSERT INTO op_verifiers VALUES (?1,?2,?3)",
                (op.id, created, verifier.expose()),
            )
            .db()?;
        }
        tx.commit().db()?;
        Ok(Write::Committed(stored))
    }
    /// One [`maintain`] batch in its own IMMEDIATE transaction: at most [`PRUNE_BATCH`]
    /// expired rows, each with its account-create verifier (`ON DELETE CASCADE`).
    pub(crate) fn prune(&self, wall_ms: i64) -> Result<usize, Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = immediate(&mut db)?;
        let removed = maintain(&tx, now(&tx, wall_ms)?)?;
        tx.commit().db()?;
        Ok(removed)
    }
    /// Host startup: [`HostDb::prune`] until a batch comes back short or [`MAX_PRUNE_BATCHES`]
    /// batches have run (262,144 rows per boot), so startup can never stall or infinite-loop
    /// under a huge backlog or hostile trigger. The lock is released between batches. `reserved`
    /// rows and rows `org_create_staging` still names stay for their reconciliation.
    pub(crate) fn prune_expired(&self, wall_ms: i64) -> Result<PruneOutcome, Error> {
        self.prune_expired_capped(wall_ms, MAX_PRUNE_BATCHES)
    }

    pub(crate) fn prune_expired_capped(
        &self,
        wall_ms: i64,
        max_batches: usize,
    ) -> Result<PruneOutcome, Error> {
        let mut removed: u64 = 0;
        for _ in 0..max_batches {
            let batch = self.prune(wall_ms)?;
            removed = removed.saturating_add(u64::try_from(batch).unwrap_or(u64::MAX));
            if batch < usize::from(PRUNE_BATCH) {
                return Ok(PruneOutcome {
                    removed,
                    more_remaining: false,
                });
            }
        }
        Ok(PruneOutcome {
            removed,
            more_remaining: true,
        })
    }
}

/// The caller's account mutation and verifier invalidation share its IMMEDIATE transaction.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "no account change invalidates verifiers yet")
)]
pub(crate) fn delete_verifiers(tx: &Transaction<'_>, created_account: &str) -> Result<(), Error> {
    tx.execute(
        "DELETE FROM op_verifiers WHERE created_account=?1",
        [created_account],
    )
    .db()?;
    Ok(())
}
pub(super) fn immediate(db: &mut Connection) -> Result<Transaction<'_>, Error> {
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .db()?;
    #[cfg(test)]
    tests::after_begin();
    Ok(tx)
}
pub(super) fn maintain(tx: &Transaction<'_>, now: i64) -> Result<usize, Error> {
    advance_clock(tx, now)?;
    tx.execute(
        "DELETE FROM op_journal WHERE operation_id IN (
         SELECT j.operation_id FROM op_journal j WHERE j.expires_at_ms<=?1
         AND j.state<>'reserved' AND NOT EXISTS
         (SELECT 1 FROM org_create_staging s WHERE s.operation_id=j.operation_id)
         ORDER BY j.expires_at_ms,j.operation_id LIMIT ?2)",
        (now, PRUNE_BATCH),
    )
    .db()
}
/// The stored row of `op`, as `(state, result)`, once it is known to be `op`'s: unexpired, of
/// the same kind, fingerprint and account, and not reserved. `None`: no row, and the
/// operation ID is fresh enough to be used (not older than 29 days, not more than five
/// minutes ahead). Shared by every journaled op's lookup.
pub(super) fn stored_row(
    db: &Connection,
    op: &Attempt<'_>,
    now: i64,
) -> Result<Option<(String, String)>, Error> {
    let row = db.query_row(
        "SELECT kind,fingerprint,account,state,result,expires_at_ms FROM op_journal WHERE operation_id=?1",
        [op.id], |r| Ok((r.get::<_,String>(0)?,r.get::<_,Vec<u8>>(1)?,r.get::<_,String>(2)?,
            r.get::<_,String>(3)?,r.get::<_,Option<String>>(4)?,r.get::<_,i64>(5)?)),
    ).optional().db()?;
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
        return Ok(Some((state, result.ok_or(Error::Unsafe)?)));
    }
    let issued = id_time(op.id)?;
    if issued < now.saturating_sub(29 * DAY) {
        return Err(Error::Expired);
    }
    if issued > now.saturating_add(300_000) {
        return Err(Error::ClockSkew);
    }
    Ok(None)
}
fn lookup(db: &Connection, op: &Attempt<'_>, now: i64) -> Result<Lookup, Error> {
    if let Some((state, result)) = stored_row(db, op, now)? {
        let kind = op.kind;
        let stored: Stored = serde_json::from_str(&result).map_err(|_| Error::Unsafe)?;
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
                .optional()
                .db()?
                .ok_or(Error::OperationReused)?;
            return Ok(Lookup::NeedsVerification {
                verifier: Verifier::new(verifier)?,
                stored,
            });
        }
        return Ok(Lookup::Replay(stored));
    }
    Ok(Lookup::Vacant)
}

#[cfg(test)]
mod tests;
