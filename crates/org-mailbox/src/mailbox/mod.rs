//! The office's [`Mailbox`]: its state and shared checks here, sending and reading in
//! `delivery`, deadline expiry in `expiry` (triage and queries add to it in `crate::triage`
//! and `crate::queries`).

mod delivery;
mod expiry;

use std::sync::{Arc, Mutex, MutexGuard};

use org_proto::msg::{
    json_string_len, MessageHeader, MessageLifecycleState, MessageStateChangedData,
    KIND_DELIVERY_FAILED, MAX_HANDLE_BYTES, OFFICE_SENDER,
};
use org_proto::{AgentStatus, Member, MemberKind, Position};

use crate::snoozes::SnoozeTimers;
use crate::waiters::WaiterRegistry;
use crate::{
    format_utc, parse_utc, Clock, LocalStateTransition, MailboxError, MemberDirectory,
    MessageEvent, MessageEvents, MessageStore, TriageStore,
};

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // Protected operations only change bookkeeping and never invoke user code.
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

/// The most extra expiry peeks one `read` makes before claiming, or one `wait`
/// makes before blocking. A concurrent backlog cannot make a cleanup turn unbounded.
pub(crate) const MAX_RECHECKS: u32 = 10;

#[derive(Debug, Clone)]
pub struct OfficeIdentity {
    pub office_id: String,
    pub incarnation: String,
    pub is_hq: bool,
}

/// The caller supplies the canonical member id of its authenticated and authorized
/// principal, never an actor selected by request JSON. Transport authorization stays at
/// the caller boundary. Database calls never run while a registry lock (waiters, snoozes) is
/// held; only `triage_writes` is held across one.
pub struct Mailbox {
    pub(crate) store: Arc<dyn MessageStore + Send + Sync>,
    /// `None` for a Mailbox made with [`Mailbox::new`]: triage calls are `unavailable`.
    pub(crate) triage: Option<Arc<dyn TriageStore + Send + Sync>>,
    pub(crate) directory: Arc<dyn MemberDirectory + Send + Sync>,
    pub(crate) office: OfficeIdentity,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) events: Arc<dyn MessageEvents>,
    pub(crate) waiters: WaiterRegistry,
    /// Never locked across a store call.
    pub(crate) snoozes: Mutex<SnoozeTimers>,
    /// Held across a local triage write (`update_state_local`) and the snooze timer update
    /// that follows it, so the timers apply concurrent writes in the order the store did:
    /// otherwise two defers of one message could reach the store as A then B but the timers
    /// as B then A, leaving A's deadline (or none) while the store holds B's. Taken before
    /// `snoozes`, never inside it. Events and waiter wake-ups happen after it is released.
    pub(crate) triage_writes: Mutex<()>,
}

impl Mailbox {
    pub fn new(
        store: Arc<dyn MessageStore + Send + Sync>,
        directory: Arc<dyn MemberDirectory + Send + Sync>,
        office: OfficeIdentity,
        clock: Arc<dyn Clock>,
        events: Arc<dyn MessageEvents>,
    ) -> Self {
        Self {
            store,
            triage: None,
            directory,
            office,
            clock,
            events,
            waiters: WaiterRegistry::default(),
            snoozes: Mutex::new(SnoozeTimers::default()),
            triage_writes: Mutex::new(()),
        }
    }

    /// A Mailbox with triage (`defer`, `done`, snooze release and expiry), its snooze timers
    /// seeded from the store's active time snoozes (ADR-0003 §2.2, `L-a`). `store` and
    /// `triage` are normally the same office.db store.
    pub fn with_triage(
        store: Arc<dyn MessageStore + Send + Sync>,
        triage: Arc<dyn TriageStore + Send + Sync>,
        directory: Arc<dyn MemberDirectory + Send + Sync>,
        office: OfficeIdentity,
        clock: Arc<dyn Clock>,
        events: Arc<dyn MessageEvents>,
    ) -> Result<Self, MailboxError> {
        let mut timers = SnoozeTimers::default();
        for deadline in triage.list_active_snooze_deadlines()? {
            // An unreadable deadline is due at once: the next wait hands it to check_notify.
            let at_ms = parse_utc(&deadline.at_utc).map_or(0, epoch_ms);
            timers.insert(at_ms, &deadline.to_member_id, &deadline.id);
        }
        let mut mailbox = Self::new(store, directory, office, clock, events);
        mailbox.triage = Some(triage);
        mailbox.snoozes = Mutex::new(timers);
        Ok(mailbox)
    }

    /// Held `wait` registrations right now. Diagnostics and tests: a cancelled, finished or
    /// timed-out wait leaves none behind.
    pub fn held_waits(&self) -> usize {
        self.waiters.len()
    }

    pub(crate) fn triage(&self) -> Result<&(dyn TriageStore + Send + Sync), MailboxError> {
        self.triage.as_deref().ok_or(MailboxError::Unavailable)
    }

    /// A local transition of `id` (addressed to `member_id`) to `state`, with no snooze,
    /// reason or flag.
    pub(crate) fn transition(
        &self,
        id: &str,
        member_id: &str,
        state: MessageLifecycleState,
        position: Position,
        now: &str,
    ) -> LocalStateTransition {
        LocalStateTransition {
            id: id.to_owned(),
            to_member_id: member_id.to_owned(),
            state,
            snoozed_until: None,
            snooze_after_task: false,
            reason: None,
            now_utc: now.to_owned(),
            owner_office_id: self.office.office_id.clone(),
            origin_incarnation: self.office.incarnation.clone(),
            org_position: position,
            expired_waiting_person: false,
        }
    }

    /// Publishes `message.state_changed` for a header a triage call returned. A repeated
    /// `Done` or `Expired` returns `Ok` with the header unchanged, and the Mailbox can't tell
    /// a repeat from a change (a known limit), so a repeat publishes again. Events are
    /// idempotent state, so that is harmless.
    pub(crate) fn publish_state(&self, header: &MessageHeader, reason: Option<String>, now: &str) {
        self.events
            .publish(MessageEvent::StateChanged(MessageStateChangedData {
                id: header.id.clone(),
                thread_id: header.thread_id.clone(),
                to_member_id: header.to_member_id.clone(),
                state: header.state,
                triage_gen: header.triage_gen,
                snoozed_until: header.snoozed_until.clone(),
                snooze_after_task: header.snooze_after_task,
                reason,
                expired_waiting_person: header.expired_waiting_person,
                updated_at: now.to_owned(),
            }));
    }

    pub(crate) fn now_utc(&self) -> Result<String, MailboxError> {
        format_utc(self.clock.now_ms()).ok_or(MailboxError::StorageError)
    }

    pub(crate) fn bound_member(&self, id: &str) -> Result<(Member, Position), MailboxError> {
        let resolved = self
            .directory
            .resolve_member(id)?
            .ok_or(MailboxError::NotBound)?;
        if resolved.0.id != id || !can_message(&resolved.0) || !self.owns_inbox(&resolved.0) {
            return Err(MailboxError::NotBound);
        }
        Ok(resolved)
    }

    fn owns_inbox(&self, member: &Member) -> bool {
        match member.kind {
            MemberKind::Person => self.office.is_hq,
            MemberKind::Agent => {
                member.office_id.as_deref() == Some(self.office.office_id.as_str())
            }
            _ => false,
        }
    }

    fn resolve_sender_filter(
        &self,
        reference: Option<&str>,
    ) -> Result<Option<String>, MailboxError> {
        let Some(reference) = reference else {
            return Ok(None);
        };
        let Some(reference) = address(reference)? else {
            return Err(MailboxError::NotFound);
        };
        self.directory
            .resolve_member(reference)?
            .map(|(member, _)| Some(member.id))
            .ok_or(MailboxError::NotFound)
    }
}

/// Whether `row` is the office's `delivery_failed` notice for `original`: from the office,
/// of that kind, replying to `original` and addressed to its sender. Whatever else sits
/// under the notice's id (a member's message sent there first) doesn't tell the sender.
fn is_notice_for(row: &MessageHeader, original: &MessageHeader) -> bool {
    row.from_member_id == OFFICE_SENDER
        && row.kind == KIND_DELIVERY_FAILED
        && row.reply_to.as_deref() == Some(original.id.as_str())
        && row.to_member_id == original.from_member_id
}

/// Whether a failed `delivery_failed` notice fails the call that expired the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoticeFailure {
    Fail,
    Ignore,
}

/// Wall-clock epoch milliseconds as the snooze timers key them.
pub(crate) fn epoch_ms(ms: u64) -> i64 {
    i64::try_from(ms).unwrap_or(i64::MAX)
}

fn can_message(member: &Member) -> bool {
    match member.kind {
        MemberKind::Person => true,
        MemberKind::Agent => member.agent.as_ref().is_some_and(|profile| {
            matches!(profile.status, AgentStatus::Active | AgentStatus::Paused)
        }),
        _ => false,
    }
}

/// msg.rs Addressing: check the wire size, trim leading and trailing ASCII whitespace
/// only, and refuse the reserved `office`. `None` when nothing is left (names no member).
pub(crate) fn address(reference: &str) -> Result<Option<&str>, MailboxError> {
    size(reference, MAX_HANDLE_BYTES)?;
    let trimmed = reference.trim_ascii();
    if trimmed.eq_ignore_ascii_case(OFFICE_SENDER) {
        return Err(MailboxError::InvalidRequest);
    }
    Ok((!trimmed.is_empty()).then_some(trimmed))
}

pub(crate) fn size(value: &str, limit: usize) -> Result<(), MailboxError> {
    if json_string_len(value) > limit {
        Err(MailboxError::TooLarge)
    } else {
        Ok(())
    }
}

pub(crate) fn optional_size(value: Option<&str>, limit: usize) -> Result<(), MailboxError> {
    value.map(|value| size(value, limit)).unwrap_or(Ok(()))
}

#[cfg(test)]
mod tests;
