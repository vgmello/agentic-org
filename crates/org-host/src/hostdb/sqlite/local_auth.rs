//! Persistent local-HQ authless mode (ADR-0034 / TASK-76).
//!
//! Fresh local HTTP instances start without an authentication wall so the browser opens
//! directly into the organization/onboarding wizard. While auth is disabled, `meta` records
//! `'local_authless_account' -> <account_id>` pointing to an unrevoked local owner row in
//! `accounts` whose `password_phc` is [`LOCAL_AUTHLESS_VERIFIER`] (unusable for password
//! verification). Enabling authentication (`enable_local_auth`) atomically upgrades that
//! exact account with a real login and Argon2id verifier and deletes the meta key, preserving
//! every organization link and ownership record seamlessly.

use rusqlite::{OptionalExtension, Transaction, TransactionBehavior};

use super::clock::{advance_clock, now};
use super::{Error, HostDb};
use crate::accounts::identity::{AccountId, LoginKey};
use crate::accounts::rules::Username;
use crate::hostdb::journal::Verifier;

pub(super) const LOCAL_AUTHLESS_META: &str = "local_authless_account";
pub(crate) use crate::hostdb::LOCAL_AUTHLESS_VERIFIER;
const LOCAL_OWNER_LOGIN: &str = "local-owner";

pub(super) fn local_authless_id(tx: &Transaction<'_>) -> Result<Option<String>, Error> {
    Ok(tx
        .query_row(
            "SELECT value FROM meta WHERE key=?1",
            [LOCAL_AUTHLESS_META],
            |r| r.get::<_, String>(0),
        )
        .optional()?)
}

impl HostDb {
    /// Activates local authless mode on a brand-new local HTTP registry that has zero accounts
    /// and zero issued registration links. Existing registries, seeded administrators, and
    /// link-initialized registries stay untouched (`Ok(None)` -> `auth_enabled() == true`).
    pub(crate) fn init_local_authless_if_empty(
        &self,
        now_ms: i64,
    ) -> Result<Option<AccountId>, Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = local_authless_id(&tx)? {
            let id = AccountId::parse(&existing).map_err(|_| Error::Unsafe)?;
            return Ok(Some(id));
        }
        let has_accounts: bool =
            tx.query_row("SELECT EXISTS(SELECT 1 FROM accounts)", [], |r| r.get(0))?;
        let has_links: bool =
            tx.query_row("SELECT EXISTS(SELECT 1 FROM one_time_links)", [], |r| {
                r.get(0)
            })?;
        if has_accounts || has_links {
            return Ok(None);
        }
        let ts = now(&tx, now_ms)?;
        let raw_id = crate::ids::uuid_v7().map_err(|_| Error::Unsafe)?;
        let id = AccountId::parse(&raw_id).map_err(|_| Error::Unsafe)?;
        let key = LoginKey::new(LOCAL_OWNER_LOGIN).map_err(|_| Error::Unsafe)?;
        tx.execute(
            "INSERT INTO accounts VALUES (?1,?2,?3,?4,1,?5,?5,NULL)",
            (
                id.as_str(),
                key.as_str(),
                LOCAL_OWNER_LOGIN,
                LOCAL_AUTHLESS_VERIFIER,
                ts,
            ),
        )?;
        tx.execute(
            "INSERT INTO meta(key,value) VALUES (?1,?2)",
            (LOCAL_AUTHLESS_META, id.as_str()),
        )?;
        advance_clock(&tx, ts)?;
        tx.commit()?;
        Ok(Some(id))
    }

    /// Returns the implicit local owner account ID while authentication is disabled, or `None`
    /// once authentication is enabled.
    pub(crate) fn local_authless_account(&self) -> Result<Option<AccountId>, Error> {
        let db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let Some(raw) = db
            .query_row(
                "SELECT value FROM meta WHERE key=?1",
                [LOCAL_AUTHLESS_META],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        else {
            return Ok(None);
        };
        let active: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE account_id=?1 AND admin=1 AND revoked_at_ms IS NULL)",
            [&raw],
            |r| r.get(0),
        )?;
        if !active {
            return Err(Error::Unsafe);
        }
        AccountId::parse(&raw).map(Some).map_err(|_| Error::Unsafe)
    }

    /// Whether password/session authentication is currently enforced (`true` by default on all
    /// seeded, link-registered, or HTTPS instances; `false` on fresh local HTTP HQ until enabled).
    pub(crate) fn auth_enabled(&self) -> Result<bool, Error> {
        Ok(self.local_authless_account()?.is_none())
    }

    /// Upgrades the local authless owner account to a real password-authenticated administrator
    /// and enables authentication permanently, preserving all existing organization links/owners.
    pub(crate) fn enable_local_auth(
        &self,
        username: &Username,
        verifier: &Verifier,
        now_ms: i64,
    ) -> Result<AccountId, Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(raw_id) = local_authless_id(&tx)? else {
            return Err(Error::Conflict);
        };
        let account_id = AccountId::parse(&raw_id).map_err(|_| Error::Unsafe)?;
        let ts = now(&tx, now_ms)?;
        let key = username.key().as_str();
        let taken: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE login_key=?1 AND account_id<>?2)",
            (key, account_id.as_str()),
            |r| r.get(0),
        )?;
        if taken {
            return Err(Error::Conflict);
        }
        let updated = tx.execute(
            "UPDATE accounts SET login=?1, login_key=?2, password_phc=?3, password_changed_at_ms=?4 WHERE account_id=?5 AND admin=1 AND revoked_at_ms IS NULL",
            (
                username.display(),
                key,
                verifier.expose(),
                ts,
                account_id.as_str(),
            ),
        )?;
        if updated != 1 {
            return Err(Error::Unsafe);
        }
        tx.execute("DELETE FROM meta WHERE key=?1", [LOCAL_AUTHLESS_META])?;
        advance_clock(&tx, ts)?;
        tx.commit()?;
        Ok(account_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::store::{AccountStore, NewAccount, Registration};
    use crate::host_secrets::HostSecrets;
    use crate::hostdb::HostDbInit;
    use crate::links::{LinkKind, OneTimeLinks, Subject};

    #[derive(Default)]
    struct MemSecrets(std::sync::Mutex<std::collections::BTreeMap<String, Vec<u8>>>);
    impl HostSecrets for MemSecrets {
        fn load(
            &self,
            name: &crate::host_secrets::Name,
        ) -> Result<Option<crate::host_secrets::Value>, crate::host_secrets::Error> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .get(name.as_str())
                .map(|b| crate::host_secrets::Value::new(b.clone()).unwrap()))
        }
        fn put(
            &self,
            name: &crate::host_secrets::Name,
            value: &crate::host_secrets::Value,
        ) -> Result<(), crate::host_secrets::Error> {
            self.0
                .lock()
                .unwrap()
                .insert(name.as_str().to_owned(), value.expose().to_vec());
            Ok(())
        }
        fn delete(
            &self,
            name: &crate::host_secrets::Name,
        ) -> Result<(), crate::host_secrets::Error> {
            self.0.lock().unwrap().remove(name.as_str());
            Ok(())
        }
        fn list(&self) -> Result<Vec<crate::host_secrets::Name>, crate::host_secrets::Error> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .keys()
                .map(|k| crate::host_secrets::Name::parse(k).unwrap())
                .collect())
        }
    }

    fn fresh_db() -> (tempfile::TempDir, HostDb) {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let secrets = MemSecrets::default();
        let db = HostDb::create(
            dir.path(),
            &secrets,
            &HostDbInit {
                instance_id: crate::ids::uuid_v7().unwrap(),
                clock_high_ms: 1_000,
            },
        )
        .unwrap();
        (dir, db)
    }

    #[test]
    fn local_authless_hides_credentials_and_upgrades_via_enable_or_link() {
        let (_d1, db1) = fresh_db();
        let id1 = db1.init_local_authless_if_empty(2_000).unwrap().unwrap();
        assert!(!db1.auth_enabled().unwrap());
        assert!(!db1.administrator_exists().unwrap());
        // Password lookup for the synthetic local-owner account MUST return None (no oracle / no 503).
        let local_key = LoginKey::new(LOCAL_OWNER_LOGIN).unwrap();
        assert!(db1.account_credentials(&local_key).unwrap().is_none());

        let verifier = crate::authentication::hash_password(
            &org_host_control::Password::new("synthetic test password".into()).unwrap(),
        )
        .unwrap();
        let upgraded = db1
            .enable_local_auth(&Username::new("Owner").unwrap(), &verifier, 3_000)
            .unwrap();
        assert_eq!(upgraded, id1);
        assert!(db1.auth_enabled().unwrap());
        assert!(db1.administrator_exists().unwrap());
        assert!(db1
            .account_credentials(&LoginKey::new("Owner").unwrap())
            .unwrap()
            .is_some());
        assert!(matches!(
            db1.enable_local_auth(&Username::new("Owner2").unwrap(), &verifier, 4_000),
            Err(Error::Conflict)
        ));

        // Link registration also atomically upgrades a local-authless account preserving its ID.
        let (_d2, db2) = fresh_db();
        let id2 = db2.init_local_authless_if_empty(2_000).unwrap().unwrap();
        let issued = db2
            .issue_first(
                LinkKind::Registration,
                &Subject::new("administrator").unwrap(),
                3_000,
            )
            .unwrap()
            .unwrap();
        let new = NewAccount::new(Username::new("LinkOwner").unwrap(), verifier).unwrap();
        let reg = db2
            .register_administrator(&issued.token, &new, 4_000)
            .unwrap();
        assert!(matches!(reg, Registration::Registered { account_id } if account_id == id2));
        assert!(db2.auth_enabled().unwrap());
        assert!(db2.administrator_exists().unwrap());
    }

    #[test]
    fn local_authless_refuses_invites_and_hides_pseudo_account_from_org_access() {
        let (_dir, db) = fresh_db();
        let local_id = db.init_local_authless_if_empty(10_000).unwrap().unwrap();
        let org_id = crate::ids::uuid_v7().unwrap();
        let owner_member = crate::ids::uuid_v7().unwrap();
        let sam_member = crate::ids::uuid_v7().unwrap();
        db.seed_owner_for_test(local_id.as_str(), &org_id, &owner_member)
            .unwrap();

        // Local authless pseudo-account must never appear as a password account in org access.
        assert!(db.list_org_access(&org_id, 10_100).unwrap().is_empty());

        // Issuing invites is refused while authentication is disabled.
        assert_eq!(
            db.issue_org_invite(&org_id, &sam_member, 10_200)
                .unwrap_err(),
            super::super::accounts::multi_person::InviteIssueError::AuthDisabled
        );
    }
}
