//! Whether the instance has an administrator (A2 status, link issuing).
use super::{Error, HostDb, SqliteResultExt};

impl HostDb {
    /// An unrevoked account carries the administrator flag.
    pub(crate) fn administrator_exists(&self) -> Result<bool, Error> {
        let db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        exists(&db).db()
    }
}

/// Whether an unrevoked password-authenticated administrator account exists, read on `db`.
/// Excludes the implicit local-authless placeholder account (`!local-authless`, ADR-0034).
pub(super) fn exists(db: &rusqlite::Connection) -> rusqlite::Result<bool> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM accounts WHERE admin=1 AND revoked_at_ms IS NULL AND password_phc <> ?1)",
        [super::local_auth::LOCAL_AUTHLESS_VERIFIER],
        |r| r.get(0),
    )
}
