//! Independent status-line and current-task halves of each member's local runtime row.
use rusqlite::Connection;

use crate::error::OfficeError;

/// A member's local status line and current task, independently optional.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StatusLine {
    pub member_id: String,
    pub status: Option<(String, String)>,
    pub current_task_id: Option<String>,
}

pub(crate) fn create_table(tx: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE member_status (
             member_id TEXT PRIMARY KEY, status_text TEXT, current_task_id TEXT,
             updated_at TEXT NOT NULL, expires_at TEXT,
             CHECK((status_text IS NULL) = (expires_at IS NULL)),
             CHECK(status_text IS NOT NULL OR current_task_id IS NOT NULL)
         ) STRICT;",
    )
}

fn storage(error: rusqlite::Error) -> OfficeError {
    OfficeError::storage(error)
}

/// Sets `member_id`'s line, or clears it with `None`, and clears every line that lapsed by
/// `now` (all times `YYYY-MM-DDTHH:MM:SS.mmmZ`, so they order as text). Runs on the writer.
pub(crate) fn write(
    db: &Connection,
    member_id: &str,
    line: Option<(&str, &str)>,
    now: &str,
) -> Result<(), OfficeError> {
    sweep(db, now)?;
    match line {
        Some((text, expires_at)) => db.execute(
            "INSERT INTO member_status (member_id, status_text, updated_at, expires_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(member_id) DO UPDATE SET status_text = excluded.status_text,
                 updated_at = excluded.updated_at, expires_at = excluded.expires_at",
            (member_id, text, now, expires_at),
        ),
        None => db.execute(
            "DELETE FROM member_status WHERE member_id = ?1 AND current_task_id IS NULL",
            [member_id],
        ),
    }
    .map_err(storage)?;
    if line.is_none() {
        db.execute("UPDATE member_status SET status_text=NULL, expires_at=NULL, updated_at=?2 WHERE member_id=?1", (member_id, now)).map_err(storage)?;
    }
    Ok(())
}

/// Every stored line (lapsed ones included: the reader compares instants).
pub(crate) fn all(db: &Connection) -> Result<Vec<StatusLine>, OfficeError> {
    let mut statement = db
        .prepare("SELECT member_id, status_text, expires_at, current_task_id FROM member_status")
        .map_err(storage)?;
    let rows = statement
        .query_map([], |row| {
            Ok(StatusLine {
                member_id: row.get(0)?,
                status: row.get::<_, Option<String>>(1)?.zip(row.get(2)?),
                current_task_id: row.get(3)?,
            })
        })
        .map_err(storage)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(storage)
}

/// Rebuild from the released v3-v5 shape using exactly the fresh table definition.
pub(crate) fn upgrade(tx: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch("ALTER TABLE member_status RENAME TO member_status_v5;")?;
    create_table(tx)?;
    tx.execute_batch(
        "INSERT INTO member_status (member_id,status_text,current_task_id,updated_at,expires_at)
        SELECT member_id,status_text,current_task_id,updated_at,expires_at FROM member_status_v5;
        DROP TABLE member_status_v5;",
    )
}

fn sweep(db: &Connection, now: &str) -> Result<(), OfficeError> {
    db.execute(
        "DELETE FROM member_status WHERE expires_at <= ?1 AND current_task_id IS NULL",
        [now],
    )
    .map_err(storage)?;
    db.execute(
        "UPDATE member_status SET status_text=NULL, expires_at=NULL WHERE expires_at <= ?1",
        [now],
    )
    .map_err(storage)?;
    Ok(())
}

pub(crate) fn write_current(
    db: &Connection,
    member: &str,
    item: Option<&str>,
    now: &str,
) -> Result<(), OfficeError> {
    sweep(db, now)?;
    match item {
        Some(item) => {
            db.execute("INSERT INTO member_status (member_id,current_task_id,updated_at) VALUES (?1,?2,?3)
            ON CONFLICT(member_id) DO UPDATE SET current_task_id=excluded.current_task_id, updated_at=excluded.updated_at", (member,item,now)).map_err(storage)?;
        }
        None => {
            db.execute(
                "DELETE FROM member_status WHERE member_id=?1 AND status_text IS NULL",
                [member],
            )
            .map_err(storage)?;
            db.execute(
                "UPDATE member_status SET current_task_id=NULL, updated_at=?2 WHERE member_id=?1",
                (member, now),
            )
            .map_err(storage)?;
        }
    }
    Ok(())
}

/// The stored task is advisory; validate against the same transaction as the row read.
pub(crate) fn checked_current(
    db: &Connection,
    line: &StatusLine,
) -> Result<Option<String>, OfficeError> {
    let Some(id) = &line.current_task_id else {
        return Ok(None);
    };
    Ok(super::work::get_in(db, id)?
        .filter(|item| {
            !item.deleted
                && item.kind == org_proto::work::ItemKind::Task
                && item.assignee_id.as_deref() == Some(line.member_id.as_str())
        })
        .map(|item| item.id))
}

/// Reads a member's status and checked task in the caller's local transaction.
pub(crate) fn projection(
    db: &Connection,
    member: &str,
) -> Result<(org_proto::directory::StatusSetResult, Option<String>), OfficeError> {
    let line = all(db)?.into_iter().find(|line| line.member_id == member);
    let current = line
        .as_ref()
        .map(|line| checked_current(db, line))
        .transpose()?
        .flatten();
    let (status, expires_at) = line.and_then(|line| line.status).unzip();
    Ok((
        org_proto::directory::StatusSetResult { status, expires_at },
        current,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Opening, Store};
    use crate::{office::OFFICE_SCHEMA, Key};
    const EARLY: &str = "2026-11-11T10:00:00.000Z";
    const EXPIRY: &str = "2026-11-11T11:00:00.000Z";
    const LATER: &str = "2026-11-11T12:00:00.000Z";
    fn open(dir: &tempfile::TempDir, opening: Opening) -> Store {
        Store::open(
            &dir.path().join("office.db"),
            &Key::from_bytes([42; 32]),
            opening,
            &OFFICE_SCHEMA,
        )
        .unwrap()
    }
    fn edit(store: &Store, f: impl Fn(&Connection) -> Result<(), OfficeError> + Send + 'static) {
        store
            .write(move |db| {
                f(db)?;
                Ok(((), None))
            })
            .unwrap();
    }
    fn at(store: &Store, member: &str) -> String {
        store
            .read(|db| {
                db.query_row(
                    "SELECT updated_at FROM member_status WHERE member_id=?1",
                    [member],
                    |r| r.get(0),
                )
                .map_err(storage)
            })
            .unwrap()
    }
    #[test]
    fn status_task_rows_preserve_the_other_half_sweep_in_order_and_keep_last_write_time() {
        let dir = tempfile::tempdir().unwrap();
        let store = open(&dir, Opening::New);
        edit(&store, |db| {
            write_current(db, "task", Some("item"), EARLY)?;
            write(db, "task", Some(("working", EXPIRY)), EARLY)?;
            write(db, "line", Some(("expires", EXPIRY)), EARLY)?;
            write(db, "future", Some(("stays", LATER)), EARLY)
        });
        edit(&store, |db| {
            write(db, "other", None, "2026-11-11T10:59:59.999Z")
        });
        assert_eq!(
            store.read(all).unwrap().len(),
            3,
            "one millisecond before expiry"
        );
        edit(&store, |db| write(db, "other", None, EXPIRY));
        let rows = store.read(all).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(!rows.iter().any(|row| row.member_id == "line"));
        let task = rows.iter().find(|row| row.member_id == "task").unwrap();
        assert_eq!(task.status, None);
        assert_eq!(task.current_task_id.as_deref(), Some("item"));
        assert_eq!(at(&store, "task"), EARLY, "sweep does not reset updated_at");
        assert_eq!(
            rows.iter()
                .find(|row| row.member_id == "future")
                .unwrap()
                .status
                .as_ref()
                .unwrap()
                .0,
            "stays"
        );
        edit(&store, |db| write(db, "task", None, EXPIRY));
        assert_eq!(
            at(&store, "task"),
            EXPIRY,
            "explicit status clear counts as a write"
        );
        edit(&store, |db| {
            write(db, "task", Some(("fresh", LATER)), EXPIRY)?;
            write_current(db, "task", None, EXPIRY)
        });
        let task = store
            .read(all)
            .unwrap()
            .into_iter()
            .find(|row| row.member_id == "task")
            .unwrap();
        assert_eq!(task.status, Some(("fresh".into(), LATER.into())));
        assert_eq!(task.current_task_id, None);
        edit(&store, |db| write(db, "task", None, EXPIRY));
        assert!(!store
            .read(all)
            .unwrap()
            .iter()
            .any(|row| row.member_id == "task"));
        edit(&store, |db| write_current(db, "future", None, LATER));
        assert!(
            store.read(all).unwrap().is_empty(),
            "task clear also sweeps lapsed status-only rows"
        );
    }
    #[test]
    fn each_explicit_current_set_or_clear_updates_the_timestamp_even_for_the_same_id() {
        let dir = tempfile::tempdir().unwrap();
        let store = open(&dir, Opening::New);
        edit(&store, |db| {
            write_current(db, "agent", Some("same"), EARLY)?;
            write(db, "agent", Some(("line", LATER)), EARLY)
        });
        edit(&store, |db| {
            write(db, "agent", Some(("changed", LATER)), EXPIRY)
        });
        assert_eq!(at(&store, "agent"), EXPIRY);
        edit(&store, |db| {
            write(db, "agent", Some(("line", LATER)), EARLY)
        });
        edit(&store, |db| {
            write_current(db, "agent", Some("same"), EXPIRY)
        });
        assert_eq!(at(&store, "agent"), EXPIRY);
        edit(&store, |db| write_current(db, "agent", Some("same"), EARLY));
        assert_eq!(at(&store, "agent"), EARLY);
        edit(&store, |db| write_current(db, "agent", None, EXPIRY));
        assert_eq!(at(&store, "agent"), EXPIRY);
        assert_eq!(
            store.read(all).unwrap()[0].status.as_ref().unwrap().0,
            "line"
        );
    }

    #[test]
    fn status_pair_and_nonempty_checks_reject_each_invalid_insert() {
        let dir = tempfile::tempdir().unwrap();
        let store = open(&dir, Opening::New);
        edit(&store, |db| {
            for (status, current, expiry) in [
                (Some("text"), Some("item"), None),
                (None, Some("item"), Some(EXPIRY)),
                (None, None, None),
            ] {
                let error = db
                    .execute(
                        "INSERT INTO member_status VALUES ('bad',?1,?2,?3,?4)",
                        (status, current, EARLY, expiry),
                    )
                    .unwrap_err();
                assert_eq!(
                    error.sqlite_error_code(),
                    Some(rusqlite::ErrorCode::ConstraintViolation)
                );
            }
            Ok(())
        });
        assert!(store.read(all).unwrap().is_empty());
    }
    #[test]
    fn status_task_conflict_replace_rollback_and_reopen_preserve_rows() {
        let dir = tempfile::tempdir().unwrap();
        let store = open(&dir, Opening::New);
        edit(&store, |db| write_current(db, "agent", Some("one"), EARLY));
        let before = store.read(all).unwrap();
        let failed: Result<(), OfficeError> = store.write(|db| {
            write(db, "agent", Some(("rolled back", LATER)), EARLY)?;
            write_current(db, "agent", Some("rolled back"), EXPIRY)?;
            Err(OfficeError::Invalid("rollback control"))
        });
        assert!(matches!(
            failed,
            Err(OfficeError::Invalid("rollback control"))
        ));
        assert_eq!(store.read(all).unwrap(), before);
        edit(&store, |db| {
            assert!(db
                .execute(
                    "INSERT INTO member_status VALUES ('agent',NULL,'conflict',?1,NULL)",
                    [EXPIRY]
                )
                .is_err());
            assert!(db
                .execute(
                    "INSERT OR REPLACE INTO member_status VALUES ('agent',NULL,NULL,?1,NULL)",
                    [EXPIRY]
                )
                .is_err());
            db.execute(
                "INSERT OR REPLACE INTO member_status VALUES ('agent',NULL,'two',?1,NULL)",
                [EXPIRY],
            )
            .map_err(storage)?;
            Ok(())
        });
        assert_eq!(at(&store, "agent"), EXPIRY);
        drop(store);
        let store = open(&dir, Opening::Existing);
        assert_eq!(
            store.read(all).unwrap()[0].current_task_id.as_deref(),
            Some("two")
        );
        edit(&store, |db| write_current(db, "agent", None, LATER));
        assert!(store.read(all).unwrap().is_empty());
    }
}
