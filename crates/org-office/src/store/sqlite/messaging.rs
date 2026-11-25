//! SQLite adapter for [`MessagingRead`] and [`MessagingWrite`] backed exclusively by the
//! isolated `0700`/`0600` database `runtime/<office-id>/protected/messaging.db` using
//! `docs/design/organization-data/messaging-schema.sql` (`doc-90`, `TASK-10`, `ENG-1`).
//!
//! No messaging table ever enters `office.db`.

use std::fs;
use std::path::{Path, PathBuf};

use org_store::{Cursor, EngineError, Id, Sha256};
use rusqlite::{params, types::Value, Connection, OptionalExtension, Row};

use crate::messaging::{
    ports::{MessagingRead, MessagingWrite},
    records::{
        auto_scope_str, conversation_kind_str, delivered_because_str, lifecycle_str, mute_mode_str,
        parse_auto_scope, parse_conversation_kind, parse_delivered_because, parse_lifecycle,
        parse_mute_mode, parse_priority, parse_reaction, priority_str, reaction_str,
        ConversationMembership, ConversationPin, ConversationPreferenceRecord,
        ConversationReadMarkerRecord, ConversationRecord, MessageAttachmentRecord,
        MessageReactionRecord, MessageRecord, ParticipantAnswerFile, RecipientStateRecord,
        SenderKind, SCHEMA_VERSION,
    },
};

use super::{
    connect, connect_reader, connect_writer, failed, open_writer,
    organization::{all, id, number, one, optional_id, optional_id_value, put, run, text, whole},
    refused, remove_database, SqliteSnapshot, SqliteTx,
};

/// Canonical messaging schema (`docs/design/organization-data/messaging-schema.sql`).
const MESSAGING_CONTRACT: &str =
    include_str!("../../../../../docs/design/organization-data/messaging-schema.sql");

/// Adapter-only metadata tables inside `protected/messaging.db` (never in `office.db`).
const MESSAGING_ADAPTER: &str = "
CREATE TABLE messaging_schema (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  digest TEXT NOT NULL
) STRICT;
CREATE TABLE messaging_cursor (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  cursor TEXT NOT NULL
) STRICT;
CREATE TABLE messaging_local_meta (
  organization_id TEXT NOT NULL,
  message_id TEXT NOT NULL,
  local_seq INTEGER NOT NULL CHECK (local_seq > 0),
  mailbox_content_hash TEXT,
  PRIMARY KEY (organization_id, message_id)
) STRICT;
CREATE INDEX messaging_local_meta_seq ON messaging_local_meta(organization_id, local_seq);
CREATE TABLE messaging_pins (
  organization_id TEXT NOT NULL,
  conversation_id TEXT NOT NULL,
  message_id TEXT NOT NULL,
  pinned_by TEXT NOT NULL,
  pinned_at TEXT NOT NULL,
  PRIMARY KEY (organization_id, conversation_id, message_id)
) STRICT;
CREATE TABLE messaging_answers (
  organization_id TEXT NOT NULL,
  conversation_id TEXT NOT NULL,
  message_id TEXT NOT NULL,
  employee_id TEXT NOT NULL,
  revision INTEGER NOT NULL CHECK (revision >= 1),
  accepted_by TEXT,
  accepted_at TEXT,
  reason TEXT,
  answer_json TEXT,
  PRIMARY KEY (organization_id, message_id, employee_id)
) STRICT;
";

const MESSAGING_ADAPTER_FORMAT: u32 = 1;

pub(super) fn messaging_db_path(office_db_path: &Path) -> PathBuf {
    let dir = office_db_path
        .parent()
        .expect("office.db sits inside <runtime>/<office-id>/");
    dir.join("protected").join("messaging.db")
}

pub(super) fn messaging_schema_digest() -> String {
    let fmt = MESSAGING_ADAPTER_FORMAT.to_be_bytes();
    let len1 = (MESSAGING_CONTRACT.len() as u64).to_be_bytes();
    let len2 = (MESSAGING_ADAPTER.len() as u64).to_be_bytes();
    Sha256::of_parts(&[
        b"messaging-adapter",
        &fmt,
        &len1,
        MESSAGING_CONTRACT.as_bytes(),
        &len2,
        MESSAGING_ADAPTER.as_bytes(),
    ])
    .to_hex()
}

pub(super) fn create_messaging_schema(db: &Connection) -> Result<(), EngineError> {
    let digest = messaging_schema_digest();
    let create = || -> rusqlite::Result<()> {
        db.execute_batch("BEGIN IMMEDIATE")?;
        db.execute_batch(MESSAGING_CONTRACT)?;
        db.execute_batch(MESSAGING_ADAPTER)?;
        db.execute(
            "INSERT INTO messaging_schema (id, digest) VALUES (1, ?1)",
            [digest],
        )?;
        db.execute_batch("COMMIT")
    };
    create().map_err(|error| failed(db, "creating the messaging schema", &error))
}

fn messaging_schema_matches(db: &Connection) -> Result<bool, EngineError> {
    let expected = messaging_schema_digest();
    let read = || -> rusqlite::Result<bool> {
        let count: i64 = db.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name = 'messaging_schema'",
            [],
            |row| row.get(0),
        )?;
        if count == 0 {
            return Ok(false);
        }
        let stored: Option<Option<String>> = db
            .query_row(
                "SELECT digest FROM messaging_schema WHERE id = 1",
                [],
                |row| Ok(row.get::<_, String>(0).ok()),
            )
            .optional()?;
        Ok(stored.flatten().is_some_and(|d| d == expected))
    };
    match read() {
        Err(error)
            if matches!(
                error.sqlite_error_code(),
                Some(rusqlite::ErrorCode::NotADatabase | rusqlite::ErrorCode::DatabaseCorrupt)
            ) =>
        {
            Ok(false)
        }
        other => other.map_err(|error| failed(db, "reading messaging schema digest", &error)),
    }
}

pub(super) fn ensure_protected_dir(msg_path: &Path) -> Result<(), EngineError> {
    let folder = msg_path.parent().expect("messaging.db sits in protected/");
    fs::create_dir_all(folder)
        .and_then(|()| org_platform::fs::set_owner_only_dir(folder))
        .map_err(|e| refused("creating protected messaging directory", e))
}

pub(super) fn protect_messaging_file(msg_path: &Path) -> Result<(), EngineError> {
    if msg_path.exists() {
        crate::private_fs::private_file(msg_path)
            .map_err(|e| refused("protecting messaging.db permissions", e))?;
    }
    Ok(())
}

/// Opens or recreates `protected/messaging.db`. Returns `(writer_conn, recreated)`.
pub(super) fn open_messaging_writer(msg_path: &Path) -> Result<(Connection, bool), EngineError> {
    ensure_protected_dir(msg_path)?;
    remove_database(&super::beside(msg_path, ".new"))?;
    let present = msg_path
        .try_exists()
        .map_err(|e| refused("finding messaging.db", e))?;
    if present {
        match open_writer(msg_path) {
            Ok(conn) if messaging_schema_matches(&conn)? => {
                protect_messaging_file(msg_path)?;
                return Ok((conn, false));
            }
            Ok(conn) => {
                let _ = conn.close();
            }
            Err(error) if error.says_no_database() => {}
            Err(error) => return Err(error.into()),
        }
    }
    remove_database(msg_path)?;
    let conn = connect_writer(msg_path)?;
    create_messaging_schema(&conn)?;
    protect_messaging_file(msg_path)?;
    Ok((conn, true))
}

pub(super) fn open_messaging_replacement(new_path: &Path) -> Result<Connection, EngineError> {
    ensure_protected_dir(new_path)?;
    let flags =
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_CREATE;
    let db = connect(new_path, flags)?;
    db.execute_batch(
        "PRAGMA journal_mode = DELETE; PRAGMA synchronous = FULL; \
         PRAGMA foreign_keys = ON; PRAGMA recursive_triggers = ON;",
    )
    .map_err(|error| refused("configuring messaging replacement", error))?;
    create_messaging_schema(&db)?;
    protect_messaging_file(new_path)?;
    Ok(db)
}

pub(super) fn open_messaging_reader(msg_path: &Path) -> Result<Connection, EngineError> {
    connect_reader(msg_path)
}

pub(super) fn put_messaging_cursor(db: &Connection, cursor: &Cursor) -> Result<(), EngineError> {
    let text = serde_json::to_string(cursor).expect("cursor serializes");
    db.prepare_cached(
        "INSERT INTO messaging_cursor (id, cursor) VALUES (1, ?1) \
         ON CONFLICT (id) DO UPDATE SET cursor = excluded.cursor",
    )
    .and_then(|mut w| w.execute([text]))
    .map_err(|e| failed(db, "writing messaging cursor", &e))?;
    Ok(())
}

pub(super) fn read_messaging_cursor(db: &Connection) -> Result<Option<Cursor>, EngineError> {
    let stored = db
        .prepare_cached("SELECT cursor FROM messaging_cursor WHERE id = 1")
        .and_then(|mut r| r.query_row([], |row| row.get::<_, String>(0)).optional())
        .map_err(|e| failed(db, "reading messaging cursor", &e))?;
    stored
        .map(|s| {
            serde_json::from_str(&s)
                .map_err(|_| EngineError::new("corrupt messaging cursor in messaging.db"))
        })
        .transpose()
}

// ---- Identity mirror helpers inside protected/messaging.db ----

fn ensure_org(db: &Connection, org_id: &str) -> Result<(), EngineError> {
    run(
        db,
        "INSERT OR IGNORE INTO messaging_organizations (id) VALUES (?1)",
        vec![text(org_id)],
    )
}

fn ensure_team(db: &Connection, org_id: &str, team_id: &str) -> Result<(), EngineError> {
    ensure_org(db, org_id)?;
    run(
        db,
        "INSERT OR IGNORE INTO messaging_teams (organization_id, id) VALUES (?1, ?2)",
        vec![text(org_id), text(team_id)],
    )
}

fn ensure_project(db: &Connection, org_id: &str, project_id: &str) -> Result<(), EngineError> {
    ensure_org(db, org_id)?;
    run(
        db,
        "INSERT OR IGNORE INTO messaging_projects (organization_id, id) VALUES (?1, ?2)",
        vec![text(org_id), text(project_id)],
    )
}

fn ensure_employee(db: &Connection, org_id: &str, employee_id: &str) -> Result<(), EngineError> {
    ensure_org(db, org_id)?;
    run(
        db,
        "INSERT OR IGNORE INTO messaging_employees (organization_id, id) VALUES (?1, ?2)",
        vec![text(org_id), text(employee_id)],
    )
}

fn ensure_office(db: &Connection, org_id: &str, office_id: &str) -> Result<(), EngineError> {
    ensure_org(db, org_id)?;
    run(
        db,
        "INSERT OR IGNORE INTO messaging_offices (organization_id, id) VALUES (?1, ?2)",
        vec![text(org_id), text(office_id)],
    )
}

fn ensure_auto_conversation_membership(
    db: &Connection,
    org_id: &str,
    conversation_id: &str,
    employee_id: &str,
    joined_at: &str,
) -> Result<(), EngineError> {
    run(
        db,
        "INSERT OR IGNORE INTO conversation_memberships \
           (organization_id, conversation_id, employee_id, joined_at, left_at) \
         SELECT organization_id, id, ?3, ?4, NULL \
         FROM conversations \
         WHERE organization_id = ?1 AND id = ?2 AND auto_scope IS NOT NULL",
        vec![
            text(org_id),
            text(conversation_id),
            text(employee_id),
            text(joined_at),
        ],
    )
}

// ---- Row decoders ----

trait MsgView {
    fn msg_view(&self) -> Result<(&Connection, &str), EngineError>;
}

fn unscoped() -> EngineError {
    EngineError::new("the projection was opened for no organization")
}

impl MsgView for SqliteTx {
    fn msg_view(&self) -> Result<(&Connection, &str), EngineError> {
        let org = self.organization.as_ref().ok_or_else(unscoped)?;
        Ok((self.msg_db(), org.as_str()))
    }
}

impl MsgView for SqliteSnapshot {
    fn msg_view(&self) -> Result<(&Connection, &str), EngineError> {
        let org = self.organization.as_ref().ok_or_else(unscoped)?;
        Ok((self.msg_db(), org.as_str()))
    }
}

fn foreign_col(col: usize, what: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(col, rusqlite::types::Type::Text, what.into())
}

fn conversation_row(row: &Row<'_>) -> rusqlite::Result<ConversationRecord> {
    let kind_raw: String = row.get(4)?;
    let kind = parse_conversation_kind(&kind_raw).map_err(|_| foreign_col(4, "kind"))?;
    let auto_raw: Option<String> = row.get(6)?;
    let auto_scope = auto_raw
        .as_deref()
        .map(parse_auto_scope)
        .transpose()
        .map_err(|_| foreign_col(6, "auto_scope"))?;
    let archived: i64 = row.get(9)?;
    Ok(ConversationRecord {
        organization_id: id(row, 0)?,
        id: id(row, 1)?,
        schema_version: number(row, 2)? as u32,
        revision: number(row, 3)?,
        kind,
        name: row.get(5)?,
        auto_scope,
        team_id: optional_id(row, 7)?,
        project_id: optional_id(row, 8)?,
        archived: archived != 0,
        created_at: row.get(10)?,
        memberships: Vec::new(),
        pins: Vec::new(),
    })
}

fn attach_conversation_children(
    db: &Connection,
    scope: &str,
    mut conv: ConversationRecord,
) -> Result<ConversationRecord, EngineError> {
    if conv.auto_scope.is_none() {
        conv.memberships = all(
            db,
            "SELECT employee_id, joined_at, left_at FROM conversation_memberships \
             WHERE organization_id = ?1 AND conversation_id = ?2 ORDER BY employee_id",
            vec![text(scope), text(conv.id.as_str())],
            |row| {
                Ok(ConversationMembership {
                    employee_id: id(row, 0)?,
                    joined_at: row.get(1)?,
                    left_at: row.get(2)?,
                })
            },
        )?;
    }
    conv.pins = all(
        db,
        "SELECT message_id, pinned_by, pinned_at FROM messaging_pins \
         WHERE organization_id = ?1 AND conversation_id = ?2 ORDER BY pinned_at, message_id",
        vec![text(scope), text(conv.id.as_str())],
        |row| {
            Ok(ConversationPin {
                message_id: row.get(0)?,
                pinned_by: id(row, 1)?,
                pinned_at: row.get(2)?,
            })
        },
    )?;
    Ok(conv)
}

fn message_row(row: &Row<'_>) -> rusqlite::Result<MessageRecord> {
    let sender_kind_raw: String = row.get(3)?;
    let sender_kind = match sender_kind_raw.as_str() {
        "employee" => SenderKind::Employee,
        "office" => SenderKind::Office,
        _ => return Err(foreign_col(3, "sender_kind")),
    };
    let priority_raw: String = row.get(8)?;
    let priority = parse_priority(&priority_raw).map_err(|_| foreign_col(8, "priority"))?;
    let meta_raw: String = row.get(11)?;
    let metadata = serde_json::from_str(&meta_raw).map_err(|_| foreign_col(11, "metadata_json"))?;
    let also: i64 = row.get(14)?;
    let ttl_opt: Option<i64> = row.get(17)?;
    let ttl_ms = ttl_opt
        .map(|v| u64::try_from(v).map_err(|_| foreign_col(17, "ttl_ms")))
        .transpose()?;
    Ok(MessageRecord {
        schema_version: SCHEMA_VERSION,
        organization_id: id(row, 0)?,
        id: row.get(1)?,
        conversation_id: id(row, 2)?,
        sender_kind,
        sender_employee_id: optional_id(row, 4)?,
        sender_office_id: optional_id(row, 5)?,
        sender_display_name: row.get(6)?,
        kind: row.get(7)?,
        priority,
        subject: row.get(9)?,
        body_markdown: row.get(10)?,
        metadata,
        thread_root_id: row.get(12)?,
        reply_to_message_id: row.get(13)?,
        also_in_channel: also != 0,
        created_at: row.get(15)?,
        expires_at: row.get(16)?,
        ttl_ms,
        origin_office_id: id(row, 18)?,
        origin_incarnation: row.get(19)?,
        origin_sequence: number(row, 20)?,
        org_epoch: number(row, 21)?,
        org_sequence: number(row, 22)?,
        content_sha256: row.get(23)?,
        attachments: Vec::new(),
        mailbox_content_hash: row.get(24)?,
    })
}

const MSG_SELECT: &str = "m.organization_id, m.id, m.conversation_id, m.sender_kind, \
    m.sender_employee_id, m.sender_office_id, m.sender_display_name, m.kind, m.priority, \
    m.subject, m.body_markdown, m.metadata_json, m.thread_root_id, m.reply_to_message_id, \
    m.also_in_channel, m.created_at, m.expires_at, m.ttl_ms, m.origin_office_id, \
    m.origin_incarnation, m.origin_sequence, m.org_epoch, m.org_sequence, m.content_sha256, \
    lm.mailbox_content_hash";

fn attach_attachments(
    db: &Connection,
    scope: &str,
    mut msg: MessageRecord,
) -> Result<MessageRecord, EngineError> {
    msg.attachments = all(
        db,
        "SELECT id, content_ref, media_type, byte_count, content_sha256 \
         FROM message_attachments WHERE organization_id = ?1 AND message_id = ?2 ORDER BY id",
        vec![text(scope), text(&msg.id)],
        |row| {
            Ok(MessageAttachmentRecord {
                id: row.get(0)?,
                content_ref: row.get(1)?,
                media_type: row.get(2)?,
                byte_count: number(row, 3)?,
                content_sha256: row.get(4)?,
            })
        },
    )?;
    Ok(msg)
}

fn recipient_state_row(row: &Row<'_>) -> rusqlite::Result<RecipientStateRecord> {
    let state_raw: String = row.get(5)?;
    let state = parse_lifecycle(&state_raw).map_err(|_| foreign_col(5, "state"))?;
    let snooze_after: i64 = row.get(8)?;
    let because_raw: String = row.get(14)?;
    let delivered_because =
        parse_delivered_because(&because_raw).map_err(|_| foreign_col(14, "delivered_because"))?;
    let related_raw: String = row.get(15)?;
    let related_message_ids: Vec<String> =
        serde_json::from_str(&related_raw).map_err(|_| foreign_col(15, "related"))?;
    let exp_person: i64 = row.get(16)?;
    Ok(RecipientStateRecord {
        organization_id: id(row, 0)?,
        message_id: row.get(1)?,
        conversation_id: id(row, 2)?,
        employee_id: id(row, 3)?,
        recipient_display_name: row.get(4)?,
        state,
        triage_generation: number(row, 6)?,
        snoozed_until: row.get(7)?,
        snooze_after_task: snooze_after != 0,
        snooze_note: row.get(9)?,
        triaged_at: row.get(10)?,
        read_at: row.get(11)?,
        updated_at: row.get(12)?,
        reason: row.get(13)?,
        delivered_because,
        related_message_ids,
        expired_waiting_person: exp_person != 0,
        owner_office_id: id(row, 17)?,
        origin_incarnation: row.get(18)?,
        origin_sequence: number(row, 19)?,
        org_epoch: number(row, 20)?,
        org_sequence: number(row, 21)?,
    })
}

const STATE_SELECT: &str = "organization_id, message_id, conversation_id, employee_id, \
    recipient_display_name, state, triage_generation, snoozed_until, snooze_after_task, \
    snooze_note, triaged_at, read_at, updated_at, reason, delivered_because, \
    related_message_ids_json, expired_waiting_person, owner_office_id, origin_incarnation, \
    origin_sequence, org_epoch, org_sequence";

macro_rules! sqlite_messaging_read {
    ($($view:ty),+) => {$(
        impl MessagingRead for $view {
            fn conversation(&self, conv_id: &Id) -> Result<Option<ConversationRecord>, EngineError> {
                let (db, scope) = self.msg_view()?;
                let Some(conv) = one(
                    db,
                    "SELECT organization_id, id, schema_version, revision, kind, name, auto_scope, \
                     team_id, project_id, archived, created_at FROM conversations \
                     WHERE organization_id = ?1 AND id = ?2",
                    vec![text(scope), text(conv_id.as_str())],
                    conversation_row,
                )? else {
                    return Ok(None);
                };
                attach_conversation_children(db, scope, conv).map(Some)
            }

            fn conversations(&self) -> Result<Vec<ConversationRecord>, EngineError> {
                let (db, scope) = self.msg_view()?;
                let rows = all(
                    db,
                    "SELECT organization_id, id, schema_version, revision, kind, name, auto_scope, \
                     team_id, project_id, archived, created_at FROM conversations \
                     WHERE organization_id = ?1 ORDER BY created_at, id",
                    vec![text(scope)],
                    conversation_row,
                )?;
                rows.into_iter()
                    .map(|c| attach_conversation_children(db, scope, c))
                    .collect()
            }

            fn message(&self, msg_id: &str) -> Result<Option<MessageRecord>, EngineError> {
                let (db, scope) = self.msg_view()?;
                let sql = format!(
                    "SELECT {MSG_SELECT} FROM messages m \
                     LEFT JOIN messaging_local_meta lm \
                       ON lm.organization_id = m.organization_id AND lm.message_id = m.id \
                     WHERE m.organization_id = ?1 AND m.id = ?2"
                );
                let Some(msg) = one(db, &sql, vec![text(scope), text(msg_id)], message_row)? else {
                    return Ok(None);
                };
                attach_attachments(db, scope, msg).map(Some)
            }

            fn messages_in_conversation(
                &self,
                conversation_id: &Id,
            ) -> Result<Vec<MessageRecord>, EngineError> {
                let (db, scope) = self.msg_view()?;
                let sql = format!(
                    "SELECT {MSG_SELECT} FROM messages m \
                     LEFT JOIN messaging_local_meta lm \
                       ON lm.organization_id = m.organization_id AND lm.message_id = m.id \
                     WHERE m.organization_id = ?1 AND m.conversation_id = ?2 \
                     ORDER BY m.created_at, COALESCE(lm.local_seq, 0), m.id"
                );
                let rows = all(
                    db,
                    &sql,
                    vec![text(scope), text(conversation_id.as_str())],
                    message_row,
                )?;
                rows.into_iter()
                    .map(|m| attach_attachments(db, scope, m))
                    .collect()
            }

            fn all_messages(&self) -> Result<Vec<MessageRecord>, EngineError> {
                let (db, scope) = self.msg_view()?;
                let sql = format!(
                    "SELECT {MSG_SELECT} FROM messages m \
                     LEFT JOIN messaging_local_meta lm \
                       ON lm.organization_id = m.organization_id AND lm.message_id = m.id \
                     WHERE m.organization_id = ?1 \
                     ORDER BY COALESCE(lm.local_seq, 0), m.created_at, m.id"
                );
                let rows = all(db, &sql, vec![text(scope)], message_row)?;
                rows.into_iter()
                    .map(|m| attach_attachments(db, scope, m))
                    .collect()
            }

            fn recipient_state(
                &self,
                message_id: &str,
                employee_id: &Id,
            ) -> Result<Option<RecipientStateRecord>, EngineError> {
                let (db, scope) = self.msg_view()?;
                let sql = format!(
                    "SELECT {STATE_SELECT} FROM message_recipient_states \
                     WHERE organization_id = ?1 AND message_id = ?2 AND employee_id = ?3"
                );
                one(
                    db,
                    &sql,
                    vec![text(scope), text(message_id), text(employee_id.as_str())],
                    recipient_state_row,
                )
            }

            fn recipient_states_for_employee(
                &self,
                employee_id: &Id,
            ) -> Result<Vec<RecipientStateRecord>, EngineError> {
                let (db, scope) = self.msg_view()?;
                let sql = format!(
                    "SELECT {STATE_SELECT} FROM message_recipient_states \
                     WHERE organization_id = ?1 AND employee_id = ?2"
                );
                all(
                    db,
                    &sql,
                    vec![text(scope), text(employee_id.as_str())],
                    recipient_state_row,
                )
            }

            fn all_recipient_states(&self) -> Result<Vec<RecipientStateRecord>, EngineError> {
                let (db, scope) = self.msg_view()?;
                let sql = format!(
                    "SELECT {STATE_SELECT} FROM message_recipient_states WHERE organization_id = ?1"
                );
                all(db, &sql, vec![text(scope)], recipient_state_row)
            }

            fn read_marker(
                &self,
                conversation_id: &Id,
                viewer_employee_id: &Id,
            ) -> Result<Option<ConversationReadMarkerRecord>, EngineError> {
                let (db, scope) = self.msg_view()?;
                one(
                    db,
                    "SELECT organization_id, conversation_id, viewer_employee_id, read_at, last_read_message_id \
                     FROM conversation_read_markers \
                     WHERE organization_id = ?1 AND conversation_id = ?2 AND viewer_employee_id = ?3",
                    vec![
                        text(scope),
                        text(conversation_id.as_str()),
                        text(viewer_employee_id.as_str()),
                    ],
                    |row| {
                        Ok(ConversationReadMarkerRecord {
                            organization_id: id(row, 0)?,
                            conversation_id: id(row, 1)?,
                            viewer_employee_id: id(row, 2)?,
                            read_at: row.get(3)?,
                            last_read_message_id: row.get(4)?,
                        })
                    },
                )
            }

            fn preferences(
                &self,
                conversation_id: &Id,
                viewer_employee_id: &Id,
            ) -> Result<Vec<ConversationPreferenceRecord>, EngineError> {
                let (db, scope) = self.msg_view()?;
                all(
                    db,
                    "SELECT muted_employee_id, mute_mode FROM conversation_preferences \
                     WHERE organization_id = ?1 AND conversation_id = ?2 AND viewer_employee_id = ?3 \
                     ORDER BY muted_employee_id",
                    vec![
                        text(scope),
                        text(conversation_id.as_str()),
                        text(viewer_employee_id.as_str()),
                    ],
                    |row| {
                        let mode_raw: String = row.get(1)?;
                        let mute_mode =
                            parse_mute_mode(&mode_raw).map_err(|_| foreign_col(1, "mute_mode"))?;
                        Ok(ConversationPreferenceRecord {
                            muted_employee_id: id(row, 0)?,
                            mute_mode,
                        })
                    },
                )
            }

            fn reactions_for_message(
                &self,
                message_id: &str,
            ) -> Result<Vec<MessageReactionRecord>, EngineError> {
                let (db, scope) = self.msg_view()?;
                all(
                    db,
                    "SELECT organization_id, message_id, employee_id, reaction, created_at \
                     FROM message_reactions WHERE organization_id = ?1 AND message_id = ?2 \
                     ORDER BY created_at, employee_id, reaction",
                    vec![text(scope), text(message_id)],
                    |row| {
                        let r_raw: String = row.get(3)?;
                        let reaction =
                            parse_reaction(&r_raw).map_err(|_| foreign_col(3, "reaction"))?;
                        Ok(MessageReactionRecord {
                            organization_id: id(row, 0)?,
                            message_id: row.get(1)?,
                            employee_id: id(row, 2)?,
                            reaction,
                            created_at: row.get(4)?,
                        })
                    },
                )
            }

            fn answer_for_message(
                &self,
                message_id: &str,
            ) -> Result<Option<ParticipantAnswerFile>, EngineError> {
                let (db, scope) = self.msg_view()?;
                one(
                    db,
                    "SELECT organization_id, conversation_id, message_id, employee_id, revision, \
                     accepted_by, accepted_at, reason, answer_json \
                     FROM messaging_answers WHERE organization_id = ?1 AND message_id = ?2 \
                     ORDER BY CASE WHEN reason = 'origin-close' AND answer_json IS NOT NULL THEN 0 ELSE 1 END, \
                     COALESCE(accepted_at, '') ASC, employee_id ASC LIMIT 1",
                    vec![text(scope), text(message_id)],
                    |row| {
                        let ans_raw: Option<String> = row.get(8)?;
                        let answer = ans_raw
                            .as_deref()
                            .map(serde_json::from_str)
                            .transpose()
                            .map_err(|_| foreign_col(8, "answer_json"))?;
                        Ok(ParticipantAnswerFile {
                            schema_version: SCHEMA_VERSION,
                            organization_id: id(row, 0)?,
                            conversation_id: id(row, 1)?,
                            message_id: row.get(2)?,
                            employee_id: id(row, 3)?,
                            revision: number(row, 4)?,
                            accepted_by: optional_id(row, 5)?,
                            accepted_at: row.get(6)?,
                            reason: row.get(7)?,
                            answer,
                        })
                    },
                )
            }

            fn max_origin_sequence(
                &self,
                origin_office_id: &Id,
                origin_incarnation: &str,
            ) -> Result<u64, EngineError> {
                let (db, scope) = self.msg_view()?;
                let val: Option<i64> = db
                    .prepare_cached(
                        "SELECT MAX(origin_sequence) FROM messages \
                         WHERE organization_id = ?1 AND origin_office_id = ?2 AND origin_incarnation = ?3",
                    )
                    .and_then(|mut s| {
                        s.query_row(
                            params![scope, origin_office_id.as_str(), origin_incarnation],
                            |r| r.get(0),
                        )
                    })
                    .map_err(|e| failed(db, "reading max origin_sequence", &e))?;
                Ok(val.unwrap_or(0).max(0) as u64)
            }

            fn max_state_sequence(
                &self,
                owner_office_id: &Id,
                origin_incarnation: &str,
            ) -> Result<u64, EngineError> {
                let (db, scope) = self.msg_view()?;
                let val: Option<i64> = db
                    .prepare_cached(
                        "SELECT MAX(origin_sequence) FROM message_recipient_states \
                         WHERE organization_id = ?1 AND owner_office_id = ?2 AND origin_incarnation = ?3",
                    )
                    .and_then(|mut s| {
                        s.query_row(
                            params![scope, owner_office_id.as_str(), origin_incarnation],
                            |r| r.get(0),
                        )
                    })
                    .map_err(|e| failed(db, "reading max state origin_sequence", &e))?;
                Ok(val.unwrap_or(0).max(0) as u64)
            }

            fn max_local_seq(&self) -> Result<u64, EngineError> {
                let (db, scope) = self.msg_view()?;
                let val: Option<i64> = db
                    .prepare_cached(
                        "SELECT MAX(local_seq) FROM messaging_local_meta WHERE organization_id = ?1",
                    )
                    .and_then(|mut s| s.query_row([scope], |r| r.get(0)))
                    .map_err(|e| failed(db, "reading max local_seq", &e))?;
                Ok(val.unwrap_or(0).max(0) as u64)
            }

            fn message_local_seq(&self, message_id: &str) -> Result<Option<u64>, EngineError> {
                let (db, scope) = self.msg_view()?;
                let val: Option<i64> = db
                    .prepare_cached(
                        "SELECT local_seq FROM messaging_local_meta \
                         WHERE organization_id = ?1 AND message_id = ?2",
                    )
                    .and_then(|mut s| {
                        s.query_row(params![scope, message_id], |r| r.get(0))
                            .optional()
                    })
                    .map_err(|e| failed(db, "reading message local_seq", &e))?;
                Ok(val.map(|v| v.max(0) as u64))
            }
        }
    )+};
}

sqlite_messaging_read!(SqliteTx, SqliteSnapshot);

fn put_message_meta_and_attachments(
    db: &Connection,
    scope: &str,
    record: &MessageRecord,
    local_seq: u64,
) -> Result<(), EngineError> {
    let existing_seq: Option<i64> = db
        .prepare_cached(
            "SELECT local_seq FROM messaging_local_meta \
             WHERE organization_id = ?1 AND message_id = ?2",
        )
        .and_then(|mut s| {
            s.query_row(params![scope, &record.id], |r| r.get(0))
                .optional()
        })
        .map_err(|e| failed(db, "reading local_seq", &e))?;
    let eff_seq = existing_seq.map_or(local_seq, |v| v.max(1) as u64);
    put(
        db,
        "messaging_local_meta",
        "organization_id, message_id, local_seq, mailbox_content_hash",
        2,
        vec![
            text(scope),
            text(&record.id),
            whole(eff_seq)?,
            record
                .mailbox_content_hash
                .as_deref()
                .map_or(Value::Null, text),
        ],
    )?;
    run(
        db,
        "DELETE FROM message_attachments WHERE organization_id = ?1 AND message_id = ?2",
        vec![text(scope), text(&record.id)],
    )?;
    for att in &record.attachments {
        put(
            db,
            "message_attachments",
            "organization_id, message_id, id, content_ref, media_type, byte_count, content_sha256",
            3,
            vec![
                text(scope),
                text(&record.id),
                text(&att.id),
                text(&att.content_ref),
                text(&att.media_type),
                whole(att.byte_count)?,
                text(&att.content_sha256),
            ],
        )?;
    }
    Ok(())
}

impl MessagingWrite for SqliteTx {
    fn put_conversation(&mut self, record: &ConversationRecord) -> Result<(), EngineError> {
        let (db, scope) = self.msg_view()?;
        ensure_org(db, scope)?;
        if let Some(team_id) = &record.team_id {
            ensure_team(db, scope, team_id.as_str())?;
        }
        if let Some(project_id) = &record.project_id {
            ensure_project(db, scope, project_id.as_str())?;
        }
        let values = vec![
            text(scope),
            text(record.id.as_str()),
            whole(u64::from(record.schema_version))?,
            whole(record.revision)?,
            text(conversation_kind_str(record.kind)),
            record.name.as_deref().map_or(Value::Null, text),
            record
                .auto_scope
                .map_or(Value::Null, |s| text(auto_scope_str(s))),
            optional_id_value(record.team_id.as_ref()),
            optional_id_value(record.project_id.as_ref()),
            Value::Integer(i64::from(record.archived)),
            text(&record.created_at),
        ];
        put(
            db,
            "conversations",
            "organization_id, id, schema_version, revision, kind, name, auto_scope, team_id, \
             project_id, archived, created_at",
            2,
            values,
        )?;
        if record.auto_scope.is_some() && record.memberships.is_empty() {
            run(
                db,
                "DELETE FROM conversation_memberships \
                 WHERE organization_id = ?1 AND conversation_id = ?2 \
                   AND employee_id NOT IN ( \
                     SELECT employee_id FROM message_recipient_states \
                     WHERE organization_id = ?1 AND conversation_id = ?2 \
                   )",
                vec![text(scope), text(record.id.as_str())],
            )?;
            run(
                db,
                "INSERT OR IGNORE INTO conversation_memberships \
                   (organization_id, conversation_id, employee_id, joined_at, left_at) \
                 SELECT organization_id, conversation_id, employee_id, MIN(updated_at), NULL \
                 FROM message_recipient_states \
                 WHERE organization_id = ?1 AND conversation_id = ?2 \
                 GROUP BY organization_id, conversation_id, employee_id",
                vec![text(scope), text(record.id.as_str())],
            )?;
        } else {
            run(
                db,
                "DELETE FROM conversation_memberships \
                 WHERE organization_id = ?1 AND conversation_id = ?2",
                vec![text(scope), text(record.id.as_str())],
            )?;
            for m in &record.memberships {
                ensure_employee(db, scope, m.employee_id.as_str())?;
                put(
                    db,
                    "conversation_memberships",
                    "organization_id, conversation_id, employee_id, joined_at, left_at",
                    3,
                    vec![
                        text(scope),
                        text(record.id.as_str()),
                        text(m.employee_id.as_str()),
                        text(&m.joined_at),
                        m.left_at.as_deref().map_or(Value::Null, text),
                    ],
                )?;
            }
        }
        run(
            db,
            "DELETE FROM messaging_pins WHERE organization_id = ?1 AND conversation_id = ?2",
            vec![text(scope), text(record.id.as_str())],
        )?;
        for pin in &record.pins {
            put(
                db,
                "messaging_pins",
                "organization_id, conversation_id, message_id, pinned_by, pinned_at",
                3,
                vec![
                    text(scope),
                    text(record.id.as_str()),
                    text(&pin.message_id),
                    text(pin.pinned_by.as_str()),
                    text(&pin.pinned_at),
                ],
            )?;
        }
        Ok(())
    }

    fn remove_conversation(&mut self, conv_id: &Id) -> Result<(), EngineError> {
        let (db, scope) = self.msg_view()?;
        let pair = || vec![text(scope), text(conv_id.as_str())];
        run(
            db,
            "DELETE FROM messaging_pins WHERE organization_id = ?1 AND conversation_id = ?2",
            pair(),
        )?;
        run(
            db,
            "DELETE FROM conversation_memberships WHERE organization_id = ?1 AND conversation_id = ?2",
            pair(),
        )?;
        run(
            db,
            "DELETE FROM conversations WHERE organization_id = ?1 AND id = ?2",
            pair(),
        )
    }

    fn put_message(&mut self, record: &MessageRecord, local_seq: u64) -> Result<(), EngineError> {
        let (db, scope) = self.msg_view()?;
        ensure_org(db, scope)?;
        if let Some(emp) = &record.sender_employee_id {
            ensure_employee(db, scope, emp.as_str())?;
        }
        if let Some(off) = &record.sender_office_id {
            ensure_office(db, scope, off.as_str())?;
        }
        ensure_office(db, scope, record.origin_office_id.as_str())?;
        let meta_json = serde_json::to_string(&record.metadata).unwrap_or_else(|_| "{}".into());
        let values = vec![
            text(scope),
            text(&record.id),
            text(record.conversation_id.as_str()),
            text(record.sender_kind.as_str()),
            optional_id_value(record.sender_employee_id.as_ref()),
            optional_id_value(record.sender_office_id.as_ref()),
            text(&record.sender_display_name),
            text(&record.kind),
            text(priority_str(record.priority)),
            record.subject.as_deref().map_or(Value::Null, text),
            text(&record.body_markdown),
            text(&meta_json),
            record.thread_root_id.as_deref().map_or(Value::Null, text),
            record
                .reply_to_message_id
                .as_deref()
                .map_or(Value::Null, text),
            Value::Integer(i64::from(record.also_in_channel)),
            text(&record.created_at),
            record.expires_at.as_deref().map_or(Value::Null, text),
            match record.ttl_ms {
                Some(v) => whole(v)?,
                None => Value::Null,
            },
            text(record.origin_office_id.as_str()),
            text(&record.origin_incarnation),
            whole(record.origin_sequence)?,
            whole(record.org_epoch)?,
            whole(record.org_sequence)?,
            text(&record.content_sha256),
        ];
        put(
            db,
            "messages",
            "organization_id, id, conversation_id, sender_kind, sender_employee_id, \
             sender_office_id, sender_display_name, kind, priority, subject, body_markdown, \
             metadata_json, thread_root_id, reply_to_message_id, also_in_channel, created_at, \
             expires_at, ttl_ms, origin_office_id, origin_incarnation, origin_sequence, \
             org_epoch, org_sequence, content_sha256",
            2,
            values,
        )?;
        put_message_meta_and_attachments(db, scope, record, local_seq)
    }

    fn remove_message(&mut self, msg_id: &str) -> Result<(), EngineError> {
        let (db, scope) = self.msg_view()?;
        let pair = || vec![text(scope), text(msg_id)];
        run(
            db,
            "DELETE FROM message_attachments WHERE organization_id = ?1 AND message_id = ?2",
            pair(),
        )?;
        run(
            db,
            "DELETE FROM messaging_local_meta WHERE organization_id = ?1 AND message_id = ?2",
            pair(),
        )?;
        run(
            db,
            "DELETE FROM messages WHERE organization_id = ?1 AND id = ?2",
            pair(),
        )
    }

    fn put_recipient_state(&mut self, record: &RecipientStateRecord) -> Result<(), EngineError> {
        let (db, scope) = self.msg_view()?;
        ensure_employee(db, scope, record.employee_id.as_str())?;
        ensure_office(db, scope, record.owner_office_id.as_str())?;
        ensure_auto_conversation_membership(
            db,
            scope,
            record.conversation_id.as_str(),
            record.employee_id.as_str(),
            &record.updated_at,
        )?;
        let related_json =
            serde_json::to_string(&record.related_message_ids).unwrap_or_else(|_| "[]".into());
        let values = vec![
            text(scope),
            text(&record.message_id),
            text(record.employee_id.as_str()),
            text(record.conversation_id.as_str()),
            record
                .recipient_display_name
                .as_deref()
                .map_or(Value::Null, text),
            text(lifecycle_str(record.state)),
            whole(record.triage_generation)?,
            record.snoozed_until.as_deref().map_or(Value::Null, text),
            Value::Integer(i64::from(record.snooze_after_task)),
            record.snooze_note.as_deref().map_or(Value::Null, text),
            record.triaged_at.as_deref().map_or(Value::Null, text),
            record.read_at.as_deref().map_or(Value::Null, text),
            text(&record.updated_at),
            record.reason.as_deref().map_or(Value::Null, text),
            text(delivered_because_str(record.delivered_because)),
            text(&related_json),
            Value::Integer(i64::from(record.expired_waiting_person)),
            text(record.owner_office_id.as_str()),
            text(&record.origin_incarnation),
            whole(record.origin_sequence)?,
            whole(record.org_epoch)?,
            whole(record.org_sequence)?,
        ];
        put(
            db,
            "message_recipient_states",
            "organization_id, message_id, employee_id, conversation_id, recipient_display_name, \
             state, triage_generation, snoozed_until, snooze_after_task, snooze_note, triaged_at, \
             read_at, updated_at, reason, delivered_because, related_message_ids_json, \
             expired_waiting_person, owner_office_id, origin_incarnation, origin_sequence, \
             org_epoch, org_sequence",
            3,
            values,
        )
    }

    fn remove_recipient_states_for_participant(
        &mut self,
        conversation_id: &Id,
        employee_id: &Id,
    ) -> Result<(), EngineError> {
        let (db, scope) = self.msg_view()?;
        run(
            db,
            "DELETE FROM message_recipient_states \
             WHERE organization_id = ?1 AND conversation_id = ?2 AND employee_id = ?3",
            vec![
                text(scope),
                text(conversation_id.as_str()),
                text(employee_id.as_str()),
            ],
        )
    }

    fn put_read_marker(
        &mut self,
        record: &ConversationReadMarkerRecord,
    ) -> Result<(), EngineError> {
        let (db, scope) = self.msg_view()?;
        ensure_employee(db, scope, record.viewer_employee_id.as_str())?;
        put(
            db,
            "conversation_read_markers",
            "organization_id, conversation_id, viewer_employee_id, read_at, last_read_message_id",
            3,
            vec![
                text(scope),
                text(record.conversation_id.as_str()),
                text(record.viewer_employee_id.as_str()),
                record.read_at.as_deref().map_or(Value::Null, text),
                record
                    .last_read_message_id
                    .as_deref()
                    .map_or(Value::Null, text),
            ],
        )
    }

    fn put_preferences(
        &mut self,
        conversation_id: &Id,
        viewer_employee_id: &Id,
        prefs: &[ConversationPreferenceRecord],
    ) -> Result<(), EngineError> {
        let (db, scope) = self.msg_view()?;
        ensure_employee(db, scope, viewer_employee_id.as_str())?;
        run(
            db,
            "DELETE FROM conversation_preferences \
             WHERE organization_id = ?1 AND conversation_id = ?2 AND viewer_employee_id = ?3",
            vec![
                text(scope),
                text(conversation_id.as_str()),
                text(viewer_employee_id.as_str()),
            ],
        )?;
        for pref in prefs {
            ensure_employee(db, scope, pref.muted_employee_id.as_str())?;
            put(
                db,
                "conversation_preferences",
                "organization_id, conversation_id, viewer_employee_id, muted_employee_id, mute_mode",
                4,
                vec![
                    text(scope),
                    text(conversation_id.as_str()),
                    text(viewer_employee_id.as_str()),
                    text(pref.muted_employee_id.as_str()),
                    text(mute_mode_str(pref.mute_mode)),
                ],
            )?;
        }
        Ok(())
    }

    fn put_participant_reactions(
        &mut self,
        message_id: &str,
        employee_id: &Id,
        reactions: &[MessageReactionRecord],
    ) -> Result<(), EngineError> {
        let (db, scope) = self.msg_view()?;
        ensure_employee(db, scope, employee_id.as_str())?;
        run(
            db,
            "DELETE FROM message_reactions \
             WHERE organization_id = ?1 AND message_id = ?2 AND employee_id = ?3",
            vec![text(scope), text(message_id), text(employee_id.as_str())],
        )?;
        for r in reactions {
            put(
                db,
                "message_reactions",
                "organization_id, message_id, employee_id, reaction, created_at",
                4,
                vec![
                    text(scope),
                    text(&r.message_id),
                    text(r.employee_id.as_str()),
                    text(reaction_str(r.reaction)),
                    text(&r.created_at),
                ],
            )?;
        }
        Ok(())
    }

    fn put_participant_answer(
        &mut self,
        answer: &ParticipantAnswerFile,
    ) -> Result<(), EngineError> {
        let (db, scope) = self.msg_view()?;
        let ans_json = answer
            .answer
            .as_ref()
            .and_then(|v| serde_json::to_string(v).ok());
        put(
            db,
            "messaging_answers",
            "organization_id, message_id, employee_id, conversation_id, revision, accepted_by, \
             accepted_at, reason, answer_json",
            3,
            vec![
                text(scope),
                text(&answer.message_id),
                text(answer.employee_id.as_str()),
                text(answer.conversation_id.as_str()),
                whole(answer.revision)?,
                optional_id_value(answer.accepted_by.as_ref()),
                answer.accepted_at.as_deref().map_or(Value::Null, text),
                answer.reason.as_deref().map_or(Value::Null, text),
                ans_json.as_deref().map_or(Value::Null, text),
            ],
        )
    }

    fn remove_participant_answer(
        &mut self,
        message_id: &str,
        employee_id: &Id,
    ) -> Result<(), EngineError> {
        let (db, scope) = self.msg_view()?;
        run(
            db,
            "DELETE FROM messaging_answers \
             WHERE organization_id = ?1 AND message_id = ?2 AND employee_id = ?3",
            vec![text(scope), text(message_id), text(employee_id.as_str())],
        )
    }
}
