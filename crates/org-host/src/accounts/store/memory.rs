//! In-memory `AccountStore` adapter. It proves the storage seam (ADR-0018
//! ENG-2) and runs the same contract suite as the SQLite registry. It owns its
//! registration links, as the registry owns its link table.
use std::collections::BTreeMap;
use std::sync::Mutex;

use super::{
    backoff_ms, AccountStore, Attempt, Credentials, NewAccount, Registration, StoreError, Throttle,
    ThrottleKey,
};
use crate::accounts::identity::{AccountId, LoginKey};
use crate::accounts::verifier::Verifier;
use crate::links::memory::MemoryLinks;
use crate::links::{LinkDigest, LinkKind, OneTimeLinks, Token};

struct Account {
    account_id: AccountId,
    key: String,
    login: String,
    admin: bool,
    revoked: bool,
    phc: String,
}
struct Counter {
    failures: i64,
    until: i64,
}
struct State {
    accounts: Vec<Account>,
    throttle: BTreeMap<String, Counter>,
    /// The latest time applied; calls never observe an earlier one.
    high_ms: i64,
}

pub(crate) struct MemoryAccounts {
    links: MemoryLinks,
    state: Mutex<State>,
}
impl MemoryAccounts {
    pub(crate) fn new(instance_id: &str, clock_high_ms: i64) -> Self {
        Self {
            links: MemoryLinks::new(instance_id),
            state: Mutex::new(State {
                accounts: Vec::new(),
                throttle: BTreeMap::new(),
                high_ms: clock_high_ms,
            }),
        }
    }
    /// The registration links this store redeems.
    pub(crate) fn links(&self) -> &MemoryLinks {
        &self.links
    }
    /// Tests only: revokes an account, as an administrator would.
    pub(crate) fn revoke(&self, account_id: &AccountId) {
        let mut state = self.state.lock().unwrap();
        for account in &mut state.accounts {
            if account.account_id == *account_id {
                account.revoked = true;
            }
        }
    }
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>, StoreError> {
        self.state.lock().map_err(|_| StoreError::Unavailable)
    }
}

/// The store's monotone time for a call at `now_ms`; negative times are
/// refused as the registry's clock refuses them.
fn clock(state: &State, now_ms: i64) -> Result<i64, StoreError> {
    if now_ms < 0 {
        return Err(StoreError::Unavailable);
    }
    Ok(now_ms.max(state.high_ms))
}
fn throttle(until: i64, now: i64) -> Throttle {
    Throttle {
        locked: now < until,
        retry_after_ms: (until - now).max(0),
    }
}

impl AccountStore for MemoryAccounts {
    /// The registry's order: link, administrator, login key, then insert.
    /// Every check precedes the redeem, so a refusal changes nothing.
    fn register_administrator(
        &self,
        token: &Token,
        new: &NewAccount,
        now_ms: i64,
    ) -> Result<Registration, StoreError> {
        let mut state = self.lock()?;
        let now = clock(&state, now_ms)?;
        let digest = LinkDigest::of(self.links.instance_id(), LinkKind::Registration, token);
        let unavailable = |_| StoreError::Unavailable;
        if let Err(refused) = self
            .links
            .would_redeem(LinkKind::Registration, digest, now)
            .map_err(unavailable)?
        {
            return Ok(Registration::Link(refused));
        }
        if state.accounts.iter().any(|a| a.admin && !a.revoked) {
            return Ok(Registration::AdministratorExists);
        }
        let key = new.username.key().as_str();
        if state.accounts.iter().any(|a| a.key == key) {
            return Ok(Registration::LoginTaken);
        }
        if state
            .accounts
            .iter()
            .any(|a| a.account_id == new.account_id)
        {
            return Err(StoreError::Unavailable);
        }
        self.links
            .redeem(LinkKind::Registration, digest, now)
            .map_err(unavailable)?
            .map_err(|_| StoreError::Unavailable)?;
        state.accounts.push(Account {
            account_id: new.account_id.clone(),
            key: key.to_owned(),
            login: new.username.display().to_owned(),
            admin: true,
            revoked: false,
            phc: new.verifier.expose().to_owned(),
        });
        state.high_ms = now;
        Ok(Registration::Registered {
            account_id: new.account_id.clone(),
        })
    }
    fn credentials(&self, key: &LoginKey) -> Result<Option<Credentials>, StoreError> {
        let state = self.lock()?;
        let Some(a) = state.accounts.iter().find(|a| a.key == key.as_str()) else {
            return Ok(None);
        };
        Ok(Some(Credentials {
            account_id: a.account_id.clone(),
            login: a.login.clone(),
            admin: a.admin,
            revoked: a.revoked,
            verifier: Verifier::new(a.phc.clone()).map_err(|_| StoreError::Unavailable)?,
        }))
    }
    fn administrator_exists(&self) -> Result<bool, StoreError> {
        Ok(self.lock()?.accounts.iter().any(|a| a.admin && !a.revoked))
    }
    fn throttle(&self, key: &ThrottleKey<'_>, now_ms: i64) -> Result<Throttle, StoreError> {
        let state = self.lock()?;
        let now = clock(&state, now_ms)?;
        let until = state.throttle.get(key.as_str()).map_or(now, |c| c.until);
        Ok(throttle(until, now))
    }
    fn record(
        &self,
        key: &ThrottleKey<'_>,
        attempt: Attempt,
        now_ms: i64,
    ) -> Result<Throttle, StoreError> {
        let mut state = self.lock()?;
        let now = clock(&state, now_ms)?;
        let current = state.throttle.get(key.as_str());
        if let Some(until) = current.map(|c| c.until).filter(|u| now < *u) {
            if attempt != Attempt::Reset {
                return Ok(throttle(until, now));
            }
        }
        let mut until = now;
        if attempt == Attempt::Failure {
            let failures = (current.map_or(0, |c| c.failures) + 1).min(63);
            until = now + backoff_ms(failures, key.cap_ms());
            state
                .throttle
                .insert(key.as_str().to_owned(), Counter { failures, until });
        } else {
            state.throttle.remove(key.as_str());
        }
        state.high_ms = now;
        Ok(throttle(until, now))
    }
}
