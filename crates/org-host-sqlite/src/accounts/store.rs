//! SQLite adapter for the account contract (`org_host::ports::accounts`). It
//! composes the registry's existing account, administrator, link and lockout
//! code; the throttle keys use the control socket's lockout rows.
use rusqlite::{OptionalExtension, TransactionBehavior};

use super::super::{administrator, clock, links, local_auth, HostDb};
use org_host::ports::accounts::{
    AccountId, AccountStore, Attempt, Credentials, LoginKey, NewAccount, Registration, StoreError,
    Throttle, ThrottleKey,
};
use org_host::ports::host_store::lockout;
use org_host::ports::links::{LinkDigest, LinkKind, Token};

fn unavailable<T>(_: T) -> StoreError {
    StoreError::Unavailable
}
fn wall(now_ms: i64) -> Result<u64, StoreError> {
    u64::try_from(now_ms).map_err(unavailable)
}
fn throttle(state: &lockout::State) -> Throttle {
    Throttle {
        locked: state.is_locked(),
        retry_after_ms: if state.is_locked() {
            state.retry_after_ms()
        } else {
            0
        },
    }
}

impl AccountStore for HostDb {
    fn register_administrator(
        &self,
        token: &Token,
        new: &NewAccount,
        now_ms: i64,
    ) -> Result<Registration, StoreError> {
        let digest = LinkDigest::of(&self.instance_id, LinkKind::Registration, token);
        let mut db = self.connection.lock().map_err(unavailable)?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(unavailable)?;
        let now = clock::now(&tx, now_ms).map_err(unavailable)?;
        // The link first: an unauthorized caller learns nothing about accounts.
        // Every early return drops `tx`, rolling the redeem back.
        if let Err(refused) =
            links::redeem_in(&tx, LinkKind::Registration, digest, now).map_err(unavailable)?
        {
            return Ok(Registration::Link(refused));
        }
        if administrator::exists(&tx).map_err(unavailable)? {
            return Ok(Registration::AdministratorExists);
        }
        let local_authless = local_auth::local_authless_id(&tx).map_err(unavailable)?;
        let key = new.username.key().as_str();
        let taken = tx
            .query_row(
                "SELECT 1 FROM accounts WHERE login_key=?1 AND (?2 IS NULL OR account_id<>?2)",
                (key, local_authless.as_deref()),
                |_| Ok(()),
            )
            .optional()
            .map_err(unavailable)?
            .is_some();
        if taken {
            return Ok(Registration::LoginTaken);
        }
        let account_id = if let Some(existing_id) = local_authless {
            let updated = tx
                .execute(
                    "UPDATE accounts SET login=?1, login_key=?2, password_phc=?3, password_changed_at_ms=?4 WHERE account_id=?5 AND admin=1 AND revoked_at_ms IS NULL",
                    (
                        new.username.display(),
                        key,
                        new.verifier.expose(),
                        now,
                        existing_id.as_str(),
                    ),
                )
                .map_err(unavailable)?;
            if updated != 1 {
                return Err(StoreError::Unavailable);
            }
            tx.execute(
                "DELETE FROM meta WHERE key=?1",
                [local_auth::LOCAL_AUTHLESS_META],
            )
            .map_err(unavailable)?;
            AccountId::parse(&existing_id).map_err(unavailable)?
        } else {
            tx.execute(
                "INSERT INTO accounts VALUES (?1,?2,?3,?4,1,?5,?5,NULL)",
                (
                    new.account_id.as_str(),
                    key,
                    new.username.display(),
                    new.verifier.expose(),
                    now,
                ),
            )
            .map_err(unavailable)?;
            new.account_id.clone()
        };
        clock::advance_clock(&tx, now).map_err(unavailable)?;
        tx.commit().map_err(unavailable)?;
        Ok(Registration::Registered { account_id })
    }
    fn credentials(&self, key: &LoginKey) -> Result<Option<Credentials>, StoreError> {
        Ok(self
            .account_credentials(key)
            .map_err(unavailable)?
            .map(|c| {
                let (account, verifier) = c.into_parts();
                Credentials {
                    account_id: account.id().clone(),
                    login: account.login().to_owned(),
                    admin: account.admin(),
                    revoked: account.revoked_at_ms().is_some(),
                    verifier,
                }
            }))
    }
    fn administrator_exists(&self) -> Result<bool, StoreError> {
        HostDb::administrator_exists(self).map_err(unavailable)
    }
    fn throttle(&self, key: &ThrottleKey<'_>, now_ms: i64) -> Result<Throttle, StoreError> {
        let state = self
            .login_lockout(lockout::Socket::Control, key.as_str(), wall(now_ms)?)
            .map_err(unavailable)?;
        Ok(throttle(&state))
    }
    fn record(
        &self,
        key: &ThrottleKey<'_>,
        attempt: Attempt,
        now_ms: i64,
    ) -> Result<Throttle, StoreError> {
        let outcome = match attempt {
            Attempt::Failure => lockout::Outcome::Failure,
            Attempt::Success => lockout::Outcome::Success,
            Attempt::Reset => lockout::Outcome::Reset,
        };
        let state = self
            .record_login_capped(
                lockout::Socket::Control,
                key.as_str(),
                wall(now_ms)?,
                outcome,
                key.cap_ms(),
            )
            .map_err(unavailable)?;
        Ok(throttle(&state))
    }
}
