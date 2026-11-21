//! Messaging family projector (`MessagingFiles`): reads and validates `durable/messaging/`
//! files into [`MessagingWrite`], rebuilding identically from disk on `runtime/<office-id>/`
//! deletion (`doc-90`, `TASK-45`).

use org_store::{DurablePath, Engine, Id, ProjectionError, Projector};

use super::{
    ports::{MessagingRead, MessagingWrite},
    records::{
        self, ConversationRecord, Kind, MessageRecord, ParticipantAnswerFile,
        ParticipantReactionsFile, ParticipantReceivedSnapshot, ParticipantStateFile,
        SCHEMA_VERSION,
    },
};

/// Projects canonical files under `durable/messaging/conversations/...`.
pub struct MessagingFiles {
    organization_id: Id,
}

impl MessagingFiles {
    pub fn new(organization_id: Id) -> Self {
        Self { organization_id }
    }

    fn apply_conversation<W: MessagingWrite>(
        &self,
        tx: &mut W,
        path: &DurablePath,
        cid: &Id,
        bytes: &[u8],
    ) -> Result<(), ProjectionError> {
        let rec: ConversationRecord =
            records::decode_json(bytes).map_err(|e| refuse(path, e.to_string()))?;
        agree(path, rec.schema_version == SCHEMA_VERSION, "schema_version")?;
        agree(
            path,
            rec.organization_id == self.organization_id,
            "organization_id",
        )?;
        agree(path, rec.id == *cid, "conversation_id")?;
        tx.put_conversation(&rec)?;
        Ok(())
    }

    fn apply_message<W: MessagingWrite + MessagingRead>(
        &self,
        tx: &mut W,
        path: &DurablePath,
        cid: &Id,
        mid: &str,
        bytes: &[u8],
    ) -> Result<(), ProjectionError> {
        let rec: MessageRecord =
            records::decode_json(bytes).map_err(|e| refuse(path, e.to_string()))?;
        agree(path, rec.schema_version == SCHEMA_VERSION, "schema_version")?;
        agree(
            path,
            rec.organization_id == self.organization_id,
            "organization_id",
        )?;
        agree(path, rec.conversation_id == *cid, "conversation_id")?;
        agree(path, rec.id == mid, "message_id")?;
        if rec.content_sha256 != rec.compute_content_sha256() {
            return Err(refuse(path, "message content_sha256 mismatch"));
        }
        let next_seq = tx.max_local_seq()?.saturating_add(1);
        tx.put_message(&rec, next_seq)?;
        Ok(())
    }

    fn apply_participant_state<W: MessagingWrite>(
        &self,
        tx: &mut W,
        path: &DurablePath,
        cid: &Id,
        eid: &Id,
        bytes: &[u8],
    ) -> Result<(), ProjectionError> {
        let rec: ParticipantStateFile =
            records::decode_json(bytes).map_err(|e| refuse(path, e.to_string()))?;
        agree(path, rec.schema_version == SCHEMA_VERSION, "schema_version")?;
        agree(
            path,
            rec.organization_id == self.organization_id,
            "organization_id",
        )?;
        agree(path, rec.conversation_id == *cid, "conversation_id")?;
        agree(path, rec.employee_id == *eid, "employee_id")?;
        tx.put_preferences(cid, eid, &rec.preferences)?;
        if let Some(marker) = &rec.read_marker {
            tx.put_read_marker(marker)?;
        }
        Ok(())
    }

    fn apply_received_snapshot<W: MessagingWrite>(
        &self,
        tx: &mut W,
        path: &DurablePath,
        cid: &Id,
        eid: &Id,
        bytes: &[u8],
    ) -> Result<(), ProjectionError> {
        let snap: ParticipantReceivedSnapshot =
            records::decode_json(bytes).map_err(|e| refuse(path, e.to_string()))?;
        agree(
            path,
            snap.schema_version == SCHEMA_VERSION,
            "schema_version",
        )?;
        agree(
            path,
            snap.organization_id == self.organization_id,
            "organization_id",
        )?;
        agree(path, snap.conversation_id == *cid, "conversation_id")?;
        agree(path, snap.employee_id == *eid, "employee_id")?;
        tx.remove_recipient_states_for_participant(cid, eid)?;
        for state in snap.states.values() {
            tx.put_recipient_state(state)?;
        }
        Ok(())
    }

    fn apply_reactions<W: MessagingWrite>(
        &self,
        tx: &mut W,
        path: &DurablePath,
        cid: &Id,
        eid: &Id,
        mid: &str,
        bytes: &[u8],
    ) -> Result<(), ProjectionError> {
        let file: ParticipantReactionsFile =
            records::decode_json(bytes).map_err(|e| refuse(path, e.to_string()))?;
        agree(
            path,
            file.schema_version == SCHEMA_VERSION,
            "schema_version",
        )?;
        agree(
            path,
            file.organization_id == self.organization_id,
            "organization_id",
        )?;
        agree(path, file.conversation_id == *cid, "conversation_id")?;
        agree(path, file.employee_id == *eid, "employee_id")?;
        agree(path, file.message_id == mid, "message_id")?;
        tx.put_participant_reactions(mid, eid, &file.reactions)?;
        Ok(())
    }

    fn apply_answer<W: MessagingWrite>(
        &self,
        tx: &mut W,
        path: &DurablePath,
        cid: &Id,
        eid: &Id,
        mid: &str,
        bytes: &[u8],
    ) -> Result<(), ProjectionError> {
        let file: ParticipantAnswerFile =
            records::decode_json(bytes).map_err(|e| refuse(path, e.to_string()))?;
        agree(
            path,
            file.schema_version == SCHEMA_VERSION,
            "schema_version",
        )?;
        agree(
            path,
            file.organization_id == self.organization_id,
            "organization_id",
        )?;
        agree(path, file.conversation_id == *cid, "conversation_id")?;
        agree(path, file.employee_id == *eid, "employee_id")?;
        agree(path, file.message_id == mid, "message_id")?;
        tx.put_participant_answer(&file)?;
        Ok(())
    }
}

fn refuse(path: &DurablePath, reason: impl Into<String>) -> ProjectionError {
    ProjectionError::new(path, reason)
}

fn agree(path: &DurablePath, ok: bool, field: &str) -> Result<(), ProjectionError> {
    if ok {
        Ok(())
    } else {
        Err(refuse(path, format!("messaging record {field} mismatch")))
    }
}

impl<E: Engine> Projector<E> for MessagingFiles
where
    E::Tx: MessagingRead + MessagingWrite,
{
    fn owns(&self, path: &DurablePath) -> bool {
        records::kind_of(path).is_some()
    }

    fn apply(
        &self,
        tx: &mut E::Tx,
        path: &DurablePath,
        bytes: Option<&[u8]>,
    ) -> Result<(), ProjectionError> {
        let Some(kind) = records::kind_of(path) else {
            return Err(refuse(path, "not a messaging file"));
        };
        match (kind, bytes) {
            (Kind::Conversation(cid), Some(b)) => self.apply_conversation(tx, path, &cid, b)?,
            (Kind::Conversation(cid), None) => tx.remove_conversation(&cid)?,
            (Kind::Message(cid, mid), Some(b)) => self.apply_message(tx, path, &cid, &mid, b)?,
            (Kind::Message(_, mid), None) => tx.remove_message(&mid)?,
            (Kind::ParticipantState(cid, eid), Some(b)) => {
                self.apply_participant_state(tx, path, &cid, &eid, b)?
            }
            (Kind::ParticipantState(cid, eid), None) => tx.put_preferences(&cid, &eid, &[])?,
            (Kind::ParticipantReceivedSnapshot(cid, eid), Some(b)) => {
                self.apply_received_snapshot(tx, path, &cid, &eid, b)?
            }
            (Kind::ParticipantReceivedSnapshot(cid, eid), None) => {
                tx.remove_recipient_states_for_participant(&cid, &eid)?
            }
            (Kind::ParticipantReceivedSegment(cid, eid, _), Some(b)) => {
                let rows = records::decode_jsonl(b).map_err(|e| refuse(path, e.to_string()))?;
                for row in &rows {
                    agree(
                        path,
                        row.organization_id == self.organization_id,
                        "organization_id",
                    )?;
                    agree(path, row.conversation_id == cid, "conversation_id")?;
                    agree(path, row.employee_id == eid, "employee_id")?;
                    tx.put_recipient_state(row)?;
                }
            }
            (Kind::ParticipantReceivedSegment(_, _, _), None) => {}
            (Kind::ParticipantReactions(cid, eid, mid), Some(b)) => {
                self.apply_reactions(tx, path, &cid, &eid, &mid, b)?
            }
            (Kind::ParticipantReactions(_, eid, mid), None) => {
                tx.put_participant_reactions(&mid, &eid, &[])?
            }
            (Kind::ParticipantAnswer(cid, eid, mid), Some(b)) => {
                self.apply_answer(tx, path, &cid, &eid, &mid, b)?
            }
            (Kind::ParticipantAnswer(_, eid, mid), None) => {
                tx.remove_participant_answer(&mid, &eid)?
            }
        }
        Ok(())
    }

    fn validate(&self, tx: &E::Tx) -> Result<(), ProjectionError> {
        let conversations = tx.conversations()?;
        if conversations.is_empty() {
            return Ok(());
        }
        for conv in &conversations {
            let path = records::conversation_path(&conv.id);
            if conv.organization_id != self.organization_id {
                return Err(refuse(&path, "conversation organization_id mismatch"));
            }
        }
        Ok(())
    }
}
