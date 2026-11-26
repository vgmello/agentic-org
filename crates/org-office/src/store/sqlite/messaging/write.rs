//! [`MessagingWrite`] for the SQLite transaction over `messaging.db`.

use org_store::{EngineError, Id};
use rusqlite::{params, types::Value, Connection, OptionalExtension};

use crate::messaging::{
    ports::MessagingWrite,
    records::{
        auto_scope_str, conversation_kind_str, delivered_because_str, lifecycle_str, mute_mode_str,
        priority_str, reaction_str, ConversationPreferenceRecord, ConversationReadMarkerRecord,
        ConversationRecord, MessageReactionRecord, MessageRecord, ParticipantAnswerFile,
        RecipientStateRecord,
    },
};

use super::super::{
    failed,
    organization::{optional_id_value, put, run, text, whole},
    SqliteTx,
};

use super::mirror::{
    ensure_auto_conversation_membership, ensure_employee, ensure_office, ensure_org,
    ensure_project, ensure_team,
};
use super::MsgView;

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
