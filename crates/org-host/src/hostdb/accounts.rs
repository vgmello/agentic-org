//! Private account snapshots; checked identity inputs and caller-owned authorization.
use super::{journal::Verifier, Error, HostDb};
use crate::accounts::identity::{AccountId, LoginKey};
use rusqlite::{Connection, OptionalExtension};

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Link {
    org_id: String,
    member_id: String,
}

/// Stored authority, including revoked accounts; office IDs come from the catalog later.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Account {
    account_id: AccountId,
    login: String,
    admin: bool,
    revoked_at_ms: Option<i64>,
    password_changed_at_ms: i64,
    links: Vec<Link>,
}

/// An owned PHC for hashing outside the database lock; deliberately has no Debug.
pub(crate) struct Credentials {
    account: Account,
    verifier: Verifier,
}

impl Link {
    pub(crate) fn org_id(&self) -> &str {
        &self.org_id
    }
    pub(crate) fn member_id(&self) -> &str {
        &self.member_id
    }
}
impl Account {
    pub(crate) fn account_id(&self) -> &str {
        self.account_id.as_str()
    }
    pub(crate) fn login(&self) -> &str {
        &self.login
    }
    pub(crate) fn admin(&self) -> bool {
        self.admin
    }
    pub(crate) fn revoked_at_ms(&self) -> Option<i64> {
        self.revoked_at_ms
    }
    pub(crate) fn password_changed_at_ms(&self) -> i64 {
        self.password_changed_at_ms
    }
    pub(crate) fn links(&self) -> &[Link] {
        &self.links
    }
}
impl Credentials {
    pub(crate) fn account(&self) -> &Account {
        &self.account
    }
    /// Borrow the checked verifier; password hashing belongs outside the registry lock.
    pub(crate) fn verifier(&self) -> &Verifier {
        &self.verifier
    }
}

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
