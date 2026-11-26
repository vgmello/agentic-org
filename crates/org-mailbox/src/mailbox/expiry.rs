//! Expiry: requests past their deadline, and the `delivery_failed` notice their asker gets.

use org_proto::msg::{
    is_urgent, json_string_len, DeliveryFailedReason, Message, MessageDeliveryFailedData,
    MessageHeader, MessageLifecycleState, MessagePriority, MessageReceivedData, Meta,
    KIND_DELIVERY_FAILED, KIND_REQUEST, MAX_PAGE_LIMIT, OFFICE_SENDER,
};
use org_proto::{Member, MemberKind, Position};

use crate::ids::{delivery_failed_fallback_id, delivery_failed_id};
use crate::{
    content_hash, format_utc, past_deadline, CommitMessageOutcome, ExpiryProgress, HeaderQuery,
    HeaderScope, LocalStateTransition, MailboxError, MessageEvent, NewStoredMessage, StoreError,
};

use super::*;

/// The `delivery_failed` notice's subject: fixed text, never the original's content.
const EXPIRED_SUBJECT: &str = "Request expired";

impl Mailbox {
    /// Office-internal expiry maintenance for one canonical local recipient. No
    /// external request is authorized here: a trusted office timer owns scheduling
    /// and recipient discovery. `limit` must be in `1..=MAX_PAGE_LIMIT`.
    ///
    /// Pages inspect headers, newest first, including terminal requests whose
    /// notice previously failed. The transition/notice rules are the same ones
    /// used by read and wait. On error, retain the input cursor and retry it: a
    /// partial page is safe to replay. Completed passes restart at `None` on a
    /// later tick to cover new arrivals or deadlines crossed during the scan.
    pub fn expire_due(
        &self,
        recipient_id: &str,
        cursor: Option<u64>,
        limit: u32,
    ) -> Result<ExpiryProgress, MailboxError> {
        if limit == 0 || limit > MAX_PAGE_LIMIT {
            return Err(MailboxError::InvalidRequest);
        }
        let (recipient, position) = self.bound_member(recipient_id)?;
        self.triage()?;
        let now_ms = self.clock.now_ms();
        let now = format_utc(now_ms).ok_or(MailboxError::StorageError)?;
        let page = self.store.list_headers(HeaderQuery {
            scope: HeaderScope::Inbox(recipient_id.to_owned()),
            from_member_id: None,
            thread_id: None,
            unread_only: false,
            include_snoozed: true,
            cursor,
            limit,
        })?;
        for header in &page.headers {
            match header.state {
                MessageLifecycleState::Expired
                    if recipient.kind == MemberKind::Agent && header.kind == KIND_REQUEST =>
                {
                    // No new transition: retry the notice even if the recipient
                    // never reads the expired request again.
                    self.notify_expired(header, &now)?;
                }
                MessageLifecycleState::Unread | MessageLifecycleState::Snoozed
                    if past_deadline(header, now_ms)
                        && !header.expired_waiting_person
                        && (recipient.kind == MemberKind::Agent
                            || header.state == MessageLifecycleState::Unread) =>
                {
                    self.expire(&recipient, position, &header.id, &now, NoticeFailure::Fail)?;
                }
                _ => {}
            }
        }
        Ok(ExpiryProgress {
            inspected: page.headers.len(),
            next_cursor: page.next_cursor,
        })
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
    pub(super) fn expire(
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
                lock(&self.snoozes).remove(&recipient.id, &header.id);
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
    pub(super) fn answered_or_told(&self, original: &MessageHeader) -> Result<bool, MailboxError> {
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
    pub(super) fn notify_expired(
        &self,
        original: &MessageHeader,
        now: &str,
    ) -> Result<(), MailboxError> {
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
}
