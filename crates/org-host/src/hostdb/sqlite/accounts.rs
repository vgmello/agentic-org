//! Private account snapshots; checked identity inputs and caller-owned authorization.
use super::{Error, HostDb};
use crate::accounts::identity::{AccountId, LoginKey};
use crate::hostdb::accounts::{Account, Credentials, Link};
use crate::hostdb::journal::Verifier;
use rusqlite::{Connection, OptionalExtension};

impl HostDb {
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
    Ok(Some(account))
}

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
