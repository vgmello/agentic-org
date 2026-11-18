//! Own live channel identity, shutdown handles and serving threads before ACK.
use super::{invalid, Channel};
use crate::{revocations::Revocations, serve, startup};
use org_office_launch::{
    ChannelKind, RefusalReason, RendezvousEvent, MAX_CHANNELS, MAX_WEB_CHANNELS,
};

mod lifecycle;
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
    office: Arc<org_office::Office>,
}
impl Channels {
    pub fn new(
        shared: Arc<serve::Shared>,
        office: Arc<org_office::Office>,
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
            office,
        }
    }
    pub fn open(&mut self, channel: Channel) -> io::Result<()> {
        let Channel {
            recovery,
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
        self.reap();
        let registered = match register(&self.registry, &channel_id, kind, &stream) {
            Ok(registered) => registered,
            Err(AdmissionError::Invalid(error)) => return Err(error),
            Err(AdmissionError::Capacity) => {
                // A negative ownership receipt is valid only once the rejected FD is gone.
                drop(stream);
                return lifecycle::write(
                    &self.out,
                    &RendezvousEvent::FdsRefused {
                        of: channel_id,
                        reason: RefusalReason::Capacity,
                    },
                );
            }
        };
        let (shared, stopping) = (Arc::clone(&self.shared), Arc::clone(&self.stop));
        if recovery {
            let instance = self.hello.instance_id.clone().ok_or_else(invalid)?;
            let office = Arc::clone(&self.office);
            return self.start(channel_id, "org-recovery-public", registered, move || {
                if !stopping.load(Ordering::SeqCst) {
                    let _ = super::recovery::serve(stream, &office, &instance, &office_id);
                }
            });
        }
        let context = serve::Context::HostChannel(Box::new(serve::HostChannel {
            _channel_id: channel_id.clone(),
            _kind: kind,
            hello: self.hello.clone(),
            _revocations: Arc::clone(&self.revocations),
        }));
        self.start(channel_id, "org-host-channel", registered, move || {
            serve::serve_context(stream, &shared, 0, &stopping, context);
        })
    }
    fn start(
        &mut self,
        id: String,
        name: &str,
        registered: Registered,
        serve: impl FnOnce() + Send + 'static,
    ) -> io::Result<()> {
        let (start, ready) = std::sync::mpsc::channel();
        let out = Arc::clone(&self.out);
        let closing_id = id.clone();
        let thread = Builder::new().name(name.into()).spawn(move || {
            lifecycle::run(ready, registered, out, closing_id, serve);
        })?;
        if let Err(error) = lifecycle::write(&self.out, &RendezvousEvent::FdsReceived { of: id }) {
            drop(start);
            let _ = thread.join();
            return Err(error);
        }
        if start.send(()).is_err() {
            let _ = thread.join();
            return Err(invalid());
        }
        self.threads.push(thread);
        Ok(())
    }
    fn reap(&mut self) {
        let mut at = 0;
        while at < self.threads.len() {
            if self.threads[at].is_finished() {
                let _ = self.threads.swap_remove(at).join();
            } else {
                at += 1;
            }
        }
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
) -> Result<Registered, AdmissionError> {
    let mut entries = registry
        .lock()
        .map_err(|_| AdmissionError::Invalid(invalid()))?;
    if id.is_empty() || id == "handoff" || entries.contains_key(id) {
        return Err(AdmissionError::Invalid(invalid()));
    }
    if entries.len() >= MAX_CHANNELS
        || (kind == ChannelKind::Web
            && entries
                .values()
                .filter(|(kind, _)| *kind == ChannelKind::Web)
                .count()
                >= MAX_WEB_CHANNELS)
    {
        return Err(AdmissionError::Capacity);
    }
    let shutdown = stream.try_clone().map_err(AdmissionError::Invalid)?;
    entries.insert(id.to_owned(), (kind, shutdown));
    Ok(Registered {
        registry: Arc::clone(registry),
        id: id.to_owned(),
    })
}
#[derive(Debug)]
enum AdmissionError {
    Capacity,
    Invalid(io::Error),
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
