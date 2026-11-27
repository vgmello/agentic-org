//! [`MessagingRead`] for the transaction and snapshot views of `messaging.db`.

use org_store::{EngineError, Id};
use rusqlite::{params, OptionalExtension};

use crate::messaging::{
    ports::{BoundMessageFile, MessagingRead},
    records::{
        parse_mute_mode, parse_reaction, ConversationPreferenceRecord,
        ConversationReadMarkerRecord, ConversationRecord, MessageReactionRecord, MessageRecord,
        ParticipantAnswerFile, RecipientStateRecord, SCHEMA_VERSION,
    },
};

use super::super::{
    failed,
    organization::{all, id, number, one, optional_id, text},
    SqliteSnapshot, SqliteTx,
};

use super::files::{answer_files, attach_attachments, message_file};
use super::rows::{
    attach_conversation_children, conversation_row, foreign_col, message_row, recipient_state_row,
    MSG_SELECT, STATE_SELECT,
};
use super::MsgView;

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
                let found = one(
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
                            attachments: Vec::new(),
                        })
                    },
                )?;
                let Some(mut answer) = found else {
                    return Ok(None);
                };
                answer.attachments =
                    answer_files(db, scope, &answer.message_id, &answer.employee_id)?;
                Ok(Some(answer))
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

            fn message_file(
                &self,
                attachment_id: &str,
            ) -> Result<Option<BoundMessageFile>, EngineError> {
                let (db, scope) = self.msg_view()?;
                message_file(db, scope, attachment_id)
            }
        }
    )+};
}

sqlite_messaging_read!(SqliteTx, SqliteSnapshot);
