use org_proto::msg::{HeaderPage, MessageHeader, ThreadPage};
use org_proto::{Member, Position};

use crate::{
    AuthError, BindTarget, CommitMessageOutcome, DeliveredMessage, HeaderQuery,
    LocalStateTransition, MessageEvent, NewStoredMessage, NotifyCandidate, PeekUnreadOutcome,
    PeerToken, SnoozeDeadline, StoreError, StoredMessage, SyncedMessageState, ThreadQuery,
};

/// A read-only port into org.db, at the office's applied position. Returns the stored
/// member with its status, unfiltered: the Mailbox applies the policy (a paused agent may
/// bind and receive; an offboarded or Unknown-status one is refused). Resolution order:
/// "owner" (ignoring case) first, then an exact id, then a handle ignoring case (an
/// agent's messaging_name or a non-owner person's name). Display names never resolve and
/// "office" never resolves. Two members with one handle is a storage error. The Mailbox
/// passes the reference already trimmed of ASCII whitespace.
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

// The ADR-0003 §4.3 storage ports (amended 2026-11-08: store split). Four capability
// traits over office.db, with no default methods: each caller depends only on what it
// calls. All calls are synchronous; the Mailbox holds none of its locks across them.
// Implementations own transaction atomicity and their database timeouts. office.db and
// org.db (MemberDirectory) are separate databases: no transaction spans both.

/// Delivery: commit, claim, peek and look up messages. All the Mailbox's slice 1 needs.
pub trait MessageStore {
    /// Atomic insert, or a matching replay of the same id (`inserted: false`). The same id
    /// with a different sender or content hash is `StoreError::AlreadyExists`. A local
    /// commit passes `origin_seq: None`; the store allocates `local_seq` and `origin_seq`
    /// in the same transaction from the office's local counter (>= 1, increasing in commit
    /// order). An initially Read message never enters the unread index.
    ///
    /// `Ok` means durable and visible to every connection: a batching writer must not
    /// return before its COMMIT. The Mailbox wakes waiters as soon as this returns (§2.2).
    fn commit_message(&self, msg: NewStoredMessage) -> Result<CommitMessageOutcome, StoreError>;
    /// Atomically claim and mark read `id`, or with no `id` the oldest unsnoozed unread
    /// message, for `to_member_id`. `None` when `id` isn't addressed to the member, or with
    /// no `id` when nothing is unread. The claim sets `read_at` to exactly `now_utc`; an
    /// already read `id` comes back unchanged, keeping its earlier `read_at`. Concurrent
    /// claims of one message succeed once: with no `id`, the store selects the message and
    /// marks it in ONE writer transaction, never choosing it from a reader's snapshot.
    fn claim_read(
        &self,
        to_member_id: &str,
        id: Option<&str>,
        now_utc: &str,
    ) -> Result<Option<StoredMessage>, StoreError>;
    /// The oldest unexpired, unsnoozed unread message, without marking it read, plus the
    /// ids of expired unread messages passed over. Read-only: changes and deletes nothing.
    /// Each call reads a fresh snapshot, never a long-lived read transaction, so a waiter
    /// woken by a commit sees that commit (§2.2).
    fn peek_next_unread(
        &self,
        to_member_id: &str,
        from_member_id: Option<&str>,
        urgent_only: bool,
        now_utc: &str,
    ) -> Result<PeekUnreadOutcome, StoreError>;
    /// Like every read here, from a fresh snapshot.
    fn get_message(&self, id: &str) -> Result<Option<StoredMessage>, StoreError>;
    /// One page of headers, newest first by `local_seq`, without reading bodies, from a
    /// fresh snapshot.
    fn list_headers(&self, query: HeaderQuery) -> Result<HeaderPage, StoreError>;
    fn find_reply_for_ask(
        &self,
        ask_id: &str,
        responder_id: &str,
    ) -> Result<Option<StoredMessage>, StoreError>;
}

/// Triage: local state transitions, snooze release and the snooze timer seed.
pub trait TriageStore {
    fn update_state_local(&self, change: LocalStateTransition)
        -> Result<MessageHeader, StoreError>;
    fn check_notify(
        &self,
        to_member_id: &str,
        after_task: bool,
        urgent_only: bool,
        sweep_before_utc: &str,
        now_utc: &str,
    ) -> Result<Vec<NotifyCandidate>, StoreError>;
    fn list_active_snooze_deadlines(&self) -> Result<Vec<SnoozeDeadline>, StoreError>;
}

/// Sync: apply what other offices committed.
pub trait SyncStore {
    fn apply_synced_state(&self, synced: SyncedMessageState) -> Result<bool, StoreError>;
    fn resolve_pending_references(
        &self,
        applied_pos: Position,
        now_utc: &str,
    ) -> Result<Vec<DeliveredMessage>, StoreError>;
}

/// Threads: thread summaries for `msg.threads`, without reading bodies.
pub trait ThreadStore {
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
