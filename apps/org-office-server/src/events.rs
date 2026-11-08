//! Fan-out of server events to subscribed connections.

use std::sync::{Arc, Mutex, Weak};

use org_proto::EventKind;

use crate::connection::Connection;

#[derive(Default)]
pub struct EventBus {
    connections: Mutex<Vec<Weak<Connection>>>,
}

impl EventBus {
    /// Registers `connection` and marks it subscribed. Events published from now on reach it.
    pub fn subscribe(&self, connection: &Arc<Connection>) {
        connection.mark_subscribed();
        let mut connections = self.lock();
        if !connections
            .iter()
            .any(|known| known.as_ptr() == Arc::as_ptr(connection))
        {
            connections.push(Arc::downgrade(connection));
        }
    }

    /// Sends event `E` to every live subscriber. Never blocks on a slow client.
    pub fn publish<E: EventKind>(&self, data: &E::Data) {
        let Ok(data) = serde_json::to_value(data) else {
            return;
        };
        let mut connections = self.lock();
        connections.retain(|weak| match weak.upgrade() {
            Some(connection) => {
                connection.send_event(E::NAME, &data);
                true
            }
            None => false,
        });
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Weak<Connection>>> {
        self.connections
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
