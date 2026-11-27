//! Engine-independent read and write ports for the messaging family (`doc-90`, `ENG-1`).
//!
//! Extends [`ProjectsRead`] and [`ProjectsWrite`] so messaging operations validate active
//! employees, teams, and projects in one consistent snapshot or transaction.

use org_proto::resume::ResumeInboxSummary;
use org_store::{Engine, EngineError, Id};

use crate::projects::ports::{ProjectsRead, ProjectsWrite};

use super::records::{
    ConversationPreferenceRecord, ConversationReadMarkerRecord, ConversationRecord,
    MessageAttachmentRecord, MessageReactionRecord, MessageRecord, ParticipantAnswerFile,
    RecipientStateRecord,
};

/// One file bound to a message or to an ask answer, found by its attachment ID (`TASK-98.2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundMessageFile {
    /// The message the file is on, or the ask an answer's file answers.
    pub message_id: String,
    /// The answerer, for a file on an ask answer.
    pub answerer: Option<Id>,
    pub file: MessageAttachmentRecord,
}

/// Read-only queries over the messaging projection (`protected/messaging.db` on SQLite,
/// `MessagingTables` on Memory).
pub trait MessagingRead: ProjectsRead {
    fn conversation(&self, id: &Id) -> Result<Option<ConversationRecord>, EngineError>;
    fn conversations(&self) -> Result<Vec<ConversationRecord>, EngineError>;
    fn message(&self, id: &str) -> Result<Option<MessageRecord>, EngineError>;
    fn messages_in_conversation(
        &self,
        conversation_id: &Id,
    ) -> Result<Vec<MessageRecord>, EngineError>;
    fn all_messages(&self) -> Result<Vec<MessageRecord>, EngineError>;
    fn recipient_state(
        &self,
        message_id: &str,
        employee_id: &Id,
    ) -> Result<Option<RecipientStateRecord>, EngineError>;
    fn recipient_states_for_employee(
        &self,
        employee_id: &Id,
    ) -> Result<Vec<RecipientStateRecord>, EngineError>;
    fn all_recipient_states(&self) -> Result<Vec<RecipientStateRecord>, EngineError>;
    fn read_marker(
        &self,
        conversation_id: &Id,
        viewer_employee_id: &Id,
    ) -> Result<Option<ConversationReadMarkerRecord>, EngineError>;
    fn preferences(
        &self,
        conversation_id: &Id,
        viewer_employee_id: &Id,
    ) -> Result<Vec<ConversationPreferenceRecord>, EngineError>;
    fn reactions_for_message(
        &self,
        message_id: &str,
    ) -> Result<Vec<MessageReactionRecord>, EngineError>;
    fn answer_for_message(
        &self,
        message_id: &str,
    ) -> Result<Option<ParticipantAnswerFile>, EngineError>;
    /// Highest `origin_sequence` allocated by `(origin_office_id, origin_incarnation)`.
    fn max_origin_sequence(
        &self,
        origin_office_id: &Id,
        origin_incarnation: &str,
    ) -> Result<u64, EngineError>;
    /// Highest `origin_sequence` across all recipient states owned by `(owner_office_id, origin_incarnation)`.
    fn max_state_sequence(
        &self,
        owner_office_id: &Id,
        origin_incarnation: &str,
    ) -> Result<u64, EngineError>;
    /// Highest local sequence across all messages (for `org-mailbox` cursor paging).
    fn max_local_seq(&self) -> Result<u64, EngineError>;
    /// Office-local sequence assigned to `message_id` (for monotonic cursor paging).
    fn message_local_seq(&self, message_id: &str) -> Result<Option<u64>, EngineError>;
    /// The message or ask-answer file whose attachment ID is `attachment_id`.
    fn message_file(&self, attachment_id: &str) -> Result<Option<BoundMessageFile>, EngineError>;
    /// Bounded caller-scoped unread inbox counts and newest unread headers (`TASK-220`).
    fn unread_inbox_summary(
        &self,
        employee_id: &Id,
        limit: usize,
    ) -> Result<ResumeInboxSummary, EngineError>;
    /// Bounded unanswered ask candidates `(message, caller_is_recipient)` for `employee_id`.
    fn open_asks_for_employee(
        &self,
        employee_id: &Id,
        limit: usize,
    ) -> Result<Vec<(MessageRecord, bool)>, EngineError>;
}

/// Write operations on the messaging projection during a transaction.
pub trait MessagingWrite: MessagingRead + ProjectsWrite {
    fn put_conversation(&mut self, record: &ConversationRecord) -> Result<(), EngineError>;
    fn remove_conversation(&mut self, id: &Id) -> Result<(), EngineError>;
    fn put_message(&mut self, record: &MessageRecord, local_seq: u64) -> Result<(), EngineError>;
    fn remove_message(&mut self, id: &str) -> Result<(), EngineError>;
    fn put_recipient_state(&mut self, record: &RecipientStateRecord) -> Result<(), EngineError>;
    fn remove_recipient_states_for_participant(
        &mut self,
        conversation_id: &Id,
        employee_id: &Id,
    ) -> Result<(), EngineError>;
    fn put_read_marker(&mut self, record: &ConversationReadMarkerRecord)
        -> Result<(), EngineError>;
    fn put_preferences(
        &mut self,
        conversation_id: &Id,
        viewer_employee_id: &Id,
        prefs: &[ConversationPreferenceRecord],
    ) -> Result<(), EngineError>;
    fn put_participant_reactions(
        &mut self,
        message_id: &str,
        employee_id: &Id,
        reactions: &[MessageReactionRecord],
    ) -> Result<(), EngineError>;
    fn put_participant_answer(&mut self, answer: &ParticipantAnswerFile)
        -> Result<(), EngineError>;
    fn remove_participant_answer(
        &mut self,
        message_id: &str,
        employee_id: &Id,
    ) -> Result<(), EngineError>;
}

pub trait MessagingEngine: Engine<Tx: MessagingWrite, Snapshot: MessagingRead> {}

impl<E> MessagingEngine for E where E: Engine<Tx: MessagingWrite, Snapshot: MessagingRead> {}
