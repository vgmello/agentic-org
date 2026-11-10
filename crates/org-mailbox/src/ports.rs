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
// calls. All calls are synchronous; the Mailbox holds none of its registry locks across
// them. It holds one lock, which only orders its own triage writes, across an
// `update_state_local` and across the timer-driven `check_notify` that releases due snoozes
// (with a failed defer's `get_message` re-read), so a slow call there delays other local
// triage writes.
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
    /// At most one `delivery_failed` per `reply_to`: a second one (another id, replying to a
    /// request that already has one) is refused and nothing is written. The reference store
    /// refuses it with `StoreError::AlreadyExists`, but any `StoreError` is allowed, because a
    /// SQL unique index reports a storage error. The Mailbox treats any failed notice commit
    /// followed by a re-check that finds the asker told
    /// ([`Self::find_reply_for_ask`]) as success.
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
    ///
    /// With no `id`, it never returns an expired message, "expired" exactly as
    /// [`Self::peek_next_unread`] defines it at `now_utc` ([`crate::past_deadline`] holds and
    /// the message isn't flagged `expired_waiting_person`): it passes over them and leaves
    /// them unread, for the Mailbox to expire. A flagged one is claimable like any other.
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
    ///
    /// "Expired" is exactly: [`crate::past_deadline`] holds for the header at `now_utc` (in
    /// epoch milliseconds), and the message isn't flagged `expired_waiting_person`. That is,
    /// `now_utc >= expires_at + min(ttl_ms / 4, 2000)` ms (a missing `ttl_ms` counts as 0),
    /// a message with no `expires_at` never expires, and one whose `expires_at` can't be
    /// read has. A person's ask flagged `expired_waiting_person` is never expired: it stays
    /// deliverable as an ordinary unread message. An unreadable `now_utc` counts every
    /// deadline as passed: each unflagged message with an `expires_at` is expired, and only
    /// messages without one can come back as the message.
    ///
    /// `from_member_id` and `urgent_only` filter `expired_ids` as well as the message: only
    /// expired messages from that sender, or urgent ones, are reported.
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
    /// The earliest message, in commit order (by `local_seq`, not by id: a reply with a
    /// larger id committed first is the answer), replying to `ask_id` and addressed to whoever
    /// asked: a reply from `responder_id`, or the office's own `delivery_failed` notice.
    /// `None` when `ask_id` doesn't exist. The Mailbox commits a `delivery_failed` for an
    /// expired request only while this is `None`: the asker has neither an answer nor a
    /// notice.
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
    ///   `InvalidTransition`); `snooze_note = reason`, `read_at` cleared (a read message
    ///   deferred is unread again when it comes back), `triage_gen += 1`, and the message
    ///   counts as triaged from now on (`triaged_at = now_utc`). Re-snoozing always applies,
    ///   bumping `triage_gen` again. A `snoozed_until` that can't be read MAY be refused with
    ///   `StoreError::InvalidTransition` (the Mailbox validates it before calling); a store
    ///   that accepts one must release it as due in [`Self::check_notify`].
    /// - `Done`, from `unread`, `read` or `snoozed`: leaves the unread index (its snooze
    ///   cleared); `reason` is the done note (on the state row; no port reads it back).
    /// - `Read`, from `unread` or `snoozed`: leaves the unread index (its snooze cleared);
    ///   `read_at = now_utc`.
    /// - `Unread`, from `read`: back into the unread index, `read_at` cleared. Also from
    ///   `unread`, to set `expired_waiting_person` (a set flag is kept: `false` never clears
    ///   it, so unread to unread without the flag changes nothing). The flag is read only
    ///   with this target, and only from `unread`: `read` to `unread` with it set is
    ///   `InvalidTransition` (a message read meanwhile stays read).
    /// - `Expired`, from `unread` or `snoozed`: leaves the unread index (its snooze cleared).
    ///
    /// The same `Read`, `Done` or `Expired` again is idempotent: `Ok` with the header
    /// unchanged (a known limit: the caller can't tell a repeat from a change, so the
    /// Mailbox publishes a `state_changed` for each repeated `Done` or `Expired`; its reads
    /// go through [`MessageStore::claim_read`]). Any other pair is
    /// `StoreError::InvalidTransition`: nothing leaves `done`, `expired` or `dead_letter`, and
    /// `DeadLetter` and `Unknown` are never targets here (dead-lettering is a later slice).
    ///
    /// Across every target:
    ///
    /// - `triage_gen` goes up by exactly 1 on every `Snoozed` (re-snoozes included) and on
    ///   each release by [`Self::check_notify`]. `Done`, `Read`, `Expired` and `Unread` leave
    ///   it unchanged.
    /// - `expired_waiting_person`, once set, survives every later transition (and every
    ///   release and claim): nothing clears it.
    /// - `reason` is the `snooze_note` for `Snoozed` and the done note for `Done`, and is
    ///   ignored for every other target.
    /// - `Done`, `Read` and `Expired` clear `snoozed_until`, `snooze_after_task` and
    ///   `snooze_note`. `Done` and `Expired` keep `read_at`: a read message marked done still
    ///   says when it was read (and a read message can't expire, so it keeps its `read_at`).
    ///
    /// Sync metadata isn't on the header and no port reads it back. The store writes the
    /// change's `owner_office_id`, `origin_incarnation` and `org_position` onto the message's
    /// state row (`msg_state`), so the acting office becomes the state's owner, and stamps
    /// the change as a local edit with the next local sequence.
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
    /// Each message comes back at most once per call: a message released in this call
    /// carries its release reason (`Due`, or `AfterTask`) even when it is urgent or
    /// `after_task` is set, and is never also returned as `New`.
    /// A time snooze whose `snoozed_until` can't be read is released as due (reason `Due`)
    /// rather than stranded.
    /// Expiry is not its job: an overdue request comes back as it is.
    ///
    /// It takes no sync metadata: each release keeps the state row's stored
    /// `owner_office_id`, `origin_incarnation` and `org_position`, and is stamped as a local
    /// edit with the next local sequence, as [`Self::update_state_local`]'s changes are.
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
