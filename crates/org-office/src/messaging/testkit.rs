//! MemoryEngine `FamilyTable` implementation for [`MessagingRead`] and [`MessagingWrite`].

use std::{any::Any, collections::BTreeMap};

use org_store::{
    testkit::{FamilyTable, MemorySnapshot, MemoryTx},
    EngineError, Id,
};

use super::{
    ports::{MessagingRead, MessagingWrite},
    records::{
        ConversationPreferenceRecord, ConversationReadMarkerRecord, ConversationRecord,
        MessageReactionRecord, MessageRecord, ParticipantAnswerFile, RecipientStateRecord,
    },
};

#[derive(Clone, Default, Debug)]
pub struct MessagingTables {
    conversations: BTreeMap<Id, ConversationRecord>,
    messages: BTreeMap<String, (u64, MessageRecord)>,
    recipient_states: BTreeMap<(String, Id), RecipientStateRecord>,
    read_markers: BTreeMap<(Id, Id), ConversationReadMarkerRecord>,
    preferences: BTreeMap<(Id, Id), Vec<ConversationPreferenceRecord>>,
    reactions: BTreeMap<(String, Id), Vec<MessageReactionRecord>>,
    answers: BTreeMap<(String, Id), ParticipantAnswerFile>,
    max_local_seq: u64,
}

impl FamilyTable for MessagingTables {
    fn name(&self) -> &'static str {
        "messaging"
    }

    fn clone_box(&self) -> Box<dyn FamilyTable> {
        Box::new(self.clone())
    }

    fn dump(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for (k, v) in &self.conversations {
            lines.push(format!(
                "conversation {k} rev={} kind={:?}",
                v.revision, v.kind
            ));
        }
        for (k, (_, v)) in &self.messages {
            lines.push(format!(
                "message {k} conv={} sha={}",
                v.conversation_id, v.content_sha256
            ));
        }
        for ((mid, eid), v) in &self.recipient_states {
            lines.push(format!(
                "recipient_state {mid} {eid} state={:?} gen={} because={:?}",
                v.state, v.triage_generation, v.delivered_because
            ));
        }
        for ((cid, eid), v) in &self.read_markers {
            lines.push(format!(
                "read_marker {cid} {eid} last={:?}",
                v.last_read_message_id
            ));
        }
        for ((mid, eid), v) in &self.reactions {
            lines.push(format!("reactions {mid} {eid} count={}", v.len()));
        }
        for ((mid, eid), v) in &self.answers {
            lines.push(format!(
                "answer {mid} {eid} accepted_by={:?}",
                v.accepted_by
            ));
        }
        lines.sort();
        lines
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

fn read<R: Default>(
    tables: Option<&MessagingTables>,
    query: impl FnOnce(&MessagingTables) -> R,
) -> Result<R, EngineError> {
    Ok(tables.map(query).unwrap_or_default())
}

macro_rules! memory_messaging_read {
    ($($view:ty),+) => {$(
        impl MessagingRead for $view {
            fn conversation(&self, id: &Id) -> Result<Option<ConversationRecord>, EngineError> {
                read(self.table(), |t| t.conversations.get(id).cloned())
            }

            fn conversations(&self) -> Result<Vec<ConversationRecord>, EngineError> {
                read(self.table(), |t| t.conversations.values().cloned().collect())
            }

            fn message(&self, id: &str) -> Result<Option<MessageRecord>, EngineError> {
                read(self.table(), |t| t.messages.get(id).map(|(_, m)| m.clone()))
            }

            fn messages_in_conversation(
                &self,
                conversation_id: &Id,
            ) -> Result<Vec<MessageRecord>, EngineError> {
                read(self.table(), |t| {
                    let mut list: Vec<(u64, MessageRecord)> = t
                        .messages
                        .values()
                        .filter(|(_, m)| m.conversation_id == *conversation_id)
                        .cloned()
                        .collect();
                    list.sort_by(|a, b| {
                        a.1.created_at
                            .cmp(&b.1.created_at)
                            .then_with(|| a.0.cmp(&b.0))
                            .then_with(|| a.1.id.cmp(&b.1.id))
                    });
                    list.into_iter().map(|(_, m)| m).collect()
                })
            }

            fn all_messages(&self) -> Result<Vec<MessageRecord>, EngineError> {
                read(self.table(), |t| {
                    let mut list: Vec<(u64, MessageRecord)> = t.messages.values().cloned().collect();
                    list.sort_by_key(|(seq, _)| *seq);
                    list.into_iter().map(|(_, m)| m).collect()
                })
            }

            fn recipient_state(
                &self,
                message_id: &str,
                employee_id: &Id,
            ) -> Result<Option<RecipientStateRecord>, EngineError> {
                read(self.table(), |t| {
                    t.recipient_states
                        .get(&(message_id.to_owned(), employee_id.clone()))
                        .cloned()
                })
            }

            fn recipient_states_for_employee(
                &self,
                employee_id: &Id,
            ) -> Result<Vec<RecipientStateRecord>, EngineError> {
                read(self.table(), |t| {
                    t.recipient_states
                        .values()
                        .filter(|s| s.employee_id == *employee_id)
                        .cloned()
                        .collect()
                })
            }

            fn all_recipient_states(&self) -> Result<Vec<RecipientStateRecord>, EngineError> {
                read(self.table(), |t| t.recipient_states.values().cloned().collect())
            }

            fn read_marker(
                &self,
                conversation_id: &Id,
                viewer_employee_id: &Id,
            ) -> Result<Option<ConversationReadMarkerRecord>, EngineError> {
                read(self.table(), |t| {
                    t.read_markers
                        .get(&(conversation_id.clone(), viewer_employee_id.clone()))
                        .cloned()
                })
            }

            fn preferences(
                &self,
                conversation_id: &Id,
                viewer_employee_id: &Id,
            ) -> Result<Vec<ConversationPreferenceRecord>, EngineError> {
                read(self.table(), |t| {
                    t.preferences
                        .get(&(conversation_id.clone(), viewer_employee_id.clone()))
                        .cloned()
                        .unwrap_or_default()
                })
            }

            fn reactions_for_message(
                &self,
                message_id: &str,
            ) -> Result<Vec<MessageReactionRecord>, EngineError> {
                read(self.table(), |t| {
                    t.reactions
                        .iter()
                        .filter(|((mid, _), _)| mid == message_id)
                        .flat_map(|(_, list)| list.iter().cloned())
                        .collect()
                })
            }

            fn answer_for_message(
                &self,
                message_id: &str,
            ) -> Result<Option<ParticipantAnswerFile>, EngineError> {
                read(self.table(), |t| {
                    t.answers
                        .iter()
                        .filter(|((mid, _), _)| mid == message_id)
                        .min_by_key(|(_, ans)| {
                            (
                                !(ans.reason.as_deref() == Some("origin-close")
                                    && ans.answer.is_some()),
                                ans.accepted_at.as_deref().unwrap_or(""),
                                ans.employee_id.as_str(),
                            )
                        })
                        .map(|(_, ans)| ans.clone())
                })
            }

            fn max_origin_sequence(
                &self,
                origin_office_id: &Id,
                origin_incarnation: &str,
            ) -> Result<u64, EngineError> {
                read(self.table(), |t| {
                    t.messages
                        .values()
                        .filter(|(_, m)| {
                            m.origin_office_id == *origin_office_id
                                && m.origin_incarnation == origin_incarnation
                        })
                        .map(|(_, m)| m.origin_sequence)
                        .max()
                        .unwrap_or(0)
                })
            }

            fn max_state_sequence(
                &self,
                owner_office_id: &Id,
                origin_incarnation: &str,
            ) -> Result<u64, EngineError> {
                read(self.table(), |t| {
                    t.recipient_states
                        .values()
                        .filter(|s| {
                            s.owner_office_id == *owner_office_id
                                && s.origin_incarnation == origin_incarnation
                        })
                        .map(|s| s.origin_sequence)
                        .max()
                        .unwrap_or(0)
                })
            }

            fn max_local_seq(&self) -> Result<u64, EngineError> {
                read(self.table(), |t| t.max_local_seq)
            }

            fn message_local_seq(&self, message_id: &str) -> Result<Option<u64>, EngineError> {
                read(self.table(), |t| t.messages.get(message_id).map(|(seq, _)| *seq))
            }
        }
    )+};
}

memory_messaging_read!(MemoryTx, MemorySnapshot);

impl MessagingWrite for MemoryTx {
    fn put_conversation(&mut self, record: &ConversationRecord) -> Result<(), EngineError> {
        let tables: &mut MessagingTables = self.table_mut()?;
        tables
            .conversations
            .insert(record.id.clone(), record.clone());
        Ok(())
    }

    fn remove_conversation(&mut self, id: &Id) -> Result<(), EngineError> {
        let tables: &mut MessagingTables = self.table_mut()?;
        tables.conversations.remove(id);
        Ok(())
    }

    fn put_message(&mut self, record: &MessageRecord, local_seq: u64) -> Result<(), EngineError> {
        let tables: &mut MessagingTables = self.table_mut()?;
        let seq = tables
            .messages
            .get(&record.id)
            .map_or(local_seq, |(existing, _)| *existing);
        tables.max_local_seq = tables.max_local_seq.max(seq);
        tables
            .messages
            .insert(record.id.clone(), (seq, record.clone()));
        Ok(())
    }

    fn remove_message(&mut self, id: &str) -> Result<(), EngineError> {
        let tables: &mut MessagingTables = self.table_mut()?;
        tables.messages.remove(id);
        Ok(())
    }

    fn put_recipient_state(&mut self, record: &RecipientStateRecord) -> Result<(), EngineError> {
        let tables: &mut MessagingTables = self.table_mut()?;
        tables.recipient_states.insert(
            (record.message_id.clone(), record.employee_id.clone()),
            record.clone(),
        );
        Ok(())
    }

    fn remove_recipient_states_for_participant(
        &mut self,
        conversation_id: &Id,
        employee_id: &Id,
    ) -> Result<(), EngineError> {
        let tables: &mut MessagingTables = self.table_mut()?;
        tables.recipient_states.retain(|_, s| {
            !(s.conversation_id == *conversation_id && s.employee_id == *employee_id)
        });
        Ok(())
    }

    fn put_read_marker(
        &mut self,
        record: &ConversationReadMarkerRecord,
    ) -> Result<(), EngineError> {
        let tables: &mut MessagingTables = self.table_mut()?;
        tables.read_markers.insert(
            (
                record.conversation_id.clone(),
                record.viewer_employee_id.clone(),
            ),
            record.clone(),
        );
        Ok(())
    }

    fn put_preferences(
        &mut self,
        conversation_id: &Id,
        viewer_employee_id: &Id,
        prefs: &[ConversationPreferenceRecord],
    ) -> Result<(), EngineError> {
        let tables: &mut MessagingTables = self.table_mut()?;
        let key = (conversation_id.clone(), viewer_employee_id.clone());
        if prefs.is_empty() {
            tables.preferences.remove(&key);
        } else {
            tables.preferences.insert(key, prefs.to_vec());
        }
        Ok(())
    }

    fn put_participant_reactions(
        &mut self,
        message_id: &str,
        employee_id: &Id,
        reactions: &[MessageReactionRecord],
    ) -> Result<(), EngineError> {
        let tables: &mut MessagingTables = self.table_mut()?;
        let key = (message_id.to_owned(), employee_id.clone());
        if reactions.is_empty() {
            tables.reactions.remove(&key);
        } else {
            tables.reactions.insert(key, reactions.to_vec());
        }
        Ok(())
    }

    fn put_participant_answer(
        &mut self,
        answer: &ParticipantAnswerFile,
    ) -> Result<(), EngineError> {
        let tables: &mut MessagingTables = self.table_mut()?;
        tables.answers.insert(
            (answer.message_id.clone(), answer.employee_id.clone()),
            answer.clone(),
        );
        Ok(())
    }

    fn remove_participant_answer(
        &mut self,
        message_id: &str,
        employee_id: &Id,
    ) -> Result<(), EngineError> {
        let tables: &mut MessagingTables = self.table_mut()?;
        tables
            .answers
            .remove(&(message_id.to_owned(), employee_id.clone()));
        Ok(())
    }
}
