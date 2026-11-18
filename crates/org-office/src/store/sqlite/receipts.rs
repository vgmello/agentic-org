//! The SQL of the store's own rows: the applied cursor, the hash of every source file and
//! the receipts. Shared by the writer's transaction and by a read snapshot.

use org_store::{
    Ack, Actor, Cursor, DurablePath, EngineError, Id, Outcome, Receipt, Sha256, Timestamp,
};
use rusqlite::{params, Connection, OptionalExtension, Row};

use super::failed;

/// A stored value that this code did not write.
fn foreign(what: &str) -> EngineError {
    EngineError::new(format!(
        "the projection holds {what} this office did not write"
    ))
}

fn whole(value: u64, what: &str) -> Result<i64, EngineError> {
    i64::try_from(value).map_err(|_| EngineError::new(format!("{what} is too large to store")))
}

pub(super) fn cursor(db: &Connection) -> Result<Option<Cursor>, EngineError> {
    let stored = db
        .prepare_cached("SELECT cursor FROM store_cursor WHERE id = 1")
        .and_then(|mut read| read.query_row([], |row| row.get::<_, String>(0)).optional())
        .map_err(|error| failed(db, "reading the cursor", &error))?;
    let Some(text) = stored else {
        return Ok(None);
    };
    let cursor = serde_json::from_str(&text).map_err(|_| foreign("a cursor"))?;
    Ok(Some(cursor))
}

pub(super) fn put_cursor(db: &Connection, cursor: &Cursor) -> Result<(), EngineError> {
    let text = serde_json::to_string(cursor).expect("a cursor is plain data");
    db.prepare_cached(
        "INSERT INTO store_cursor (id, cursor) VALUES (1, ?1) \
         ON CONFLICT (id) DO UPDATE SET cursor = excluded.cursor",
    )
    .and_then(|mut write| write.execute([text]))
    .map_err(|error| failed(db, "writing the cursor", &error))?;
    Ok(())
}

fn hash(bytes: Vec<u8>) -> Result<Sha256, EngineError> {
    let bytes: [u8; 32] = bytes.try_into().map_err(|_| foreign("a hash"))?;
    Ok(Sha256::from_bytes(bytes))
}

pub(super) fn source_hash(
    db: &Connection,
    path: &DurablePath,
) -> Result<Option<Sha256>, EngineError> {
    let stored = db
        .prepare_cached("SELECT sha256 FROM store_sources WHERE path = ?1")
        .and_then(|mut read| {
            read.query_row([path.as_str()], |row| row.get::<_, Vec<u8>>(0))
                .optional()
        })
        .map_err(|error| failed(db, "reading a source", &error))?;
    stored.map(hash).transpose()
}

/// The path itself, or anything below it by whole components. `0` is the byte after `/`,
/// and text compares by bytes, so the range is every path that starts with `prefix/`.
pub(super) fn sources_under(
    db: &Connection,
    prefix: &DurablePath,
) -> Result<Vec<DurablePath>, EngineError> {
    let paths = db
        .prepare_cached(
            "SELECT path FROM store_sources \
             WHERE path = ?1 OR (path >= ?1 || '/' AND path < ?1 || '0') ORDER BY path",
        )
        .and_then(|mut read| {
            let rows = read.query_map([prefix.as_str()], |row| row.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<String>>>()
        })
        .map_err(|error| failed(db, "listing sources", &error))?;
    let parse = |path: String| DurablePath::parse(&path).map_err(|_| foreign("a source path"));
    paths.into_iter().map(parse).collect()
}

pub(super) fn set_source(
    db: &Connection,
    path: &DurablePath,
    hash: Option<Sha256>,
) -> Result<(), EngineError> {
    let written = match hash {
        Some(hash) => db
            .prepare_cached(
                "INSERT INTO store_sources (path, sha256) VALUES (?1, ?2) \
                 ON CONFLICT (path) DO UPDATE SET sha256 = excluded.sha256",
            )
            .and_then(|mut write| write.execute(params![path.as_str(), &hash.as_bytes()[..]])),
        None => db
            .prepare_cached("DELETE FROM store_sources WHERE path = ?1")
            .and_then(|mut write| write.execute([path.as_str()])),
    };
    written.map_err(|error| failed(db, "writing a source", &error))?;
    Ok(())
}

const fn outcome_text(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Completed => "completed",
        Outcome::Rejected => "rejected",
    }
}

const fn ack_text(ack: Ack) -> &'static str {
    match ack {
        Ack::Committed => "committed",
        Ack::Durable => "durable",
    }
}

/// A plain insert: a second receipt for the same operation fails on the primary key, and
/// the statement alone is undone.
pub(super) fn put_receipt(db: &Connection, receipt: &Receipt) -> Result<(), EngineError> {
    let (actor_kind, actor_id) = match &receipt.actor {
        Actor::Employee(id) => ("employee", Some(id.as_str())),
        Actor::System => ("system", None),
    };
    db.prepare_cached(
        "INSERT INTO store_receipts (operation_id, kind, actor_kind, actor_id, fingerprint, \
         outcome, ack, ordinal, at, reply) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )
    .and_then(|mut write| {
        write.execute(params![
            receipt.id.as_str(),
            receipt.kind,
            actor_kind,
            actor_id,
            &receipt.fingerprint.as_bytes()[..],
            outcome_text(receipt.outcome),
            ack_text(receipt.ack),
            whole(receipt.ordinal, "an ordinal").map_err(unstorable)?,
            whole(receipt.at.0, "a time").map_err(unstorable)?,
            &receipt.reply[..],
        ])
    })
    .map_err(|error| failed(db, "recording a receipt", &error))?;
    Ok(())
}

/// Carries a refusal to store through `rusqlite`'s error type.
fn unstorable(error: EngineError) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(error.message.into())
}

/// The columns of `store_receipts`, in the order [`put_receipt`] writes them.
type ReceiptRow = (
    String,
    String,
    String,
    Option<String>,
    Vec<u8>,
    String,
    String,
    i64,
    i64,
    Vec<u8>,
);

fn receipt_row(row: &Row<'_>) -> rusqlite::Result<ReceiptRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
    ))
}

pub(super) fn receipt(db: &Connection, id: &Id) -> Result<Option<Receipt>, EngineError> {
    let stored = db
        .prepare_cached(
            "SELECT operation_id, kind, actor_kind, actor_id, fingerprint, outcome, ack, \
             ordinal, at, reply FROM store_receipts WHERE operation_id = ?1",
        )
        .and_then(|mut read| read.query_row([id.as_str()], receipt_row).optional())
        .map_err(|error| failed(db, "reading a receipt", &error))?;
    let Some((id, kind, actor_kind, actor_id, fingerprint, outcome, ack, ordinal, at, reply)) =
        stored
    else {
        return Ok(None);
    };
    let stored_id = |text: &str| Id::parse(text).map_err(|_| foreign("an ID"));
    let actor = match (actor_kind.as_str(), actor_id) {
        ("employee", Some(id)) => Actor::Employee(stored_id(&id)?),
        ("system", None) => Actor::System,
        _ => return Err(foreign("an actor")),
    };
    let outcome = match outcome.as_str() {
        "completed" => Outcome::Completed,
        "rejected" => Outcome::Rejected,
        _ => return Err(foreign("an outcome")),
    };
    let ack = match ack.as_str() {
        "committed" => Ack::Committed,
        "durable" => Ack::Durable,
        _ => return Err(foreign("an acknowledgement class")),
    };
    let unsigned = |value: i64| u64::try_from(value).map_err(|_| foreign("a negative number"));
    Ok(Some(Receipt {
        id: stored_id(&id)?,
        kind,
        actor,
        fingerprint: hash(fingerprint)?,
        outcome,
        ack,
        ordinal: unsigned(ordinal)?,
        at: Timestamp(unsigned(at)?),
        reply: reply.into(),
    }))
}
