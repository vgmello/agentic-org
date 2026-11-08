use org_proto::msg::{HeaderPage, MessageHeader, ThreadPage};
use org_proto::{Member, Position};

use crate::{
    AuthError, BindTarget, CommitMessageOutcome, DeliveredMessage, HeaderQuery,
    LocalStateTransition, MessageEvent, NewStoredMessage, NotifyCandidate, PeekUnreadOutcome,
    PeerToken, SnoozeDeadline, StoreError, StoredMessage, SyncedMessageState, ThreadQuery,
};

/// org.db at the office's applied position. Returns stored status, without filtering:
/// Mailbox applies the policy (paused is allowed, offboarded/unknown is refused).
/// Owner before exact ids before case-insensitive handles; display names never resolve.
pub trait MemberDirectory {
    fn resolve_member(&self, id_or_handle: &str) -> Result<Option<(Member, Position)>, StoreError>;
}

/// Live run tokens and peer ancestry; implemented by the office's session service.
pub trait SessionAuth {
    fn requires_token(&self, member_id: &str) -> Result<bool, AuthError>;
    fn verify(
        &self,
        who: BindTarget<'_>,
        token: &str,
        peer: PeerToken,
    ) -> Result<String, AuthError>;
    fn allowed_recipients(&self, run_id: &str) -> Option<Vec<String>>;
}

/// The ADR-0003 §4.3 storage port. All calls are synchronous and hold no mailbox
/// locks. Implementations own transaction atomicity and their database timeouts.
pub trait MessageStore {
    /// Atomic insert or matching replay; an id with different sender/hash is AlreadyExists.
    /// Never insert an initially Read message into the unread index.
    fn commit_message(&self, msg: NewStoredMessage) -> Result<CommitMessageOutcome, StoreError>;
    fn claim_read(
        &self,
        to_member_id: &str,
        id: Option<&str>,
        now_utc: &str,
    ) -> Result<Option<StoredMessage>, StoreError>;
    fn update_state_local(&self, change: LocalStateTransition)
        -> Result<MessageHeader, StoreError>;
    fn apply_synced_state(&self, synced: SyncedMessageState) -> Result<bool, StoreError>;
    fn check_notify(
        &self,
        to_member_id: &str,
        after_task: bool,
        urgent_only: bool,
        sweep_before_utc: &str,
        now_utc: &str,
    ) -> Result<Vec<NotifyCandidate>, StoreError>;
    fn list_active_snooze_deadlines(&self) -> Result<Vec<SnoozeDeadline>, StoreError>;
    fn peek_next_unread(
        &self,
        to_member_id: &str,
        from_member_id: Option<&str>,
        urgent_only: bool,
        now_utc: &str,
    ) -> Result<PeekUnreadOutcome, StoreError>;
    fn get_message(&self, id: &str) -> Result<Option<StoredMessage>, StoreError>;
    fn find_reply_for_ask(
        &self,
        ask_id: &str,
        responder_id: &str,
    ) -> Result<Option<StoredMessage>, StoreError>;
    fn resolve_pending_references(
        &self,
        applied_pos: Position,
        now_utc: &str,
    ) -> Result<Vec<DeliveredMessage>, StoreError>;
    fn list_headers(&self, query: HeaderQuery) -> Result<HeaderPage, StoreError>;
    fn list_threads(&self, query: ThreadQuery) -> Result<ThreadPage, StoreError>;
}

/// Called after a successful commit, outside every mailbox lock.
pub trait MessageEvents: Send + Sync {
    fn publish(&self, event: MessageEvent);
}

#[derive(Debug, Default)]
pub struct NoMessageEvents;

impl MessageEvents for NoMessageEvents {
    fn publish(&self, _event: MessageEvent) {}
}
