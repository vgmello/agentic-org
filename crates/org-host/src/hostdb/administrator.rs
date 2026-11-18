//! Whether the instance has an administrator (A2 status, link issuing).
use super::{Error, HostDb};

impl HostDb {
    /// An unrevoked account carries the administrator flag.
    pub(crate) fn administrator_exists(&self) -> Result<bool, Error> {
        let db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        Ok(db.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE admin=1 AND revoked_at_ms IS NULL)",
            [],
            |r| r.get(0),
        )?)
    }
}
