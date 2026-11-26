//! The live-event hub: [`HubEvent`]s published by the host connection and the office
//! bridge, broadcast to every open SSE stream (`sse`).
//!
//! A leaf: the office bridge (`office`) publishes into it and the SSE streams (`sse`)
//! subscribe to it, so neither depends on the other through the hub.

use org_proto::web::{Feed, ResyncReason};
use serde_json::Value;
use tokio::sync::broadcast;

/// Bounded broadcast capacity shared by all active SSE listeners.
const HUB_CAPACITY: usize = 256;

/// Internal live event published into [`EventHub`] or a per-stream office poller.
#[derive(Clone, Debug)]
pub enum HubEvent {
    SessionEnded {
        session_id: String,
    },
    InstanceStatus {
        event: String,
        data: Value,
    },
    OrganizationChanged {
        org_id: String,
        cursor: String,
        event: String,
        data: Value,
    },
    MessagingEvent {
        org_id: String,
        cursor: String,
        event: String,
        data: Value,
    },
    PresenceChanged {
        org_id: String,
        cursor: String,
        data: Value,
    },
    OrgResync {
        org_id: String,
        feed: Feed,
        reason: ResyncReason,
    },
}

/// Broadcast hub connecting host/office events to active SSE streams.
#[derive(Clone)]
pub struct EventHub {
    sender: broadcast::Sender<HubEvent>,
}

impl Default for EventHub {
    fn default() -> Self {
        let (sender, _) = broadcast::channel(HUB_CAPACITY);
        Self { sender }
    }
}

impl EventHub {
    pub fn publish(&self, event: HubEvent) {
        let _ = self.sender.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<HubEvent> {
        self.sender.subscribe()
    }

    /// Translates a host-control event pushed on the admitted web connection into a
    /// hub broadcast when relevant to live SSE subscribers.
    pub fn observe_host_event(&self, name: &str, data: &Value) {
        use org_host_control::{
            browser::{SessionEnded, SESSION_ENDED},
            event,
        };
        if name == SESSION_ENDED {
            if let Ok(ended) = serde_json::from_value::<SessionEnded>(data.clone()) {
                self.publish(HubEvent::SessionEnded {
                    session_id: ended.session_id,
                });
            }
        } else if matches!(
            name,
            event::ORGANIZATION_CREATED
                | event::ORGANIZATION_CHANGED
                | event::OFFICE_STATUS_CHANGED
        ) {
            self.publish(HubEvent::InstanceStatus {
                event: name.to_owned(),
                data: data.clone(),
            });
        }
    }
}
