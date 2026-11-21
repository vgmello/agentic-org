//! Host-owned launch connections. Keys go only to the authenticated reader;
//! channel descriptors are retained until that worker acknowledges ownership.
use super::child::Signal;
use crate::{cloexec::SPAWN_LOCK, sync::lock};
use org_platform_host::admission::{Admission, Child, NotTheChild};
use org_proto::{frame, GrantKey, OfficeSecretsKey};
use std::{
    collections::BTreeMap,
    fs::DirBuilder,
    io::{self, BufRead, BufReader, Read, Write},
    os::{
        fd::{AsFd, OwnedFd},
        unix::{
            fs::DirBuilderExt,
            net::{UnixDatagram, UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Sender},
        Arc, Mutex, Weak,
    },
    time::{Duration, Instant},
};

const DEADLINE: Duration = Duration::from_secs(5);
const MAX_PENDING: usize = 16;
mod registry;
pub(crate) use registry::ChannelSlot;
use registry::Registry;

enum Receipt {
    Accepted,
    Capacity,
}
struct Pending {
    descriptor: Arc<OwnedFd>,
    receipt: Sender<Receipt>,
    ordinary: bool,
}

#[derive(Debug)]
pub(super) struct Broker {
    pub instance: String,
    runtime: PathBuf,
    endpoints: Mutex<BTreeMap<String, Weak<Endpoint>>>,
    pub(super) writers: Option<Arc<crate::writer::Writers>>,
    /// Set when the host stops ([`Broker::stop`]): no launch handshake waits any more.
    stopping: AtomicBool,
    /// The launch handshakes under way, by a number of their own: each one's connection to
    /// its worker, which [`Broker::stop`] shuts down.
    handshakes: Mutex<(u64, BTreeMap<u64, UnixStream>)>,
}

/// A launch handshake under way, registered with its broker until it is dropped.
struct Handshake<'a> {
    broker: &'a Broker,
    key: u64,
}
impl Drop for Handshake<'_> {
    fn drop(&mut self) {
        lock(&self.broker.handshakes).1.remove(&self.key);
    }
}
fn host_stopping() -> io::Error {
    io::Error::other("the host is stopping")
}
/// What a create or start handoff carries, borrowed from its holder (never copied).
#[derive(Clone, Copy)]
pub(super) enum HandoffSecrets<'a> {
    /// A start: the office's secrets key, or `None` when the host has none for it. The handoff
    /// then carries `null` and the start proceeds; only what the key unwraps is unavailable.
    Start(Option<&'a OfficeSecretsKey>),
    /// An HQ create: the new office's secrets key (always present) and its recovery code.
    Create {
        office_secrets_key: &'a OfficeSecretsKey,
        recovery: &'a org_proto::RecoveryCode,
    },
}
pub(super) struct Rendezvous {
    listener: UnixListener,
    path: PathBuf,
}
impl Drop for Rendezvous {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
pub(crate) struct Endpoint {
    pub office: String,
    pub key: GrantKey,
    transfer: UnixDatagram,
    close: UnixStream,
    live: Arc<AtomicBool>,
    pending: Mutex<BTreeMap<String, Pending>>,
    channels: Mutex<Registry>,
    signals: Sender<Signal>,
    generation: u64,
}
impl std::fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Endpoint")
            .field("office", &self.office)
            .finish_non_exhaustive()
    }
}
fn invalid() -> io::Error {
    io::Error::other("worker channel unavailable")
}
fn launch<T>(_: T) -> io::Error {
    invalid()
}

/// Static phase and OS category only: paths, wire bytes and secret values must
/// never enter host diagnostics, even when a child sends an invalid message.
fn phase<T>(name: &'static str, result: io::Result<T>) -> io::Result<T> {
    result.inspect_err(|error| {
        crate::host_log::log(&format!("office launch {name} failed ({:?})", error.kind()));
    })
}

impl Broker {
    pub fn new(instance: String, runtime: PathBuf) -> Self {
        Self {
            instance,
            runtime,
            endpoints: Mutex::default(),
            writers: None,
            stopping: AtomicBool::new(false),
            handshakes: Mutex::default(),
        }
    }
    /// The host is stopping: every launch handshake ends now, as one that failed (its
    /// worker is killed by its owner, which is told the launch failed), and none waits from
    /// here on. A worker still in its handshake would otherwise hold its owner for up to
    /// [`DEADLINE`], past the host's stop.
    pub fn stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        for stream in lock(&self.handshakes).1.values() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }
    /// Every office needs writer custody to start or be created: returns what its writer is
    /// bound to. A host without writer custody (no protected registry) starts no office.
    pub fn custody(&self, office: &str) -> Result<crate::writer::Bound, crate::writer::AdmitError> {
        self.writers
            .as_ref()
            .ok_or(crate::writer::AdmitError::Refused)?
            .admit(office)
    }
    pub fn update_owner(
        &self,
        office: &str,
        org_id: &str,
        owner_account: &str,
        owner_member: &str,
    ) -> io::Result<()> {
        self.writers
            .as_ref()
            .ok_or_else(|| io::Error::other("writer custody unavailable"))?
            .update_owner(office, org_id, owner_account, owner_member)
    }
    pub fn bind(&self) -> io::Result<Rendezvous> {
        let directory = self.runtime.join("r");
        match DirBuilder::new().mode(0o700).create(&directory) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
        crate::home::owner_only_dir(&directory).map_err(launch)?;
        let nonce = crate::secrets::generate_office_secrets_key()?;
        let name: String = nonce.expose_bytes()[..8]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let path = directory.join(name);
        // Fresh unguessable path: never remove a pre-existing socket to bind it.
        let listener = {
            let _fence = lock(&SPAWN_LOCK);
            UnixListener::bind(&path)?
        };
        let bound = Rendezvous { listener, path };
        std::fs::set_permissions(
            &bound.path,
            std::os::unix::fs::PermissionsExt::from_mode(0o600),
        )?;
        bound.listener.set_nonblocking(true)?;
        Ok(bound)
    }
    pub fn endpoint(&self, office: &str) -> Option<Arc<Endpoint>> {
        lock(&self.endpoints)
            .get(office)
            .and_then(Weak::upgrade)
            .filter(|e| e.live.load(Ordering::SeqCst))
    }
    #[allow(clippy::too_many_arguments)]
    pub fn accept(
        &self,
        rendezvous: Rendezvous,
        office: &str,
        admission: &Admission,
        child: &mut Child,
        secrets: HandoffSecrets<'_>,
        generation: u64,
        signals: Sender<Signal>,
    ) -> io::Result<Arc<Endpoint>> {
        // Every start and create needs writer custody. No custody, no office.
        let writers = self.writers.as_ref().ok_or_else(invalid)?;
        let (stream, deadline, _handshake) =
            self.connect(rendezvous, Instant::now() + DEADLINE, &mut || {
                child.try_wait().unwrap_or(false)
            })?;
        let endpoint = self.handoff(
            stream, office, admission, child, secrets, generation, signals, deadline,
        )?;
        // The writer control goes first after the handoff, before the worker opens or
        // creates anything.
        let (host, worker) = {
            let _fence = lock(&SPAWN_LOCK);
            UnixStream::pair()?
        };
        phase(
            "writer authority channel",
            writers.serve(office, host, Arc::clone(&endpoint.live)),
        )?;
        phase("writer descriptor receipt", endpoint.writer_control(worker))?;
        Ok(endpoint)
    }
    /// The one connection to a bound rendezvous, by `deadline`, with blocking I/O bounded by
    /// what is left of it. The wait ends early when `exited` says the worker is gone: a worker
    /// that refuses its first line (or crashes) before connecting never will, and its refusal
    /// went to stdout, which the host does not read. That is a crash, not a fatal code. It
    /// also ends when the host stops; the handshake that follows is registered until the
    /// returned [`Handshake`] is dropped, so a stop ends it too.
    fn connect(
        &self,
        rendezvous: Rendezvous,
        deadline: Instant,
        exited: &mut dyn FnMut() -> bool,
    ) -> io::Result<(UnixStream, Instant, Handshake<'_>)> {
        let mut gone = false;
        let stream = loop {
            if self.stopping.load(Ordering::SeqCst) {
                return phase("rendezvous accept", Err(host_stopping()));
            }
            match rendezvous.listener.accept() {
                Ok((stream, _)) => break stream,
                // One more accept after the exit was seen: a worker may connect and then exit.
                Err(e) if e.kind() == io::ErrorKind::WouldBlock && gone => {
                    return phase(
                        "rendezvous accept",
                        Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "the worker exited without connecting",
                        )),
                    );
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock && Instant::now() >= deadline => {
                    return phase(
                        "rendezvous accept",
                        Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "no worker connected in time",
                        )),
                    );
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    gone = exited();
                    if !gone {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
                Err(e) => return phase("rendezvous accept", Err(e)),
            }
        };
        let left = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(invalid)?;
        drop(rendezvous);
        // macOS accepts inherit O_NONBLOCK from this polling listener. A
        // timeout alone does not clear it: a delayed READY would fail at once.
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(left))?;
        stream.set_write_timeout(Some(left))?;
        let handshake = {
            let mut handshakes = lock(&self.handshakes);
            let key = handshakes.0;
            handshakes.0 += 1;
            handshakes.1.insert(key, stream.try_clone()?);
            Handshake { broker: self, key }
        };
        // Registered first: a stop that came meanwhile either finds it or is seen here.
        if self.stopping.load(Ordering::SeqCst) {
            return phase("rendezvous accept", Err(host_stopping()));
        }
        Ok((stream, deadline, handshake))
    }
    /// Authenticate the socket's reader, read its `ready_for_secrets` and check its image again:
    /// the steps before any secret goes out, shared by every op.
    fn ready(
        stream: UnixStream,
        admission: &Admission,
        child: &Child,
        deadline: Instant,
    ) -> io::Result<BufReader<UnixStream>> {
        // Authenticate the socket's actual reader, not merely a process that was
        // admitted before it ran. Release macOS requires the exact audit token.
        phase(
            "initial reader identity",
            reader_is_child(admission, child, &stream),
        )?;
        let mut reader = BufReader::new(stream);
        let _: org_office_launch::ReadyForSecrets = phase(
            "ready for secrets",
            frame::read_message(&mut ByDeadline::new(&mut reader, deadline)).map_err(launch),
        )?
        .ok_or_else(invalid)?;
        if !reader.buffer().is_empty() {
            return Err(invalid());
        }
        phase(
            "handoff reader identity",
            reader_is_child(admission, child, reader.get_ref()),
        )?;
        phase(
            "handoff image identity",
            admission.recheck(child).map_err(launch),
        )?;
        Ok(reader)
    }
    /// A `--discard` run over the rendezvous: the handoff carries no secret (a plain office
    /// opens nothing that needs a key) and names no descriptors, so no transfer pair is made,
    /// no marker is sent and no `fds_received` is awaited. The worker's `discarded`/`failed` arrive on the stream and go to `signals` as
    /// [`Signal::Event`]; the stream's end (or any other event) is [`Signal::Closed`]. No
    /// endpoint is registered: a discard serves no channels.
    pub fn discard(
        &self,
        rendezvous: Rendezvous,
        admission: &Admission,
        child: &mut Child,
        generation: u64,
        signals: Sender<Signal>,
    ) -> io::Result<()> {
        let (stream, deadline, _handshake) =
            self.connect(rendezvous, Instant::now() + DEADLINE, &mut || {
                child.try_wait().unwrap_or(false)
            })?;
        Self::discard_handoff(stream, admission, child, generation, signals, deadline)
    }
    fn discard_handoff(
        stream: UnixStream,
        admission: &Admission,
        child: &Child,
        generation: u64,
        signals: Sender<Signal>,
        deadline: Instant,
    ) -> io::Result<()> {
        let mut reader = Self::ready(stream, admission, child, deadline)?;
        // Still sent only to the authenticated child reader, after `ready_for_secrets`, so a
        // discard keeps the start/create handshake order; the header itself holds no secret.
        let header = org_office_launch::encode_discard_handoff().map_err(launch)?;
        phase("handoff header send", reader.get_mut().write_all(&header))?;
        // The report may take as long as the supervisor's discard timeout allows; it kills the
        // worker at that deadline, which ends this stream.
        set_timeout_unless_peer_closed(reader.get_ref(), None)?;
        std::thread::Builder::new()
            .name(format!("office-{}-launch-discard", child.id()))
            .spawn(move || {
                while let Ok(Some(org_office_launch::RendezvousEvent::Server(event))) =
                    org_office_launch::read_server_event(&mut reader)
                {
                    if signals.send(Signal::Event { generation, event }).is_err() {
                        return;
                    }
                }
                let _ = signals.send(Signal::Closed { generation });
            })?;
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn handoff(
        &self,
        stream: UnixStream,
        office: &str,
        admission: &Admission,
        child: &Child,
        secrets: HandoffSecrets<'_>,
        generation: u64,
        signals: Sender<Signal>,
        deadline: Instant,
    ) -> io::Result<Arc<Endpoint>> {
        let mut reader = Self::ready(stream, admission, child, deadline)?;
        let (transfer, worker) = {
            let _fence = lock(&SPAWN_LOCK);
            UnixDatagram::pair()?
        };
        org_socket::rights::set_buffer_sizes(
            transfer.as_fd(),
            org_office_launch::TRANSFER_SOCKET_BUFFER_BYTES,
        )?;
        org_socket::rights::set_buffer_sizes(
            worker.as_fd(),
            org_office_launch::TRANSFER_SOCKET_BUFFER_BYTES,
        )?;
        let random = crate::secrets::generate_office_secrets_key()?;
        let key = GrantKey::from_bytes(*random.expose_bytes());
        let mut header = match secrets {
            HandoffSecrets::Create {
                office_secrets_key,
                recovery,
            } => org_office_launch::encode_create_handoff(office_secrets_key, &key, recovery),
            HandoffSecrets::Start(office_secrets_key) => {
                org_office_launch::encode_start_handoff(office_secrets_key, &key)
            }
        }
        .map_err(launch)?;
        let sent = reader.get_mut().write_all(&header);
        crate::secrets::zero(&mut header);
        phase("secret header send", sent)?;
        phase(
            "handoff descriptor send",
            org_socket::rights::send(
                reader.get_ref().as_fd(),
                &[org_office_launch::FD_MARKER],
                &[worker.as_fd()],
                deadline,
            )
            .map_err(launch),
        )?;
        match phase(
            "handoff receipt decode",
            org_office_launch::read_server_event(&mut ByDeadline::new(&mut reader, deadline))
                .map_err(launch),
        )? {
            Some(org_office_launch::RendezvousEvent::FdsReceived { of }) if of == "handoff" => {}
            _ => return Err(invalid()),
        }
        drop(worker); // Receiver ownership proved before sender drops its copy.
        set_timeout_unless_peer_closed(reader.get_ref(), None)?;
        let endpoint = Arc::new(Endpoint {
            office: office.into(),
            key,
            transfer,
            close: reader.get_ref().try_clone()?,
            live: Arc::new(AtomicBool::new(true)),
            pending: Mutex::default(),
            channels: Mutex::default(),
            signals: signals.clone(),
            generation,
        });
        lock(&self.endpoints).insert(office.into(), Arc::downgrade(&endpoint));
        let receiving = Arc::clone(&endpoint);
        std::thread::Builder::new()
            .name(format!("office-{}-launch", child.id()))
            .spawn(move || {
                while let Ok(Some(event)) = org_office_launch::read_server_event(&mut reader) {
                    match event {
                        org_office_launch::RendezvousEvent::Server(event) => {
                            if signals.send(Signal::Event { generation, event }).is_err() {
                                break;
                            }
                        }
                        event => {
                            if receiving.receipt(event).is_err() {
                                break;
                            }
                        }
                    }
                }
                receiving.fail();
            })?;
        Ok(endpoint)
    }
}
impl Rendezvous {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Endpoint {
    /// A separate one-shot channel to the already authenticated live worker.
    /// It shares descriptor/channel budgets, never the writer-control stream.
    pub(crate) fn recovery_public(
        self: &Arc<Self>,
        request: &org_office_launch::recovery::Request,
    ) -> io::Result<org_office_launch::recovery::PublicRecord> {
        use org_office_launch::recovery as wire;
        use sha2::{Digest, Sha256};
        request.check()?;
        if request.office_id != self.office {
            return Err(invalid());
        }
        let (_, mut client, _slot) =
            self.ordinary_channel(org_office_launch::ChannelKind::Local, |id| {
                org_office_launch::TransferRecord::RecoveryPublicRead {
                    version: 1,
                    office_id: self.office.clone(),
                    channel_id: id.into(),
                }
            })?;
        client.set_nonblocking(false)?;
        client.set_read_timeout(Some(DEADLINE))?;
        client.set_write_timeout(Some(DEADLINE))?;
        wire::write_request(&mut client, request)?;
        client.shutdown(std::net::Shutdown::Write)?;
        let record = wire::read_record(&mut BufReader::new(client))?;
        let bytes = record.package()?;
        if !self.is_live()
            || record.binding() != *request
            || record.package_sha256 != wire::hex(&Sha256::digest(bytes))
        {
            return Err(invalid());
        }
        Ok(record)
    }
    #[cfg(test)]
    pub(crate) fn for_grant_test() -> (Arc<Self>, ChannelSlot, UnixDatagram, UnixStream) {
        let (transfer, worker) = UnixDatagram::pair().unwrap();
        let (close, peer) = UnixStream::pair().unwrap();
        let (signals, _events) = mpsc::channel();
        let endpoint = Arc::new(Self {
            office: "office".into(),
            key: GrantKey::from_bytes([71; 32]),
            transfer,
            close,
            live: Arc::new(AtomicBool::new(true)),
            pending: Mutex::default(),
            channels: Mutex::default(),
            signals,
            generation: 1,
        });
        let slot = {
            let mut channels = lock(&endpoint.channels);
            let slot = channels
                .reserve("channel", org_office_launch::ChannelKind::Local)
                .unwrap();
            assert!(channels.acknowledge("channel"));
            slot
        };
        (endpoint, slot, worker, peer)
    }
    fn writer_control(&self, worker: UnixStream) -> io::Result<()> {
        let id = crate::ids::generate()?;
        let header = org_office_launch::encode_transfer_record(
            &org_office_launch::TransferRecord::WriterControl {
                office_id: self.office.clone(),
                channel_id: id.clone(),
            },
        )
        .map_err(launch)?;
        self.transfer_descriptor(&id, worker, &header, false)
    }
    fn transfer_descriptor(
        &self,
        id: &str,
        worker: UnixStream,
        header: &[u8],
        ordinary: bool,
    ) -> io::Result<()> {
        let descriptor = Arc::new(OwnedFd::from(worker));
        let (sent, receipt) = mpsc::channel();
        {
            let mut pending = lock(&self.pending);
            if !self.is_live() || pending.len() >= MAX_PENDING || pending.contains_key(id) {
                return Err(invalid());
            }
            pending.insert(
                id.into(),
                Pending {
                    descriptor: Arc::clone(&descriptor),
                    receipt: sent,
                    ordinary,
                },
            );
        }
        // Arc retains this single descriptor during send without holding either registry lock.
        let send = org_socket::rights::send(
            self.transfer.as_fd(),
            header,
            &[descriptor.as_fd()],
            Instant::now() + DEADLINE,
        );
        drop(descriptor); // Only the pending ownership record now retains the sender copy.
        if send.is_err() {
            self.fail();
            return Err(invalid());
        }
        match receipt.recv_timeout(DEADLINE) {
            Ok(Receipt::Accepted) => Ok(()),
            Ok(Receipt::Capacity) => Err(invalid()),
            Err(_) => {
                self.fail();
                Err(invalid())
            }
        }
    }
    fn receipt(&self, event: org_office_launch::RendezvousEvent) -> io::Result<()> {
        let (id, refused) = match event {
            org_office_launch::RendezvousEvent::ChannelClosed { of } => {
                return lock(&self.channels)
                    .remove(&of, true)
                    .then_some(())
                    .ok_or_else(invalid);
            }
            org_office_launch::RendezvousEvent::FdsReceived { of } => (of, false),
            org_office_launch::RendezvousEvent::FdsRefused {
                of,
                reason: org_office_launch::RefusalReason::Capacity,
            } => (of, true),
            _ => return Err(invalid()),
        };
        let mut pending = lock(&self.pending);
        let record = pending.get(&id).ok_or_else(invalid)?;
        if refused && !record.ordinary {
            return Err(invalid());
        }
        if record.ordinary {
            let mut channels = lock(&self.channels);
            let valid = if refused {
                channels.remove(&id, false)
            } else {
                channels.acknowledge(&id)
            };
            if !valid {
                return Err(invalid());
            }
        }
        let Pending {
            descriptor,
            receipt,
            ..
        } = pending.remove(&id).ok_or_else(invalid)?;
        drop(descriptor);
        drop(pending);
        let _ = receipt.send(if refused {
            Receipt::Capacity
        } else {
            Receipt::Accepted
        });
        Ok(())
    }
    pub fn is_live(&self) -> bool {
        self.live.load(Ordering::SeqCst)
    }
    pub fn fail(&self) {
        let first = self.live.swap(false, Ordering::SeqCst);
        lock(&self.channels).fail();
        lock(&self.pending).clear();
        if first {
            let _ = self.close.shutdown(std::net::Shutdown::Both);
            let _ = self.signals.send(Signal::Closed {
                generation: self.generation,
            });
        }
    }
    pub fn open(
        self: &Arc<Self>,
        kind: org_office_launch::ChannelKind,
    ) -> io::Result<(String, UnixStream, ChannelSlot)> {
        self.ordinary_channel(kind, |id| org_office_launch::TransferRecord::Channel {
            kind,
            office_id: self.office.clone(),
            channel_id: id.into(),
        })
    }
    fn ordinary_channel(
        &self,
        kind: org_office_launch::ChannelKind,
        record: impl FnOnce(&str) -> org_office_launch::TransferRecord,
    ) -> io::Result<(String, UnixStream, ChannelSlot)> {
        let id = crate::ids::generate()?;
        let slot = lock(&self.channels).reserve(&id, kind)?;
        let result = (|| {
            let (client, worker) = {
                let _fence = lock(&SPAWN_LOCK);
                UnixStream::pair()?
            };
            let header = org_office_launch::encode_transfer_record(&record(&id)).map_err(launch)?;
            self.transfer_descriptor(&id, worker, &header, true)?;
            Ok((id.clone(), client, slot))
        })();
        if result.is_err() {
            // Only a not-yet-acknowledged reservation may be rolled back here.
            lock(&self.channels).remove(&id, false);
        }
        result
    }
    pub fn revoke(&self, session: &str) -> io::Result<()> {
        let header = org_office_launch::encode_transfer_record(
            &org_office_launch::TransferRecord::Revoked {
                session_ids: vec![session.into()],
            },
        )
        .map_err(launch)?;
        if !self.live.load(Ordering::SeqCst)
            || org_socket::rights::send(
                self.transfer.as_fd(),
                &header,
                &[],
                Instant::now() + Duration::from_millis(100),
            )
            .is_err()
        {
            self.fail();
            return Err(invalid());
        }
        Ok(())
    }
}
impl Drop for Endpoint {
    fn drop(&mut self) {
        self.fail();
    }
}

/// A launch read bounded by one deadline, however slowly the worker's bytes come: each read
/// from the socket waits only for what is left of `deadline` (the socket's single timeout
/// would restart with every byte a trickling worker sends). Bytes already buffered are served
/// without a wait, so nothing past a line is lost to the next reader of the same buffer.
pub(crate) struct ByDeadline<'a> {
    reader: &'a mut BufReader<UnixStream>,
    deadline: Instant,
}

impl<'a> ByDeadline<'a> {
    pub(crate) fn new(reader: &'a mut BufReader<UnixStream>, deadline: Instant) -> Self {
        Self { reader, deadline }
    }
}

impl Read for ByDeadline<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let available = self.fill_buf()?;
        let n = available.len().min(buf.len());
        buf[..n].copy_from_slice(&available[..n]);
        self.consume(n);
        Ok(n)
    }
}

impl BufRead for ByDeadline<'_> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.reader.buffer().is_empty() {
            let left = self.deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "the launch deadline passed",
                ));
            }
            set_timeout_unless_peer_closed(self.reader.get_ref(), Some(left))?;
        }
        self.reader.fill_buf()
    }

    fn consume(&mut self, amount: usize) {
        self.reader.consume(amount);
    }
}

/// Sets `stream`'s read timeout, except where the worker has already closed its end.
///
/// Darwin's `setsockopt` answers `EINVAL` on a socket whose both directions are shut down,
/// which is what a unix stream becomes once its peer has closed. A worker may report a fatal
/// `failed` right after `fds_received` (or after a discard header) and exit before the host
/// changes the timeout. Its buffered events stay readable and the read then ends at EOF without
/// blocking, so the timeout no longer matters: treating that as an error would lose the
/// worker's failure report and turn it into a crash. Any other error is still an error.
pub(crate) fn set_timeout_unless_peer_closed(
    stream: &UnixStream,
    timeout: Option<Duration>,
) -> io::Result<()> {
    match stream.set_read_timeout(timeout) {
        Err(error) if error.raw_os_error() == Some(libc::EINVAL) => Ok(()),
        other => other,
    }
}

/// Whether the process at the other end of `stream` is `child`, as the image `admission`
/// admitted (on macOS the same audit token and pid). A system without an admission adapter
/// has no child to ask about: no weaker check stands in for it (ADR-0017).
pub(crate) fn reader_is_child(
    admission: &Admission,
    child: &Child,
    stream: &UnixStream,
) -> io::Result<()> {
    admission.verify_reader(child, stream).map_err(|error| {
        if NotTheChild::is(&error) {
            invalid()
        } else {
            error
        }
    })
}

#[cfg(test)]
pub(crate) mod tests;
