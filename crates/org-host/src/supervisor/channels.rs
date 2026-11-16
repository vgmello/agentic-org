//! Host-owned launch-v3 connections. Keys go only to the authenticated reader;
//! channel descriptors are retained until that worker acknowledges ownership.
use super::child::Signal;
use crate::{cloexec::SPAWN_LOCK, peer::AuditToken, server_check::ServerCheck, sync::lock};
use org_office_launch::v3;
use org_proto::{frame, DbKey, GrantKey};
use std::{
    collections::BTreeMap,
    fs::DirBuilder,
    io::{self, BufReader, Write},
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
const MAX_CHANNELS: usize = 64;

#[derive(Debug)]
pub(super) struct Broker {
    pub instance: String,
    runtime: PathBuf,
    endpoints: Mutex<BTreeMap<String, Weak<Endpoint>>>,
    pub(super) writers: Option<Arc<crate::writer::Writers>>,
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
    pending: Mutex<BTreeMap<String, (OwnedFd, Sender<()>)>>,
    channels: Mutex<usize>,
    signals: Sender<Signal>,
    generation: u64,
}
pub(crate) struct ChannelSlot(Weak<Endpoint>);
impl Drop for ChannelSlot {
    fn drop(&mut self) {
        if let Some(endpoint) = self.0.upgrade() {
            let mut count = lock(&endpoint.channels);
            *count = count.saturating_sub(1);
        }
    }
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
        crate::host_log::log(&format!("office v3 {name} failed ({:?})", error.kind()));
    })
}

impl Broker {
    pub fn new(instance: String, runtime: PathBuf) -> Self {
        Self {
            instance,
            runtime,
            endpoints: Mutex::default(),
            writers: None,
        }
    }
    pub fn writer_required(&self, office: &str) -> io::Result<bool> {
        self.writers
            .as_ref()
            .map_or(Ok(false), |writers| writers.required(office))
    }
    pub fn bind(&self) -> io::Result<Rendezvous> {
        let directory = self.runtime.join("r");
        match DirBuilder::new().mode(0o700).create(&directory) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
        crate::home::owner_only_dir(&directory).map_err(launch)?;
        let nonce = crate::secrets::generate_db_key()?;
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
        pid: u32,
        admitted: Option<AuditToken>,
        check: &ServerCheck,
        db_key: &DbKey,
        recovery: Option<&org_proto::RecoveryCode>,
        generation: u64,
        signals: Sender<Signal>,
    ) -> io::Result<Arc<Endpoint>> {
        let deadline = Instant::now() + DEADLINE;
        let stream = loop {
            match rendezvous.listener.accept() {
                Ok((stream, _)) => break stream,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
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
        self.handoff(
            stream, office, pid, admitted, check, db_key, recovery, generation, signals, deadline,
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn handoff(
        &self,
        stream: UnixStream,
        office: &str,
        pid: u32,
        admitted: Option<AuditToken>,
        check: &ServerCheck,
        db_key: &DbKey,
        recovery: Option<&org_proto::RecoveryCode>,
        generation: u64,
        signals: Sender<Signal>,
        deadline: Instant,
    ) -> io::Result<Arc<Endpoint>> {
        // Authenticate the socket's actual reader, not merely a process that was
        // admitted before it ran. Release macOS requires the exact audit token.
        phase(
            "initial reader identity",
            reader_is_child(&stream, pid, admitted),
        )?;
        let mut reader = BufReader::new(stream);
        let _: v3::ReadyForSecrets = phase(
            "ready for secrets",
            frame::read_message(&mut reader).map_err(launch),
        )?
        .ok_or_else(invalid)?;
        if !reader.buffer().is_empty() {
            return Err(invalid());
        }
        phase(
            "handoff reader identity",
            reader_is_child(reader.get_ref(), pid, admitted),
        )?;
        phase(
            "handoff image identity",
            check.still(pid, admitted).map_err(launch),
        )?;
        let (transfer, worker) = {
            let _fence = lock(&SPAWN_LOCK);
            UnixDatagram::pair()?
        };
        org_socket::rights::set_buffer_sizes(transfer.as_fd(), v3::TRANSFER_SOCKET_BUFFER_BYTES)?;
        org_socket::rights::set_buffer_sizes(worker.as_fd(), v3::TRANSFER_SOCKET_BUFFER_BYTES)?;
        let random = crate::secrets::generate_db_key()?;
        let key = GrantKey::from_bytes(*random.expose_bytes());
        let mut header = match recovery {
            Some(code) => v3::encode_create_handoff(db_key, &key, code),
            None => v3::encode_start_handoff(db_key, &key),
        }
        .map_err(launch)?;
        let sent = reader.get_mut().write_all(&header);
        crate::secrets::zero(&mut header);
        phase("secret header send", sent)?;
        phase(
            "handoff descriptor send",
            org_socket::rights::send(
                reader.get_ref().as_fd(),
                &[v3::FD_MARKER],
                &[worker.as_fd()],
                deadline,
            )
            .map_err(launch),
        )?;
        reader.get_ref().set_read_timeout(Some(
            deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(invalid)?,
        ))?;
        match phase(
            "handoff receipt decode",
            v3::read_server_event(&mut reader).map_err(launch),
        )? {
            Some(v3::RendezvousEvent::FdsReceived { of }) if of == "handoff" => {}
            _ => return Err(invalid()),
        }
        drop(worker); // Receiver ownership proved before sender drops its copy.
        reader.get_ref().set_read_timeout(None)?;
        let endpoint = Arc::new(Endpoint {
            office: office.into(),
            key,
            transfer,
            close: reader.get_ref().try_clone()?,
            live: Arc::new(AtomicBool::new(true)),
            pending: Mutex::default(),
            channels: Mutex::new(0),
            signals: signals.clone(),
            generation,
        });
        lock(&self.endpoints).insert(office.into(), Arc::downgrade(&endpoint));
        let receiving = Arc::clone(&endpoint);
        std::thread::Builder::new()
            .name(format!("office-{pid}-v3"))
            .spawn(move || {
                while let Ok(Some(event)) = v3::read_server_event(&mut reader) {
                    match event {
                        v3::RendezvousEvent::Server(event) => {
                            if signals.send(Signal::Event { generation, event }).is_err() {
                                break;
                            }
                        }
                        v3::RendezvousEvent::FdsReceived { of } => {
                            let Some((_descriptor, receipt)) = lock(&receiving.pending).remove(&of)
                            else {
                                break;
                            };
                            let _ = receipt.send(());
                        }
                        _ => break,
                    }
                }
                receiving.fail();
            })?;
        if let Some(writers) = &self.writers {
            if writers.required(office)? {
                let (host, worker) = {
                    let _fence = lock(&SPAWN_LOCK);
                    UnixStream::pair()?
                };
                phase(
                    "writer authority channel",
                    writers.serve(office, host, Arc::clone(&endpoint.live)),
                )?;
                phase("writer descriptor receipt", endpoint.writer_control(worker))?;
            }
        }
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
        let mut count = lock(&self.channels);
        if request.office_id != self.office || !self.is_live() || *count >= MAX_CHANNELS {
            return Err(invalid());
        }
        let id = crate::ids::generate()?;
        let (mut client, worker) = {
            let _fence = lock(&SPAWN_LOCK);
            UnixStream::pair()?
        };
        let (sent, receipt) = mpsc::channel();
        {
            let mut pending = lock(&self.pending);
            if pending.len() >= MAX_PENDING {
                return Err(invalid());
            }
            pending.insert(id.clone(), (worker.into(), sent));
        }
        let header = v3::encode_transfer_record(&v3::TransferRecord::RecoveryPublicRead {
            version: 1,
            office_id: self.office.clone(),
            channel_id: id.clone(),
        })
        .map_err(launch)?;
        let send = {
            let pending = lock(&self.pending);
            org_socket::rights::send(
                self.transfer.as_fd(),
                &header,
                &[pending.get(&id).ok_or_else(invalid)?.0.as_fd()],
                Instant::now() + DEADLINE,
            )
        };
        if send.is_err() || receipt.recv_timeout(DEADLINE).is_err() {
            self.fail();
            return Err(invalid());
        }
        *count += 1;
        let _slot = ChannelSlot(Arc::downgrade(self));
        drop(count);
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
            channels: Mutex::new(1),
            signals,
            generation: 1,
        });
        let slot = ChannelSlot(Arc::downgrade(&endpoint));
        (endpoint, slot, worker, peer)
    }
    fn writer_control(&self, worker: UnixStream) -> io::Result<()> {
        let id = crate::ids::generate()?;
        let (sent, receipt) = mpsc::channel();
        lock(&self.pending).insert(id.clone(), (worker.into(), sent));
        let header = v3::encode_transfer_record(&v3::TransferRecord::WriterControl {
            office_id: self.office.clone(),
            channel_id: id.clone(),
        })
        .map_err(launch)?;
        let send = {
            let pending = lock(&self.pending);
            org_socket::rights::send(
                self.transfer.as_fd(),
                &header,
                &[pending.get(&id).ok_or_else(invalid)?.0.as_fd()],
                Instant::now() + DEADLINE,
            )
        };
        if send.is_err() || receipt.recv_timeout(DEADLINE).is_err() {
            self.fail();
            return Err(invalid());
        }
        Ok(())
    }
    pub fn is_live(&self) -> bool {
        self.live.load(Ordering::SeqCst)
    }
    pub fn fail(&self) {
        if self.live.swap(false, Ordering::SeqCst) {
            lock(&self.pending).clear();
            let _ = self.close.shutdown(std::net::Shutdown::Both);
            let _ = self.signals.send(Signal::Closed {
                generation: self.generation,
            });
        }
    }
    pub fn open(
        self: &Arc<Self>,
        kind: v3::ChannelKind,
    ) -> io::Result<(String, UnixStream, ChannelSlot)> {
        let mut count = lock(&self.channels);
        // One local slot is reserved even when all gateway slots are in use.
        let limit = if kind == v3::ChannelKind::Local {
            MAX_CHANNELS
        } else {
            MAX_CHANNELS - 1
        };
        if !self.live.load(Ordering::SeqCst) || *count >= limit {
            return Err(invalid());
        }
        let id = crate::ids::generate()?;
        let (client, worker) = {
            let _fence = lock(&SPAWN_LOCK);
            UnixStream::pair()?
        };
        let (sent, receipt) = mpsc::channel();
        {
            let mut pending = lock(&self.pending);
            if pending.len() >= MAX_PENDING {
                return Err(invalid());
            }
            pending.insert(id.clone(), (worker.into(), sent));
        }
        let header = v3::encode_transfer_record(&v3::TransferRecord::Channel {
            kind,
            office_id: self.office.clone(),
            channel_id: id.clone(),
        })
        .map_err(launch)?;
        let send = {
            let pending = lock(&self.pending);
            org_socket::rights::send(
                self.transfer.as_fd(),
                &header,
                &[pending.get(&id).ok_or_else(invalid)?.0.as_fd()],
                Instant::now() + DEADLINE,
            )
        };
        if send.is_err() || receipt.recv_timeout(DEADLINE).is_err() {
            self.fail();
            return Err(invalid());
        }
        *count += 1;
        Ok((id, client, ChannelSlot(Arc::downgrade(self))))
    }
    pub fn revoke(&self, session: &str) -> io::Result<()> {
        let header = v3::encode_transfer_record(&v3::TransferRecord::Revoked {
            session_ids: vec![session.into()],
        })
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

pub(crate) fn reader_is_child(
    stream: &UnixStream,
    pid: u32,
    admitted: Option<AuditToken>,
) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let token = crate::peer::audit_token(stream)?;
        let peer_pid = u32::from_ne_bytes(token[20..24].try_into().map_err(launch)?);
        if admitted.is_some_and(|expected| expected != token) || peer_pid != pid {
            return Err(invalid());
        }
    }
    #[cfg(target_os = "linux")]
    {
        if admitted.is_some() || !org_socket::peer_process_allowed(stream, crate::home::euid(), pid)
        {
            return Err(invalid());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
