//! Protected REC-DEL-1 facts and deletion intents. Never stores a code or key.
use super::{provisioning, Error, HostDb};
use crate::host_secrets::{HostSecrets, Name};
use org_office_launch::recovery::{PublicRecord, Request};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

pub(super) const DDL: &str = "CREATE TABLE selected_recovery (
 operation_id TEXT PRIMARY KEY REFERENCES selected_provisioning(operation_id),
 organization_id TEXT NOT NULL UNIQUE, record TEXT NOT NULL
) STRICT, WITHOUT ROWID;
CREATE TABLE selected_recovery_audit (
 operation_id TEXT NOT NULL REFERENCES selected_recovery(operation_id),
 ordinal INTEGER NOT NULL CHECK(ordinal>0),
 action TEXT NOT NULL CHECK(action IN ('show','ack_pending','acknowledged')),
 account TEXT NOT NULL REFERENCES accounts(account_id), at_ms INTEGER NOT NULL CHECK(at_ms>=0),
 PRIMARY KEY(operation_id,ordinal,action)
) STRICT, WITHOUT ROWID;";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum State {
    Pending,
    Delivered,
    AckPending,
    Acknowledged,
}
impl From<State> for org_host_control::recovery::DeliveryState {
    fn from(s: State) -> Self {
        match s {
            State::Pending => Self::Pending,
            State::Delivered => Self::Delivered,
            State::AckPending => Self::AckPending,
            State::Acknowledged => Self::Acknowledged,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Ledger {
    pub version: u32,
    pub binding: Request,
    pub public_key: String,
    pub package_sha256: String,
    pub state: State,
    pub ordinal: i64,
    pub first_shown_ms: Option<i64>,
    pub last_redelivered_ms: Option<i64>,
    pub redelivery_count: i64,
    pub acknowledged_ms: Option<i64>,
    pub acknowledged_by: Option<String>,
}
fn administrator(db: &Connection, account: &str) -> Result<(), Error> {
    let valid:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM accounts WHERE account_id=?1 AND admin=1 AND revoked_at_ms IS NULL)",[account],|r|r.get(0))?;
    if valid {
        Ok(())
    } else {
        Err(Error::Unsafe)
    }
}
fn source(db: &Connection, binding: &Request) -> Result<provisioning::Reservation, Error> {
    binding.check().map_err(|_| Error::Unsafe)?;
    let instance: String =
        db.query_row("SELECT value FROM meta WHERE key='instance_id'", [], |r| {
            r.get(0)
        })?;
    let r = provisioning::read(db, &binding.operation_id)?.ok_or(Error::Unsafe)?;
    if instance != binding.instance_id
        || !r.linked
        || r.key_fingerprint.is_none()
        || r.prepared.office_id != binding.office_id
        || r.prepared.organization_id != binding.organization_id
        || r.recovery_handle != format!("recovery.{}", binding.operation_id)
    {
        return Err(Error::Unsafe);
    }
    Ok(r)
}
fn validate(db: &Connection, l: &Ledger) -> Result<(), Error> {
    source(db, &l.binding)?;
    validate_public_fields(l)?;
    validate_delivery_state(l)?;
    validate_acknowledgement(l)
}

fn validate_public_fields(l: &Ledger) -> Result<(), Error> {
    let hex = |s: &str, n| {
        s.len() == n
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    if l.version != 1
        || !hex(&l.public_key, 130)
        || !l.public_key.starts_with("04")
        || !hex(&l.package_sha256, 64)
        || l.ordinal < 0
        || l.redelivery_count < 0
        || l.first_shown_ms.is_some_and(|n| n < 0)
        || l.last_redelivered_ms.is_some_and(|n| n < 0)
        || l.acknowledged_ms.is_some_and(|n| n < 0)
    {
        return Err(Error::Unsafe);
    }
    Ok(())
}

fn validate_delivery_state(l: &Ledger) -> Result<(), Error> {
    if l.state == State::Pending {
        if l.ordinal != 0
            || l.first_shown_ms.is_some()
            || l.last_redelivered_ms.is_some()
            || l.redelivery_count != 0
            || l.acknowledged_ms.is_some()
            || l.acknowledged_by.is_some()
        {
            return Err(Error::Unsafe);
        }
    } else if l.ordinal <= 0
        || l.first_shown_ms.is_none()
        || l.redelivery_count != l.ordinal - 1
        || (l.redelivery_count > 0) != l.last_redelivered_ms.is_some()
        || l.last_redelivered_ms
            .zip(l.first_shown_ms)
            .is_some_and(|(a, b)| a < b)
    {
        return Err(Error::Unsafe);
    }
    Ok(())
}

fn validate_acknowledgement(l: &Ledger) -> Result<(), Error> {
    let ack = matches!(l.state, State::AckPending | State::Acknowledged);
    if ack != (l.acknowledged_ms.is_some() && l.acknowledged_by.is_some())
        || (!ack && (l.acknowledged_ms.is_some() || l.acknowledged_by.is_some()))
        || l.acknowledged_by
            .as_ref()
            .is_some_and(|a| org_proto::ulid_time_ms(a).is_none())
        || l.acknowledged_ms
            .zip(l.last_redelivered_ms.or(l.first_shown_ms))
            .is_some_and(|(a, b)| a < b)
    {
        return Err(Error::Unsafe);
    }
    Ok(())
}
fn audit_valid(db: &Connection, l: &Ledger) -> Result<(), Error> {
    let mut shows=db.prepare("SELECT ordinal,account,at_ms FROM selected_recovery_audit WHERE operation_id=?1 AND action='show' ORDER BY ordinal")?;
    let mut rows = shows.query([&l.binding.operation_id])?;
    validate_show_audit(&mut rows, l)?;
    let mut rows=db.prepare("SELECT ordinal,action,account,at_ms FROM selected_recovery_audit WHERE operation_id=?1 AND action<>'show'")?;
    let rows = rows.query_map([&l.binding.operation_id], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
        ))
    })?;
    let mut n = 0;
    for row in rows {
        let (ord, action, account, at) = row?;
        n += 1;
        if ord != l.ordinal
            || Some(account) != l.acknowledged_by
            || Some(at) != l.acknowledged_ms
            || (action == "acknowledged" && l.state != State::Acknowledged)
        {
            return Err(Error::Unsafe);
        }
    }
    let expected = match l.state {
        State::Pending | State::Delivered => 0,
        State::AckPending => 1,
        State::Acknowledged => 2,
    };
    if n != expected {
        return Err(Error::Unsafe);
    }
    Ok(())
}

fn validate_show_audit(rows: &mut rusqlite::Rows<'_>, l: &Ledger) -> Result<(), Error> {
    let (mut count, mut first, mut last) = (0i64, None, None);
    while let Some(row) = rows.next()? {
        let ordinal: i64 = row.get(0)?;
        let account: String = row.get(1)?;
        let at: i64 = row.get(2)?;
        count = count.checked_add(1).ok_or(Error::Unsafe)?;
        if ordinal != count
            || org_proto::ulid_time_ms(&account).is_none()
            || at < 0
            || last.is_some_and(|prior| at < prior)
        {
            return Err(Error::Unsafe);
        }
        first.get_or_insert(at);
        last = Some(at);
    }
    if count != l.ordinal
        || first != l.first_shown_ms
        || (count > 1 && last != l.last_redelivered_ms)
    {
        return Err(Error::Unsafe);
    }
    Ok(())
}
fn read(db: &Connection, op: &str) -> Result<Option<Ledger>, Error> {
    // Bound each row before allocation, without imposing a total-state cap.
    let row:Option<(String,String)>=db.query_row("SELECT organization_id,CASE WHEN length(CAST(record AS BLOB))<=4096 THEN record ELSE NULL END FROM selected_recovery WHERE operation_id=?1",[op],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    row.map(|(org, raw)| {
        let l: Ledger = serde_json::from_str(&raw).map_err(|_| Error::Unsafe)?;
        if l.binding.operation_id != op || l.binding.organization_id != org {
            return Err(Error::Unsafe);
        }
        validate(db, &l)?;
        audit_valid(db, &l)?;
        Ok(l)
    })
    .transpose()
}
fn write(db: &Connection, l: &Ledger) -> Result<(), Error> {
    validate(db, l)?;
    let raw = serde_json::to_string(l).map_err(|_| Error::Unsafe)?;
    if raw.len() > 4096 {
        return Err(Error::Unsafe);
    }
    if db.execute(
        "UPDATE selected_recovery SET record=?2 WHERE operation_id=?1",
        (&l.binding.operation_id, raw),
    )? != 1
    {
        return Err(Error::Unsafe);
    }
    Ok(())
}
fn audit(db: &Connection, l: &Ledger, action: &str, account: &str, at: i64) -> Result<(), Error> {
    db.execute(
        "INSERT INTO selected_recovery_audit VALUES(?1,?2,?3,?4,?5)",
        params![l.binding.operation_id, l.ordinal, action, account, at],
    )?;
    Ok(())
}
pub(super) fn verify_all(db: &Connection) -> Result<(), Error> {
    let mut stmt =
        db.prepare("SELECT operation_id FROM selected_recovery ORDER BY operation_id")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let op: String = row.get(0)?;
        read(db, &op)?.ok_or(Error::Unsafe)?;
    }
    let orphan:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM selected_recovery_audit a LEFT JOIN selected_recovery r USING(operation_id) WHERE r.operation_id IS NULL)",[],|r|r.get(0))?;
    if orphan {
        return Err(Error::Unsafe);
    }
    Ok(())
}
impl HostDb {
    #[cfg(test)]
    pub(crate) fn recovery_db_unlocked_for_test(&self) -> bool {
        self.connection.try_lock().is_ok()
    }
    pub(crate) fn recovery_observed(&self, operation: &str) -> Result<Ledger, Error> {
        let db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        read(&db, operation)?.ok_or(Error::Unsafe)
    }
    pub(crate) fn recovery_source(&self, org: &str) -> Result<provisioning::Reservation, Error> {
        let db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let mut statement = db.prepare("SELECT operation_id FROM selected_provisioning WHERE linked=1 AND json_extract(prepared,'$.organization_id')=?1 LIMIT 2")?;
        let mut rows = statement.query([org])?;
        let op: String = rows.next()?.ok_or(Error::Unsafe)?.get(0)?;
        if rows.next()?.is_some() {
            return Err(Error::Unsafe);
        }
        let r = provisioning::read(&db, &op)?.ok_or(Error::Unsafe)?;
        if !r.linked || r.prepared.organization_id != org {
            return Err(Error::Unsafe);
        }
        Ok(r)
    }
    pub(crate) fn recovery_status(
        &self,
        expected: &Ledger,
        after: i64,
    ) -> Result<org_host_control::recovery::StatusResult, Error> {
        if after < 0 {
            return Err(Error::InvalidId);
        }
        let db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let l = read(&db, &expected.binding.operation_id)?.ok_or(Error::Unsafe)?;
        if &l != expected {
            return Err(Error::Conflict);
        }
        let end = after.saturating_add(32).min(l.ordinal);
        let mut statement = db.prepare("SELECT ordinal,action,account,at_ms FROM selected_recovery_audit WHERE operation_id=?1 AND ordinal>?2 AND ordinal<=?3 ORDER BY ordinal,CASE action WHEN 'show' THEN 0 WHEN 'ack_pending' THEN 1 ELSE 2 END")?;
        let audit = statement
            .query_map(params![l.binding.operation_id, after, end], |r| {
                Ok(org_host_control::recovery::Audit {
                    ordinal: r.get::<_, i64>(0)?.to_string(),
                    action: r.get(1)?,
                    account: r.get(2)?,
                    at_ms: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(org_host_control::recovery::StatusResult {
            version: 1,
            org_id: l.binding.organization_id.clone(),
            state: l.state.into(),
            delivery_ordinal: l.ordinal.to_string(),
            first_shown_ms: l.first_shown_ms,
            last_redelivered_ms: l.last_redelivered_ms,
            redelivery_count: l.redelivery_count.to_string(),
            acknowledged_ms: l.acknowledged_ms,
            acknowledged_by: l.acknowledged_by.clone(),
            audit,
            next_after_ordinal: (end < l.ordinal).then(|| end.to_string()),
            hq_loss_recovery_supported: false,
        })
    }
    pub(crate) fn recovery_prepare(&self, public: &PublicRecord) -> Result<Ledger, Error> {
        public.package().map_err(|_| Error::Unsafe)?;
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        source(&tx, &public.binding())?;
        if let Some(l) = read(&tx, &public.operation_id)? {
            if l.binding != public.binding()
                || l.public_key != public.public_key
                || l.package_sha256 != public.package_sha256
            {
                return Err(Error::Conflict);
            }
            return Ok(l);
        }
        let l = Ledger {
            version: 1,
            binding: public.binding(),
            public_key: public.public_key.clone(),
            package_sha256: public.package_sha256.clone(),
            state: State::Pending,
            ordinal: 0,
            first_shown_ms: None,
            last_redelivered_ms: None,
            redelivery_count: 0,
            acknowledged_ms: None,
            acknowledged_by: None,
        };
        validate(&tx, &l)?;
        tx.execute(
            "INSERT INTO selected_recovery VALUES(?1,?2,?3)",
            (
                &public.operation_id,
                &public.organization_id,
                serde_json::to_string(&l).map_err(|_| Error::Unsafe)?,
            ),
        )?;
        tx.commit()?;
        Ok(l)
    }
    pub(crate) fn recovery_delivered(
        &self,
        expected: &Ledger,
        account: &str,
        redeliver: bool,
        wall: i64,
    ) -> Result<Ledger, Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        administrator(&tx, account)?;
        let mut l = read(&tx, &expected.binding.operation_id)?.ok_or(Error::Unsafe)?;
        if &l != expected
            || !matches!(l.state, State::Pending | State::Delivered)
            || (l.state == State::Delivered && !redeliver)
        {
            return Err(Error::Conflict);
        }
        let now = super::clock::now(&tx, wall)?;
        l.ordinal = l.ordinal.checked_add(1).ok_or(Error::Unsafe)?;
        l.state = State::Delivered;
        l.first_shown_ms.get_or_insert(now);
        if l.ordinal > 1 {
            l.last_redelivered_ms = Some(now);
            l.redelivery_count = l.ordinal - 1;
        }
        write(&tx, &l)?;
        audit(&tx, &l, "show", account, now)?;
        super::clock::advance_clock(&tx, now)?;
        #[cfg(all(test, debug_assertions))]
        tests::crash::checkpoint(1);
        tx.commit()?;
        #[cfg(all(test, debug_assertions))]
        tests::crash::checkpoint(2);
        Ok(l)
    }
    pub(crate) fn recovery_ack_intent(
        &self,
        expected: &Ledger,
        account: &str,
        wall: i64,
    ) -> Result<Ledger, Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        administrator(&tx, account)?;
        let mut l = read(&tx, &expected.binding.operation_id)?.ok_or(Error::Unsafe)?;
        if &l != expected || l.state != State::Delivered {
            return Err(Error::Conflict);
        }
        let now = super::clock::now(&tx, wall)?;
        l.state = State::AckPending;
        l.acknowledged_by = Some(account.into());
        l.acknowledged_ms = Some(now);
        write(&tx, &l)?;
        audit(&tx, &l, "ack_pending", account, now)?;
        super::clock::advance_clock(&tx, now)?;
        #[cfg(all(test, debug_assertions))]
        tests::crash::checkpoint(3);
        tx.commit()?;
        #[cfg(all(test, debug_assertions))]
        tests::crash::checkpoint(4);
        Ok(l)
    }
    /// Safe at startup and for exact ACK retries. Frozen intent precedes deletion;
    /// no DB lock is held while the provider executes. Missing before intent fails.
    pub(crate) fn recovery_reconcile(&self, custody: &dyn HostSecrets) -> Result<(), Error> {
        let mut previous = String::new();
        loop {
            let op: Option<String> = {
                let db = self.connection.lock().map_err(|_| Error::Unsafe)?;
                db.query_row("SELECT operation_id FROM selected_provisioning WHERE linked=1 AND operation_id>?1 ORDER BY operation_id LIMIT 1",[&previous],|r|r.get(0)).optional()?
            };
            let Some(op) = op else {
                break;
            };
            if org_proto::ulid_time_ms(&op).is_none() {
                return Err(Error::Unsafe);
            }
            previous = op.clone();
            let (r, l) = {
                let db = self.connection.lock().map_err(|_| Error::Unsafe)?;
                (
                    provisioning::read(&db, &op)?.ok_or(Error::Unsafe)?,
                    read(&db, &op)?,
                )
            };
            let name = Name::parse(&r.recovery_handle)?;
            match l.as_ref().map(|l| l.state) {
                Some(State::AckPending) => {
                    #[cfg(all(test, debug_assertions))]
                    tests::crash::checkpoint(5);
                    custody.delete(&name)?;
                    #[cfg(all(test, debug_assertions))]
                    tests::crash::checkpoint(6);
                    if custody.load(&name)?.is_some() {
                        return Err(Error::Unsafe);
                    }
                    let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
                    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                    let mut current = read(&tx, &op)?.ok_or(Error::Unsafe)?;
                    if current.state == State::Acknowledged {
                        continue;
                    }
                    if Some(&current) != l.as_ref() {
                        return Err(Error::Conflict);
                    }
                    current.state = State::Acknowledged;
                    write(&tx, &current)?;
                    audit(
                        &tx,
                        &current,
                        "acknowledged",
                        current.acknowledged_by.as_deref().ok_or(Error::Unsafe)?,
                        current.acknowledged_ms.ok_or(Error::Unsafe)?,
                    )?;
                    #[cfg(all(test, debug_assertions))]
                    tests::crash::checkpoint(7);
                    tx.commit()?;
                    #[cfg(all(test, debug_assertions))]
                    tests::crash::checkpoint(8);
                }
                Some(State::Acknowledged) => {
                    if custody.load(&name)?.is_some() {
                        return Err(Error::Unsafe);
                    }
                }
                _ => {
                    crate::provisioning::pending_recovery(custody, &r)
                        .map_err(|_| Error::Unsafe)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
