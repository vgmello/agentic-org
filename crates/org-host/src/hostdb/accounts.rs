//! Private account snapshots; callers own normalization and authorization decisions.
use super::{journal::Verifier, Error, HostDb};
use rusqlite::{Connection, OptionalExtension};

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Link {
    pub(crate) org_id: String,
    pub(crate) member_id: String,
}

/// Stored authority, including revoked accounts; office IDs come from the catalog later.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Account {
    pub(crate) account_id: String,
    pub(crate) login: String,
    pub(crate) admin: bool,
    pub(crate) revoked_at_ms: Option<i64>,
    pub(crate) links: Vec<Link>,
}

/// An owned PHC for hashing outside the database lock; deliberately has no Debug.
pub(crate) struct Credentials {
    pub(crate) account: Account,
    pub(crate) verifier: Verifier,
}

impl HostDb {
    /// The key is already normalized. Unknown keys return None; storage errors propagate.
    /// Revoked credentials are retained for the verifier's uniform password-check path.
    pub(crate) fn account_credentials(
        &self,
        login_key: &str,
    ) -> Result<Option<Credentials>, Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = db.transaction()?;
        let row: Option<(String, String)> = tx
            .query_row(
                "SELECT account_id,password_phc FROM accounts WHERE login_key=?1",
                [login_key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((account_id, phc)) = row else {
            return Ok(None);
        };
        let verifier = Verifier::new(phc);
        let account = read(&tx, &account_id)?.ok_or(Error::Unsafe)?;
        tx.commit()?;
        Ok(Some(Credentials { account, verifier }))
    }

    /// Re-read current authority and links in one snapshot; unknown IDs return None.
    /// This does not authorize a session. Storage errors propagate without SQL payloads.
    pub(crate) fn account_authority(&self, account_id: &str) -> Result<Option<Account>, Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = db.transaction()?;
        let account = read(&tx, account_id)?;
        tx.commit()?;
        Ok(account)
    }
}

fn read(db: &Connection, account_id: &str) -> Result<Option<Account>, Error> {
    let row = db
        .query_row(
            "SELECT account_id,login,admin,revoked_at_ms FROM accounts WHERE account_id=?1",
            [account_id],
            |r| {
                Ok(Account {
                    account_id: r.get(0)?,
                    login: r.get(1)?,
                    admin: r.get(2)?,
                    revoked_at_ms: r.get(3)?,
                    links: vec![],
                })
            },
        )
        .optional()?;
    let Some(mut account) = row else {
        return Ok(None);
    };
    #[cfg(test)]
    super::tests::accounts::after_row();
    account.links = db
        .prepare("SELECT org_id,member_id FROM account_links WHERE account_id=?1 ORDER BY org_id")?
        .query_map([account_id], |r| {
            Ok(Link {
                org_id: r.get(0)?,
                member_id: r.get(1)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(Some(account))
}
