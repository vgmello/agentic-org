//! One administrator account in a host registry, as a redeemed registration link leaves it:
//! for this crate's tests and, through [`super::fixture::seed_administrator`], for the
//! real-process tests of a `test-support` debug build. Never in a release build.
use org_host::ports::accounts::{AccountId, LoginKey};
use org_host::test_support::process::hash_password;
use org_host_control::Password;
use org_host_sqlite::HostDb;

/// The seeded administrator's login.
pub const LOGIN: &str = "Owner";
/// The seeded administrator's password: synthetic, for tests only.
pub const PASSWORD: &str = "synthetic first password";

/// Adds the administrator [`LOGIN`] with `password` to `db`, created at `now_ms`.
pub fn administrator(db: &HostDb, password: &str, now_ms: i64) -> Result<(), String> {
    let password = Password::new(password.into()).map_err(|_| "an invalid password")?;
    let verifier = hash_password(&password).map_err(|e| e.message)?;
    let id = org_host::ids::generate().map_err(|e| e.to_string())?;
    AccountId::parse(&id).map_err(|_| "an invalid account id")?;
    LoginKey::new(LOGIN).map_err(|_| "an invalid login")?;
    db.seed_account(&id, LOGIN, &verifier, true, now_ms)
        .map_err(|e| format!("{e:?}"))
}
