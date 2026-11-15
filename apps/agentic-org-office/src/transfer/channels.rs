//! Own live channel identity, shutdown handles and serving threads before ACK.
use super::{invalid, Channel};
use crate::{revocations::Revocations, serve, startup};
use org_office_launch::v3::{ChannelKind, RendezvousEvent};
use std::collections::HashMap;
use std::io;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
#[cfg(not(test))]
use std::thread::Builder;
use std::thread::JoinHandle;
#[cfg(test)]
use tests::Builder;

type Registry = Arc<Mutex<HashMap<String, (ChannelKind, UnixStream)>>>;
pub(crate) struct Channels {
    registry: Registry,
    threads: Vec<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    shared: Arc<serve::Shared>,
    hello: org_proto::HelloResult,
    out: startup::EventWriter,
    revocations: Arc<Mutex<Revocations>>,
}
impl Channels {
    pub fn new(
        shared: Arc<serve::Shared>,
        instance: String,
        out: startup::EventWriter,
        revocations: Arc<Mutex<Revocations>>,
    ) -> Self {
        let mut hello = shared.hello.clone().with_instance_id(instance);
        hello.api_version = 2;
        Self {
            registry: Arc::default(),
            threads: Vec::new(),
            stop: Arc::default(),
            shared,
            hello,
            out,
            revocations,
        }
    }
    pub fn open(&mut self, channel: Channel) -> io::Result<()> {
        let Channel {
            kind,
            office_id,
            channel_id,
            fd,
        } = channel;
        if office_id != self.hello.office.as_ref().expect("office hello").office.id {
            return Err(invalid());
        }
        let stream = org_socket::rights::connected_unix_stream(fd)?;
        stream.set_nonblocking(false)?;
        let registered = register(&self.registry, &channel_id, kind, &stream)?;
        let (shared, stopping) = (Arc::clone(&self.shared), Arc::clone(&self.stop));
        let context = serve::Context::HostChannel(Box::new(serve::HostChannel {
            _channel_id: channel_id.clone(),
            _kind: kind,
            hello: self.hello.clone(),
            _revocations: Arc::clone(&self.revocations),
        }));
        let thread = Builder::new()
            .name("org-host-channel".into())
            .spawn(move || {
                // Serving drops its handles first; removing the registration releases
                // the ID before its last handle closes and the peer observes EOF.
                let _registered = registered;
                serve::serve_context(stream, &shared, 0, &stopping, context);
            })?;
        self.threads.retain(|thread| !thread.is_finished());
        self.threads.push(thread);
        startup::write_event(&self.out, &RendezvousEvent::FdsReceived { of: channel_id })
            .map_err(|_| invalid())
    }
}
impl Drop for Channels {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        for (_, stream) in self
            .registry
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
        {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}
fn register(
    registry: &Registry,
    id: &str,
    kind: ChannelKind,
    stream: &UnixStream,
) -> io::Result<Registered> {
    let mut entries = registry.lock().map_err(|_| invalid())?;
    if id.is_empty() || id == "handoff" || entries.contains_key(id) {
        return Err(invalid());
    }
    if entries.len() >= 8
        || (kind == ChannelKind::Gateway
            && entries
                .values()
                .filter(|(kind, _)| *kind == ChannelKind::Gateway)
                .count()
                >= 7)
    {
        return Err(invalid());
    }
    entries.insert(id.to_owned(), (kind, stream.try_clone()?));
    Ok(Registered {
        registry: Arc::clone(registry),
        id: id.to_owned(),
    })
}
struct Registered {
    registry: Registry,
    id: String,
}
impl Drop for Registered {
    fn drop(&mut self) {
        self.registry
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.id);
    }
}

#[cfg(test)]
mod tests;
