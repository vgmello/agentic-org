//! SQLite adapter for the one-time link contract (`crate::links`).
use rusqlite::{OptionalExtension, TransactionBehavior};

use super::HostDb;
use crate::links::{
    redeem_rule, Error, LinkDigest, LinkKind, NewLink, OneTimeLinks, Pending, Refused, Subject,
};

pub(super) const DDL: &str = r#"
CREATE TABLE one_time_links (
 link_id TEXT PRIMARY KEY CHECK (length(link_id) = 36),
 kind TEXT NOT NULL CHECK (kind IN ('registration')),
 subject TEXT NOT NULL CHECK (length(subject) BETWEEN 1 AND 128),
 digest BLOB NOT NULL UNIQUE CHECK (length(digest) = 32),
 issued_at_ms INTEGER NOT NULL,
 expires_at_ms INTEGER NOT NULL CHECK (expires_at_ms > issued_at_ms),
 state TEXT NOT NULL CHECK (state IN ('pending','used','cancelled')),
 ended_at_ms INTEGER,
 CHECK ((state = 'pending') = (ended_at_ms IS NULL))
) STRICT, WITHOUT ROWID;
CREATE UNIQUE INDEX one_time_links_pending ON one_time_links(kind, subject) WHERE state = 'pending';
"#;

fn store(_: impl Sized) -> Error {
    Error::Store
}

impl OneTimeLinks for HostDb {
    fn instance_id(&self) -> &str {
        &self.instance_id
    }
    fn replace_pending(&self, link: NewLink<'_>) -> Result<(), Error> {
        let mut db = self.connection.lock().map_err(store)?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store)?;
        tx.execute(
            "UPDATE one_time_links SET state='cancelled', ended_at_ms=?3 WHERE kind=?1 AND subject=?2 AND state='pending'",
            (link.kind.as_str(), link.subject.as_str(), link.issued_at_ms),
        )
        .map_err(store)?;
        tx.execute(
            "INSERT INTO one_time_links VALUES (?1,?2,?3,?4,?5,?6,'pending',NULL)",
            (
                link.link_id,
                link.kind.as_str(),
                link.subject.as_str(),
                &link.digest.0[..],
                link.issued_at_ms,
                link.expires_at_ms,
            ),
        )
        .map_err(store)?;
        tx.commit().map_err(store)
    }
    fn redeem(
        &self,
        kind: LinkKind,
        digest: LinkDigest,
        now_ms: i64,
    ) -> Result<Result<String, Refused>, Error> {
        let mut db = self.connection.lock().map_err(store)?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store)?;
        let row: Option<(String, String, i64)> = tx
            .query_row(
                "SELECT link_id,state,expires_at_ms FROM one_time_links WHERE kind=?1 AND digest=?2",
                (kind.as_str(), &digest.0[..]),
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(store)?;
        let Some((link_id, state, expires_at_ms)) = row else {
            return Ok(Err(Refused::Unknown));
        };
        if let Err(refused) = redeem_rule(&state, expires_at_ms, now_ms) {
            return Ok(Err(refused));
        }
        tx.execute(
            "UPDATE one_time_links SET state='used', ended_at_ms=?2 WHERE link_id=?1 AND state='pending'",
            (&link_id, now_ms),
        )
        .map_err(store)?;
        tx.commit().map_err(store)?;
        Ok(Ok(link_id))
    }
    fn pending_row(&self, kind: LinkKind, subject: &Subject) -> Result<Option<Pending>, Error> {
        let db = self.connection.lock().map_err(store)?;
        db.query_row(
            "SELECT link_id,issued_at_ms,expires_at_ms FROM one_time_links WHERE kind=?1 AND subject=?2 AND state='pending'",
            (kind.as_str(), subject.as_str()),
            |r| {
                Ok(Pending {
                    link_id: r.get(0)?,
                    issued_at_ms: r.get(1)?,
                    expires_at_ms: r.get(2)?,
                })
            },
        )
        .optional()
        .map_err(store)
    }
}

#[cfg(test)]
mod tests {
    use crate::hostdb::tests::fixture;

    #[test]
    fn the_partial_index_allows_one_pending_link_per_kind_and_subject() {
        let (_home, _secrets, db) = fixture();
        let c = db.connection.lock().unwrap();
        let insert = |id: &str, digest: u8, subject: &str, state: &str, ended: Option<i64>| {
            c.execute(
                "INSERT INTO one_time_links VALUES (?1,'registration',?2,?3,1,2,?4,?5)",
                (id, subject, vec![digest; 32], state, ended),
            )
        };
        let id = |n: u8| format!("0190a1b2-c3d4-7e5f-8a9b-{:012}", n);
        insert(&id(1), 1, "s", "pending", None).unwrap();
        assert!(insert(&id(2), 2, "s", "pending", None).is_err());
        insert(&id(3), 3, "t", "pending", None).unwrap();
        insert(&id(4), 4, "s", "used", Some(2)).unwrap();
        insert(&id(5), 5, "s", "cancelled", Some(2)).unwrap();
        // Digest uniqueness, state values and the ended/pending pairing.
        assert!(insert(&id(6), 1, "u", "pending", None).is_err());
        assert!(insert(&id(7), 7, "u", "expired", None).is_err());
        assert!(insert(&id(8), 8, "u", "used", None).is_err());
        assert!(insert(&id(9), 9, "u", "pending", Some(2)).is_err());
        assert!(insert("short", 10, "u", "pending", None).is_err());
    }
}
