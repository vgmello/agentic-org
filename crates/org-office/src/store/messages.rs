//! `office.db`'s message tables and the delivery queries over them (ADR-0003 §1.1): the SQL behind
//! `org_mailbox::MessageStore`. Part of the store module, so SQL and rusqlite types stay in it
//! (ADR-0002 §5). Every function runs inside the writer's savepoint or on a reader connection.
//!
//! Tables: `msg_headers` (fixed-width, immutable once written), `msg_bodies` (the cold payload on
//! its own pages), `msg_state` (the current lifecycle state, upserted), `msg_unread` (the hot
//! index of open messages), `msg_threads` and `msg_thread_participants` (conversation summaries).
//! One counter (`meta.message_sequence`) allocates `local_seq` and, for a local message, its
//! `origin_seq`.
//!
//! **Bodies and meta are stored as given** (raw UTF-8 text and compact JSON, `compression = 0`):
//! the database is already encrypted, and a compressed form belongs with the sync work.

use std::collections::BTreeSet;

use org_mailbox::{
    parse_utc, CommitMessageOutcome, HeaderQuery, HeaderScope, NewStoredMessage, PeekUnreadOutcome,
    StoreError, StoredMessage,
};
use org_proto::msg::{
    is_urgent, HeaderPage, Message, MessageHeader, MessageLifecycleState as State, MessagePriority,
    KIND_DELIVERY_FAILED, OFFICE_SENDER,
};
use org_proto::Position;
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::error::OfficeError;
use crate::store::Store;

const SEQUENCE_KEY: &str = "message_sequence";
const INCARNATION_KEY: &str = "incarnation";
/// `msg_headers.flags`: an ask to a person whose deadline passed (it stays unread).
const FLAG_EXPIRED_WAITING_PERSON: i64 = 8;

/// Creates the message tables. A new `office.db` gets them with its schema; one made before them
/// gets them in the upgrade.
pub(crate) fn create_tables(tx: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE msg_threads (
             thread_id TEXT PRIMARY KEY, subject TEXT, created_at TEXT NOT NULL,
             last_msg_id TEXT NOT NULL, last_msg_at TEXT NOT NULL,
             message_count INTEGER NOT NULL, participants_json TEXT NOT NULL
         ) STRICT;
         CREATE INDEX idx_msg_threads_last ON msg_threads (last_msg_at DESC, thread_id DESC);
         CREATE TABLE msg_thread_participants (
             member_id TEXT NOT NULL,
             thread_id TEXT NOT NULL REFERENCES msg_threads (thread_id) ON DELETE CASCADE,
             PRIMARY KEY (member_id, thread_id)
         ) STRICT;
         CREATE TABLE msg_headers (
             id TEXT PRIMARY KEY, thread_id TEXT NOT NULL, reply_to TEXT,
             origin_office_id TEXT NOT NULL, origin_incarnation TEXT NOT NULL,
             origin_seq INTEGER NOT NULL, local_seq INTEGER NOT NULL UNIQUE,
             org_epoch INTEGER NOT NULL, org_seq INTEGER NOT NULL,
             from_member_id TEXT NOT NULL, from_display_name TEXT NOT NULL,
             to_member_id TEXT NOT NULL, to_display_name TEXT NOT NULL,
             kind TEXT NOT NULL, priority TEXT NOT NULL DEFAULT 'normal', subject TEXT,
             created_at TEXT NOT NULL, expires_at TEXT, ttl_ms INTEGER,
             body_bytes INTEGER NOT NULL, meta_bytes INTEGER NOT NULL DEFAULT 0,
             content_hash BLOB NOT NULL, flags INTEGER NOT NULL DEFAULT 0, dead_reason TEXT
         ) STRICT;
         CREATE UNIQUE INDEX idx_msg_origin
             ON msg_headers (origin_office_id, origin_incarnation, origin_seq);
         CREATE UNIQUE INDEX idx_msg_delivery_failed_once
             ON msg_headers (reply_to) WHERE kind = 'delivery_failed';
         CREATE INDEX idx_msg_to_seq ON msg_headers (to_member_id, local_seq DESC);
         CREATE INDEX idx_msg_thread_seq ON msg_headers (thread_id, local_seq DESC);
         CREATE INDEX idx_msg_reply_to ON msg_headers (reply_to) WHERE reply_to IS NOT NULL;
         CREATE TABLE msg_bodies (
             id TEXT PRIMARY KEY REFERENCES msg_headers (id) ON DELETE CASCADE,
             compression INTEGER NOT NULL, body_blob BLOB NOT NULL, meta_blob BLOB
         ) STRICT;
         CREATE TABLE msg_state (
             id TEXT PRIMARY KEY REFERENCES msg_headers (id) ON DELETE CASCADE,
             state TEXT NOT NULL, triage_gen INTEGER NOT NULL DEFAULT 0,
             snoozed_until TEXT, snooze_after_task INTEGER NOT NULL DEFAULT 0,
             reason TEXT, read_at TEXT, updated_at TEXT NOT NULL,
             owner_office_id TEXT NOT NULL, origin_incarnation TEXT NOT NULL,
             origin_seq INTEGER NOT NULL, org_epoch INTEGER NOT NULL, org_seq INTEGER NOT NULL
         ) STRICT;
         CREATE TABLE msg_unread (
             to_member_id TEXT NOT NULL, local_seq INTEGER NOT NULL,
             id TEXT NOT NULL UNIQUE REFERENCES msg_headers (id) ON DELETE CASCADE,
             from_member_id TEXT NOT NULL, kind TEXT NOT NULL,
             priority TEXT NOT NULL DEFAULT 'normal', urgent INTEGER NOT NULL DEFAULT 0,
             triage_gen INTEGER NOT NULL DEFAULT 0, reply_to TEXT, created_at TEXT NOT NULL,
             expires_at TEXT, snoozed_until TEXT,
             snooze_after_task INTEGER NOT NULL DEFAULT 0, snooze_note TEXT, triaged_at TEXT,
             PRIMARY KEY (to_member_id, local_seq)
         ) STRICT;
         CREATE INDEX idx_msg_unread_due ON msg_unread (to_member_id, snoozed_until)
             WHERE snoozed_until IS NOT NULL;
         CREATE INDEX idx_msg_unread_after_task ON msg_unread (to_member_id)
             WHERE snooze_after_task = 1;",
    )
}

fn storage(error: rusqlite::Error) -> OfficeError {
    OfficeError::storage(error)
}

type Outcome<T> = Result<Result<T, StoreError>, OfficeError>;

fn state_text(state: State) -> Option<&'static str> {
    Some(match state {
        State::Unread => "unread",
        State::Snoozed => "snoozed",
        State::Read => "read",
        State::Done => "done",
        State::Expired => "expired",
        State::DeadLetter => "dead_letter",
        _ => return None,
    })
}

fn state_of(text: &str) -> State {
    match text {
        "unread" => State::Unread,
        "snoozed" => State::Snoozed,
        "read" => State::Read,
        "done" => State::Done,
        "expired" => State::Expired,
        "dead_letter" => State::DeadLetter,
        _ => State::Unknown,
    }
}

fn priority_text(priority: MessagePriority) -> Option<&'static str> {
    Some(match priority {
        MessagePriority::Normal => "normal",
        MessagePriority::High => "high",
        MessagePriority::Urgent => "urgent",
        _ => return None,
    })
}

fn priority_of(text: &str) -> MessagePriority {
    match text {
        "normal" => MessagePriority::Normal,
        "high" => MessagePriority::High,
        "urgent" => MessagePriority::Urgent,
        _ => MessagePriority::Unknown,
    }
}

/// The columns of a header read, in the order [`stored_from`] reads them; `with_body` adds the
/// two payload columns after them.
const HEADER_COLUMNS: &str = "h.id, h.thread_id, h.reply_to, h.origin_office_id,
    h.origin_incarnation, h.origin_seq, h.local_seq, h.org_epoch, h.org_seq, h.from_member_id,
    h.from_display_name, h.to_member_id, h.to_display_name, h.kind, h.priority, h.subject,
    h.created_at, h.expires_at, h.ttl_ms, h.body_bytes, h.meta_bytes, h.content_hash, h.flags,
    s.state, s.triage_gen, s.snoozed_until, s.snooze_after_task, s.reason, s.read_at";

fn select_with_body() -> String {
    format!(
        "SELECT {HEADER_COLUMNS}, b.body_blob, b.meta_blob FROM msg_headers h
         JOIN msg_state s ON s.id = h.id LEFT JOIN msg_bodies b ON b.id = h.id"
    )
}

fn select_headers() -> String {
    format!("SELECT {HEADER_COLUMNS} FROM msg_headers h JOIN msg_state s ON s.id = h.id")
}

/// A stored message from a row of [`select_with_body`] (body and meta set) or
/// [`select_headers`] (empty body and meta).
fn stored_from(row: &Row<'_>, with_body: bool) -> rusqlite::Result<StoredMessage> {
    let text = |i: usize| row.get::<_, String>(i);
    let optional = |i: usize| row.get::<_, Option<String>>(i);
    let number = |i: usize| row.get::<_, i64>(i);
    let hash: Vec<u8> = row.get(21)?;
    let content_hash: [u8; 32] = hash.try_into().map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            21,
            rusqlite::types::Type::Blob,
            "a content hash is 32 bytes".into(),
        )
    })?;
    let kind = text(13)?;
    let priority = priority_of(&text(14)?);
    let (body, meta) = if with_body {
        let body: Vec<u8> = row.get(29)?;
        let meta: Option<Vec<u8>> = row.get(30)?;
        (
            String::from_utf8_lossy(&body).into_owned(),
            meta.and_then(|json| serde_json::from_slice(&json).ok())
                .unwrap_or_default(),
        )
    } else {
        (String::new(), Default::default())
    };
    Ok(StoredMessage {
        message: Message {
            header: MessageHeader {
                id: text(0)?,
                thread_id: text(1)?,
                reply_to: optional(2)?,
                from_member_id: text(9)?,
                from_display_name: text(10)?,
                to_member_id: text(11)?,
                to_display_name: text(12)?,
                urgent: is_urgent(&kind, priority),
                kind,
                priority,
                subject: optional(15)?,
                created_at: text(16)?,
                expires_at: optional(17)?,
                ttl_ms: row.get::<_, Option<i64>>(18)?.map(|n| n as u64),
                body_bytes: number(19)? as u64,
                meta_bytes: number(20)? as u64,
                local_seq: number(6)? as u64,
                state: state_of(&text(23)?),
                triage_gen: number(24)? as u64,
                snoozed_until: optional(25)?,
                snooze_after_task: number(26)? != 0,
                snooze_note: optional(27)?,
                read_at: optional(28)?,
                expired_waiting_person: number(22)? & FLAG_EXPIRED_WAITING_PERSON != 0,
            },
            body,
            meta,
        },
        origin_office_id: text(3)?,
        origin_incarnation: text(4)?,
        origin_seq: number(5)? as u64,
        org_position: Position {
            epoch: number(7)? as u64,
            seq: number(8)? as u64,
        },
        content_hash,
    })
}

fn load(db: &Connection, id: &str) -> Result<Option<StoredMessage>, OfficeError> {
    db.query_row(
        &format!("{} WHERE h.id = ?1", select_with_body()),
        [id],
        |row| stored_from(row, true),
    )
    .optional()
    .map_err(storage)
}

/// Past its deadline plus the bounded skew grace (a quarter of the ttl, at most 2 s). An ask to
/// a person is never expired: it waits for them.
fn expired(header: &MessageHeader, now: &str) -> bool {
    if header.expired_waiting_person {
        return false;
    }
    let Some(expires_at) = header.expires_at.as_deref() else {
        return false;
    };
    let (Some(deadline), Some(now)) = (parse_utc(expires_at), parse_utc(now)) else {
        return true;
    };
    now >= deadline.saturating_add((header.ttl_ms.unwrap_or(0) / 4).min(2000))
}

/// Runs `work` in `store`'s writer savepoint (the office's `office.db`). `work` may run again if
/// SQLite aborts the batch, so it takes what it needs by clone. The inner result is the port's
/// answer; an outer error is a storage failure.
pub(crate) fn write<R: Send + 'static>(
    store: &Store,
    mut work: impl FnMut(&Connection) -> Result<Result<R, StoreError>, OfficeError> + Send + 'static,
) -> Result<Result<R, StoreError>, OfficeError> {
    store.write(move |db| Ok((work(db)?, None)))
}

/// This office.db's incarnation: a ULID made the first time it is asked for and kept in `meta`.
/// A message's origin is (office, incarnation, origin_seq). The value lives inside the file, so a
/// restore from a backup brings the old one back: whoever restores must make a new one (that
/// is the restore tool's job, not this function's). Runs on the writer.
pub(crate) fn incarnation(db: &Connection) -> Result<String, OfficeError> {
    let existing: Option<String> = db
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            [INCARNATION_KEY],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage)?;
    if let Some(existing) = existing {
        return Ok(existing);
    }
    let made = crate::ulid::generate().map_err(OfficeError::storage)?;
    db.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)",
        (INCARNATION_KEY, &made),
    )
    .map_err(storage)?;
    Ok(made)
}

/// The next `local_seq` (>= 1, increasing in commit order).
fn next_sequence(db: &Connection) -> Result<u64, OfficeError> {
    let current: Option<String> = db
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            [SEQUENCE_KEY],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage)?;
    let current = match current {
        Some(text) => text
            .parse::<u64>()
            .map_err(|_| OfficeError::Invalid("the message sequence is not a number"))?,
        None => 0,
    };
    let next = current
        .checked_add(1)
        .ok_or(OfficeError::Invalid("the message sequence is exhausted"))?;
    db.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        (SEQUENCE_KEY, next.to_string()),
    )
    .map_err(storage)?;
    Ok(next)
}

/// Atomic insert, or a matching replay (`inserted: false`) of the same id (or, for a message from
/// another office, the same origin). The same id with another sender or content is
/// `AlreadyExists`. Nothing is written before every check has passed.
pub(crate) fn commit_message(
    db: &Connection,
    mut new: NewStoredMessage,
) -> Outcome<CommitMessageOutcome> {
    let header = &new.message.header;
    let existing: Option<(Vec<u8>, String)> = db
        .query_row(
            "SELECT content_hash, from_member_id FROM msg_headers WHERE id = ?1",
            [&header.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(storage)?;
    if let Some((hash, from)) = existing {
        if hash != new.content_hash || from != header.from_member_id {
            return Ok(Err(StoreError::AlreadyExists));
        }
        let message =
            load(db, &header.id)?.ok_or(OfficeError::Invalid("a message went missing"))?;
        return Ok(Ok(CommitMessageOutcome {
            message,
            inserted: false,
        }));
    }
    let (Some(state), Some(priority)) = (
        state_text(new.initial_state)
            .filter(|_| matches!(new.initial_state, State::Unread | State::Read)),
        priority_text(header.priority),
    ) else {
        return Ok(Err(StoreError::InvalidTransition));
    };
    if new.origin_seq == Some(0) {
        return Ok(Err(StoreError::InvalidTransition));
    }
    if let Some(origin_seq) = new.origin_seq {
        let same_origin: Option<String> = db
            .query_row(
                "SELECT id FROM msg_headers
                 WHERE origin_office_id = ?1 AND origin_incarnation = ?2 AND origin_seq = ?3",
                (
                    &new.origin_office_id,
                    &new.origin_incarnation,
                    origin_seq as i64,
                ),
                |row| row.get(0),
            )
            .optional()
            .map_err(storage)?;
        if let Some(id) = same_origin {
            let message = load(db, &id)?.ok_or(OfficeError::Invalid("a message went missing"))?;
            return Ok(Ok(CommitMessageOutcome {
                message,
                inserted: false,
            }));
        }
    }

    let sequence = next_sequence(db)?;
    // A local message's origin sequence is its local one. That cannot clash with an explicit
    // `origin_seq` unless a message of this very office and incarnation is ingested with one
    // at or above the local counter; sync must give such messages a new incarnation.
    let origin_seq = new.origin_seq.unwrap_or(sequence);
    let header = &mut new.message.header;
    header.local_seq = sequence;
    header.state = new.initial_state;
    header.triage_gen = 0;
    header.snoozed_until = None;
    header.snooze_after_task = false;
    header.snooze_note = None;
    header.read_at = (new.initial_state == State::Read).then(|| header.created_at.clone());
    let header = &new.message.header;
    let flags = if header.expired_waiting_person {
        FLAG_EXPIRED_WAITING_PERSON
    } else {
        0
    };
    let meta_json = (!new.message.meta.is_empty())
        .then(|| serde_json::to_vec(&new.message.meta))
        .transpose()
        .map_err(OfficeError::storage)?;
    db.execute(
        "INSERT INTO msg_headers (id, thread_id, reply_to, origin_office_id, origin_incarnation,
             origin_seq, local_seq, org_epoch, org_seq, from_member_id, from_display_name,
             to_member_id, to_display_name, kind, priority, subject, created_at, expires_at, ttl_ms,
             body_bytes, meta_bytes, content_hash, flags)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23)",
        params![
            header.id,
            header.thread_id,
            header.reply_to,
            new.origin_office_id,
            new.origin_incarnation,
            origin_seq as i64,
            sequence as i64,
            new.org_position.epoch as i64,
            new.org_position.seq as i64,
            header.from_member_id,
            header.from_display_name,
            header.to_member_id,
            header.to_display_name,
            header.kind,
            priority,
            header.subject,
            header.created_at,
            header.expires_at,
            header.ttl_ms.map(|n| n as i64),
            header.body_bytes as i64,
            header.meta_bytes as i64,
            new.content_hash.to_vec(),
            flags,
        ],
    )
    .map_err(storage)?;
    db.execute(
        "INSERT INTO msg_bodies (id, compression, body_blob, meta_blob) VALUES (?1, 0, ?2, ?3)",
        params![header.id, new.message.body.as_bytes(), meta_json],
    )
    .map_err(storage)?;
    db.execute(
        "INSERT INTO msg_state (id, state, triage_gen, snoozed_until, snooze_after_task, reason,
             read_at, updated_at, owner_office_id, origin_incarnation, origin_seq, org_epoch, org_seq)
         VALUES (?1, ?2, 0, NULL, 0, NULL, ?3, ?4, ?5, ?6, 0, ?7, ?8)",
        params![
            header.id,
            state,
            header.read_at,
            header.created_at,
            new.origin_office_id,
            new.origin_incarnation,
            new.org_position.epoch as i64,
            new.org_position.seq as i64,
        ],
    )
    .map_err(storage)?;
    // An initially read message never enters the unread index.
    if new.initial_state == State::Unread {
        db.execute(
            "INSERT INTO msg_unread (to_member_id, local_seq, id, from_member_id, kind, priority,
                 urgent, triage_gen, reply_to, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9, ?10)",
            params![
                header.to_member_id,
                sequence as i64,
                header.id,
                header.from_member_id,
                header.kind,
                priority,
                header.urgent,
                header.reply_to,
                header.created_at,
                header.expires_at,
            ],
        )
        .map_err(storage)?;
    }
    upsert_thread(db, header)?;

    let stored = StoredMessage {
        message: new.message,
        origin_office_id: new.origin_office_id,
        origin_incarnation: new.origin_incarnation,
        origin_seq,
        org_position: new.org_position,
        content_hash: new.content_hash,
    };
    Ok(Ok(CommitMessageOutcome {
        message: stored,
        inserted: true,
    }))
}

/// Creates or updates the message's thread summary: the count goes up for a new header only, the
/// earliest `created_at` and the latest `(created_at, id)` win whatever order messages arrive in,
/// and the subject is the root message's (a first subject only fills an empty one).
fn upsert_thread(db: &Connection, header: &MessageHeader) -> Result<(), OfficeError> {
    let existing: Option<(Option<String>, String, String, String, String)> = db
        .query_row(
            "SELECT subject, created_at, last_msg_id, last_msg_at, participants_json
             FROM msg_threads WHERE thread_id = ?1",
            [&header.thread_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()
        .map_err(storage)?;
    let mut participants: BTreeSet<String> = BTreeSet::new();
    participants.insert(header.from_member_id.clone());
    participants.insert(header.to_member_id.clone());
    match existing {
        None => {
            db.execute(
                "INSERT INTO msg_threads (thread_id, subject, created_at, last_msg_id, last_msg_at,
                     message_count, participants_json) VALUES (?1, ?2, ?3, ?4, ?3, 1, ?5)",
                params![
                    header.thread_id,
                    header.subject,
                    header.created_at,
                    header.id,
                    serde_json::to_string(&participants).map_err(OfficeError::storage)?,
                ],
            )
            .map_err(storage)?;
        }
        Some((subject, created_at, last_id, last_at, people)) => {
            participants.extend(serde_json::from_str::<Vec<String>>(&people).unwrap_or_default());
            let newer = (header.created_at.as_str(), header.id.as_str())
                > (last_at.as_str(), last_id.as_str());
            let subject = if header.id == header.thread_id || subject.is_none() {
                header.subject.clone().or(subject)
            } else {
                subject
            };
            db.execute(
                "UPDATE msg_threads SET subject = ?2, created_at = ?3, last_msg_id = ?4,
                     last_msg_at = ?5, message_count = message_count + 1, participants_json = ?6
                 WHERE thread_id = ?1",
                params![
                    header.thread_id,
                    subject,
                    created_at.min(header.created_at.clone()),
                    if newer { header.id.clone() } else { last_id },
                    if newer {
                        header.created_at.clone()
                    } else {
                        last_at
                    },
                    serde_json::to_string(&participants).map_err(OfficeError::storage)?,
                ],
            )
            .map_err(storage)?;
        }
    }
    for member in &participants {
        db.execute(
            "INSERT OR IGNORE INTO msg_thread_participants (member_id, thread_id) VALUES (?1, ?2)",
            (member, &header.thread_id),
        )
        .map_err(storage)?;
    }
    Ok(())
}

/// Atomically claims and marks read `id` (if it is addressed to `member`), or with no `id` the
/// oldest open, unexpired unread message. An already read message comes back unchanged.
pub(crate) fn claim_read(
    db: &Connection,
    member: &str,
    id: Option<&str>,
    now: &str,
) -> Outcome<Option<StoredMessage>> {
    let id = match id {
        Some(id) => db
            .query_row(
                "SELECT id FROM msg_headers WHERE id = ?1 AND to_member_id = ?2",
                (id, member),
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(storage)?,
        None => first_unexpired_open(db, member, now)?,
    };
    let Some(id) = id else { return Ok(Ok(None)) };
    let Some(mut stored) = load(db, &id)? else {
        return Ok(Err(StoreError::NotFound));
    };
    if matches!(stored.message.header.state, State::Unread | State::Snoozed) {
        db.execute(
            "UPDATE msg_state SET state = 'read', snoozed_until = NULL, snooze_after_task = 0,
                 reason = NULL, read_at = ?2, updated_at = ?2 WHERE id = ?1",
            (&id, now),
        )
        .map_err(storage)?;
        db.execute("DELETE FROM msg_unread WHERE id = ?1", [&id])
            .map_err(storage)?;
        let header = &mut stored.message.header;
        header.state = State::Read;
        header.snoozed_until = None;
        header.snooze_after_task = false;
        header.snooze_note = None;
        header.read_at = Some(now.to_owned());
    }
    Ok(Ok(Some(stored)))
}

/// The open (unread, not snoozed) messages for `member`, oldest first, from the hot index.
/// Headers only (empty body and meta): a flood of big unread bodies must not be read to pick one.
fn open_unread(
    db: &Connection,
    member: &str,
    from: Option<&str>,
    urgent_only: bool,
) -> Result<Vec<StoredMessage>, OfficeError> {
    let mut statement = db.prepare(&open_unread_sql()).map_err(storage)?;
    let rows = statement
        .query_map(params![member, from, urgent_only], |row| {
            stored_from(row, false)
        })
        .map_err(storage)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(storage)
}

/// The id of the oldest open message that is not expired. It stops at the first one, so a claim
/// (which runs on the writer) does not read a whole backlog.
fn first_unexpired_open(
    db: &Connection,
    member: &str,
    now: &str,
) -> Result<Option<String>, OfficeError> {
    let mut statement = db.prepare(&open_unread_sql()).map_err(storage)?;
    let mut rows = statement
        .query(params![member, None::<&str>, false])
        .map_err(storage)?;
    while let Some(row) = rows.next().map_err(storage)? {
        let header = stored_from(row, false).map_err(storage)?.message.header;
        if !expired(&header, now) {
            return Ok(Some(header.id));
        }
    }
    Ok(None)
}

/// `?1` member, `?2` sender or NULL, `?3` urgent only; oldest first.
fn open_unread_sql() -> String {
    format!(
        "{} JOIN msg_unread u ON u.id = h.id
         WHERE u.to_member_id = ?1 AND s.state = 'unread'
           AND u.snoozed_until IS NULL AND u.snooze_after_task = 0
           AND (?2 IS NULL OR h.from_member_id = ?2)
           AND (?3 = 0 OR u.urgent = 1)
         ORDER BY u.local_seq",
        select_headers()
    )
}

/// The oldest unexpired open message, without marking it read, plus the ids of the expired ones
/// passed over. Read-only.
pub(crate) fn peek_next_unread(
    db: &Connection,
    member: &str,
    from: Option<&str>,
    urgent_only: bool,
    now: &str,
) -> Result<PeekUnreadOutcome, OfficeError> {
    let open = open_unread(db, member, from, urgent_only)?;
    let expired_ids = open
        .iter()
        .filter(|stored| expired(&stored.message.header, now))
        .map(|stored| stored.message.header.id.clone())
        .collect();
    let first = open
        .into_iter()
        .find(|stored| !expired(&stored.message.header, now));
    let message = match first {
        Some(stored) => load(db, &stored.message.header.id)?,
        None => None,
    };
    Ok(PeekUnreadOutcome {
        message,
        expired_ids,
    })
}

pub(crate) fn get_message(db: &Connection, id: &str) -> Result<Option<StoredMessage>, OfficeError> {
    load(db, id)
}

/// The page query for `scope`. The scope and the thread are separate conditions (not bound
/// switches) so that SQLite can answer an inbox from `idx_msg_to_seq` and a thread from
/// `idx_msg_thread_seq` instead of walking every header.
pub(crate) fn headers_sql(scope: &HeaderScope, by_thread: bool) -> String {
    let scope = match scope {
        HeaderScope::Inbox(_) => "h.to_member_id = ?1",
        HeaderScope::Participant(_) => "(h.to_member_id = ?1 OR h.from_member_id = ?1)",
        HeaderScope::All => "?1 IS NULL",
    };
    let thread = if by_thread {
        "h.thread_id = ?3"
    } else {
        "?3 IS NULL"
    };
    format!(
        "{} WHERE {scope}
           AND (?2 IS NULL OR h.from_member_id = ?2)
           AND {thread}
           AND h.local_seq < ?4
           AND (?5 = 1 OR NOT (s.state = 'snoozed' OR s.snoozed_until IS NOT NULL
                               OR s.snooze_after_task = 1))
           AND (?6 = 0 OR s.state = 'unread' OR (?5 = 1 AND s.state = 'snoozed'))
         ORDER BY h.local_seq DESC LIMIT ?7",
        select_headers()
    )
}

/// One page of headers, newest first by `local_seq`, without reading bodies.
pub(crate) fn list_headers(
    db: &Connection,
    query: &HeaderQuery,
) -> Result<HeaderPage, OfficeError> {
    let member = match &query.scope {
        HeaderScope::Inbox(member) | HeaderScope::Participant(member) => Some(member.as_str()),
        HeaderScope::All => None,
    };
    let cursor = query
        .cursor
        .map_or(i64::MAX, |c| i64::try_from(c).unwrap_or(i64::MAX));
    let limit = i64::from(query.limit);
    let mut statement = db
        .prepare(&headers_sql(&query.scope, query.thread_id.is_some()))
        .map_err(storage)?;
    let rows = statement
        .query_map(
            params![
                member,
                query.from_member_id,
                query.thread_id,
                cursor,
                query.include_snoozed,
                query.unread_only,
                limit + 1,
            ],
            |row| stored_from(row, false),
        )
        .map_err(storage)?;
    let mut headers: Vec<MessageHeader> = rows
        .map(|row| row.map(|stored| stored.message.header))
        .collect::<rusqlite::Result<_>>()
        .map_err(storage)?;
    let more = headers.len() > query.limit as usize;
    headers.truncate(query.limit as usize);
    let next_cursor = if more {
        headers.last().map(|header| header.local_seq)
    } else {
        None
    };
    Ok(HeaderPage {
        headers,
        next_cursor,
    })
}

/// The first reply to the ask `ask` from `responder` (or the office's own `delivery_failed`
/// notice), addressed to whoever asked.
pub(crate) fn find_reply_for_ask(
    db: &Connection,
    ask: &str,
    responder: &str,
) -> Result<Option<StoredMessage>, OfficeError> {
    let asker: Option<String> = db
        .query_row(
            "SELECT from_member_id FROM msg_headers WHERE id = ?1",
            [ask],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage)?;
    let Some(asker) = asker else { return Ok(None) };
    let id: Option<String> = db
        .query_row(
            "SELECT id FROM msg_headers
             WHERE reply_to = ?1 AND to_member_id = ?2
               AND (from_member_id = ?3 OR (from_member_id = ?4 AND kind = ?5))
             ORDER BY local_seq LIMIT 1",
            params![ask, asker, responder, OFFICE_SENDER, KIND_DELIVERY_FAILED],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage)?;
    match id {
        Some(id) => load(db, &id),
        None => Ok(None),
    }
}
