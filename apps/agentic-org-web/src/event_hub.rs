//! The live-event hub: [`HubEvent`]s published by the host connection and the office
//! bridge, broadcast to every open SSE stream (`sse`).
//!
//! A leaf: the office bridge (`office`) publishes into it and the SSE streams (`sse`)
//! subscribe to it, so neither depends on the other through the hub.
//!
//! The streams reach it through [`EventBus`], a two-method seam (`doc-104` P0): today one
//! in-process implementation, [`InMemoryEventBus`] over [`EventHub`]'s bounded broadcast.
//! A shared bus for several web processes would be a second implementation, nothing more; no
//! broker is linked.

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
    /// An agent's coalesced coarse live activity (`member.activity_changed` on the
    /// `presence` feed, `TASK-103`): a phase and a tool's name only.
    ///
    /// **Invariant:** the [`EventBus`] is shared by every subscriber of the organization, so
    /// only what every member may see is published here. A caller-scoped or owner-private
    /// frame (such as the Chief of Staff's `active_tool_name`, which only the owner sees) must
    /// never be published on the bus; it reaches its caller only through that caller's own
    /// poll, filtered by the office.
    ActivityChanged {
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

/// Publishes live events and subscribes streams to them (`doc-104` P0). Delivery is a bounded
/// broadcast: a subscriber that falls behind is told it lagged
/// ([`broadcast::error::RecvError::Lagged`]) and resyncs; nothing is replayed.
pub trait EventBus: Send + Sync {
    /// Hands `event` to every current subscriber; never blocks.
    fn publish(&self, event: HubEvent);
    /// Every event published from now on.
    fn subscribe(&self) -> broadcast::Receiver<HubEvent>;
}

/// The in-process [`EventBus`]: this web process's [`EventHub`].
#[derive(Clone, Default)]
pub struct InMemoryEventBus {
    hub: EventHub,
}

impl InMemoryEventBus {
    pub fn new(hub: EventHub) -> Self {
        Self { hub }
    }
}

impl EventBus for InMemoryEventBus {
    fn publish(&self, event: HubEvent) {
        self.hub.publish(event);
    }

    fn subscribe(&self) -> broadcast::Receiver<HubEvent> {
        self.hub.subscribe()
    }
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
