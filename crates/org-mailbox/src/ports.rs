use org_proto::msg::{HeaderPage, MessageHeader, ThreadPage};
use org_proto::{Member, Position};

use crate::{
    AuthError, BindTarget, CommitMessageOutcome, DeliveredMessage, HeaderQuery,
    LocalStateTransition, MessageEvent, NewStoredMessage, NotifyCandidate, PeekUnreadOutcome,
    PeerToken, SnoozeDeadline, StoreError, StoredMessage, SyncedMessageState, ThreadQuery,
};

/// The most expired ids one `peek_next_unread` reports ([`PeekUnreadOutcome::expired_ids`]).
pub const MAX_EXPIRED_IDS: usize = 100;

/// A read-only port into org.db, at the office's applied position. Returns the stored
/// member with its status, unfiltered: the Mailbox applies the policy (a paused agent may
/// bind and receive; an offboarded or Unknown-status one is refused). Resolution order:
/// "owner" (ignoring ASCII case) first, then an exact id, then a handle ignoring case (an
/// agent's messaging_name or a non-owner person's trimmed name). Handles fold case by
/// Unicode lower case (`str::to_lowercase`, as org-office keys them), not ASCII-only:
/// SQLite's `lower()` and `NOCASE` don't match "Émile" to "émile". Display names never
/// resolve and "office" never resolves. Two members with one handle is a storage error.
/// The Mailbox passes the reference already trimmed of ASCII whitespace.
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
    /// ids of the expired open unread messages: at most [`MAX_EXPIRED_IDS`], the oldest
    /// first (commit order), so the ids one call reports, and the batch the caller expires,
    /// stay bounded however many requests expired; once the caller expires them, the next
    /// call reports the rest. Read-only: changes and deletes nothing. Each call reads a
    /// fresh snapshot, never a long-lived read transaction, so a waiter woken by a commit
    /// sees that commit (§2.2).
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

/// Triage: local state transitions, snooze release and the snooze timer seed. Every call is
/// one atomic step; an `Ok` from a call that changes state gives the same guarantee as
/// [`MessageStore::commit_message`] (durable and visible to every connection), because the
/// Mailbox wakes waiters right after it. The conformance cases are the testkit's
/// `triage_conformance_tests!`.
pub trait TriageStore {
    /// Change one message's lifecycle state at its owning office; returns its header after the
    /// change. `StoreError::NotFound` when `change.id` doesn't exist or isn't addressed to
    /// `change.to_member_id`. The allowed transitions, by target `change.state`:
    ///
    /// - `Snoozed`, from `unread`, `read` or `snoozed`: back into the unread index, with
    ///   exactly one of `snoozed_until` and `snooze_after_task` set (both or neither is
    ///   `InvalidTransition`); `snooze_note = reason`, `triage_gen += 1`, and the message
    ///   counts as triaged from now on (`triaged_at = now_utc`). Re-snoozing always applies,
    ///   bumping `triage_gen` again.
    /// - `Done`, from `unread`, `read` or `snoozed`: leaves the unread index (its snooze
    ///   cleared); `reason` is the done note.
    /// - `Read`, from `unread` or `snoozed`: leaves the unread index (its snooze cleared);
    ///   `read_at = now_utc`.
    /// - `Unread`, from `read`: back into the unread index, `read_at` cleared. Also from
    ///   `unread`, to set `expired_waiting_person` (a set flag is kept: `false` never clears
    ///   it, so unread to unread without the flag changes nothing). The flag is read only
    ///   with this target, and only from `unread`: `read` to `unread` with it set is
    ///   `InvalidTransition` (a message read meanwhile stays read).
    /// - `Expired`, from `unread` or `snoozed`: leaves the unread index (its snooze cleared).
    ///
    /// The same `Read`, `Done` or `Expired` again is idempotent: the header comes back
    /// unchanged. Any other pair is
    /// `StoreError::InvalidTransition`: nothing leaves `done`, `expired` or `dead_letter`, and
    /// `DeadLetter` and `Unknown` are never targets here (dead-lettering is a later slice).
    fn update_state_local(&self, change: LocalStateTransition)
        -> Result<MessageHeader, StoreError>;
    /// In one atomic step, for `to_member_id`: release every time snooze with
    /// `snoozed_until <= now_utc` (to `unread`, `triage_gen += 1`, reason `Due`) and, with
    /// `after_task`, every after-task snooze too (reason `AfterTask`); and select every open
    /// (unread, unsnoozed) message, reason `New` when `after_task` is set or the message is
    /// urgent, `Sweep` when it was never triaged (never snoozed) and its `created_at` is
    /// before `sweep_before_utc`, and otherwise not returned. `urgent_only` filters only the
    /// returned list to urgent messages: due snoozes are released whatever it says. Each
    /// candidate carries its header after the release. No limit and no ordering guarantee.
    /// Expiry is not its job: an overdue request comes back as it is.
    fn check_notify(
        &self,
        to_member_id: &str,
        after_task: bool,
        urgent_only: bool,
        sweep_before_utc: &str,
        now_utc: &str,
    ) -> Result<Vec<NotifyCandidate>, StoreError>;
    /// Every time-based snooze in the unread index, to seed the Mailbox's snooze timers.
    /// After-task snoozes aren't timers and aren't listed.
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
