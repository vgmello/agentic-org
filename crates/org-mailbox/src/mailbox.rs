use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use org_proto::msg::{
    is_urgent, json_string_len, DeliveryFailedReason, HeaderPage, Message,
    MessageDeliveryFailedData, MessageHeader, MessageLifecycleState, MessagePriority,
    MessageReceivedData, MessageStateChangedData, Meta, MsgDeferParams, MsgDeferResult,
    MsgDoneParams, MsgDoneResult, MsgInboxParams, MsgReadParams, MsgReadResult, MsgSendParams,
    MsgWaitParams, MsgWaitResult, NotifyReason, SendResult, SnoozeTarget, DEFAULT_PAGE_LIMIT,
    KIND_DELIVERY_FAILED, KIND_REQUEST, MAX_BODY_BYTES, MAX_HANDLE_BYTES, MAX_ID_BYTES,
    MAX_META_BYTES, MAX_NOTE_BYTES, MAX_PAGE_LIMIT, MAX_SUBJECT_BYTES, MAX_WAIT_MS, OFFICE_SENDER,
};
use org_proto::{ulid_time_ms, AgentStatus, Member, MemberKind, Position};

use crate::ids::{delivery_failed_fallback_id, delivery_failed_id};
use crate::snoozes::SnoozeTimers;
use crate::waiters::{WaitOutcome, WaiterRegistry};
use crate::{
    content_hash, format_utc, parse_utc, past_deadline, Cancellation, Clock, CommitMessageOutcome,
    HeaderQuery, HeaderScope, LocalStateTransition, MailboxError, MemberDirectory, MessageEvent,
    MessageEvents, MessageStore, NewStoredMessage, StoreError, StoredMessage, TriageStore,
    MAX_EXPIRED_IDS,
};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // Protected operations only change bookkeeping and never invoke user code.
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

/// The `delivery_failed` notice's subject: fixed text, never the original's content.
const EXPIRED_SUBJECT: &str = "Request expired";

/// The most extra expiry peeks one `read` makes before claiming, or one `wait`
/// makes before blocking. A concurrent backlog cannot make a cleanup turn unbounded.
const MAX_RECHECKS: u32 = 10;

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
    store: Arc<dyn MessageStore + Send + Sync>,
    /// `None` for a Mailbox made with [`Mailbox::new`]: triage calls are `unavailable`.
    triage: Option<Arc<dyn TriageStore + Send + Sync>>,
    directory: Arc<dyn MemberDirectory + Send + Sync>,
    office: OfficeIdentity,
    clock: Arc<dyn Clock>,
    events: Arc<dyn MessageEvents>,
    waiters: WaiterRegistry,
    /// Never locked across a store call.
    snoozes: Mutex<SnoozeTimers>,
    /// Held across a local triage write (`update_state_local`) and the snooze timer update
    /// that follows it, so the timers apply concurrent writes in the order the store did:
    /// otherwise two defers of one message could reach the store as A then B but the timers
    /// as B then A, leaving A's deadline (or none) while the store holds B's. Taken before
    /// `snoozes`, never inside it. Events and waiter wake-ups happen after it is released.
    triage_writes: Mutex<()>,
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

    pub fn send(
        &self,
        bound_member_id: &str,
        params: MsgSendParams,
    ) -> Result<SendResult, MailboxError> {
        let (sender, _) = self.bound_member(bound_member_id)?;
        size(&params.id, MAX_ID_BYTES)?;
        if ulid_time_ms(&params.id).is_none() {
            return Err(MailboxError::InvalidRequest);
        }
        size(&params.kind, MAX_ID_BYTES)?;
        if params.kind.is_empty()
            || matches!(params.kind.as_str(), KIND_REQUEST | KIND_DELIVERY_FAILED)
        {
            return Err(MailboxError::InvalidRequest);
        }
        let priority = params.priority.unwrap_or_default();
        if !matches!(
            priority,
            MessagePriority::Normal | MessagePriority::High | MessagePriority::Urgent
        ) {
            return Err(MailboxError::InvalidRequest);
        }
        optional_size(params.subject.as_deref(), MAX_SUBJECT_BYTES)?;
        optional_size(params.reply_to.as_deref(), MAX_ID_BYTES)?;
        size(&params.body, MAX_BODY_BYTES)?;
        let meta_bytes = if params.meta.is_empty() {
            0
        } else {
            serde_json::to_vec(&params.meta)
                .map_err(|_| MailboxError::InvalidRequest)?
                .len()
        };
        if meta_bytes > MAX_META_BYTES {
            return Err(MailboxError::TooLarge);
        }
        let to = address(&params.to)?;
        let existing = self.store.get_message(&params.id)?;
        if existing
            .as_ref()
            .is_some_and(|stored| stored.message.header.from_member_id != bound_member_id)
        {
            return Err(MailboxError::AlreadyExists);
        }
        let resolved = match to {
            Some(to) => self.directory.resolve_member(to)?,
            None => None,
        };
        // An exact recipient id stays retryable after removal; a handle must
        // still resolve, so a reassigned handle cannot redirect a retry.
        let to_id = match (&existing, &resolved) {
            (Some(stored), _) if to == Some(stored.message.header.to_member_id.as_str()) => {
                stored.message.header.to_member_id.clone()
            }
            (_, Some((member, _))) => member.id.clone(),
            _ => return Err(MailboxError::UnknownRecipient),
        };
        let hash = content_hash(
            &to_id,
            &params.kind,
            priority,
            params.subject.as_deref(),
            params.reply_to.as_deref(),
            &params.body,
            &params.meta,
        )?;
        if let Some(stored) = existing {
            if stored.content_hash != hash {
                return Err(MailboxError::AlreadyExists);
            }
            return Ok(send_result(&stored));
        }
        let (recipient, position) = resolved.ok_or(MailboxError::UnknownRecipient)?;
        if !can_message(&recipient) {
            return Err(MailboxError::UnknownRecipient);
        }
        // Slice 1 is local/HQ. Cross-office routing is a separate capability.
        if !self.owns_inbox(&recipient) {
            return Err(MailboxError::Unavailable);
        }
        let thread_id = if let Some(parent_id) = params.reply_to.as_deref() {
            let parent = self
                .store
                .get_message(parent_id)?
                .ok_or(MailboxError::NotFound)?;
            let header = &parent.message.header;
            if header.from_member_id != bound_member_id && header.to_member_id != bound_member_id {
                return Err(MailboxError::NotFound);
            }
            if header.kind == KIND_DELIVERY_FAILED {
                return Err(MailboxError::InvalidRequest);
            }
            header.thread_id.clone()
        } else {
            params.id.clone()
        };
        let now = self.now_utc()?;
        let urgent = is_urgent(&params.kind, priority);
        let header = MessageHeader {
            id: params.id,
            thread_id,
            reply_to: params.reply_to,
            from_member_id: sender.id,
            from_display_name: sender.name,
            to_member_id: recipient.id,
            to_display_name: recipient.name,
            kind: params.kind,
            priority,
            urgent,
            subject: params.subject,
            created_at: now,
            expires_at: None,
            ttl_ms: None,
            body_bytes: json_string_len(&params.body) as u64,
            meta_bytes: meta_bytes as u64,
            local_seq: 0,
            state: MessageLifecycleState::Unread,
            triage_gen: 0,
            snoozed_until: None,
            snooze_after_task: false,
            snooze_note: None,
            read_at: None,
            expired_waiting_person: false,
        };
        let outcome = self.store.commit_message(NewStoredMessage {
            message: Message {
                header,
                body: params.body,
                meta: params.meta,
            },
            origin_office_id: self.office.office_id.clone(),
            origin_incarnation: self.office.incarnation.clone(),
            origin_seq: None,
            org_position: position,
            content_hash: hash,
            initial_state: MessageLifecycleState::Unread,
        })?;
        if outcome.inserted {
            let header = &outcome.message.message.header;
            self.waiters.notify(&header.to_member_id);
            self.events
                .publish(MessageEvent::Received(Box::new(MessageReceivedData {
                    header: header.clone(),
                })));
        }
        Ok(send_result(&outcome.message))
    }

    pub fn inbox(
        &self,
        bound_member_id: &str,
        params: MsgInboxParams,
    ) -> Result<HeaderPage, MailboxError> {
        self.bound_member(bound_member_id)?;
        optional_size(params.thread_id.as_deref(), MAX_ID_BYTES)?;
        let limit = params.limit.unwrap_or(DEFAULT_PAGE_LIMIT);
        if limit > MAX_PAGE_LIMIT {
            return Err(MailboxError::InvalidRequest);
        }
        let from_member_id = self.resolve_sender_filter(params.from.as_deref())?;
        Ok(self.store.list_headers(HeaderQuery {
            scope: HeaderScope::Inbox(bound_member_id.to_owned()),
            from_member_id,
            thread_id: params.thread_id,
            unread_only: !params.include_all,
            include_snoozed: params.include_all,
            cursor: params.cursor,
            limit,
        })?)
    }

    pub fn read(
        &self,
        bound_member_id: &str,
        params: MsgReadParams,
    ) -> Result<MsgReadResult, MailboxError> {
        let (member, position) = self.bound_member(bound_member_id)?;
        optional_size(params.id.as_deref(), MAX_ID_BYTES)?;
        let now_ms = self.clock.now_ms();
        let now = format_utc(now_ms).ok_or(MailboxError::StorageError)?;
        if self.triage.is_some() {
            match params.id.as_deref() {
                // An agent's request past its deadline is expired, never read (§3.3.2).
                Some(id) if member.kind == MemberKind::Agent => {
                    let request = self.store.get_message(id)?.filter(|stored| {
                        let header = &stored.message.header;
                        header.to_member_id == bound_member_id && header.kind == KIND_REQUEST
                    });
                    if let Some(request) = request {
                        let header = &request.message.header;
                        match header.state {
                            MessageLifecycleState::Unread | MessageLifecycleState::Snoozed
                                if past_deadline(header, now_ms) =>
                            {
                                if self
                                    .expire(&member, position, id, &now, NoticeFailure::Fail)?
                                    .is_some()
                                {
                                    let stored = self
                                        .store
                                        .get_message(id)?
                                        .ok_or(MailboxError::NotFound)?;
                                    return Ok(MsgReadResult {
                                        message: Some(stored.message),
                                    });
                                }
                            }
                            // Already expired: make sure the asker was told. A notice an
                            // earlier expiry failed to commit is committed now. Whether the
                            // asker has an answer or a notice is asked of the store on the
                            // read path first (not by the notice's id, which changes with the
                            // office incarnation), so the writer is used only when neither
                            // exists.
                            MessageLifecycleState::Expired if !self.answered_or_told(header)? => {
                                self.notify_expired(header, &now)?
                            }
                            _ => {}
                        }
                    }
                }
                Some(_) => {}
                None => {
                    // As wait does: release due snoozes, then expire what peek passes over.
                    // Drain full batches before claiming. The atomic claim still skips
                    // expired rows if a larger/concurrent backlog exceeds this budget;
                    // later calls continue its cleanup and delivery-failed notices.
                    self.release_due_snoozes(bound_member_id, now_ms, &now)?;
                    for _ in 0..=MAX_RECHECKS {
                        let peek =
                            self.store
                                .peek_next_unread(bound_member_id, None, false, &now)?;
                        for id in &peek.expired_ids {
                            self.expire(&member, position, id, &now, NoticeFailure::Ignore)?;
                        }
                        if peek.expired_ids.len() < MAX_EXPIRED_IDS {
                            break;
                        }
                    }
                }
            }
        }
        let stored = self
            .store
            .claim_read(bound_member_id, params.id.as_deref(), &now)?;
        if params.id.is_some() && stored.is_none() {
            return Err(MailboxError::NotFound);
        }
        if let Some(stored) = &stored {
            let header = &stored.message.header;
            // The store stamps read_at with this call's time only when it made the claim;
            // an already read message keeps its earlier read_at and publishes nothing. (A
            // re-read in the same millisecond as the claim may publish a duplicate
            // state_changed event; events are idempotent state, so that is harmless.)
            if header.state == MessageLifecycleState::Read
                && header.read_at.as_deref() == Some(now.as_str())
            {
                self.publish_state(header, None, &now);
            }
        }
        Ok(MsgReadResult {
            message: stored.map(|stored| stored.message),
        })
    }

    /// Register before checking storage. Notifications carry generations, so a
    /// commit during the check cannot be lost just before the condition wait.
    /// The deadline bounds blocking waits; synchronous store calls cannot be
    /// preempted by this port and must themselves be kept bounded by the backend.
    pub fn wait(
        &self,
        bound_member_id: &str,
        params: MsgWaitParams,
        deadline: Instant,
        cancellation: &Cancellation,
    ) -> Result<MsgWaitResult, MailboxError> {
        if params.timeout_ms > MAX_WAIT_MS {
            return Err(MailboxError::InvalidRequest);
        }
        let duration_deadline = Instant::now()
            .checked_add(Duration::from_millis(params.timeout_ms))
            .ok_or(MailboxError::InvalidRequest)?;
        let deadline = deadline.min(duration_deadline);
        self.bound_member(bound_member_id)?;
        let from = self.resolve_sender_filter(params.from.as_deref())?;
        if cancellation.is_cancelled() {
            return Err(MailboxError::NotBound);
        }
        let registration = self.waiters.register(bound_member_id)?;
        cancellation.attach(&registration);
        // Peeks again before blocking, at most MAX_RECHECKS times per block (so a store that
        // keeps reporting the same ids can't spin the wait): after flagging a person's overdue
        // ask, which is no longer passed over, or after expiring a full batch of
        // MAX_EXPIRED_IDS, behind which more may wait.
        let mut rechecks = 0;
        loop {
            if cancellation.is_cancelled() {
                return Err(MailboxError::NotBound);
            }
            let observed = registration.generation();
            let (member, position) = self.bound_member(bound_member_id)?;
            let now_ms = self.clock.now_ms();
            let now = format_utc(now_ms).ok_or(MailboxError::StorageError)?;
            self.release_due_snoozes(bound_member_id, now_ms, &now)?;
            let peek = self.store.peek_next_unread(
                bound_member_id,
                from.as_deref(),
                params.urgent_only,
                &now,
            )?;
            if cancellation.is_cancelled() {
                return Err(MailboxError::NotBound);
            }
            // peek itself never changes anything; the Mailbox expires what it passed over
            // (needs triage; without it they stay as they are, as in slice 1).
            let mut flagged = false;
            for id in &peek.expired_ids {
                let expired = self.expire(&member, position, id, &now, NoticeFailure::Ignore)?;
                flagged |= expired.is_some_and(|header| header.expired_waiting_person);
            }
            // A just-flagged ask may be older than the message peek found: peek again, so
            // the oldest comes first.
            let full_batch = self.triage.is_some() && peek.expired_ids.len() >= MAX_EXPIRED_IDS;
            let recheck = flagged || (full_batch && peek.message.is_none());
            if recheck && rechecks < MAX_RECHECKS {
                rechecks += 1;
                continue;
            }
            if let Some(message) = peek.message {
                return Ok(MsgWaitResult {
                    message: Some(message.message),
                });
            }
            rechecks = 0;
            // Block until the request's deadline or the member's next snooze deadline,
            // whichever is first. A snooze deadline loops round (release, then peek). The
            // wall clock is read again, after this round's store calls.
            let next_snooze = lock(&self.snoozes).next_for(bound_member_id);
            let block_until = next_snooze
                .and_then(|at_ms| {
                    let ahead = at_ms.saturating_sub(epoch_ms(self.clock.now_ms())).max(0);
                    Instant::now().checked_add(Duration::from_millis(ahead.unsigned_abs()))
                })
                .map_or(deadline, |snooze| snooze.min(deadline));
            match registration.wait(observed, block_until, cancellation) {
                WaitOutcome::Cancelled => return Err(MailboxError::NotBound),
                WaitOutcome::Deadline if Instant::now() >= deadline => {
                    return Ok(MsgWaitResult { message: None })
                }
                WaitOutcome::Deadline | WaitOutcome::Notified => {}
            }
        }
    }

    /// `msg.defer`: snooze a message addressed to the bound member until a time or its next
    /// break (ADR-0003 §2.3). It leaves the default inbox and every wait until it's due, and
    /// its `triage_gen` goes up. A time snooze gets a timer, so a held wait returns it when
    /// it comes due.
    pub fn defer(
        &self,
        bound_member_id: &str,
        params: MsgDeferParams,
    ) -> Result<MsgDeferResult, MailboxError> {
        let (_, position) = self.bound_member(bound_member_id)?;
        let triage = self.triage()?;
        size(&params.id, MAX_ID_BYTES)?;
        optional_size(params.note.as_deref(), MAX_NOTE_BYTES)?;
        let now_ms = self.clock.now_ms();
        let now = format_utc(now_ms).ok_or(MailboxError::StorageError)?;
        let (snoozed_until, snooze_after_task) = match params.until {
            SnoozeTarget::Until { at } => {
                // parse_utc reads only canonical 24-byte timestamps, so `at` is bounded.
                if !parse_utc(&at).is_some_and(|at_ms| at_ms > now_ms) {
                    return Err(MailboxError::InvalidRequest);
                }
                (Some(at), false)
            }
            SnoozeTarget::AfterTask => (None, true),
            _ => return Err(MailboxError::InvalidRequest),
        };
        let header = {
            let _ordered = lock(&self.triage_writes);
            let header = triage.update_state_local(LocalStateTransition {
                snoozed_until,
                snooze_after_task,
                reason: params.note,
                ..self.transition(
                    &params.id,
                    bound_member_id,
                    MessageLifecycleState::Snoozed,
                    position,
                    &now,
                )
            })?;
            let mut timers = lock(&self.snoozes);
            match header.snoozed_until.as_deref() {
                Some(at) => timers.insert(
                    parse_utc(at).map_or(0, epoch_ms),
                    bound_member_id,
                    &header.id,
                ),
                None => timers.remove(&header.id),
            }
            header
        };
        // A held wait recomputes its bound with the new timer.
        self.waiters.notify(bound_member_id);
        self.publish_state(&header, header.snooze_note.clone(), &now);
        Ok(MsgDeferResult {
            id: header.id,
            triage_gen: header.triage_gen,
            snoozed_until: header.snoozed_until,
            snooze_after_task: header.snooze_after_task,
            snooze_note: header.snooze_note,
        })
    }

    /// `msg.done`: close a message addressed to the bound member without replying. It never
    /// resurfaces. Done again is accepted (the store keeps the first close); `resolved_at`
    /// is this call's time, and its `state_changed` carries this call's note.
    pub fn done(
        &self,
        bound_member_id: &str,
        params: MsgDoneParams,
    ) -> Result<MsgDoneResult, MailboxError> {
        let (_, position) = self.bound_member(bound_member_id)?;
        let triage = self.triage()?;
        size(&params.id, MAX_ID_BYTES)?;
        optional_size(params.note.as_deref(), MAX_NOTE_BYTES)?;
        let now = self.now_utc()?;
        let header = {
            let _ordered = lock(&self.triage_writes);
            let header = triage.update_state_local(LocalStateTransition {
                reason: params.note.clone(),
                ..self.transition(
                    &params.id,
                    bound_member_id,
                    MessageLifecycleState::Done,
                    position,
                    &now,
                )
            })?;
            lock(&self.snoozes).remove(&header.id);
            header
        };
        self.waiters.notify(bound_member_id);
        self.publish_state(&header, params.note, &now);
        Ok(MsgDoneResult {
            id: header.id,
            resolved_at: now,
        })
    }

    /// Held `wait` registrations right now. Diagnostics and tests: a cancelled, finished or
    /// timed-out wait leaves none behind.
    pub fn held_waits(&self) -> usize {
        self.waiters.len()
    }

    fn triage(&self) -> Result<&(dyn TriageStore + Send + Sync), MailboxError> {
        self.triage.as_deref().ok_or(MailboxError::Unavailable)
    }

    /// A local transition of `id` (addressed to `member_id`) to `state`, with no snooze,
    /// reason or flag.
    fn transition(
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

    /// Releases the member's due time snoozes (§2.2) through `check_notify`, before a wait
    /// peeks. Only when the timers say one is due, and only with triage.
    fn release_due_snoozes(
        &self,
        member: &str,
        now_ms: u64,
        now: &str,
    ) -> Result<(), MailboxError> {
        let Some(triage) = self.triage.as_deref() else {
            return Ok(());
        };
        let due = lock(&self.snoozes).take_due(member, epoch_ms(now_ms));
        if due.is_empty() {
            return Ok(());
        }
        // Sweep nothing (the epoch): only the releases matter here, not what is listed.
        let sweep_before = format_utc(0).ok_or(MailboxError::StorageError)?;
        let candidates = match triage.check_notify(member, false, false, &sweep_before, now) {
            Ok(candidates) => candidates,
            Err(error) => {
                let mut timers = lock(&self.snoozes);
                for (at_ms, id) in due {
                    timers.restore(at_ms, member, &id);
                }
                return Err(error.into());
            }
        };
        // The due timers were taken above; a released message's timer is not removed here,
        // since a defer since check_notify may have set a new one.
        let released: Vec<_> = candidates
            .into_iter()
            .filter(|candidate| candidate.reason == NotifyReason::Due)
            .collect();
        if !released.is_empty() {
            self.waiters.notify(member);
        }
        for candidate in &released {
            self.publish_state(&candidate.header, None, now);
        }
        Ok(())
    }

    /// Acts on a message past its deadline, addressed to `recipient` (with triage; without
    /// it, nothing). An agent's is expired, and for a request its sender gets one
    /// `delivery_failed` (ADR-0003 §3.2 item 3, §3.3.2). A person's or the owner's ask never
    /// expires: it stays unread, flagged `expired_waiting_person`. `None` when someone acted
    /// first (`InvalidTransition` or `NotFound`), else the header after the change.
    ///
    /// The notice is committed after the transition, so a request someone answered or closed
    /// first is never reported. With `NoticeFailure::Ignore` (wait, and read with no id, whose
    /// caller is not the party at fault) a failed notice doesn't fail the call; the request
    /// stays expired without its notice until a read of it by id retries it (known gap: the
    /// store's transition and the notice aren't one transaction yet).
    fn expire(
        &self,
        recipient: &Member,
        position: Position,
        id: &str,
        now: &str,
        on_notice_failure: NoticeFailure,
    ) -> Result<Option<MessageHeader>, MailboxError> {
        let Some(triage) = self.triage.as_deref() else {
            return Ok(None);
        };
        let person = recipient.kind == MemberKind::Person;
        let change = LocalStateTransition {
            expired_waiting_person: person,
            ..self.transition(
                id,
                &recipient.id,
                if person {
                    MessageLifecycleState::Unread
                } else {
                    MessageLifecycleState::Expired
                },
                position,
                now,
            )
        };
        let header = {
            let _ordered = lock(&self.triage_writes);
            let header = match triage.update_state_local(change) {
                Ok(header) => header,
                Err(StoreError::InvalidTransition | StoreError::NotFound) => return Ok(None),
                Err(error) => return Err(error.into()),
            };
            // An expired message has no snooze; a person's flagged ask keeps whatever it has.
            if header.state == MessageLifecycleState::Expired {
                lock(&self.snoozes).remove(&header.id);
            }
            header
        };
        self.publish_state(&header, None, now);
        if header.state == MessageLifecycleState::Expired && header.kind == KIND_REQUEST {
            let notified = self.notify_expired(&header, now);
            if on_notice_failure == NoticeFailure::Fail {
                notified?;
            }
        }
        Ok(Some(header))
    }

    /// Whether the asker of `original` already has an answer from its recipient or the
    /// office's `delivery_failed` notice ([`MessageStore::find_reply_for_ask`]). Either way
    /// the asker needs no (other) notice.
    fn answered_or_told(&self, original: &MessageHeader) -> Result<bool, MailboxError> {
        Ok(self
            .store
            .find_reply_for_ask(&original.id, &original.to_member_id)?
            .is_some())
    }

    /// Commits the `delivery_failed` notice for an expired request to its sender, when the
    /// sender is a member who may still receive, unless the sender already has an answer from
    /// the recipient or a notice ([`Self::answered_or_told`], asked of the store: never
    /// decided by the notice's id, which changes when office.db is restored and the
    /// incarnation with it). So a request gets one notice, and an answered one none.
    ///
    /// The notice is committed under [`delivery_failed_id`] first: a racing expiry of the
    /// same request replays the same commit (`inserted: false`) and publishes nothing. When
    /// that id is taken by a row that isn't the notice ([`is_notice_for`]: a member sent a
    /// message there first) or by other content, the store is asked again (a racing expiry
    /// may have committed the notice meanwhile), and if the sender still has neither, the
    /// notice is committed once more under [`delivery_failed_fallback_id`].
    ///
    /// A store keeps at most one `delivery_failed` per request
    /// ([`MessageStore::commit_message`]), and refuses a second with any error (a SQL unique
    /// index reports a storage error, not `AlreadyExists`). So after any failed commit, of
    /// either id, the store is asked once more ([`Self::told_after_failure`]): two expiries
    /// racing each other to the fallback, or one landing its notice while this one commits,
    /// leave the asker told, and that is success. Only when the asker still has neither is
    /// the commit's error returned. It commits at this office, as slice 1's local/HQ
    /// messaging does: routing a notice to a sender another office owns comes with
    /// cross-office routing.
    fn notify_expired(&self, original: &MessageHeader, now: &str) -> Result<(), MailboxError> {
        if original.from_member_id == OFFICE_SENDER || original.kind == KIND_DELIVERY_FAILED {
            return Ok(());
        }
        if self.answered_or_told(original)? {
            return Ok(());
        }
        let Some((sender, position)) = self.directory.resolve_member(&original.from_member_id)?
        else {
            return Ok(());
        };
        if sender.id != original.from_member_id || !can_message(&sender) {
            return Ok(());
        }
        let notice = self.expired_notice(
            original,
            sender,
            position,
            delivery_failed_id(original, &self.office.incarnation),
            now,
        )?;
        match self.store.commit_message(notice.clone()) {
            Ok(outcome) if is_notice_for(&outcome.message.message.header, original) => {
                self.announce_notice(&outcome, original);
                return Ok(());
            }
            // The id is held by a row that isn't the notice, or by other content (an earlier
            // notice under an old display name, which the check below then finds).
            Ok(_) | Err(StoreError::AlreadyExists) => {}
            Err(error) => return self.told_after_failure(original, error),
        }
        if self.answered_or_told(original)? {
            return Ok(());
        }
        let fallback = NewStoredMessage {
            message: Message {
                header: MessageHeader {
                    id: delivery_failed_fallback_id(&original.id, &self.office.incarnation, now),
                    ..notice.message.header
                },
                ..notice.message
            },
            ..notice
        };
        let outcome = match self.store.commit_message(fallback) {
            Ok(outcome) => outcome,
            Err(error) => return self.told_after_failure(original, error),
        };
        if !is_notice_for(&outcome.message.message.header, original) {
            return Err(MailboxError::StorageError);
        }
        self.announce_notice(&outcome, original);
        Ok(())
    }

    /// After a failed commit of `original`'s notice: `Ok` when the asker has been told or
    /// answered meanwhile (another expiry's notice took the request's one `delivery_failed`),
    /// else the commit's `error`. A failed re-check leaves the commit's error standing.
    fn told_after_failure(
        &self,
        original: &MessageHeader,
        error: StoreError,
    ) -> Result<(), MailboxError> {
        match self.answered_or_told(original) {
            Ok(true) => Ok(()),
            Ok(false) | Err(_) => Err(error.into()),
        }
    }

    /// The `delivery_failed` notice for `original`, to its sender, under `id`. It holds no
    /// content of the original: fixed text and the recipient's id and name.
    fn expired_notice(
        &self,
        original: &MessageHeader,
        sender: Member,
        position: Position,
        id: String,
        now: &str,
    ) -> Result<NewStoredMessage, MailboxError> {
        let body = format!(
            "Your request to {} expired before they acted on it.",
            original.to_display_name
        );
        let mut meta = Meta::new();
        meta.insert("recipient_id".into(), original.to_member_id.clone().into());
        meta.insert(
            "recipient_name".into(),
            original.to_display_name.clone().into(),
        );
        meta.insert("reason".into(), "expired".into());
        let meta_bytes = serde_json::to_vec(&meta)
            .map_err(|_| MailboxError::StorageError)?
            .len();
        let priority = MessagePriority::High;
        let hash = content_hash(
            &sender.id,
            KIND_DELIVERY_FAILED,
            priority,
            Some(EXPIRED_SUBJECT),
            Some(&original.id),
            &body,
            &meta,
        )?;
        let header = MessageHeader {
            id,
            thread_id: original.thread_id.clone(),
            reply_to: Some(original.id.clone()),
            from_member_id: OFFICE_SENDER.into(),
            from_display_name: OFFICE_SENDER.into(),
            to_member_id: sender.id,
            to_display_name: sender.name,
            kind: KIND_DELIVERY_FAILED.into(),
            priority,
            urgent: is_urgent(KIND_DELIVERY_FAILED, priority),
            subject: Some(EXPIRED_SUBJECT.into()),
            created_at: now.to_owned(),
            expires_at: None,
            ttl_ms: None,
            body_bytes: json_string_len(&body) as u64,
            meta_bytes: meta_bytes as u64,
            local_seq: 0,
            state: MessageLifecycleState::Unread,
            triage_gen: 0,
            snoozed_until: None,
            snooze_after_task: false,
            snooze_note: None,
            read_at: None,
            expired_waiting_person: false,
        };
        Ok(NewStoredMessage {
            message: Message { header, body, meta },
            origin_office_id: self.office.office_id.clone(),
            origin_incarnation: self.office.incarnation.clone(),
            origin_seq: None,
            org_position: position,
            content_hash: hash,
            initial_state: MessageLifecycleState::Unread,
        })
    }

    /// Wakes the asker and publishes the notice's events, when this commit inserted it (a
    /// replay publishes nothing).
    fn announce_notice(&self, outcome: &CommitMessageOutcome, original: &MessageHeader) {
        if !outcome.inserted {
            return;
        }
        let header = &outcome.message.message.header;
        self.waiters.notify(&header.to_member_id);
        self.events
            .publish(MessageEvent::Received(Box::new(MessageReceivedData {
                header: header.clone(),
            })));
        self.events
            .publish(MessageEvent::DeliveryFailed(MessageDeliveryFailedData {
                id: header.id.clone(),
                original_id: original.id.clone(),
                thread_id: original.thread_id.clone(),
                from_member_id: original.from_member_id.clone(),
                recipient_id: original.to_member_id.clone(),
                recipient_name: original.to_display_name.clone(),
                reason: DeliveryFailedReason::Expired,
            }));
    }

    /// Publishes `message.state_changed` for a header a triage call returned. A repeated
    /// `Done` or `Expired` returns `Ok` with the header unchanged, and the Mailbox can't tell
    /// a repeat from a change (a known limit), so a repeat publishes again. Events are
    /// idempotent state, so that is harmless.
    fn publish_state(&self, header: &MessageHeader, reason: Option<String>, now: &str) {
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

    fn now_utc(&self) -> Result<String, MailboxError> {
        format_utc(self.clock.now_ms()).ok_or(MailboxError::StorageError)
    }

    fn bound_member(&self, id: &str) -> Result<(Member, Position), MailboxError> {
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
fn epoch_ms(ms: u64) -> i64 {
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
fn address(reference: &str) -> Result<Option<&str>, MailboxError> {
    size(reference, MAX_HANDLE_BYTES)?;
    let trimmed = reference.trim_ascii();
    if trimmed.eq_ignore_ascii_case(OFFICE_SENDER) {
        return Err(MailboxError::InvalidRequest);
    }
    Ok((!trimmed.is_empty()).then_some(trimmed))
}

fn size(value: &str, limit: usize) -> Result<(), MailboxError> {
    if json_string_len(value) > limit {
        Err(MailboxError::TooLarge)
    } else {
        Ok(())
    }
}

fn optional_size(value: Option<&str>, limit: usize) -> Result<(), MailboxError> {
    value.map(|value| size(value, limit)).unwrap_or(Ok(()))
}

fn send_result(stored: &StoredMessage) -> SendResult {
    let header = &stored.message.header;
    SendResult {
        id: header.id.clone(),
        thread_id: header.thread_id.clone(),
        local_seq: header.local_seq,
        origin_seq: stored.origin_seq,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{mpsc, Barrier};
    use std::thread;

    use org_proto::msg::{Meta, KIND_MSG};

    use super::*;
    use crate::testkit::fixture::*;
    use crate::testkit::{InMemoryDirectory, InMemoryStore};
    use crate::{NoMessageEvents, NotifyCandidate, SnoozeDeadline};

    /// A triage store whose first `update_state_local` applies, then holds before returning
    /// until the test lets it go: the window in which another defer could overtake it.
    struct GatedTriage {
        inner: InMemoryStore,
        gated: AtomicBool,
        applied: Mutex<mpsc::Sender<()>>,
        release: Barrier,
    }

    impl TriageStore for GatedTriage {
        fn update_state_local(
            &self,
            change: LocalStateTransition,
        ) -> Result<MessageHeader, StoreError> {
            let result = self.inner.update_state_local(change);
            if self.gated.swap(false, Ordering::SeqCst) {
                lock(&self.applied).send(()).unwrap();
                self.release.wait();
            }
            result
        }
        fn check_notify(
            &self,
            to_member_id: &str,
            after_task: bool,
            urgent_only: bool,
            sweep_before_utc: &str,
            now_utc: &str,
        ) -> Result<Vec<NotifyCandidate>, StoreError> {
            self.inner.check_notify(
                to_member_id,
                after_task,
                urgent_only,
                sweep_before_utc,
                now_utc,
            )
        }
        fn list_active_snooze_deadlines(&self) -> Result<Vec<SnoozeDeadline>, StoreError> {
            self.inner.list_active_snooze_deadlines()
        }
    }

    /// A message store over an [`InMemoryStore`] that races the Mailbox's notice commits. Its
    /// `fail_commit`th `commit_message` call (from 1) fails with the given error after landing
    /// another expiry's notice first (the same notice under [`RACING_NOTICE`]), and its next
    /// `blind_finds` `find_reply_for_ask` calls answer `None`, as if no notice had landed yet.
    #[derive(Default)]
    struct RacingStore {
        inner: InMemoryStore,
        commits: AtomicUsize,
        fail_commit: Mutex<Option<(usize, StoreError)>>,
        blind_finds: Mutex<usize>,
    }

    /// The id another expiry's notice lands under in [`RacingStore`].
    const RACING_NOTICE: &str = "01JCRACINGNOTICE0000000000";

    impl MessageStore for RacingStore {
        fn commit_message(
            &self,
            msg: NewStoredMessage,
        ) -> Result<CommitMessageOutcome, StoreError> {
            let call = self.commits.fetch_add(1, Ordering::SeqCst) + 1;
            let failure = {
                let mut fail = lock(&self.fail_commit);
                match fail.take() {
                    Some((at, error)) if at == call => Some(error),
                    other => {
                        *fail = other;
                        None
                    }
                }
            };
            let Some(error) = failure else {
                return self.inner.commit_message(msg);
            };
            let racing = NewStoredMessage {
                message: Message {
                    header: MessageHeader {
                        id: RACING_NOTICE.into(),
                        ..msg.message.header
                    },
                    ..msg.message
                },
                ..msg
            };
            assert!(
                self.inner
                    .commit_message(racing)
                    .expect("the racing notice")
                    .inserted
            );
            Err(error)
        }
        fn claim_read(
            &self,
            to_member_id: &str,
            id: Option<&str>,
            now_utc: &str,
        ) -> Result<Option<StoredMessage>, StoreError> {
            self.inner.claim_read(to_member_id, id, now_utc)
        }
        fn peek_next_unread(
            &self,
            to_member_id: &str,
            from_member_id: Option<&str>,
            urgent_only: bool,
            now_utc: &str,
        ) -> Result<crate::PeekUnreadOutcome, StoreError> {
            self.inner
                .peek_next_unread(to_member_id, from_member_id, urgent_only, now_utc)
        }
        fn get_message(&self, id: &str) -> Result<Option<StoredMessage>, StoreError> {
            self.inner.get_message(id)
        }
        fn list_headers(&self, query: HeaderQuery) -> Result<HeaderPage, StoreError> {
            self.inner.list_headers(query)
        }
        fn find_reply_for_ask(
            &self,
            ask_id: &str,
            responder_id: &str,
        ) -> Result<Option<StoredMessage>, StoreError> {
            {
                let mut blind = lock(&self.blind_finds);
                if *blind > 0 {
                    *blind -= 1;
                    return Ok(None);
                }
            }
            self.inner.find_reply_for_ask(ask_id, responder_id)
        }
    }

    /// A Mailbox (no triage) over `store`, recording its events.
    fn racing_mailbox(store: &Arc<RacingStore>, events: &Arc<RecordingEvents>) -> Mailbox {
        Mailbox::new(
            store.clone(),
            Arc::new(InMemoryDirectory::new(members(), POSITION)),
            identity(),
            Arc::new(ManualClock::new(NOW_MS)),
            events.clone(),
        )
    }

    /// Commits `msg_id(1)`, an expired request from Pat to Ada, and returns its header.
    fn expired_ask(store: &impl MessageStore) -> MessageHeader {
        use crate::testkit::conformance::{commit, expired_request};

        let mut ask = expired_request(1, PAT_ID, ADA_ID);
        ask.message.header.to_display_name = ADA_NAME.into();
        commit(store, ask);
        store
            .get_message(&msg_id(1))
            .unwrap()
            .unwrap()
            .message
            .header
    }

    /// The ids of the `delivery_failed` notices in Pat's inbox replying to `msg_id(1)`.
    fn notices(store: &impl MessageStore) -> Vec<String> {
        store
            .list_headers(HeaderQuery {
                scope: HeaderScope::Inbox(PAT_ID.into()),
                from_member_id: None,
                thread_id: None,
                unread_only: false,
                include_snoozed: true,
                cursor: None,
                limit: 100,
            })
            .unwrap()
            .headers
            .into_iter()
            .filter(|header| {
                header.kind == KIND_DELIVERY_FAILED
                    && header.reply_to.as_deref() == Some(msg_id(1).as_str())
            })
            .map(|header| header.id)
            .collect()
    }

    /// A notice commit that fails after another expiry's notice landed leaves the asker told:
    /// `Ok`, with exactly that one notice, and nothing published by the loser. That is two
    /// expiries racing to the fallback (SQL's unique index fails the second with a storage
    /// error, the memory store with `AlreadyExists`), or a first-id commit losing that way.
    #[test]
    fn a_notice_commit_failing_after_another_notice_landed_is_told() {
        use crate::testkit::conformance::{commit, new_message};

        let errors: [fn() -> StoreError; 2] = [
            || StoreError::new("UNIQUE constraint failed: msg_headers.reply_to"),
            || StoreError::AlreadyExists,
        ];
        // (the failing commit, whether the first id is squatted so the fallback is tried)
        for (failing, squatted) in [(2, true), (1, false)] {
            for error in errors {
                let store = Arc::new(RacingStore::default());
                let events = Arc::new(RecordingEvents::default());
                let mailbox = racing_mailbox(&store, &events);
                let request = expired_ask(&store.inner);
                if squatted {
                    let mut squat = new_message(2, REMY_ID, PAT_ID, MessagePriority::Normal);
                    squat.message.header.id = delivery_failed_id(&request, INCARNATION);
                    commit(&store.inner, squat);
                }
                *lock(&store.fail_commit) = Some((failing, error()));

                assert_eq!(
                    mailbox.notify_expired(&request, NOW_UTC),
                    Ok(()),
                    "commit {failing} failed after the racing notice landed"
                );
                assert_eq!(store.commits.load(Ordering::SeqCst), failing);
                assert_eq!(notices(&store.inner), [RACING_NOTICE]);
                assert!(events.delivery_failed().is_empty());
            }
        }
    }

    /// Without another notice, a failed commit is still an error.
    #[test]
    fn a_failed_notice_commit_without_another_notice_fails() {
        let store = Arc::new(RacingStore::default());
        let mailbox = racing_mailbox(&store, &Arc::new(RecordingEvents::default()));
        let request = expired_ask(&store.inner);
        // The commit fails (landing its racing notice first), but the asker never shows up as
        // told: every check misses it.
        *lock(&store.fail_commit) = Some((1, StoreError::new("disk I/O error")));
        *lock(&store.blind_finds) = usize::MAX;
        assert_eq!(
            mailbox.notify_expired(&request, NOW_UTC),
            Err(MailboxError::StorageError)
        );
    }

    fn identity() -> OfficeIdentity {
        OfficeIdentity {
            office_id: OFFICE_ID.into(),
            incarnation: INCARNATION.into(),
            is_hq: true,
        }
    }

    fn defer_at(offset_ms: u64) -> MsgDeferParams {
        MsgDeferParams {
            id: msg_id(1),
            until: SnoozeTarget::Until {
                at: format_utc(NOW_MS + offset_ms).unwrap(),
            },
            note: None,
        }
    }

    /// Two defers of one message: the second one starts while the first has written the
    /// store but not yet its timer. It must wait for the first to finish, so the timers end
    /// at the deadline the store holds (the second's).
    #[test]
    fn concurrent_defers_leave_the_timer_at_the_stores_deadline() {
        let store = InMemoryStore::new();
        let (applied, first_applied) = mpsc::channel();
        let triage = Arc::new(GatedTriage {
            inner: store.clone(),
            gated: AtomicBool::new(false),
            applied: Mutex::new(applied),
            release: Barrier::new(2),
        });
        let mailbox = Arc::new(
            Mailbox::with_triage(
                Arc::new(store.clone()),
                triage.clone(),
                Arc::new(InMemoryDirectory::new(members(), POSITION)),
                identity(),
                Arc::new(ManualClock::new(NOW_MS)),
                Arc::new(NoMessageEvents),
            )
            .unwrap(),
        );
        let sent = MsgSendParams {
            id: msg_id(1),
            to: ADA_ID.into(),
            kind: KIND_MSG.into(),
            priority: None,
            subject: None,
            body: "hello".into(),
            meta: Meta::new(),
            reply_to: None,
        };
        mailbox.send(PAT_ID, sent).unwrap();

        triage.gated.store(true, Ordering::SeqCst);
        let first = {
            let mailbox = mailbox.clone();
            thread::spawn(move || mailbox.defer(ADA_ID, defer_at(1_000)))
        };
        // Bounded, so a first defer that fails before its store write fails the test rather
        // than hanging it.
        first_applied
            .recv_timeout(Duration::from_secs(10))
            .expect("the first defer reached its store write");
        let (finished, second_done) = mpsc::channel();
        let second = {
            let mailbox = mailbox.clone();
            thread::spawn(move || {
                let result = mailbox.defer(ADA_ID, defer_at(2_000));
                finished.send(()).unwrap();
                result
            })
        };
        // Without the ordering the second defer writes the store and its timer now, and the
        // first then overwrites the timer with its own (older) deadline.
        assert!(
            second_done
                .recv_timeout(Duration::from_millis(300))
                .is_err(),
            "the second defer finished while the first's timer update was pending"
        );
        triage.release.wait();
        assert_eq!(first.join().unwrap().unwrap().triage_gen, 1);
        let second = second.join().unwrap().unwrap();
        assert_eq!(second.triage_gen, 2);

        let stored = store
            .get_message(&msg_id(1))
            .unwrap()
            .unwrap()
            .message
            .header;
        let deadline = format_utc(NOW_MS + 2_000).unwrap();
        assert_eq!(stored.snoozed_until.as_deref(), Some(deadline.as_str()));
        let timers = lock(&mailbox.snoozes);
        assert_eq!(timers.next_for(ADA_ID), Some(epoch_ms(NOW_MS + 2_000)));
    }

    /// Expiring a request more than once commits one notice and announces it once:
    ///
    /// - a later expiry finds the notice through the store (`find_reply_for_ask`) and makes
    ///   no commit at all;
    /// - one racing the first, whose check doesn't see the notice yet, commits under the same
    ///   first id and gets the replay (`inserted: false`), so it publishes nothing;
    /// - one racing it with other content (the recipient renamed since) is refused
    ///   (`AlreadyExists`), asks the store again, finds the notice and tries no fallback.
    ///
    /// Each counts as told.
    #[test]
    fn expiring_twice_commits_exactly_one_notice() {
        let store = Arc::new(RacingStore::default());
        let events = Arc::new(RecordingEvents::default());
        let mailbox = racing_mailbox(&store, &events);
        let request = expired_ask(&store.inner);
        let commits = || store.commits.load(Ordering::SeqCst);
        let notice = delivery_failed_id(&request, INCARNATION);

        assert_eq!(mailbox.notify_expired(&request, NOW_UTC), Ok(()));
        assert_eq!(commits(), 1);
        assert_eq!(notices(&store.inner), std::slice::from_ref(&notice));

        // Found by the store: no commit.
        assert_eq!(mailbox.notify_expired(&request, NOW_UTC), Ok(()));
        assert_eq!(commits(), 1);

        // Racing, same content: the replay of the first id.
        *lock(&store.blind_finds) = 1;
        assert_eq!(mailbox.notify_expired(&request, NOW_UTC), Ok(()));
        assert_eq!(*lock(&store.blind_finds), 0, "the check missed the notice");
        assert_eq!(commits(), 2, "committed again under the first id");

        // Racing, other content: refused, then found on the second check.
        let renamed = MessageHeader {
            to_display_name: "Ada L.".into(),
            ..request.clone()
        };
        *lock(&store.blind_finds) = 1;
        assert_eq!(mailbox.notify_expired(&renamed, NOW_UTC), Ok(()));
        assert_eq!(
            *lock(&store.blind_finds),
            0,
            "the first check missed the notice"
        );
        assert_eq!(commits(), 3, "no fallback after the refusal");

        assert_eq!(notices(&store.inner), std::slice::from_ref(&notice));
        let stored = store.inner.get_message(&notice).unwrap().unwrap();
        assert!(is_notice_for(&stored.message.header, &request));
        assert_eq!(stored.message.header.to_display_name, PAT_HANDLE);
        assert_eq!(events.received(), [notice]);
        assert_eq!(events.delivery_failed().len(), 1);
    }
}
