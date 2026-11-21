//! Selected HQ reservations. No code/key bytes enter this table.
use super::{Error, HostDb};
use crate::hostdb::accounts::Account;
pub(crate) use crate::hostdb::provisioning::{Reservation, Taken};
use org_proto::{selected_bootstrap::PreparedV1, OrganizationName};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
pub(super) const DDL: &str = "CREATE TABLE selected_provisioning (
 operation_id TEXT PRIMARY KEY, account TEXT NOT NULL REFERENCES accounts(account_id),
 fingerprint BLOB NOT NULL CHECK(length(fingerprint)=32), name_key TEXT NOT NULL UNIQUE,
 slug TEXT NOT NULL UNIQUE, prepared TEXT NOT NULL, owner_member TEXT NOT NULL,
 recovery_handle TEXT NOT NULL UNIQUE, key_fingerprint TEXT,
 linked INTEGER NOT NULL DEFAULT 0 CHECK(linked IN (0,1)), expires_at_ms INTEGER NOT NULL
) STRICT, WITHOUT ROWID;";
fn administrator(db: &rusqlite::Connection, account: &str) -> Result<(), Error> {
    let allowed:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM accounts WHERE account_id=?1 AND admin=1 AND revoked_at_ms IS NULL)",[account],|r|r.get(0))?;
    if !allowed {
        return Err(Error::NotAdministrator);
    }
    Ok(())
}
pub(super) fn read(
    db: &rusqlite::Connection,
    operation: &str,
) -> Result<Option<Reservation>, Error> {
    let row=db.query_row("SELECT account,fingerprint,prepared,owner_member,recovery_handle,key_fingerprint,linked FROM selected_provisioning WHERE operation_id=?1",[operation],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Vec<u8>>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,Option<String>>(5)?,r.get::<_,bool>(6)?))).optional()?;
    row.map(|(account, fp, prepared, member, handle, key, linked)| {
        let prepared: PreparedV1 = serde_json::from_str(&prepared).map_err(|_| Error::Unsafe)?;
        if !prepared.valid() {
            return Err(Error::Unsafe);
        }
        Ok(Reservation {
            operation_id: operation.into(),
            account,
            fingerprint: fp.try_into().map_err(|_| Error::Unsafe)?,
            prepared,
            owner_member: member,
            recovery_handle: handle,
            key_fingerprint: key,
            linked,
        })
    })
    .transpose()
}
/// Names compare by their case-folded key; a slug is already lowercase.
fn taken(db: &rusqlite::Connection, name: &str, slug: &str) -> Result<Option<Taken>, Error> {
    let key = OrganizationName::parse(name).map_err(|_| Error::Unsafe)?;
    let exists = |sql: &str, value: &str| -> Result<bool, Error> {
        Ok(db.query_row(sql, [value], |r| r.get(0))?)
    };
    if exists(
        "SELECT EXISTS(SELECT 1 FROM selected_provisioning WHERE name_key=?1)",
        key.key(),
    )? {
        return Ok(Some(Taken::Name));
    }
    if exists(
        "SELECT EXISTS(SELECT 1 FROM selected_provisioning WHERE slug=?1)",
        slug,
    )? {
        return Ok(Some(Taken::Slug));
    }
    Ok(None)
}
fn select(db: &rusqlite::Connection, filter: &str, value: &str) -> Result<Vec<Reservation>, Error> {
    let mut statement = db.prepare(&format!(
        "SELECT operation_id FROM selected_provisioning WHERE {filter} ORDER BY operation_id"
    ))?;
    let ids = statement
        .query_map([value], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    ids.iter()
        .map(|id| read(db, id)?.ok_or(Error::Unsafe))
        .collect()
}
impl HostDb {
    /// Whether another create already holds the plan's name or slug. Read-only: a refusal
    /// from this check has stored nothing anywhere.
    pub(crate) fn provisioning_taken(
        &self,
        name: &str,
        slug: &str,
    ) -> Result<Option<Taken>, Error> {
        let db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        taken(&db, name, slug)
    }
    /// Whether a reservation exists for the operation, whoever made it.
    pub(crate) fn provisioning_known(&self, operation: &str) -> Result<bool, Error> {
        let db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        Ok(db.query_row(
            "SELECT EXISTS(SELECT 1 FROM selected_provisioning WHERE operation_id=?1)",
            [operation],
            |r| r.get(0),
        )?)
    }
    /// The linked create that made the organization; `None` when no reservation did.
    pub(crate) fn provisioning_linked_to(&self, org: &str) -> Result<Option<Reservation>, Error> {
        let db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let operation: Option<String> = db
            .query_row(
                "SELECT operation_id FROM selected_provisioning WHERE linked=1 AND json_extract(prepared,'$.organization_id')=?1 ORDER BY operation_id LIMIT 1",
                [org],
                |r| r.get(0),
            )
            .optional()?;
        match operation {
            Some(operation) => read(&db, &operation),
            None => Ok(None),
        }
    }
    /// Every create that reserved its identity and has not been linked: what a starting host
    /// continues without waiting for a client.
    pub(crate) fn provisioning_reserved(&self) -> Result<Vec<Reservation>, Error> {
        let db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        select(&db, "linked=0 AND ?1=?1", "")
    }
    /// The account's creates, linked or not.
    pub(crate) fn provisioning_of(&self, account: &str) -> Result<Vec<Reservation>, Error> {
        let db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        select(&db, "account=?1", account)
    }
    /// One of the account's own creates. Another account's operation reads as absent.
    pub(crate) fn provisioning_lookup(
        &self,
        account: &str,
        operation: &str,
    ) -> Result<Option<Reservation>, Error> {
        let db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        Ok(read(&db, operation)?.filter(|r| r.account == account))
    }
    pub(crate) fn provisioning_offices(&self) -> Result<std::collections::BTreeSet<String>, Error> {
        let db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let mut statement =
            db.prepare("SELECT prepared FROM selected_provisioning ORDER BY operation_id")?;
        let mut rows = statement.query([])?;
        let mut offices = std::collections::BTreeSet::new();
        while let Some(row) = rows.next()? {
            let p: PreparedV1 =
                serde_json::from_str(&row.get::<_, String>(0)?).map_err(|_| Error::Unsafe)?;
            if !p.valid() || !offices.insert(p.office_id) {
                return Err(Error::Unsafe);
            }
        }
        Ok(offices)
    }
    pub(crate) fn provisioning(
        &self,
        account: &Account,
        operation: &str,
        fingerprint: &[u8; 32],
        wall: i64,
    ) -> Result<Option<Reservation>, Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        administrator(&tx, account.account_id())?;
        let now = super::clock::now(&tx, wall)?;
        let result = read(&tx, operation)?;
        if let Some(r) = &result {
            if r.account != account.account_id() || &r.fingerprint != fingerprint {
                return Err(Error::OperationReused);
            }
            // An operation the host knows answers its stored state whatever its age: its
            // organization exists or is still being created. Only an id it has never
            // seen can be too old.
        } else {
            let time = org_proto::uuid_v7_time_ms(operation).ok_or(Error::InvalidId)? as i64;
            if time < now.saturating_sub(29 * 86_400_000) {
                return Err(Error::Expired);
            }
            if time > now.saturating_add(300_000) {
                return Err(Error::ClockSkew);
            }
        }
        super::clock::advance_clock(&tx, now)?;
        tx.commit()?;
        Ok(result)
    }
    pub(crate) fn reserve_provisioning(
        &self,
        account: &Account,
        frozen: &Reservation,
    ) -> Result<Reservation, Error> {
        if !frozen.prepared.valid()
            || frozen.account != account.account_id()
            || frozen.prepared.creator.employee_id != frozen.owner_member
            || frozen.recovery_handle != format!("recovery.{}", frozen.operation_id)
            || frozen.key_fingerprint.is_some()
            || frozen.linked
        {
            return Err(Error::Unsafe);
        }
        let time = org_proto::uuid_v7_time_ms(&frozen.operation_id).ok_or(Error::InvalidId)? as i64;
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        administrator(&tx, account.account_id())?;
        if let Some(prior) = read(&tx, &frozen.operation_id)? {
            if prior.account != frozen.account
                || prior.fingerprint != frozen.fingerprint
                || prior.prepared != frozen.prepared
                || prior.owner_member != frozen.owner_member
            {
                return Err(Error::OperationReused);
            }
            return Ok(prior);
        }
        // The same check as admission, repeated in the inserting transaction; the UNIQUE
        // constraints remain underneath it.
        if taken(&tx, &frozen.prepared.plan.name, &frozen.prepared.plan.slug)?.is_some() {
            return Err(Error::Conflict);
        }
        let name =
            OrganizationName::parse(&frozen.prepared.plan.name).map_err(|_| Error::Unsafe)?;
        let prepared = serde_json::to_string(&frozen.prepared).map_err(|_| Error::Unsafe)?;
        tx.execute(
            "INSERT INTO selected_provisioning VALUES(?1,?2,?3,?4,?5,?6,?7,?8,NULL,0,?9)",
            params![
                frozen.operation_id,
                frozen.account,
                frozen.fingerprint,
                name.key(),
                frozen.prepared.plan.slug,
                prepared,
                frozen.owner_member,
                frozen.recovery_handle,
                time + 30 * 86_400_000
            ],
        )?;
        tx.commit()?;
        Ok(frozen.clone())
    }
    pub(crate) fn provision_key_stored(
        &self,
        r: &Reservation,
        fingerprint: &str,
    ) -> Result<(), Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        administrator(&tx, &r.account)?;
        let prior = read(&tx, &r.operation_id)?.ok_or(Error::Unsafe)?;
        if prior.account != r.account
            || prior.fingerprint != r.fingerprint
            || prior
                .key_fingerprint
                .as_ref()
                .is_some_and(|p| p != fingerprint)
        {
            return Err(Error::Conflict);
        }
        tx.execute(
            "UPDATE selected_provisioning SET key_fingerprint=?2 WHERE operation_id=?1",
            (&r.operation_id, fingerprint),
        )?;
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn provision_linked(&self, r: &Reservation) -> Result<(), Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        administrator(&tx, &r.account)?;
        let prior = read(&tx, &r.operation_id)?.ok_or(Error::Unsafe)?;
        if prior.account != r.account
            || prior.fingerprint != r.fingerprint
            || prior.key_fingerprint.is_none()
        {
            return Err(Error::Conflict);
        }
        tx.execute(
            "UPDATE selected_provisioning SET linked=1 WHERE operation_id=?1",
            [&r.operation_id],
        )?;
        tx.commit()?;
        Ok(())
    }
}
