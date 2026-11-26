//! Delivery: `send`, and reading the inbox (`inbox`, `read`, `wait`).

use std::time::{Duration, Instant};

use org_proto::msg::{
    is_urgent, json_string_len, HeaderPage, Message, MessageHeader, MessageLifecycleState,
    MessagePriority, MessageReceivedData, MsgInboxParams, MsgReadParams, MsgReadResult,
    MsgSendParams, MsgWaitParams, MsgWaitResult, SendResult, DEFAULT_PAGE_LIMIT,
    KIND_DELIVERY_FAILED, KIND_REQUEST, MAX_BODY_BYTES, MAX_ID_BYTES, MAX_META_BYTES,
    MAX_PAGE_LIMIT, MAX_SUBJECT_BYTES, MAX_WAIT_MS,
};
use org_proto::{ulid_time_ms, uuid_v7_time_ms, MemberKind};

use super::{
    address, can_message, epoch_ms, lock, optional_size, size, Mailbox, NoticeFailure, MAX_RECHECKS,
};
use crate::waiters::WaitOutcome;
use crate::{
    content_hash, format_utc, past_deadline, Cancellation, HeaderQuery, HeaderScope, MailboxError,
    MessageEvent, NewStoredMessage, StoredMessage, MAX_EXPIRED_IDS,
};

impl Mailbox {
    pub fn send(
        &self,
        bound_member_id: &str,
        params: MsgSendParams,
    ) -> Result<SendResult, MailboxError> {
        let (sender, _) = self.bound_member(bound_member_id)?;
        size(&params.id, MAX_ID_BYTES)?;
        if uuid_v7_time_ms(&params.id).is_none() && ulid_time_ms(&params.id).is_none() {
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
        let (stored, claimed) = {
            let _ordered = lock(&self.triage_writes);
            let stored = self
                .store
                .claim_read(bound_member_id, params.id.as_deref(), &now)?;
            let claimed = stored.as_ref().is_some_and(|stored| {
                let header = &stored.message.header;
                header.state == MessageLifecycleState::Read
                    && header.read_at.as_deref() == Some(now.as_str())
            });
            if claimed {
                if let Some(stored) = &stored {
                    lock(&self.snoozes).remove(bound_member_id, &stored.message.header.id);
                }
            }
            (stored, claimed)
        };
        if params.id.is_some() && stored.is_none() {
            return Err(MailboxError::NotFound);
        }
        if claimed {
            if let Some(stored) = &stored {
                self.publish_state(&stored.message.header, None, &now);
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
