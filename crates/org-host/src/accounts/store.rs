//! Account storage for browser sign-in (A3 plan B3): registering the first
//! administrator from a registration link, credentials lookup, and the
//! sign-in throttle keys.
//!
//! `AccountStore` is the consumer-owned, engine-independent host-storage
//! contract (ADR-0018). The SQLite registry implements it in
//! `hostdb::accounts::store` over the existing account, administrator, link
//! and lockout code; `memory` is a second implementation that proves the seam,
//! and `contract_tests` runs one suite against both. The shared rules (the
//! throttle schedule and its caps, the throttle keys) live here.
//!
//! Time is host wall milliseconds supplied by the caller. Adapters make it
//! monotone and refuse a negative time as `Unavailable`.
use std::net::IpAddr;

use super::identity::{AccountId, LoginKey};
use super::rules::Username;
use crate::accounts::verifier::Verifier;
use crate::links::{Refused, Token};

#[cfg(test)]
mod contract_tests;
#[cfg(test)]
pub(crate) mod memory;

/// The per-login key's delay ceiling: 15 minutes (the existing schedule).
pub(crate) const LOGIN_DELAY_CAP_MS: i64 = 900_000;
/// The per-client-address key's delay ceiling (Revision 3): an attacker who
/// floods one login cannot hold an address for more than a minute.
pub(crate) const ADDRESS_DELAY_CAP_MS: i64 = 60_000;
/// Failures are free until this many have accrued.
const FREE_FAILURES: i64 = 6;

/// The delay after `failures` consecutive failures: none for the first five,
/// then 1 s doubling, at most `cap_ms` (and never above the 15-minute ceiling).
pub(crate) fn backoff_ms(failures: i64, cap_ms: i64) -> i64 {
    if failures < FREE_FAILURES {
        return 0;
    }
    let shift = (failures - FREE_FAILURES).clamp(0, 10) as u32;
    (1_000_i64 << shift).min(LOGIN_DELAY_CAP_MS).min(cap_ms)
}

/// The peer address the web process reports, as an opaque throttle key. An
/// IPv6 peer is grouped by its /64; an IPv4-mapped IPv6 peer is its IPv4.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClientAddress(String);
impl ClientAddress {
    pub(crate) fn of(ip: IpAddr) -> Self {
        let ip = match ip {
            IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
            v4 => v4,
        };
        // Upper case: a login key is case folded, so it never begins "ADDR".
        Self(match ip {
            IpAddr::V4(v4) => format!("ADDR {v4}"),
            IpAddr::V6(v6) => {
                let prefix = u128::from(v6) & !((1u128 << 64) - 1);
                format!("ADDR {}/64", std::net::Ipv6Addr::from(prefix))
            }
        })
    }
}

/// A sign-in throttle key on the web path (Revision 3). There is no
/// socket-wide key here; the native CLI path keeps its own keys.
pub(crate) enum ThrottleKey<'a> {
    /// Shared with the native per-login key: one account, one guess budget.
    Login(&'a LoginKey),
    Address(&'a ClientAddress),
}
impl ThrottleKey<'_> {
    /// The stored key. Login keys are case folded and address keys are upper
    /// case, so the two never collide.
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Self::Login(key) => key.as_str(),
            Self::Address(address) => &address.0,
        }
    }
    pub(crate) fn cap_ms(&self) -> i64 {
        match self {
            Self::Login(_) => LOGIN_DELAY_CAP_MS,
            Self::Address(_) => ADDRESS_DELAY_CAP_MS,
        }
    }
}

/// What a password check reports to the throttle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Attempt {
    Failure,
    Success,
    /// An authorized reset or set-password redemption (SIGN-32).
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "A3 B8: set-password redemption")
    )]
    Reset,
}

/// A key's state after a read or an attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Throttle {
    pub(crate) locked: bool,
    /// Zero when not locked.
    pub(crate) retry_after_ms: i64,
}

/// The account to create, already checked and hashed: no raw password.
pub(crate) struct NewAccount {
    pub(crate) account_id: AccountId,
    pub(crate) username: Username,
    pub(crate) verifier: Verifier,
}
impl NewAccount {
    pub(crate) fn new(username: Username, verifier: Verifier) -> Result<Self, StoreError> {
        let id = crate::ids::uuid_v7().map_err(|_| StoreError::Unavailable)?;
        Ok(Self {
            account_id: AccountId::parse(&id).map_err(|_| StoreError::Unavailable)?,
            username,
            verifier,
        })
    }
}

/// The result of `register_administrator`. Every refusal leaves the store
/// unchanged: the link stays as it was and no account exists.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Registration {
    Registered {
        account_id: AccountId,
    },
    /// The link does not redeem. Checked first, so an unauthorized caller
    /// learns nothing about accounts.
    Link(Refused),
    /// An unrevoked administrator exists; the link stays pending.
    AdministratorExists,
    /// The username's key belongs to another (revoked) account.
    LoginTaken,
}

/// What a password check needs. Revoked accounts are returned so the caller
/// runs the same password work for them as for any other account.
#[cfg_attr(not(test), expect(dead_code, reason = "A3 B7/B8 read login and admin"))]
pub(crate) struct Credentials {
    pub(crate) account_id: AccountId,
    pub(crate) login: String,
    pub(crate) admin: bool,
    pub(crate) revoked: bool,
    pub(crate) verifier: Verifier,
}

/// Storage failed. A failure is never reported as a valid answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StoreError {
    Unavailable,
}

/// The host-storage contract for browser accounts. Every call runs in one
/// transaction.
pub(crate) trait AccountStore {
    /// Redeems the registration link `token` and creates `new` as the
    /// administrator, atomically: either both happen or neither.
    fn register_administrator(
        &self,
        token: &Token,
        new: &NewAccount,
        now_ms: i64,
    ) -> Result<Registration, StoreError>;
    /// The account whose login key is `key`, revoked or not.
    fn credentials(&self, key: &LoginKey) -> Result<Option<Credentials>, StoreError>;
    /// Whether an unrevoked administrator exists.
    #[cfg_attr(not(test), expect(dead_code, reason = "A3 B8: hand-over"))]
    fn administrator_exists(&self) -> Result<bool, StoreError>;
    /// Reads a throttle key without changing it.
    fn throttle(&self, key: &ThrottleKey<'_>, now_ms: i64) -> Result<Throttle, StoreError>;
    /// Records an attempt on `key`. A failure lengthens the delay by the
    /// schedule capped at `key.cap_ms()`; success and reset clear the key. An
    /// attempt while locked changes nothing, except a reset.
    fn record(
        &self,
        key: &ThrottleKey<'_>,
        attempt: Attempt,
        now_ms: i64,
    ) -> Result<Throttle, StoreError>;
}
