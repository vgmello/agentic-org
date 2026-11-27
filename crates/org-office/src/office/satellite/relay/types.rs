//! Shared relay types: the public inbound outcome summaries and outbound journal entries, the
//! parsed relayed deltas and recipient context passed between the inbound stages, and the
//! conversation read operations and message expiry used by both directions.

use org_store::Id;
use serde::{Deserialize, Serialize};

use crate::office::satellite::placement::PlacementContext;

/// Structured answer summary returned when an inbound relayed reply closes a local ask whose
/// asker is an active agent owned by this office (`TASK-52` P6-#2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayedAskAnswer {
    pub ask_id: String,
    pub asker_id: String,
    pub question_summary: String,
    pub answer_summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachment_ids: Vec<String>,
}

/// Structured withdrawal summary returned when an inbound relayed withdrawal closes a local ask
/// whose asker is an active agent owned by this office (`TASK-52` P13-#4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayedAskWithdrawal {
    pub ask_id: String,
    pub asker_id: String,
}

/// Exact wake action derived for one local active agent recipient of a relayed message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayedRecipientWake {
    pub employee_id: String,
    pub kind: String,
}

/// Summary of a durably committed inbound relayed native message so the destination office can
/// derive and deduplicate local agent wakes, resume suspended required askers, and notify local
/// waiters/live streams (`TASK-52` P4-#8 / P6-#2, `TASK-117`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RelayedMessageOutcome {
    pub message_id: String,
    pub conversation_id: String,
    pub sender_id: String,
    pub sender_name: String,
    pub sender_is_person: bool,
    pub is_dm: bool,
    #[serde(default)]
    pub inserted: bool,
    #[serde(default)]
    pub conversation_kind: Option<org_proto::chat::ConversationKind>,
    pub summary: String,
    pub local_agent_recipients: Vec<String>,
    #[serde(default)]
    pub local_recipient_wakes: Vec<RelayedRecipientWake>,
    #[serde(default)]
    pub local_delivered_headers: Vec<org_proto::msg::MessageHeader>,
    #[serde(default)]
    pub answered_ask: Option<RelayedAskAnswer>,
    #[serde(default)]
    pub withdrawn_ask: Option<RelayedAskWithdrawal>,
    #[serde(default)]
    pub closed_ask_id: Option<String>,
    #[serde(default)]
    pub closed_ask_reason: Option<String>,
    #[serde(default)]
    pub handoff_accepted: bool,
    #[serde(skip)]
    pub delivered_message: Option<crate::messaging::records::MessageRecord>,
}

pub(super) struct ParsedOriginAskClose {
    pub(super) conv: Option<crate::messaging::records::ConversationRecord>,
    pub(super) ask_msg: Option<crate::messaging::records::MessageRecord>,
    pub(super) cid: Id,
    pub(super) emp_id: Id,
    pub(super) ask_id: String,
    pub(super) answer: crate::messaging::records::ParticipantAnswerFile,
}

pub(super) struct ParsedHandoffAccept {
    pub(super) cid: Id,
    pub(super) emp_id: Id,
    pub(super) message_id: String,
    pub(super) answer: crate::messaging::records::ParticipantAnswerFile,
}

pub(super) struct RelayedRecipientCtx<'a> {
    pub(super) here: &'a org_store::OfficeIdentity,
    pub(super) owner: &'a Id,
    pub(super) placement: &'a PlacementContext,
    pub(super) employees: &'a [crate::organization::records::Employee],
    pub(super) conv: &'a crate::messaging::records::ConversationRecord,
    pub(super) active_members: &'a [Id],
    pub(super) msg: &'a crate::messaging::records::MessageRecord,
}

/// One locally committed native messaging journal entry ready for cross-office relay publication.
#[derive(Clone, Debug)]
pub struct PendingMessageJournalEntry {
    pub ordinal: u64,
    pub conversation_id: String,
    pub message_id: String,
    pub files: Vec<(String, Option<String>)>,
    /// Target offices determined by recipient placement (for DMs/GroupDMs) or channel membership
    /// (`TASK-52` P3-#1 / P4-#2).
    pub destination_offices: Vec<String>,
    /// Native message expiry timestamp in Unix ms (e.g. non-required ask expiry), preserved in
    /// transport deadlines (`TASK-52` P4-#9).
    pub expires_at_ms: Option<u64>,
}

pub(super) struct ReadConversationOp {
    pub(super) conversation_id: Id,
}

impl crate::store::open::StoreOp for ReadConversationOp {
    type Out = Result<Option<crate::messaging::records::ConversationRecord>, org_store::StoreError>;

    fn run<E: crate::store::open::OfficeEngine>(self, store: &org_store::Store<E>) -> Self::Out {
        let (conv, _) = store.read(|snap| {
            crate::messaging::ports::MessagingRead::conversation(snap, &self.conversation_id)
                .map_err(org_store::StoreError::Engine)
        })?;
        Ok(conv)
    }
}

pub(super) struct ReadConversationMembersOp {
    pub(super) conv: crate::messaging::records::ConversationRecord,
    pub(super) owner: Id,
}

impl crate::store::open::StoreOp for ReadConversationMembersOp {
    type Out = Result<Vec<Id>, org_store::StoreError>;

    fn run<E: crate::store::open::OfficeEngine>(self, store: &org_store::Store<E>) -> Self::Out {
        let (members, _) = store.read(|snap| {
            crate::messaging::features::effective_auto_members(snap, &self.conv, &self.owner)
                .map_err(org_store::StoreError::Engine)
        })?;
        Ok(members)
    }
}

pub(super) fn message_expires_at_ms(msg: &crate::messaging::records::MessageRecord) -> Option<u64> {
    if let Some(exp) = msg.expires_at.as_deref() {
        return crate::parse_rfc3339_ms(exp);
    }
    msg.ttl_ms.and_then(|ttl| {
        crate::parse_rfc3339_ms(&msg.created_at).map(|created| created.saturating_add(ttl))
    })
}
