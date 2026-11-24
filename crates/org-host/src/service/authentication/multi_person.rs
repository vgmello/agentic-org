//! Multi-person browser authentication operations (`TASK-38` / A6):
//! invitation link registration, existing-account invitation attachment (SIGN-35),
//! one-time link inspection, set-password link redemption, own password change,
//! and password-confirmed handover verification.

use argon2::password_hash::{PasswordHash, PasswordVerifier};
use org_host_control::browser::{BrowserSession, LinkRegisterParams};
use org_host_control::browser_accounts::{
    InspectLinkKind, LinkInspectParams, LinkInspectResult, LinkRegisterAttachParams,
    LinkSetPasswordParams, LinkSetPasswordResult,
};
use org_host_control::Password;
use org_proto::{ApiError, ErrorCode};

use super::browser::{
    digest, failed, not_found, origin, store_error, unless_locked, FIELD_MAX_BYTES,
};
use super::{argon, denied, hash_password, unavailable, Authentication};
use crate::accounts::identity::{AccountId, LoginKey};
use crate::accounts::rules::{check_new_password, PasswordRule};
use crate::accounts::store::{
    AccountStore, Attempt, ClientAddress, NewAccount, Registration, ThrottleKey,
};
use crate::hostdb::sqlite::accounts::multi_person::{
    parse_account_subject, parse_invite_subject, InviteAttach, InviteRegistration,
    SetPasswordOutcome,
};
use crate::links::{LinkKind, Token};
use crate::sessions::{Ended, Origin};

impl Authentication {
    pub(super) fn complete_invite_registration(
        &self,
        token: &Token,
        new: &NewAccount,
        params: &LinkRegisterParams,
        origin: Origin,
        now_ms: i64,
        ended: &mut Vec<Ended>,
    ) -> Result<BrowserSession, ApiError> {
        let accounts: &dyn AccountStore = &*self.db;
        let address = ClientAddress::of(params.client_address);
        match self
            .db
            .redeem_invite_register(token, new, now_ms)
            .map_err(store_error)?
        {
            InviteRegistration::Registered { account_id, .. } => self.start(
                &account_id,
                digest(&params.cookie_digest),
                digest(&params.proof_digest),
                origin,
                now_ms,
                ended,
            ),
            InviteRegistration::Link(_) | InviteRegistration::NotAnInvite => {
                failed(accounts, &[ThrottleKey::Address(&address)], now_ms).and(Err(not_found()))
            }
            InviteRegistration::AlreadyLinked => Err(ApiError::new(
                ErrorCode::Conflict,
                "this person already has an account in this organization",
            )),
            InviteRegistration::LoginTaken => Err(ApiError::rule("login", "taken")),
        }
    }

    pub(super) fn complete_admin_registration(
        &self,
        token: &Token,
        new: &NewAccount,
        params: &LinkRegisterParams,
        origin: Origin,
        now_ms: i64,
        ended: &mut Vec<Ended>,
    ) -> Result<BrowserSession, ApiError> {
        let accounts: &dyn AccountStore = &*self.db;
        let address = ClientAddress::of(params.client_address);
        let was_authless = self
            .db
            .local_authless_account()
            .map_err(|_| unavailable())?
            .is_some();
        match accounts
            .register_administrator(token, new, now_ms)
            .map_err(store_error)?
        {
            Registration::Registered { account_id } => {
                if was_authless {
                    ended.push(Ended {
                        id: Self::LOCAL_AUTHLESS_SESSION_ID.to_owned(),
                        account_id: account_id.as_str().to_owned(),
                    });
                }
                self.start(
                    &account_id,
                    digest(&params.cookie_digest),
                    digest(&params.proof_digest),
                    origin,
                    now_ms,
                    ended,
                )
            }
            Registration::Link(_) | Registration::AdministratorExists => {
                failed(accounts, &[ThrottleKey::Address(&address)], now_ms).and(Err(not_found()))
            }
            Registration::LoginTaken => Err(ApiError::rule("login", "taken")),
        }
    }
    /// Non-consuming preview of a one-time link (`link.inspect`). Returns `not_found` for any
    /// invalid, unknown, used, cancelled, or expired link.
    pub(crate) fn inspect_link(
        &self,
        params: &LinkInspectParams,
        now_ms: i64,
        org_name: impl Fn(&str) -> String,
    ) -> Result<LinkInspectResult, ApiError> {
        let Some(token) = Token::parse(params.link.expose()) else {
            return Err(not_found());
        };
        let kind = match params.kind {
            InspectLinkKind::Registration => LinkKind::Registration,
            InspectLinkKind::SetPassword => LinkKind::SetPassword,
        };
        let subject = self
            .db
            .peek_link_subject(kind, &token, now_ms)
            .map_err(store_error)?
            .map_err(|_| not_found())?;
        match kind {
            LinkKind::Registration => {
                if let Some((org_id, member_id)) = parse_invite_subject(&subject) {
                    Ok(LinkInspectResult::OrganizationInvite {
                        org_id: org_id.to_owned(),
                        org_name: org_name(org_id),
                        member_id: member_id.to_owned(),
                    })
                } else {
                    if self.db.administrator_exists().map_err(store_error)? {
                        return Err(not_found());
                    }
                    Ok(LinkInspectResult::AdministratorRegistration)
                }
            }
            LinkKind::SetPassword => {
                let account_id = parse_account_subject(&subject).ok_or_else(not_found)?;
                let login = self
                    .db
                    .active_account_login(&account_id)
                    .map_err(store_error)?
                    .ok_or_else(not_found)?;
                Ok(LinkInspectResult::SetPassword { login })
            }
        }
    }

    /// `link.register_attach` (SIGN-35): signs in with an existing account and atomically
    /// redeems an organization invitation link, attaching the account to the invited member.
    pub(crate) fn attach_invite(
        &self,
        params: &LinkRegisterAttachParams,
        now_ms: i64,
        ended: &mut Vec<Ended>,
    ) -> Result<BrowserSession, ApiError> {
        if params.login.len() > FIELD_MAX_BYTES || params.password.expose().len() > FIELD_MAX_BYTES
        {
            return Err(denied());
        }
        let origin = origin(&params.origin)?;
        let address = ClientAddress::of(params.client_address);
        let key = LoginKey::new(&params.login).ok();
        let mut keys = vec![ThrottleKey::Address(&address)];
        if let Some(key) = &key {
            keys.push(ThrottleKey::Login(key));
        }
        let accounts: &dyn AccountStore = &*self.db;
        unless_locked(accounts, &keys, now_ms)?;
        let Some(token) = Token::parse(params.link.expose()) else {
            failed(accounts, &[ThrottleKey::Address(&address)], now_ms)?;
            return Err(not_found());
        };
        let subject = self
            .db
            .peek_link_subject(LinkKind::Registration, &token, now_ms)
            .map_err(store_error)?;
        if !matches!(subject, Ok(ref s) if parse_invite_subject(s).is_some()) {
            failed(accounts, &[ThrottleKey::Address(&address)], now_ms)?;
            return Err(not_found());
        }
        let account_id = {
            let _permit = self.permit()?;
            let credentials = match &key {
                Some(key) => accounts.credentials(key).map_err(store_error)?,
                None => None,
            }
            .filter(|c| {
                c.verifier.expose() != crate::hostdb::sqlite::local_auth::LOCAL_AUTHLESS_VERIFIER
            });
            let verifier = credentials.as_ref().map_or(&self.dummy, |c| &c.verifier);
            let parsed = PasswordHash::new(verifier.expose()).map_err(|_| unavailable())?;
            let matches = argon()?
                .verify_password(params.password.expose().as_bytes(), &parsed)
                .is_ok();
            credentials
                .filter(|c| matches && !c.revoked)
                .map(|c| c.account_id)
        };
        let Some(account_id) = account_id else {
            failed(accounts, &keys, now_ms)?;
            return Err(denied());
        };
        if let Some(key) = &key {
            accounts
                .record(&ThrottleKey::Login(key), Attempt::Success, now_ms)
                .map_err(store_error)?;
        }
        match self
            .db
            .redeem_invite_attach(&token, &account_id, now_ms)
            .map_err(store_error)?
        {
            InviteAttach::Attached { .. } => self.start(
                &account_id,
                digest(&params.cookie_digest),
                digest(&params.proof_digest),
                origin,
                now_ms,
                ended,
            ),
            InviteAttach::Link(_) | InviteAttach::NotAnInvite => {
                failed(accounts, &[ThrottleKey::Address(&address)], now_ms)?;
                Err(not_found())
            }
            InviteAttach::AlreadyLinked => Err(ApiError::new(
                ErrorCode::Conflict,
                "this person already has an account in this organization",
            )),
            InviteAttach::AlreadyMember => Err(ApiError::new(
                ErrorCode::Conflict,
                "this account is already a member of this organization",
            )),
            InviteAttach::AccountUnavailable => Err(denied()),
        }
    }

    /// `link.set_password`: redeems a one-time set-password link (`admin reset` or People ›
    /// New sign-in link), clears lockout, and ends every session of the account.
    pub(crate) fn redeem_set_password(
        &self,
        params: &LinkSetPasswordParams,
        now_ms: i64,
        ended: &mut Vec<Ended>,
    ) -> Result<LinkSetPasswordResult, ApiError> {
        let accounts: &dyn AccountStore = &*self.db;
        let address = ClientAddress::of(params.client_address);
        let keys = [ThrottleKey::Address(&address)];
        unless_locked(accounts, &keys, now_ms)?;
        let refuse = || failed(accounts, &keys, now_ms).and(Err(not_found()));
        let Some(token) = Token::parse(params.link.expose()) else {
            return refuse();
        };
        check_new_password(params.password.expose()).map_err(|rule| match rule {
            PasswordRule::TooShort => ApiError::rule("password", "too_short"),
            PasswordRule::TooLong => ApiError::rule("password", "too_long"),
        })?;
        if self
            .db
            .peek_link_subject(LinkKind::SetPassword, &token, now_ms)
            .map_err(store_error)?
            .is_err()
        {
            return refuse();
        }
        let verifier = {
            let _permit = self.permit()?;
            hash_password(&params.password)?
        };
        match self
            .db
            .redeem_set_password(&token, &verifier, now_ms)
            .map_err(store_error)?
        {
            SetPasswordOutcome::Updated {
                login, ended: gone, ..
            } => {
                let ids: Vec<&str> = gone.iter().map(|e| e.id.as_str()).collect();
                self.touch_batcher.forget(&ids);
                ended.extend(gone);
                Ok(LinkSetPasswordResult { login })
            }
            SetPasswordOutcome::Link(_) => refuse(),
        }
    }

    /// Verifies `password` against `account_id`'s current password hash using the shared
    /// Argon2id permit pool and per-login throttle (for own password change and password-confirmed
    /// handovers).
    pub(crate) fn verify_existing_password(
        &self,
        account_id: &AccountId,
        login: &str,
        password: &Password,
        now_ms: i64,
    ) -> Result<(), ApiError> {
        if password.expose().len() > FIELD_MAX_BYTES {
            return Err(denied());
        }
        let key = LoginKey::new(login).map_err(|_| denied())?;
        let accounts: &dyn AccountStore = &*self.db;
        let throttle = [ThrottleKey::Login(&key)];
        unless_locked(accounts, &throttle, now_ms)?;
        let matches = {
            let _permit = self.permit()?;
            let credentials = self
                .db
                .account_credentials(&key)
                .map_err(store_error)?
                .filter(|c| c.account().account_id() == account_id.as_str());
            let verifier = credentials.as_ref().map_or(&self.dummy, |c| c.verifier());
            let parsed = PasswordHash::new(verifier.expose()).map_err(|_| unavailable())?;
            credentials.is_some()
                && argon()?
                    .verify_password(password.expose().as_bytes(), &parsed)
                    .is_ok()
        };
        if matches {
            accounts
                .record(&ThrottleKey::Login(&key), Attempt::Success, now_ms)
                .map_err(store_error)?;
            Ok(())
        } else {
            failed(accounts, &throttle, now_ms)?;
            Err(denied())
        }
    }

    /// Changes an account's own password and ends every OTHER browser session while keeping
    /// `session.session_id` live.
    pub(crate) fn change_own_password(
        &self,
        session: &BrowserSession,
        current_password: &Password,
        new_password: &Password,
        now_ms: i64,
        ended: &mut Vec<Ended>,
    ) -> Result<(), ApiError> {
        let account_id = AccountId::parse(&session.account.id).map_err(|_| unavailable())?;
        self.verify_existing_password(
            &account_id,
            &session.account.login,
            current_password,
            now_ms,
        )?;
        check_new_password(new_password.expose()).map_err(|rule| match rule {
            PasswordRule::TooShort => ApiError::rule("password", "too_short"),
            PasswordRule::TooLong => ApiError::rule("password", "too_long"),
        })?;
        let verifier = {
            let _permit = self.permit()?;
            hash_password(new_password)?
        };
        let gone = self
            .db
            .change_own_password(&account_id, &session.session_id, &verifier, now_ms)
            .map_err(store_error)?;
        let ids: Vec<&str> = gone.iter().map(|e| e.id.as_str()).collect();
        self.touch_batcher.forget(&ids);
        ended.extend(gone);
        Ok(())
    }

    /// Forgets buffered touches for ended sessions (e.g. after access removal).
    pub(crate) fn forget_ended_touches(&self, ended: &[Ended]) {
        let ids: Vec<&str> = ended.iter().map(|e| e.id.as_str()).collect();
        self.touch_batcher.forget(&ids);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::authentication::tests::fixture;

    #[test]
    fn verify_existing_password_throttles_failures_and_clears_on_success() {
        let (_root, auth, _c) = fixture();
        let now = i64::try_from(crate::service::authentication::wall_ms().unwrap()).unwrap();
        let (owner_id, login) = auth.db.active_administrator_account().unwrap().unwrap();
        let wrong = Password::new("wrong password here".into()).unwrap();
        let right = Password::new("synthetic test password".into()).unwrap();

        let mut locked = false;
        for step in 0..10 {
            let err = auth
                .verify_existing_password(&owner_id, &login, &wrong, now + step)
                .unwrap_err();
            if err.code == ErrorCode::RateLimited {
                locked = true;
                break;
            }
            assert_eq!(err.code, ErrorCode::NotAuthorized);
        }
        assert!(locked, "repeated wrong passwords must trigger lockout");

        // After lockout expires, valid password succeeds and clears throttle state.
        let after_lock = now + 60 * 60 * 1000;
        auth.verify_existing_password(&owner_id, &login, &right, after_lock)
            .unwrap();
        assert_eq!(
            auth.verify_existing_password(&owner_id, &login, &wrong, after_lock + 1)
                .unwrap_err()
                .code,
            ErrorCode::NotAuthorized
        );
    }
}
