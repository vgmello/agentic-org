//! Browser sessions on the web process's control connection (A3 plan B4; ADR-0013).
//!
//! Unlike native sessions these are stored records ([`BrowserSessions`]) that
//! outlive the control connection: no absolute lifetime, no per-connection or
//! per-account cap, only the 30-day idle rule and the host-wide bound. Only
//! digests of the browser's cookie and proof reach the host. Every ended session
//! is appended to the caller's `ended` list, also on a refusal, and the caller pushes `session.ended` after the delete on the
//! same ordered connection writer.
use std::sync::atomic::Ordering;

use argon2::password_hash::{PasswordHash, PasswordVerifier};
use org_host_control::browser::{
    BrowserAccount, BrowserOrigin, BrowserSession, Digest32, LinkRegisterParams,
    SessionSignInParams, SessionValidateParams,
};
use org_host_control::ops::LinkedOrg;
use org_proto::{ApiError, ErrorCode};

use super::{argon, denied, hash_password, unavailable, Authentication, HashPermit, MAX_HASHES};
use crate::accounts::identity::{AccountId, LoginKey};
use crate::accounts::rules::{check_new_password, PasswordRule, Username};
use crate::accounts::store::{
    AccountStore, Attempt, ClientAddress, NewAccount, Registration, ThrottleKey,
};
use crate::sessions::{
    Admission, BrowserSessions, Ended, NewSession, Origin, Scheme, SessionDigest, IDLE_MS,
};

/// Wire bound on the login and password fields, checked before any
/// normalization or Argon2 work.
pub(crate) const FIELD_MAX_BYTES: usize = 4096;

fn not_found() -> ApiError {
    ApiError::new(ErrorCode::NotFound, "link not found")
}
fn origin(origin: &BrowserOrigin) -> Result<Origin, ApiError> {
    Scheme::parse(&origin.scheme)
        .and_then(|scheme| Origin::new(scheme, &origin.host, origin.port).ok())
        .ok_or_else(|| ApiError::new(ErrorCode::InvalidRequest, "invalid origin"))
}
fn digest(value: &Digest32) -> SessionDigest {
    SessionDigest(value.0)
}
fn store_error<E>(_: E) -> ApiError {
    unavailable()
}

impl Authentication {
    fn permit(&self) -> Result<HashPermit<'_>, ApiError> {
        self.hashing
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < MAX_HASHES).then_some(n + 1)
            })
            .map_err(|_| unavailable())?;
        Ok(HashPermit(&self.hashing))
    }

    /// `link.register`: redeems the setup link and signs the new administrator in.
    pub(crate) fn register(
        &self,
        params: &LinkRegisterParams,
        now_ms: i64,
        ended: &mut Vec<Ended>,
    ) -> Result<BrowserSession, ApiError> {
        if params.login.len() > FIELD_MAX_BYTES {
            return Err(ApiError::rule("login", "format"));
        }
        let token = crate::links::Token::parse(params.link.expose()).ok_or_else(not_found)?;
        let username =
            Username::new(&params.login).map_err(|_| ApiError::rule("login", "format"))?;
        check_new_password(params.password.expose()).map_err(|rule| match rule {
            PasswordRule::TooShort => ApiError::rule("password", "too_short"),
            PasswordRule::TooLong => ApiError::rule("password", "too_long"),
        })?;
        let origin = origin(&params.origin)?;
        let verifier = {
            let _permit = self.permit()?;
            hash_password(&params.password)?
        };
        let new = NewAccount::new(username, verifier).map_err(store_error)?;
        let accounts: &dyn AccountStore = &*self.db;
        match accounts
            .register_administrator(&token, &new, now_ms)
            .map_err(store_error)?
        {
            Registration::Registered { account_id } => self.start(
                &account_id,
                digest(&params.cookie_digest),
                digest(&params.proof_digest),
                origin,
                now_ms,
                ended,
            ),
            Registration::Link(_) | Registration::AdministratorExists => Err(not_found()),
            Registration::LoginTaken => Err(ApiError::rule("login", "taken")),
        }
    }

    /// `session.sign_in`: one identical refusal for an unknown login, a wrong
    /// password and a revoked account; `rate_limited` before any Argon2 work.
    pub(crate) fn sign_in_browser(
        &self,
        params: &SessionSignInParams,
        now_ms: i64,
        ended: &mut Vec<Ended>,
    ) -> Result<BrowserSession, ApiError> {
        if params.login.len() > FIELD_MAX_BYTES || params.password.expose().len() > FIELD_MAX_BYTES
        {
            return Err(denied());
        }
        let origin = origin(&params.origin)?;
        // An unformattable login is an unknown login (SIGN-4 sameness).
        let key = LoginKey::new(&params.login).ok();
        let address = ClientAddress::of(params.client_address);
        let mut keys = vec![ThrottleKey::Address(&address)];
        if let Some(key) = &key {
            keys.push(ThrottleKey::Login(key));
        }
        let accounts: &dyn AccountStore = &*self.db;
        let _permit = self.permit()?;
        let mut retry = 0;
        for k in &keys {
            let state = accounts.throttle(k, now_ms).map_err(store_error)?;
            if state.locked {
                retry = retry.max(state.retry_after_ms);
            }
        }
        if retry > 0 {
            return Err(ApiError::rate_limited(u64::try_from(retry).unwrap_or(1)));
        }
        let credentials = match &key {
            Some(key) => accounts.credentials(key).map_err(store_error)?,
            None => None,
        };
        let verifier = credentials.as_ref().map_or(&self.dummy, |c| &c.verifier);
        let parsed = PasswordHash::new(verifier.expose()).map_err(|_| unavailable())?;
        let matches = argon()?
            .verify_password(params.password.expose().as_bytes(), &parsed)
            .is_ok();
        let account = credentials
            .filter(|c| matches && !c.revoked)
            .map(|c| c.account_id);
        let Some(account) = account else {
            // Every failure counts on the address and, when well formed, the login.
            for k in &keys {
                accounts
                    .record(k, Attempt::Failure, now_ms)
                    .map_err(store_error)?;
            }
            return Err(denied());
        };
        if let Some(key) = &key {
            // Success clears the login's budget only; the address keeps its count.
            accounts
                .record(&ThrottleKey::Login(key), Attempt::Success, now_ms)
                .map_err(store_error)?;
        }
        self.start(
            &account,
            digest(&params.cookie_digest),
            digest(&params.proof_digest),
            origin,
            now_ms,
            ended,
        )
    }

    /// `session.validate`: the live session, touched. A session whose account is
    /// gone or revoked ends with every other session of that account.
    pub(crate) fn validate_browser(
        &self,
        params: &SessionValidateParams,
        now_ms: i64,
        ended: &mut Vec<Ended>,
    ) -> Result<BrowserSession, ApiError> {
        let origin = origin(&params.origin)?;
        let sessions: &dyn BrowserSessions = &*self.db;
        let record = sessions
            .find(
                &digest(&params.cookie_digest),
                &digest(&params.proof_digest),
                &origin,
                now_ms,
            )
            .map_err(store_error)?
            .ok_or_else(denied)?;
        let account = AccountId::parse(&record.account_id).map_err(|_| unavailable())?;
        match self.view(&record.id, &account, now_ms)? {
            Some(view) => {
                sessions
                    .touch(&[(record.id.as_str(), now_ms)], now_ms)
                    .map_err(store_error)?;
                Ok(view)
            }
            None => {
                ended.extend(
                    sessions
                        .end_account(account.as_str(), None)
                        .map_err(store_error)?,
                );
                Err(denied())
            }
        }
    }

    /// `session.end`: deletes one session. Ending an unknown session is not an error.
    pub(crate) fn end_browser(&self, session_id: &str) -> Result<Vec<Ended>, ApiError> {
        let sessions: &dyn BrowserSessions = &*self.db;
        Ok(sessions
            .end(session_id)
            .map_err(store_error)?
            .into_iter()
            .collect())
    }

    fn start(
        &self,
        account: &AccountId,
        cookie: SessionDigest,
        proof: SessionDigest,
        origin: Origin,
        now_ms: i64,
        ended: &mut Vec<Ended>,
    ) -> Result<BrowserSession, ApiError> {
        let new = NewSession::new(account.as_str(), cookie, proof, origin).map_err(store_error)?;
        let sessions: &dyn BrowserSessions = &*self.db;
        match sessions.create(&new, now_ms).map_err(store_error)? {
            Admission::Created { reaped } => {
                ended.extend(reaped);
                self.view(&new.id, account, now_ms)?.ok_or_else(denied)
            }
            // The same body for every account (SES-6).
            Admission::Full { reaped } => {
                ended.extend(reaped);
                Err(unavailable())
            }
        }
    }

    /// The session view from current registry authority; `None` if the account
    /// no longer exists or is revoked.
    fn view(
        &self,
        session_id: &str,
        account: &AccountId,
        now_ms: i64,
    ) -> Result<Option<BrowserSession>, ApiError> {
        let Some(account) = self
            .db
            .account_authority(account)
            .map_err(|_| unavailable())?
            .filter(|a| a.revoked_at_ms().is_none())
        else {
            return Ok(None);
        };
        let ms = |v: i64| u64::try_from(v).map_err(|_| unavailable());
        Ok(Some(BrowserSession {
            session_id: session_id.to_owned(),
            account: BrowserAccount {
                id: account.account_id().into(),
                login: account.login().into(),
            },
            admin: account.admin(),
            orgs: account
                .links()
                .iter()
                .map(|l| LinkedOrg {
                    org_id: l.org_id().into(),
                    employee_id: l.member_id().into(),
                    office_ids: vec![],
                })
                .collect(),
            server_time_ms: ms(now_ms)?,
            idle_expires_at_ms: ms(now_ms.saturating_add(IDLE_MS))?,
        }))
    }
}

#[cfg(test)]
mod tests;
