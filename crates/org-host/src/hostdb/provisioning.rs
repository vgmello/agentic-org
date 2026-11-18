//! Selected HQ reservations. No code/key bytes enter this table.
use super::{accounts::Account, Error, HostDb};
use org_proto::{selected_bootstrap::PreparedV1, OrganizationName};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
pub(super) const DDL: &str = "CREATE TABLE selected_provisioning (
 operation_id TEXT PRIMARY KEY, account TEXT NOT NULL REFERENCES accounts(account_id),
 fingerprint BLOB NOT NULL CHECK(length(fingerprint)=32), name_key TEXT NOT NULL UNIQUE,
 slug TEXT NOT NULL UNIQUE, prepared TEXT NOT NULL, owner_member TEXT NOT NULL,
 recovery_handle TEXT NOT NULL UNIQUE, key_fingerprint TEXT,
 linked INTEGER NOT NULL DEFAULT 0 CHECK(linked IN (0,1)), expires_at_ms INTEGER NOT NULL
) STRICT, WITHOUT ROWID;";
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Reservation {
    pub operation_id: String,
    pub account: String,
    pub fingerprint: [u8; 32],
    pub prepared: PreparedV1,
    pub owner_member: String,
    pub recovery_handle: String,
    pub key_fingerprint: Option<String>,
    pub linked: bool,
}
fn administrator(db: &rusqlite::Connection, account: &str) -> Result<(), Error> {
    let allowed:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM accounts WHERE account_id=?1 AND admin=1 AND revoked_at_ms IS NULL)",[account],|r|r.get(0))?;
    if !allowed {
        return Err(Error::Unsafe);
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
impl HostDb {
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
            let time = org_proto::ulid_time_ms(operation).ok_or(Error::InvalidId)? as i64;
            if now >= time + 30 * 86_400_000 {
                return Err(Error::Expired);
            }
        } else {
            let time = org_proto::ulid_time_ms(operation).ok_or(Error::InvalidId)? as i64;
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
            || !org_proto::is_valid_member_id(&frozen.owner_member)
            || frozen.recovery_handle != format!("recovery.{}", frozen.operation_id)
            || frozen.key_fingerprint.is_some()
            || frozen.linked
        {
            return Err(Error::Unsafe);
        }
        let time = org_proto::ulid_time_ms(&frozen.operation_id).ok_or(Error::InvalidId)? as i64;
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
