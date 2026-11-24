//! Private account snapshots; checked identity inputs and caller-owned authorization.
use super::{Error, HostDb};
use crate::accounts::identity::{AccountId, LoginKey};
use crate::hostdb::accounts::{Account, Credentials, Link};
use crate::hostdb::journal::Verifier;
use rusqlite::{Connection, OptionalExtension};

/// Creates the additive `account_preferences` table (`FC-008`, `OPEN-1`, `TASK-53`).
pub(super) fn create_preferences_schema(tx: &rusqlite::Transaction<'_>) -> Result<(), Error> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS account_preferences (\
         account_id TEXT PRIMARY KEY REFERENCES accounts(account_id),\
         time_zone TEXT,\
         last_org_id TEXT\
         ) STRICT, WITHOUT ROWID;",
    )?;
    Ok(())
}

pub(super) fn ensure_preferences_schema(db: &mut Connection) -> Result<(), Error> {
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    create_preferences_schema(&tx)?;
    tx.commit()?;
    Ok(())
}

impl HostDb {
    /// Updates an account's persisted IANA time zone (`FC-008`) and/or last authorized
    /// organization (`OPEN-1`). Returns `(time_zone, last_org_id)` after the update.
    pub(crate) fn set_account_preferences(
        &self,
        account_id: &AccountId,
        time_zone: Option<Option<&str>>,
        last_org_id: Option<Option<&str>>,
    ) -> Result<(Option<String>, Option<String>), Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = db.transaction()?;
        let current = read(&tx, account_id)?.ok_or(Error::Unsafe)?;
        let next_tz = match time_zone {
            Some(v) => v.map(str::to_owned),
            None => current.time_zone,
        };
        let next_org = match last_org_id {
            Some(v) => v.map(str::to_owned),
            None => current.last_org_id,
        };
        tx.execute(
            "INSERT INTO account_preferences(account_id, time_zone, last_org_id) \
             VALUES (?1, ?2, ?3) \
             ON CONFLICT(account_id) DO UPDATE SET \
             time_zone = excluded.time_zone, last_org_id = excluded.last_org_id",
            (account_id.as_str(), next_tz.as_deref(), next_org.as_deref()),
        )?;
        tx.commit()?;
        Ok((next_tz, next_org))
    }

    /// The key was normalized and bounded by its owner. Unknown keys return None; errors propagate.
    /// Revoked credentials are retained for the verifier's uniform password-check path.
    pub(crate) fn account_credentials(
        &self,
        login_key: &LoginKey,
    ) -> Result<Option<Credentials>, Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = db.transaction()?;
        let row: Option<(String, String)> = tx
            .query_row(
                "SELECT account_id,password_phc FROM accounts WHERE login_key=?1",
                [login_key.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((account_id, phc)) = row else {
            return Ok(None);
        };
        if phc == super::local_auth::LOCAL_AUTHLESS_VERIFIER {
            return Ok(None);
        }
        let verifier = Verifier::new(phc)?;
        let account_id = AccountId::parse(&account_id).map_err(|_| Error::Unsafe)?;
        let account = read(&tx, &account_id)?.ok_or(Error::Unsafe)?;
        tx.commit()?;
        Ok(Some(Credentials { account, verifier }))
    }

    /// Re-read current authority and links in one snapshot; unknown IDs return None.
    /// This does not authorize a session. Storage errors propagate without SQL payloads.
    pub(crate) fn account_authority(
        &self,
        account_id: &AccountId,
    ) -> Result<Option<Account>, Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = db.transaction()?;
        let account = read(&tx, account_id)?;
        tx.commit()?;
        Ok(account)
    }
}

fn read(db: &Connection, account_id: &AccountId) -> Result<Option<Account>, Error> {
    let row = db
        .query_row(
            "SELECT account_id,login,admin,revoked_at_ms,password_changed_at_ms FROM accounts WHERE account_id=?1",
            [account_id.as_str()],
            |r| {
                Ok(Account {
                    account_id: account_id.clone(),
                    login: r.get(1)?,
                    admin: r.get(2)?,
                    revoked_at_ms: r.get(3)?,
                    password_changed_at_ms: r.get(4)?,
                    links: vec![],
                    time_zone: None,
                    last_org_id: None,
                })
            },
        )
        .optional()?;
    let Some(mut account) = row else {
        return Ok(None);
    };
    #[cfg(test)]
    tests::after_row();
    account.links = db
        .prepare("SELECT org_id,member_id FROM account_links WHERE account_id=?1 ORDER BY org_id")?
        .query_map([account_id.as_str()], |r| {
            Ok(Link {
                org_id: r.get(0)?,
                member_id: r.get(1)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    if let Some((tz, last_org)) = db
        .query_row(
            "SELECT time_zone, last_org_id FROM account_preferences WHERE account_id=?1",
            [account_id.as_str()],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                ))
            },
        )
        .optional()?
    {
        account.time_zone = tz;
        // Only expose last_org_id if the account is still linked to that organization (OPEN-1).
        account.last_org_id = last_org.filter(|org| account.links.iter().any(|l| l.org_id == *org));
    }
    Ok(Some(account))
}

pub(crate) mod multi_person;
mod store;
#[cfg(test)]
mod tests;

#[cfg(test)]
impl HostDb {
    pub(crate) fn change_authority_fixture(
        &self,
        id: &AccountId,
        revoke: bool,
    ) -> Result<(), Error> {
        let db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        if revoke {
            db.execute(
                "UPDATE accounts SET revoked_at_ms=1 WHERE account_id=?1",
                [id.as_str()],
            )?;
        } else {
            db.execute("UPDATE accounts SET password_changed_at_ms=password_changed_at_ms+1 WHERE account_id=?1", [id.as_str()])?;
        }
        Ok(())
    }
}
