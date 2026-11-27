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
use tokio::sync::{broadcast, mpsc};

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

/// Error returned when receiving from an [`EventSubscription`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventRecvError {
    /// The subscriber fell behind by `skipped` events and must resync.
    Lagged(u64),
    /// The event bus closed; no further events will arrive.
    Closed,
}

enum SubscriptionKind {
    Broadcast(broadcast::Receiver<HubEvent>),
    Channel(mpsc::Receiver<Result<HubEvent, EventRecvError>>),
}

/// One subscriber's live event stream returned by [`EventBus::subscribe`].
///
/// Wraps the in-process broadcast receiver (or a custom channel receiver) so SSE
/// handlers depend only on [`EventBus`] and never on `tokio::sync::broadcast` internals.
pub struct EventSubscription {
    inner: SubscriptionKind,
}

impl EventSubscription {
    /// Wraps a bounded Tokio broadcast receiver with zero extra allocation or tasks.
    pub fn from_broadcast(receiver: broadcast::Receiver<HubEvent>) -> Self {
        Self {
            inner: SubscriptionKind::Broadcast(receiver),
        }
    }

    /// Wraps an explicit `mpsc` receiver (e.g., for custom in-memory or bridged buses).
    pub fn from_channel(receiver: mpsc::Receiver<Result<HubEvent, EventRecvError>>) -> Self {
        Self {
            inner: SubscriptionKind::Channel(receiver),
        }
    }

    /// Receives the next live event, lag signal, or bus close.
    pub async fn recv(&mut self) -> Result<HubEvent, EventRecvError> {
        match &mut self.inner {
            SubscriptionKind::Broadcast(rx) => match rx.recv().await {
                Ok(event) => Ok(event),
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    Err(EventRecvError::Lagged(skipped))
                }
                Err(broadcast::error::RecvError::Closed) => Err(EventRecvError::Closed),
            },
            SubscriptionKind::Channel(rx) => rx.recv().await.unwrap_or(Err(EventRecvError::Closed)),
        }
    }
}

/// Publishes live events and subscribes streams to them (`doc-104` P0, `TASK-232`). Delivery is a
/// bounded fan-out: a subscriber that falls behind is told it lagged ([`EventRecvError::Lagged`])
/// and resyncs; nothing is replayed.
pub trait EventBus: Send + Sync {
    /// Hands `event` to every current subscriber; never blocks.
    fn publish(&self, event: HubEvent);
    /// Every event published from now on.
    fn subscribe(&self) -> EventSubscription;
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

    fn subscribe(&self) -> EventSubscription {
        EventSubscription::from_broadcast(self.hub.subscribe())
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
        if let Some(event) = translate_host_event(name, data) {
            self.publish(event);
        }
    }
}

/// Translates a host-control event pushed on the admitted web connection and publishes it
/// through any [`EventBus`] when relevant to live SSE subscribers.
pub fn observe_host_event_on_bus(bus: &dyn EventBus, name: &str, data: &Value) {
    if let Some(event) = translate_host_event(name, data) {
        bus.publish(event);
    }
}

fn translate_host_event(name: &str, data: &Value) -> Option<HubEvent> {
    use org_host_control::{
        browser::{SessionEnded, SESSION_ENDED},
        event,
    };
    if name == SESSION_ENDED {
        serde_json::from_value::<SessionEnded>(data.clone())
            .ok()
            .map(|ended| HubEvent::SessionEnded {
                session_id: ended.session_id,
            })
    } else if matches!(
        name,
        event::ORGANIZATION_CREATED | event::ORGANIZATION_CHANGED | event::OFFICE_STATUS_CHANGED
    ) {
        Some(HubEvent::InstanceStatus {
            event: name.to_owned(),
            data: data.clone(),
        })
    } else {
        None
    }
}
