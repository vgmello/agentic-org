//! The start gate separates descriptor ownership ACK from serving and closure.
use super::{invalid, Registered};
use crate::startup;
use org_office_launch::RendezvousEvent;
use std::{io, net::Shutdown, sync::mpsc::Receiver};

pub(super) fn write(out: &startup::EventWriter, event: &RendezvousEvent) -> io::Result<()> {
    startup::write_event(out, event).map_err(|_| {
        // Even a poisoned writer makes lifecycle uncertain: end the rendezvous.
        let stream = out.lock().unwrap_or_else(|poison| poison.into_inner());
        let _ = stream.shutdown(Shutdown::Write);
        let _ = stream.shutdown(Shutdown::Read);
        invalid()
    })
}

pub(super) fn run(
    ready: Receiver<()>,
    registered: Registered,
    out: startup::EventWriter,
    id: String,
    serve: impl FnOnce(),
) {
    if ready.recv().is_err() {
        return; // Failed ACK: captures drop, with no normal close notification.
    }
    let _closing = Closing {
        registered: Some(registered),
        out,
        id,
    };
    // On return or unwinding, the serving closure's descriptors drop before Closing.
    serve();
}

struct Closing {
    registered: Option<Registered>,
    out: startup::EventWriter,
    id: String,
}
impl Drop for Closing {
    fn drop(&mut self) {
        drop(self.registered.take());
        // Registry/shutdown FD is gone; no registry lock spans the bounded event write.
        let _ = write(
            &self.out,
            &RendezvousEvent::ChannelClosed {
                of: self.id.clone(),
            },
        );
    }
}
