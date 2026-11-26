//! Account sign-in: verifying a password and issuing a session, with or without local
//! authless onboarding.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use argon2::password_hash::{PasswordHash, PasswordVerifier};
use org_host_control::{
    ops::{AccountVerifyResult, LinkedOrg},
    Password,
};
use org_proto::ApiError;

use crate::accounts::identity::{AccountId, LoginKey};
use crate::hostdb::{
    accounts::Account,
    lockout::{Outcome, Socket},
};
use crate::service::listener::Connection;

use super::{
    argon, denied, digest, random_handle, storage, unavailable, wall_ms, Authentication,
    HashPermit, Session, MAX_CONNECTION_SESSIONS, MAX_HASHES, MAX_SESSIONS,
};

impl Authentication {
    pub(crate) fn verify(
        &self,
        connection: &Connection,
        login: &str,
        password: &Password,
    ) -> Result<AccountVerifyResult, ApiError> {
        // Hard ingress limits precede normalization or Argon2 work.
        if login.len() > 4096 || password.expose().len() > 4096 {
            return Err(denied());
        }
        let key = LoginKey::new(login).map_err(|_| denied())?;
        let now = wall_ms()?;
        let socket = if connection.channel_kind() == org_office_launch::ChannelKind::Web {
            Socket::Control
        } else {
            Socket::Local
        };
        if self
            .db
            .login_lockout(socket, key.as_str(), now)
            .map_err(storage)?
            .is_locked()
            || self
                .db
                .login_lockout(socket, "", now)
                .map_err(storage)?
                .is_locked()
        {
            return Err(denied());
        }
        // The permit follows the lockout read: a locked login or socket never
        // occupies a hash slot, so locked calls cannot starve verification.
        let permit = self
            .hashing
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < MAX_HASHES).then_some(n + 1)
            })
            .map_err(|_| unavailable())?;
        let _ = permit;
        let _permit = HashPermit(&self.hashing);
        let credentials = self.db.account_credentials(&key).map_err(storage)?;
        let verifier = credentials.as_ref().map_or(&self.dummy, |c| c.verifier());
        let parsed = PasswordHash::new(verifier.expose()).map_err(|_| unavailable())?;
        let matches = argon()?
            .verify_password(password.expose().as_bytes(), &parsed)
            .is_ok();
        let current = match &credentials {
            Some(c) if matches => self
                .db
                .account_authority(
                    &AccountId::parse(c.account().account_id()).map_err(|_| unavailable())?,
                )
                .map_err(storage)?,
            _ => None,
        };
        let allowed = current.as_ref().is_some_and(|a| {
            a.revoked_at_ms().is_none()
                && credentials.as_ref().is_some_and(|c| {
                    a.password_changed_at_ms() == c.account().password_changed_at_ms()
                })
        });
        let outcome = if allowed {
            Outcome::Success
        } else {
            Outcome::Failure
        };
        let state = self
            .db
            .record_login(socket, key.as_str(), now, outcome)
            .map_err(storage)?;
        // Empty key is the socket-wide budget; normalized login can never be empty.
        if !allowed || state.is_locked() {
            self.db
                .record_login(socket, "", now, Outcome::Failure)
                .map_err(storage)?;
            return Err(denied());
        }
        let account = current.ok_or_else(denied)?;
        self.issue(connection, account)
    }
    /// Issues a connection-bound native session (`SES-8`) for an account that approved a
    /// terminal sign-in link in the browser (`FC-173` / `SIGN-40`), without a password.
    pub(crate) fn issue_for_account(
        &self,
        connection: &Connection,
        account_id: &AccountId,
    ) -> Result<AccountVerifyResult, ApiError> {
        self.issue_account_with_mode(connection, account_id, false)
    }

    /// Issues a connection-bound native session for the local authless administrator account
    /// (`session.local`). Tagged `local_authless` so enabling authentication immediately revokes
    /// active sessions even if the wall clock did not advance.
    pub(crate) fn issue_local_authless(
        &self,
        connection: &Connection,
        account_id: &AccountId,
    ) -> Result<AccountVerifyResult, ApiError> {
        self.issue_account_with_mode(connection, account_id, true)
    }

    fn issue_account_with_mode(
        &self,
        connection: &Connection,
        account_id: &AccountId,
        local_authless: bool,
    ) -> Result<AccountVerifyResult, ApiError> {
        let account = self
            .db
            .account_authority(account_id)
            .map_err(storage)?
            .ok_or_else(denied)?;
        if account.revoked_at_ms().is_some() {
            return Err(denied());
        }
        self.issue_with_mode(connection, account, local_authless)
    }
    fn issue(
        &self,
        connection: &Connection,
        account: Account,
    ) -> Result<AccountVerifyResult, ApiError> {
        self.issue_with_mode(connection, account, false)
    }
    fn issue_with_mode(
        &self,
        connection: &Connection,
        account: Account,
        local_authless: bool,
    ) -> Result<AccountVerifyResult, ApiError> {
        self.reap();
        // Read before the session table is locked and before a session exists: a
        // registry failure refuses the sign-in and leaves nothing to end.
        let orgs = account
            .links()
            .iter()
            .map(|l| {
                Ok(LinkedOrg {
                    org_id: l.org_id().into(),
                    name: self.organization_name(l.org_id())?,
                    employee_id: l.member_id().into(),
                    office_ids: vec![],
                })
            })
            .collect::<Result<Vec<_>, ApiError>>()?;
        let handle = random_handle()?;
        let key = digest(&handle);
        let mut sessions = self.sessions.lock().map_err(|_| unavailable())?;
        let now = Instant::now();
        let alive = connection.liveness();
        if alive
            .upgrade()
            .is_none_or(|closed| closed.load(Ordering::SeqCst))
        {
            return Err(denied());
        }
        if sessions.len() >= MAX_SESSIONS
            || sessions
                .values()
                .filter(|s| s.connection.ptr_eq(&alive))
                .count()
                >= MAX_CONNECTION_SESSIONS
        {
            return Err(unavailable());
        }
        let result = AccountVerifyResult {
            session: handle,
            account: account.account_id().into(),
            admin: account.admin(),
            orgs,
        };
        let session = Session {
            account: AccountId::parse(account.account_id()).map_err(|_| unavailable())?,
            password_changed_at_ms: account.password_changed_at_ms(),
            local_authless,
            id: crate::ids::generate().map_err(|_| unavailable())?,
            connection: alive,
            created: now,
            touched: now,
            channels: BTreeMap::new(),
            authority_live: Arc::new(AtomicBool::new(true)),
        };
        if sessions.contains_key(&key) {
            return Err(unavailable());
        }
        sessions.insert(key, session);
        Ok(result)
    }
}
