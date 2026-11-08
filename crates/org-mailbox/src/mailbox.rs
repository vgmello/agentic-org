use std::sync::Arc;
use std::time::{Duration, Instant};

use org_proto::msg::{
    is_urgent, json_string_len, HeaderPage, Message, MessageHeader, MessageLifecycleState,
    MessagePriority, MessageReceivedData, MessageStateChangedData, MsgInboxParams, MsgReadParams,
    MsgReadResult, MsgSendParams, MsgWaitParams, MsgWaitResult, SendResult, DEFAULT_PAGE_LIMIT,
    KIND_DELIVERY_FAILED, KIND_REQUEST, MAX_BODY_BYTES, MAX_HANDLE_BYTES, MAX_ID_BYTES,
    MAX_META_BYTES, MAX_PAGE_LIMIT, MAX_SUBJECT_BYTES, MAX_WAIT_MS, OFFICE_SENDER,
};
use org_proto::{ulid_time_ms, AgentStatus, Member, MemberKind, Position};

use crate::waiters::{WaitOutcome, WaiterRegistry};
use crate::{
    content_hash, format_utc, Cancellation, Clock, HeaderQuery, HeaderScope, MailboxError,
    MemberDirectory, MessageEvent, MessageEvents, MessageStore, NewStoredMessage, StoredMessage,
};

#[derive(Debug, Clone)]
pub struct OfficeIdentity {
    pub office_id: String,
    pub incarnation: String,
    pub is_hq: bool,
}

/// The server supplies a canonical member id from its authenticated binding, not
/// from request JSON. Database calls never run while a registry lock is held.
pub struct Mailbox {
    store: Arc<dyn MessageStore + Send + Sync>,
    directory: Arc<dyn MemberDirectory + Send + Sync>,
    office: OfficeIdentity,
    clock: Arc<dyn Clock>,
    events: Arc<dyn MessageEvents>,
    waiters: WaiterRegistry,
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
            directory,
            office,
            clock,
            events,
            waiters: WaiterRegistry::default(),
        }
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
        self.bound_member(bound_member_id)?;
        optional_size(params.id.as_deref(), MAX_ID_BYTES)?;
        let now = self.now_utc()?;
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
                self.events
                    .publish(MessageEvent::StateChanged(MessageStateChangedData {
                        id: header.id.clone(),
                        thread_id: header.thread_id.clone(),
                        to_member_id: header.to_member_id.clone(),
                        state: header.state,
                        triage_gen: header.triage_gen,
                        snoozed_until: header.snoozed_until.clone(),
                        snooze_after_task: header.snooze_after_task,
                        reason: None,
                        expired_waiting_person: header.expired_waiting_person,
                        updated_at: now.clone(),
                    }));
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
        loop {
            if cancellation.is_cancelled() {
                return Err(MailboxError::NotBound);
            }
            let observed = registration.generation();
            self.bound_member(bound_member_id)?;
            let peek = self.store.peek_next_unread(
                bound_member_id,
                from.as_deref(),
                params.urgent_only,
                &self.now_utc()?,
            )?;
            if cancellation.is_cancelled() {
                return Err(MailboxError::NotBound);
            }
            if let Some(message) = peek.message {
                return Ok(MsgWaitResult {
                    message: Some(message.message),
                });
            }
            match registration.wait(observed, deadline, cancellation) {
                WaitOutcome::Cancelled => return Err(MailboxError::NotBound),
                WaitOutcome::Deadline => return Ok(MsgWaitResult { message: None }),
                WaitOutcome::Notified => {}
            }
        }
    }

    /// Held `wait` registrations right now. Diagnostics and tests: a cancelled, finished or
    /// timed-out wait leaves none behind.
    pub fn held_waits(&self) -> usize {
        self.waiters.len()
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
