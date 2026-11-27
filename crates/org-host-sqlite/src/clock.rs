//! One persistent nondecreasing clock for registry transactions.
use super::{Error, SqliteResultExt};
use rusqlite::{Connection, Transaction};

pub(super) fn now(db: &Connection, wall_ms: i64) -> Result<i64, Error> {
    let high: String = db
        .query_row(
            "SELECT value FROM meta WHERE key='clock_high_ms'",
            [],
            |r| r.get(0),
        )
        .db()?;
    let high: i64 = high.parse().map_err(|_| Error::Unsafe)?;
    if wall_ms < 0 || high < 0 {
        return Err(Error::Unsafe);
    }
    Ok(wall_ms.max(high))
}
/// Commit the served maximum with the decision; unchanged clocks produce no UPDATE.
pub(super) fn advance_clock(tx: &Transaction<'_>, now: i64) -> Result<(), Error> {
    tx.execute(
        "UPDATE meta SET value=?1 WHERE key='clock_high_ms' AND CAST(value AS INTEGER)<?1",
        [now],
    )
    .db()?;
    Ok(())
}
