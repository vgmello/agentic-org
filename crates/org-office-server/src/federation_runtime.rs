//! Optional federation for a served office: the HQ listener or the satellite's persistent sync,
//! with their wake and delivery callbacks.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::federation_stream::{self, SatSyncWake};
use crate::{chat_calls, federation, live, org_feed, watcher};
use org_office::Office;

pub(crate) type HqFederationSlot = Arc<std::sync::OnceLock<federation::HqFederationHandle>>;

pub(crate) struct FederationRuntime {
    hq: Option<federation::HqFederationServer>,
    sat_stop: Arc<AtomicBool>,
    sat_wake: Arc<SatSyncWake>,
    sat_thread: Option<std::thread::JoinHandle<()>>,
    /// The satellite's shared-file outbox shipper (`TASK-98.6`).
    blob_thread: Option<std::thread::JoinHandle<()>>,
}

impl FederationRuntime {
    pub(crate) fn start(
        office: &Arc<Office>,
        session_manager: &Arc<org_office_sessions::SessionManager>,
        chat_svc: &Arc<chat_calls::ChatService>,
        delivery_notifier: federation::InboundDeliveryNotifier,
        sat_wake: Arc<SatSyncWake>,
        feed_wake: Arc<org_feed::FeedWake>,
        hq_listening: &Arc<AtomicBool>,
    ) -> Result<Self, String> {
        let sat_stop = Arc::new(AtomicBool::new(false));
        let wake_handler = build_federation_wake_handler(session_manager, chat_svc);
        let hq = start_optional_hq_federation(
            office,
            Some(Arc::clone(&wake_handler)),
            Some(Arc::clone(&delivery_notifier)),
            Some(Arc::clone(&feed_wake)),
        )?;
        hq_listening.store(hq.is_some(), Ordering::SeqCst);
        let blob_thread =
            federation::spawn_satellite_blob_shipper(office, &sat_stop, Arc::clone(&sat_wake));
        let sat_thread = start_optional_satellite_sync(
            office,
            &sat_stop,
            Arc::clone(&sat_wake),
            wake_handler,
            delivery_notifier,
            Some(feed_wake),
            Arc::downgrade(session_manager),
        );
        Ok(Self {
            hq,
            sat_thread,
            sat_stop,
            sat_wake,
            blob_thread,
        })
    }

    pub(crate) fn hq_handle(&self) -> Option<federation::HqFederationHandle> {
        self.hq.as_ref().map(federation::HqFederationServer::handle)
    }

    pub(crate) fn stop(self) {
        if let Some(fed) = self.hq {
            fed.stop();
        }
        self.sat_stop.store(true, Ordering::SeqCst);
        self.sat_wake.notify();
        if let Some(handle) = self.sat_thread {
            let _ = handle.join();
        }
        if let Some(handle) = self.blob_thread {
            let _ = handle.join();
        }
    }
}

pub(crate) fn build_federation_delivery_notifier(
    owner_id: String,
    chat_svc: &Arc<chat_calls::ChatService>,
    mailbox: &Arc<org_mailbox::Mailbox>,
    live: &Arc<live::LiveEvents>,
    watcher: &Arc<watcher::ChannelWatcher>,
) -> federation::InboundDeliveryNotifier {
    let chat_svc = Arc::downgrade(chat_svc);
    let mailbox = Arc::downgrade(mailbox);
    let live = Arc::downgrade(live);
    let watcher = Arc::downgrade(watcher);
    Arc::new(move |outcome: &org_office::RelayedMessageOutcome| {
        if let Some(ask_id) = outcome.closed_ask_id.as_deref() {
            if let Some(chat) = chat_svc.upgrade() {
                chat.notify_ask_waiters();
            }
            if let Some(w) = watcher.upgrade() {
                let reason = outcome.closed_ask_reason.as_deref().unwrap_or("answered");
                w.on_question_closed(ask_id, reason);
            }
            if let Some(lv) = live.upgrade() {
                lv.record_mailbox_change();
            }
        }
        let mut owner_touched = outcome.handoff_accepted;
        for header in &outcome.local_delivered_headers {
            if let Some(mb) = mailbox.upgrade() {
                mb.notify_member(&header.to_member_id);
            }
            if header.to_member_id == owner_id {
                owner_touched = true;
            }
            if outcome.inserted {
                if let Some(lv) = live.upgrade() {
                    lv.publish_live(live::Live::Received(Box::new(
                        org_proto::chat::header_only_received(header.clone()),
                    )));
                }
            }
        }
        if owner_touched {
            if let Some(lv) = live.upgrade() {
                lv.record_mailbox_change();
            }
        }
        if outcome.inserted {
            if let (Some(msg), Some(conv_kind), Some(w)) = (
                outcome.delivered_message.as_ref(),
                outcome.conversation_kind,
                watcher.upgrade(),
            ) {
                w.on_channel_message_posted(msg, conv_kind);
            }
        }
    })
}

pub(crate) fn build_federation_wake_handler(
    session_manager: &Arc<org_office_sessions::SessionManager>,
    chat_svc: &Arc<chat_calls::ChatService>,
) -> federation::WakeRelayHandler {
    let sessions = Arc::downgrade(session_manager);
    let chat = Arc::downgrade(chat_svc);
    Arc::new(move |wake: federation::RelayedWake| {
        let Some(sessions) = sessions.upgrade() else {
            return Err("session manager shut down".to_owned());
        };
        if let Some(w) = wake.ask_withdrawal {
            return sessions.on_ask_withdrawn_durable(&w.asker_id, &w.ask_id);
        }
        if let Some(ans) = wake.ask_answer {
            // Dispatch structured answer transition and propagate persistence errors so
            // federation only commits `delivered: true` after resume intent is durable (`TASK-52` P6-#2 / P7-#4).
            let satisfied_active_waiter = chat
                .upgrade()
                .is_some_and(|svc| svc.has_active_ask_waiter(&ans.asker_id, &ans.ask_id));
            return sessions
                .on_ask_answered_durable_with_attachments(
                    &ans.asker_id,
                    &ans.ask_id,
                    &ans.question_summary,
                    &ans.answer_summary,
                    (&wake.sender_id, &wake.sender_name, wake.sender_is_person),
                    false,
                    satisfied_active_waiter,
                    &ans.attachment_ids,
                )
                .map(|_| ());
        }
        let kind = match wake.kind.as_str() {
            "mention" => org_office_sessions::WakeKind::Mention,
            "handoff" => org_office_sessions::WakeKind::Handoff,
            "ask" => org_office_sessions::WakeKind::Ask,
            "ask_answer" => org_office_sessions::WakeKind::AskAnswer,
            "task" => org_office_sessions::WakeKind::TaskAssignment,
            _ => org_office_sessions::WakeKind::DirectMessage,
        };
        let item = org_office_sessions::PendingWakeItem {
            item_id: format!("relay:{}:{}", wake.source_id, wake.employee_id),
            kind,
            sender_id: wake.sender_id,
            sender_name: wake.sender_name,
            sender_handle: None,
            sender_relation: None,
            sender_is_person: wake.sender_is_person,
            conversation_id: wake.conversation_id,
            source_id: wake.source_id,
            summary: wake.summary,
            attachment_ids: wake.attachment_ids,
            enqueued_at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        };
        sessions
            .enqueue_wake_durable(&wake.employee_id, item)
            .map(|_| ())
    })
}

fn start_optional_hq_federation(
    office: &Arc<Office>,
    wake_handler: Option<federation::WakeRelayHandler>,
    delivery_notifier: Option<federation::InboundDeliveryNotifier>,
    feed_wake: Option<Arc<org_feed::FeedWake>>,
) -> Result<Option<federation::HqFederationServer>, String> {
    if office.info().role != org_office::OfficeRole::Hq {
        return Ok(None);
    }
    let Ok(bind_raw) = std::env::var("AGENTIC_ORG_FEDERATION_BIND") else {
        return Ok(None);
    };
    let bind_raw = bind_raw.trim();
    if bind_raw.is_empty() {
        return Ok(None);
    }
    let addr: std::net::SocketAddr = bind_raw
        .parse()
        .map_err(|e| format!("invalid AGENTIC_ORG_FEDERATION_BIND `{bind_raw}`: {e}"))?;
    let mut san_hosts = vec![addr.ip().to_string(), "localhost".to_owned()];
    // Configure advertised endpoints separately from bind address so `0.0.0.0` binds cover DNS SANs (P2-16).
    if let Ok(adv) = std::env::var("AGENTIC_ORG_FEDERATION_ADVERTISE") {
        for item in adv.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            san_hosts.push(item.to_owned());
        }
    }
    federation::HqFederationServer::start_with_hooks(
        Arc::clone(office),
        addr,
        &san_hosts,
        wake_handler,
        delivery_notifier,
        feed_wake,
    )
    .map(Some)
}

pub(crate) fn start_optional_satellite_sync(
    office: &Arc<Office>,
    stop: &Arc<AtomicBool>,
    sat_wake: Arc<SatSyncWake>,
    wake_handler: federation::WakeRelayHandler,
    delivery_notifier: federation::InboundDeliveryNotifier,
    feed_wake: Option<Arc<org_feed::FeedWake>>,
    sessions_for_recovery: std::sync::Weak<org_office_sessions::SessionManager>,
) -> Option<std::thread::JoinHandle<()>> {
    if office.info().role != org_office::OfficeRole::Satellite {
        return None;
    }
    let office = Arc::clone(office);
    let stop = Arc::clone(stop);
    let hooks = federation_stream::PersistentSyncHooks {
        wake_handler,
        delivery_notifier,
        feed_wake,
        sessions: sessions_for_recovery,
        drain_inbound: federation::drain_satellite_inbound_persistent,
    };
    std::thread::Builder::new()
        .name("sat-federation-sync".into())
        .spawn(move || {
            federation_stream::run_persistent_satellite_sync(&office, &stop, &sat_wake, &hooks);
        })
        .ok()
}

/// The callback a committed cross-office write fires: it asks the office store to flush the
/// buffered entry now (`TASK-140`; a `chat.send` is `Ack::Buffered`, so without it the relay
/// publisher, which reads only confirmed entries, waits out the group-flush interval on every
/// hop), then wakes the satellite's persistent sync and every HQ peer session.
pub(crate) fn build_sync_wake_fn(
    office: &Arc<Office>,
    sat_sync_wake: Arc<SatSyncWake>,
    hq_fed_slot: HqFederationSlot,
) -> Arc<dyn Fn() + Send + Sync> {
    let office = Arc::downgrade(office);
    Arc::new(move || {
        if let Some(office) = office.upgrade() {
            office.request_store_flush();
        }
        sat_sync_wake.notify();
        if let Some(hq) = hq_fed_slot.get() {
            hq.notify_all_peers();
        }
    })
}
