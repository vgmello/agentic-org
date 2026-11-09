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

#[derive(Debug, Clone)]
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
