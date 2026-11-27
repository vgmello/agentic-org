//! Bounded zero-N+1 SQLite queries for `agent.resume` (`TASK-220`).
//!
//! All queries are strictly scoped to `(organization_id, employee_id)` inside `messaging.db`.

use org_proto::msg::{is_urgent, OFFICE_SENDER};
use org_proto::resume::{ResumeInboxItem, ResumeInboxSummary};
use org_store::{EngineError, Id};
use rusqlite::types::Value;
use rusqlite::{params, Connection, Row};

use org_office::messaging::records::{parse_delivered_because, parse_priority, MessageRecord};

use super::super::failed;
use super::super::organization::{all, id, text};
use super::rows::{foreign_col, message_row, MSG_SELECT};

fn unread_inbox_row(row: &Row<'_>) -> rusqlite::Result<ResumeInboxItem> {
    let sender_emp: Option<String> = row.get(2)?;
    let kind: String = row.get(4)?;
    let priority_raw: String = row.get(5)?;
    let priority = parse_priority(&priority_raw).map_err(|_| foreign_col(5, "priority"))?;
    let because_raw: String = row.get(7)?;
    let delivered_because =
        parse_delivered_because(&because_raw).map_err(|_| foreign_col(7, "delivered_because"))?;
    let urgent = is_urgent(&kind, priority);
    Ok(ResumeInboxItem {
        id: row.get(0)?,
        conversation_id: id(row, 1)?.as_str().to_owned(),
        from_member_id: sender_emp.unwrap_or_else(|| OFFICE_SENDER.to_owned()),
        from_display_name: row.get(3)?,
        kind,
        priority,
        urgent,
        subject: row.get(6)?,
        delivered_because,
        created_at: row.get(8)?,
    })
}

/// Counts caller's unread + urgent unread messages in one aggregate query and fetches at most
/// `limit` newest unread rows in one indexed JOIN query.
pub(super) fn unread_inbox_summary(
    db: &Connection,
    scope: &str,
    employee_id: &Id,
    limit: usize,
) -> Result<ResumeInboxSummary, EngineError> {
    let (unread_count, urgent_count): (i64, i64) = db
        .prepare_cached(
            "SELECT COUNT(*), \
                    COALESCE(SUM(CASE WHEN m.kind = 'request' OR m.priority IN ('high', 'urgent') \
                                      THEN 1 ELSE 0 END), 0) \
             FROM message_recipient_states s \
             JOIN messages m ON m.organization_id = s.organization_id AND m.id = s.message_id \
             WHERE s.organization_id = ?1 AND s.employee_id = ?2 AND s.state = 'unread'",
        )
        .and_then(|mut stmt| {
            stmt.query_row(params![scope, employee_id.as_str()], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
        })
        .map_err(|e| failed(db, "counting unread inbox summary", &e))?;

    let unread_u64 = unread_count.max(0) as u64;
    let urgent_u64 = urgent_count.max(0) as u64;
    if unread_u64 == 0 || limit == 0 {
        return Ok(ResumeInboxSummary {
            unread_count: unread_u64,
            urgent_count: urgent_u64,
            items: Vec::new(),
            truncated: unread_u64 > 0,
        });
    }

    let items = all(
        db,
        "SELECT m.id, m.conversation_id, m.sender_employee_id, m.sender_display_name, \
                m.kind, m.priority, m.subject, s.delivered_because, m.created_at \
         FROM message_recipient_states s \
         JOIN messages m ON m.organization_id = s.organization_id AND m.id = s.message_id \
         LEFT JOIN messaging_local_meta lm \
           ON lm.organization_id = m.organization_id AND lm.message_id = m.id \
         WHERE s.organization_id = ?1 AND s.employee_id = ?2 AND s.state = 'unread' \
         ORDER BY COALESCE(lm.local_seq, m.origin_sequence) DESC, m.id DESC \
         LIMIT ?3",
        vec![
            text(scope),
            text(employee_id.as_str()),
            Value::Integer(limit as i64),
        ],
        unread_inbox_row,
    )?;

    let truncated = unread_u64 > items.len() as u64;
    Ok(ResumeInboxSummary {
        unread_count: unread_u64,
        urgent_count: urgent_u64,
        items,
        truncated,
    })
}

/// Fetches up to `limit` newest unanswered `request` messages sent by or actively delivered to
/// `employee_id` in one SQLite query (zero N+1).
pub(super) fn open_asks_for_employee(
    db: &Connection,
    scope: &str,
    employee_id: &Id,
    limit: usize,
) -> Result<Vec<(MessageRecord, bool)>, EngineError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT {MSG_SELECT}, \
                EXISTS(SELECT 1 FROM message_recipient_states s \
                       WHERE s.organization_id = m.organization_id \
                         AND s.message_id = m.id \
                         AND s.employee_id = ?2 \
                         AND s.state IN ('unread', 'read', 'snoozed')) AS is_recipient \
         FROM messages m \
         LEFT JOIN messaging_local_meta lm \
           ON lm.organization_id = m.organization_id AND lm.message_id = m.id \
         WHERE m.organization_id = ?1 \
           AND m.kind = 'request' \
           AND (m.sender_employee_id = ?2 OR EXISTS( \
                 SELECT 1 FROM message_recipient_states s \
                 WHERE s.organization_id = m.organization_id \
                   AND s.message_id = m.id \
                   AND s.employee_id = ?2 \
                   AND s.state IN ('unread', 'read', 'snoozed'))) \
           AND NOT EXISTS( \
                 SELECT 1 FROM messaging_answers a \
                 WHERE a.organization_id = m.organization_id \
                   AND a.message_id = m.id \
                   AND a.answer_json IS NOT NULL) \
         ORDER BY m.created_at DESC, m.id DESC \
         LIMIT ?3"
    );
    all(
        db,
        &sql,
        vec![
            text(scope),
            text(employee_id.as_str()),
            Value::Integer(limit as i64),
        ],
        |row| {
            let message = message_row(row)?;
            let is_recipient: i64 = row.get(25)?;
            Ok((message, is_recipient != 0))
        },
    )
}
