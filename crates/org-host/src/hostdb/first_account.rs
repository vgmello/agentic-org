//! Prepared account identity and verifier; hashing precedes registry creation.
use super::{journal::Verifier, Error};
use crate::accounts::identity::{AccountId, LoginKey};

pub(crate) struct FirstAccount {
    account_id: AccountId,
    login_key: LoginKey,
    login: String,
    verifier: Verifier,
}
impl FirstAccount {
    pub(crate) fn new(account_id: &str, login: &str, verifier: Verifier) -> Result<Self, Error> {
        Ok(Self {
            account_id: AccountId::parse(account_id).map_err(|_| Error::InvalidId)?,
            login_key: LoginKey::new(login).map_err(|_| Error::Unsafe)?,
            login: login.to_owned(),
            verifier,
        })
    }
    pub(crate) fn account_id(&self) -> &str {
        self.account_id.as_str()
    }
    pub(crate) fn login_key(&self) -> &str {
        self.login_key.as_str()
    }
    pub(crate) fn login(&self) -> &str {
        &self.login
    }
    pub(crate) fn verifier(&self) -> &Verifier {
        &self.verifier
    }
}
