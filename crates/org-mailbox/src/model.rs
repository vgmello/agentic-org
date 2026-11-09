use std::fmt;

use org_proto::msg::{
    Message, MessageHeader, MessageLifecycleState, MessageReceivedData, MessageStateChangedData,
    NotifyReason, ThreadCursor,
};
use org_proto::Position;

pub type ContentHash = [u8; 32];

/// A local creation leaves origin_seq unset; the writer allocates it atomically
/// with local_seq. A replicated creation supplies its immutable origin sequence.
/// Header local_seq and initial lifecycle fields are assigned by the store.
#[derive(Debug, Clone)]
pub struct NewStoredMessage {
    pub message: Message,
    pub origin_office_id: String,
    pub origin_incarnation: String,
    pub origin_seq: Option<u64>,
    pub org_position: Position,
    pub content_hash: ContentHash,
    pub initial_state: MessageLifecycleState,
}

/// A message plus durable provenance, kept even when a cold body is pruned.
/// Message's Debug implementation redacts the body and meta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMessage {
    pub message: Message,
    pub origin_office_id: String,
    pub origin_incarnation: String,
    pub origin_seq: u64,
    pub org_position: Position,
    pub content_hash: ContentHash,
}

#[derive(Debug, Clone)]
pub struct CommitMessageOutcome {
    pub message: StoredMessage,
    /// False for an idempotent replay; it must not duplicate events or counters.
    pub inserted: bool,
}

/// One local lifecycle change ([`crate::TriageStore::update_state_local`]). `reason` is the
/// snooze or done note, so `Debug` prints only whether there is one.
#[derive(Clone)]
pub struct LocalStateTransition {
    pub id: String,
    pub to_member_id: String,
    pub state: MessageLifecycleState,
    pub snoozed_until: Option<String>,
    pub snooze_after_task: bool,
    pub reason: Option<String>,
    pub now_utc: String,
    pub owner_office_id: String,
    pub origin_incarnation: String,
    pub org_position: Position,
    /// With `Unread`: flags an ask to a person or the owner whose deadline passed (it stays
    /// unread). Ignored with every other state.
    pub expired_waiting_person: bool,
}

impl fmt::Debug for LocalStateTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalStateTransition")
            .field("id", &self.id)
            .field("to_member_id", &self.to_member_id)
            .field("state", &self.state)
            .field("snoozed_until", &self.snoozed_until)
            .field("snooze_after_task", &self.snooze_after_task)
            .field("reason", &self.reason.as_ref().map(|_| "<note>"))
            .field("now_utc", &self.now_utc)
            .field("owner_office_id", &self.owner_office_id)
            .field("origin_incarnation", &self.origin_incarnation)
            .field("org_position", &self.org_position)
            .field("expired_waiting_person", &self.expired_waiting_person)
            .finish()
    }
}

/// Domain representation of ADR-0003's synced state, until org-sync-proto lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncedMessageState {
    pub origin_office_id: String,
    pub origin_incarnation: String,
    pub origin_seq: u64,
    pub org_position: Position,
    pub msg_id: String,
    pub to_member_id: String,
    pub state: MessageLifecycleState,
    pub triage_gen: u64,
    pub snoozed_until: Option<String>,
    pub snooze_after_task: bool,
    pub reason: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct NotifyCandidate {
    pub header: MessageHeader,
    pub reason: NotifyReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnoozeDeadline {
    pub at_utc: String,
    pub to_member_id: String,
    pub id: String,
}

#[derive(Debug, Clone, Default)]
pub struct PeekUnreadOutcome {
    pub message: Option<StoredMessage>,
    /// At most [`crate::MAX_EXPIRED_IDS`], oldest first (commit order).
    pub expired_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DeliveredMessage {
    pub message: StoredMessage,
}

/// Visibility is explicit. Global scope is used only by an app-authorized caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderScope {
    Inbox(String),
    Participant(String),
    All,
}

#[derive(Debug, Clone)]
pub struct HeaderQuery {
    pub scope: HeaderScope,
    pub from_member_id: Option<String>,
    pub thread_id: Option<String>,
    pub unread_only: bool,
    pub include_snoozed: bool,
    pub cursor: Option<u64>,
    pub limit: u32,
}

#[derive(Debug, Clone)]
pub struct ThreadQuery {
    pub participant: Option<String>,
    pub cursor: Option<ThreadCursor>,
    pub limit: u32,
}

/// The kernel-supplied identity of a local caller; never accepted from wire JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerToken {
    pub pid: u32,
    pub pid_version: u32,
}

#[derive(Debug, Clone, Copy)]
pub enum BindTarget<'a> {
    Member(&'a str),
    /// No member was supplied: the verifier finds the one-off run by its token.
    OneOff,
}

/// Subscriber fan-out is outside the domain. These payloads never contain bodies.
#[derive(Debug, Clone)]
pub enum MessageEvent {
    Received(Box<MessageReceivedData>),
    StateChanged(MessageStateChangedData),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_state_transition_debug_hides_the_note() {
        let change = LocalStateTransition {
            id: "m1".into(),
            to_member_id: "ada".into(),
            state: MessageLifecycleState::Done,
            snoozed_until: None,
            snooze_after_task: false,
            reason: Some("private reasoning".into()),
            now_utc: "2026-11-08T12:00:00.000Z".into(),
            owner_office_id: "office".into(),
            origin_incarnation: "inc".into(),
            org_position: Position { epoch: 1, seq: 1 },
            expired_waiting_person: false,
        };
        let debug = format!("{change:?}");
        assert!(!debug.contains("private"), "{debug}");
        assert!(debug.contains("<note>") && debug.contains("m1"), "{debug}");
    }
}
