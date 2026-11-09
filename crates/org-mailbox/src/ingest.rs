//! The unauthenticated offline path, isolated from live connection operations.
use super::{address, Mailbox};
use crate::{format_utc, MailboxError};
use org_proto::msg::{MsgSendParams, SendResult, SpooledSend, SPOOL_VERSION};
use org_proto::{AgentStatus, MemberKind};

impl Mailbox {
    /// Accept an unauthenticated offline send only from an active hand-run agent.
    /// All normal send validation, retry deduplication and waiter/event delivery apply.
    pub fn ingest_spooled(&self, send: SpooledSend) -> Result<SendResult, MailboxError> {
        if send.v != SPOOL_VERSION || send.office_id != self.office.office_id {
            return Err(MailboxError::InvalidRequest);
        }
        let from = address(&send.from)?.ok_or(MailboxError::NotAuthorized)?;
        if from.starts_with("run:") || from.eq_ignore_ascii_case(org_proto::OWNER_HANDLE) {
            return Err(MailboxError::NotAuthorized);
        }
        let (member, _) = self
            .directory
            .resolve_member(from)?
            .ok_or(MailboxError::NotAuthorized)?;
        if member.owner
            || member.kind != MemberKind::Agent
            || !member.agent.as_ref().is_some_and(|agent| {
                agent.status == AgentStatus::Active && agent.run_mode.is_none()
            })
        {
            return Err(MailboxError::NotAuthorized);
        }
        let created =
            crate::time::parse_utc(&send.created_at).ok_or(MailboxError::InvalidRequest)?;
        let created =
            format_utc(created.min(self.clock.now_ms())).ok_or(MailboxError::InvalidRequest)?;
        self.send_at(
            &member.id,
            MsgSendParams {
                id: send.id,
                to: send.to,
                kind: send.kind,
                priority: Some(send.priority),
                subject: send.subject,
                body: send.body,
                meta: send.meta,
                reply_to: send.reply_to,
            },
            Some(created),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::fixture::*;
    use crate::testkit::{InMemoryDirectory, InMemoryStore};
    use crate::{cancellation_pair, Clock, MessageStore, OfficeIdentity};
    use org_proto::msg::{MessagePriority, MsgWaitParams};
    use std::sync::Arc;

    fn send() -> SpooledSend {
        SpooledSend {
            v: SPOOL_VERSION,
            office_id: OFFICE_ID.into(),
            from: ADA_HANDLE.into(),
            id: msg_id(1),
            to: REMY_HANDLE.into(),
            kind: "msg".into(),
            priority: MessagePriority::Normal,
            subject: None,
            body: "hello".into(),
            meta: Default::default(),
            reply_to: None,
            created_at: NOW_UTC.into(),
        }
    }

    fn setup() -> (
        Arc<Mailbox>,
        InMemoryStore,
        Arc<RecordingEvents>,
        Arc<ManualClock>,
    ) {
        let store = InMemoryStore::new();
        let events = Arc::new(RecordingEvents::default());
        let clock = Arc::new(ManualClock::new(NOW_MS));
        let mailbox = Arc::new(Mailbox::new(
            Arc::new(store.clone()),
            Arc::new(InMemoryDirectory::new(members(), POSITION)),
            OfficeIdentity {
                office_id: OFFICE_ID.into(),
                incarnation: INCARNATION.into(),
                is_hq: true,
            },
            clock.clone(),
            events.clone(),
        ));
        (mailbox, store, events, clock)
    }

    #[test]
    fn preserves_id_and_past_time_clamps_future_and_replays_without_events() {
        let (mailbox, store, events, clock) = setup();
        let mut first = send();
        first.created_at = "2020-01-01T00:00:00.000Z".into();
        let sent = mailbox.ingest_spooled(first.clone()).unwrap();
        clock.set(NOW_MS + 10000);
        assert_eq!(mailbox.ingest_spooled(first.clone()).unwrap(), sent);
        assert_eq!(
            store
                .get_message(&first.id)
                .unwrap()
                .unwrap()
                .message
                .header
                .created_at,
            first.created_at
        );
        assert_eq!(events.received(), vec![first.id.clone()]);
        first.body = "different".into();
        assert_eq!(
            mailbox.ingest_spooled(first),
            Err(MailboxError::AlreadyExists)
        );
        let mut future = send();
        future.id = msg_id(2);
        future.from = ADA_ID.into();
        future.created_at = "9999-01-01T00:00:00.000Z".into();
        mailbox.ingest_spooled(future.clone()).unwrap();
        assert_eq!(
            store
                .get_message(&future.id)
                .unwrap()
                .unwrap()
                .message
                .header
                .created_at,
            format_utc(clock.now_ms()).unwrap()
        );
    }

    #[test]
    fn spool_reply_requires_parent_visibility_and_wakes_a_waiter() {
        let (mailbox, _, _, _) = setup();
        let mut parent = send();
        parent.to = PAT_ID.into();
        mailbox.ingest_spooled(parent.clone()).unwrap();
        let mut reply = send();
        reply.id = msg_id(2);
        reply.reply_to = Some(parent.id);
        // Remy is not a participant of Ada -> Pat.
        reply.from = REMY_ID.into();
        // Paused is refused by the stronger spool rule before parent visibility.
        assert_eq!(
            mailbox.ingest_spooled(reply.clone()),
            Err(MailboxError::NotAuthorized)
        );
        reply.from = SHADOW_ID.into();
        assert_eq!(
            mailbox.ingest_spooled(reply.clone()),
            Err(MailboxError::NotFound)
        );
        reply.from = ADA_HANDLE.into();
        let (cancel, cancellation) = cancellation_pair();
        let waiting = Arc::clone(&mailbox);
        let worker = std::thread::spawn(move || {
            waiting.wait(
                REMY_ID,
                MsgWaitParams {
                    timeout_ms: 2000,
                    from: None,
                    urgent_only: false,
                },
                std::time::Instant::now() + std::time::Duration::from_secs(2),
                &cancellation,
            )
        });
        mailbox.ingest_spooled(reply).unwrap();
        let result = worker.join().unwrap().unwrap();
        assert_eq!(result.message.unwrap().header.id, msg_id(2));
        drop(cancel);
    }
}
