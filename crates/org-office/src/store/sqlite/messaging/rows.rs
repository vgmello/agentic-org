//! Row decoders and column lists for conversations, messages and recipient states.

use org_store::EngineError;
use rusqlite::{Connection, Row};

use crate::messaging::records::{
    parse_auto_scope, parse_conversation_kind, parse_delivered_because, parse_lifecycle,
    parse_priority, ConversationMembership, ConversationPin, ConversationRecord,
    MessageAttachmentRecord, MessageRecord, RecipientStateRecord, SenderKind, SCHEMA_VERSION,
};

use super::super::organization::{all, id, number, optional_id, text};

// ---- Row decoders ----

pub(super) fn foreign_col(col: usize, what: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(col, rusqlite::types::Type::Text, what.into())
}

pub(super) fn conversation_row(row: &Row<'_>) -> rusqlite::Result<ConversationRecord> {
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

pub(super) fn attach_conversation_children(
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

pub(super) fn message_row(row: &Row<'_>) -> rusqlite::Result<MessageRecord> {
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

pub(super) const MSG_SELECT: &str = "m.organization_id, m.id, m.conversation_id, m.sender_kind, \
    m.sender_employee_id, m.sender_office_id, m.sender_display_name, m.kind, m.priority, \
    m.subject, m.body_markdown, m.metadata_json, m.thread_root_id, m.reply_to_message_id, \
    m.also_in_channel, m.created_at, m.expires_at, m.ttl_ms, m.origin_office_id, \
    m.origin_incarnation, m.origin_sequence, m.org_epoch, m.org_sequence, m.content_sha256, \
    lm.mailbox_content_hash";

pub(super) fn attach_attachments(
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

pub(super) fn recipient_state_row(row: &Row<'_>) -> rusqlite::Result<RecipientStateRecord> {
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

pub(super) const STATE_SELECT: &str = "organization_id, message_id, conversation_id, employee_id, \
    recipient_display_name, state, triage_generation, snoozed_until, snooze_after_task, \
    snooze_note, triaged_at, read_at, updated_at, reason, delivered_because, \
    related_message_ids_json, expired_waiting_person, owner_office_id, origin_incarnation, \
    origin_sequence, org_epoch, org_sequence";
