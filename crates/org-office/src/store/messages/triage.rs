//! The triage port. Every mutation runs inside one writer savepoint, including the
//! release-and-select of notify. Headers are read without touching cold message bodies.
//! Local transitions use the shared message sequence for their state origin_seq. Explicit
//! transitions take owner/incarnation/position from the caller; notify releases retain those
//! fields from the state row. Sync/work-log replication is a later slice.

#[cfg(test)]
mod tests;

use org_mailbox::{LocalStateTransition, NotifyCandidate, SnoozeDeadline};
use org_proto::msg::NotifyReason;

use super::*;

/// Schema 4: keep triage history even when reading removes the hot-index row.
/// Inspect the columns so replaying this additive migration is harmless.
pub(crate) fn create_triage_schema(tx: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let exists = tx
        .prepare("PRAGMA table_info(msg_state)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .iter()
        .any(|name| name == "triaged_at");
    if !exists {
        tx.execute_batch("ALTER TABLE msg_state ADD COLUMN triaged_at TEXT;")?;
    }
    tx.execute_batch(
        "UPDATE msg_state SET triaged_at = COALESCE(
             (SELECT triaged_at FROM msg_unread WHERE msg_unread.id = msg_state.id), updated_at)
         WHERE triaged_at IS NULL AND (triage_gen > 0 OR id IN (
             SELECT id FROM msg_unread WHERE triaged_at IS NOT NULL));",
    )
}

fn header(db: &Connection, id: &str) -> Result<Option<MessageHeader>, OfficeError> {
    db.query_row(
        &format!("{} WHERE h.id = ?1", select_headers()),
        [id],
        |row| stored_from(row, false).map(|stored| stored.message.header),
    )
    .optional()
    .map_err(storage)
}

/// Rebuild just this message's hot row from the authoritative header and state.
/// It contains exactly unread and snoozed messages, including read -> snoozed/unread.
fn refresh_unread(db: &Connection, id: &str) -> Result<(), OfficeError> {
    db.execute("DELETE FROM msg_unread WHERE id = ?1", [id])
        .map_err(storage)?;
    db.execute(
        "INSERT INTO msg_unread (to_member_id, local_seq, id, from_member_id, kind,
             priority, urgent, triage_gen, reply_to, created_at, expires_at,
             snoozed_until, snooze_after_task, snooze_note, triaged_at)
         SELECT h.to_member_id, h.local_seq, h.id, h.from_member_id, h.kind, h.priority,
             (h.kind = 'request' OR h.priority IN ('high', 'urgent')), s.triage_gen,
             h.reply_to, h.created_at, h.expires_at, s.snoozed_until,
             s.snooze_after_task, s.reason, s.triaged_at
         FROM msg_headers h JOIN msg_state s ON s.id = h.id
         WHERE h.id = ?1 AND s.state IN ('unread', 'snoozed')",
        [id],
    )
    .map_err(storage)?;
    Ok(())
}

pub(crate) fn update_state_local(
    db: &Connection,
    change: &LocalStateTransition,
) -> Outcome<MessageHeader> {
    let Some(mut current) =
        header(db, &change.id)?.filter(|h| h.to_member_id == change.to_member_id)
    else {
        return Ok(Err(StoreError::NotFound));
    };
    let from = current.state;
    match (from, change.state) {
        (State::Read, State::Read)
        | (State::Done, State::Done)
        | (State::Expired, State::Expired) => return Ok(Ok(current)),
        (State::Unread | State::Read | State::Snoozed, State::Snoozed) => {
            if change.snoozed_until.is_some() == change.snooze_after_task {
                return Ok(Err(StoreError::InvalidTransition));
            }
            current.triage_gen = current
                .triage_gen
                .checked_add(1)
                .filter(|n| *n <= i64::MAX as u64)
                .ok_or(OfficeError::Invalid("the triage generation is exhausted"))?;
        }
        (State::Unread | State::Read | State::Snoozed, State::Done)
        | (State::Unread | State::Snoozed, State::Read | State::Expired) => {}
        (State::Read, State::Unread) if !change.expired_waiting_person => {}
        (State::Unread, State::Unread) => {
            if !change.expired_waiting_person || current.expired_waiting_person {
                return Ok(Ok(current));
            }
        }
        _ => return Ok(Err(StoreError::InvalidTransition)),
    }

    let snoozed = change.state == State::Snoozed;
    let reason = match change.state {
        State::Snoozed | State::Done => change.reason.as_deref(),
        _ => None,
    };
    let read_at = match change.state {
        State::Read => Some(change.now_utc.as_str()),
        State::Unread | State::Snoozed => None,
        _ => current.read_at.as_deref(),
    };
    let sequence = next_sequence(db)?;
    db.execute(
        "UPDATE msg_state SET state = ?2, triage_gen = ?3, snoozed_until = ?4,
             snooze_after_task = ?5, reason = ?6, read_at = ?7, updated_at = ?8,
             owner_office_id = ?9, origin_incarnation = ?10, origin_seq = ?11,
             org_epoch = ?12, org_seq = ?13,
             triaged_at = CASE WHEN ?14 THEN ?8 ELSE triaged_at END WHERE id = ?1",
        params![
            change.id,
            state_text(change.state),
            current.triage_gen as i64,
            change.snoozed_until.as_deref().filter(|_| snoozed),
            snoozed && change.snooze_after_task,
            reason,
            read_at,
            change.now_utc,
            change.owner_office_id,
            change.origin_incarnation,
            sequence as i64,
            change.org_position.epoch as i64,
            change.org_position.seq as i64,
            snoozed
        ],
    )
    .map_err(storage)?;
    if change.state == State::Unread && change.expired_waiting_person {
        db.execute(
            "UPDATE msg_headers SET flags = flags | ?2 WHERE id = ?1",
            params![change.id, FLAG_EXPIRED_WAITING_PERSON],
        )
        .map_err(storage)?;
    }
    refresh_unread(db, &change.id)?;
    Ok(Ok(
        header(db, &change.id)?.ok_or(OfficeError::Invalid("a message went missing"))?
    ))
}

pub(crate) fn check_notify(
    db: &Connection,
    member: &str,
    after_task: bool,
    urgent_only: bool,
    sweep_before: &str,
    now: &str,
) -> Result<Vec<NotifyCandidate>, OfficeError> {
    // Walk only the recipient's hot index. Parsing times matches the domain's UTC
    // millisecond comparison; an unreadable snooze is released rather than stranded.
    let rows = {
        let mut statement = db
            .prepare(&format!(
                "{} JOIN msg_unread u ON u.id = h.id WHERE u.to_member_id = ?1",
                select_headers()
            ))
            .map_err(storage)?;
        let rows = statement
            .query_map([member], |row| {
                stored_from(row, false).map(|stored| stored.message.header)
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?;
        rows
    };
    let mut candidates = Vec::new();
    for mut current in rows {
        let reason = if current.state == State::Snoozed {
            let due = current.snoozed_until.as_deref().is_some_and(|until| {
                match (parse_utc(until), parse_utc(now)) {
                    (Some(until), Some(now)) => until <= now,
                    _ => true,
                }
            });
            let reason = if due {
                NotifyReason::Due
            } else if after_task && current.snooze_after_task {
                NotifyReason::AfterTask
            } else {
                continue;
            };
            let sequence = next_sequence(db)?;
            db.execute(
                "UPDATE msg_state SET state = 'unread', triage_gen = triage_gen + 1,
                     snoozed_until = NULL, snooze_after_task = 0, reason = NULL,
                     updated_at = ?2, triaged_at = ?2, origin_seq = ?3 WHERE id = ?1",
                params![current.id, now, sequence as i64],
            )
            .map_err(storage)?;
            refresh_unread(db, &current.id)?;
            current =
                header(db, &current.id)?.ok_or(OfficeError::Invalid("a message went missing"))?;
            reason
        } else if current.state == State::Unread {
            if after_task || current.urgent {
                NotifyReason::New
            } else {
                let triaged: bool = db
                    .query_row(
                        "SELECT triaged_at IS NOT NULL FROM msg_state WHERE id = ?1",
                        [&current.id],
                        |row| row.get(0),
                    )
                    .map_err(storage)?;
                if !triaged
                    && matches!((parse_utc(&current.created_at), parse_utc(sweep_before)),
                    (Some(created), Some(before)) if created < before)
                {
                    NotifyReason::Sweep
                } else {
                    continue;
                }
            }
        } else {
            continue;
        };
        if !urgent_only || current.urgent {
            candidates.push(NotifyCandidate {
                header: current,
                reason,
            });
        }
    }
    Ok(candidates)
}

pub(crate) fn list_active_snooze_deadlines(
    db: &Connection,
) -> Result<Vec<SnoozeDeadline>, OfficeError> {
    let mut statement = db
        .prepare(
            "SELECT u.id, u.to_member_id, u.snoozed_until FROM msg_unread u
         JOIN msg_state s ON s.id = u.id WHERE s.state = 'snoozed' AND u.snoozed_until IS NOT NULL",
        )
        .map_err(storage)?;
    let rows = statement
        .query_map([], |row| {
            Ok(SnoozeDeadline {
                id: row.get(0)?,
                to_member_id: row.get(1)?,
                at_utc: row.get(2)?,
            })
        })
        .map_err(storage)?
        .collect::<rusqlite::Result<_>>()
        .map_err(storage);
    rows
}
