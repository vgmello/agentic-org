//! One bounded actor owns the already-admitted private host connection and
//! transfer receiver. No host.sock/path fallback, office key or registry access.
use crate::selected::OfficeAccess;
use org_client::selected::ApprovedOffice;
use org_host_control::{ops, Password, SessionHandle};
use org_proto::{selected_tasks::OfficeScope, Call, Principal};
use org_socket::rights::{DatagramReceiver, ReceiveError, StreamReceiver};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{self, Write},
    net::Shutdown,
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::net::UnixStream,
    },
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

const DEADLINE: Duration = Duration::from_secs(5);
const MAX_CONTROL_BYTES: usize = 64 * 1024;
static SPAWN_FENCE: Mutex<()> = Mutex::new(());
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    Denied,
    NotFound,
    Unavailable,
}
struct Reply {
    bytes: Zeroizing<Vec<u8>>,
    descriptor: Option<OwnedFd>,
}
struct Job {
    line: Zeroizing<Vec<u8>>,
    id: u64,
    office: Option<String>,
    until: Instant,
    answer: SyncSender<Result<Reply, Failure>>,
}
struct Cached {
    channel: String,
    office: Arc<ApprovedOffice>,
}
pub struct HostControl {
    instance: String,
    live: Arc<AtomicBool>,
    socket: UnixStream,
    jobs: SyncSender<Job>,
    next: std::sync::atomic::AtomicU64,
    channels: Mutex<BTreeMap<([u8; 32], String), Cached>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Response<T> {
    id: u64,
    result: Option<T>,
    error: Option<org_proto::ApiError>,
}
fn decode<T: DeserializeOwned>(bytes: &[u8], id: u64) -> Result<T, Failure> {
    if bytes.iter().find(|b| !b.is_ascii_whitespace()) != Some(&b'{') {
        return Err(Failure::Unavailable);
    }
    let reply: Response<T> = serde_json::from_slice(bytes).map_err(|_| Failure::Unavailable)?;
    if reply.id != id {
        return Err(Failure::Unavailable);
    }
    match (reply.result, reply.error) {
        (Some(result), None) => Ok(result),
        (None, Some(e)) if e.code == org_proto::ErrorCode::NotAuthorized => Err(Failure::Denied),
        (None, Some(e)) if e.code == org_proto::ErrorCode::NotFound => Err(Failure::NotFound),
        _ => Err(Failure::Unavailable),
    }
}

struct Counter(usize);
impl Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .filter(|n| *n < MAX_CONTROL_BYTES)
            .ok_or_else(|| io::Error::other("control request too large"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn encode<T: Serialize>(value: &T) -> Result<Zeroizing<Vec<u8>>, Failure> {
    let mut n = Counter(0);
    serde_json::to_writer(&mut n, value).map_err(|_| Failure::Unavailable)?;
    let mut bytes = Zeroizing::new(Vec::with_capacity(n.0 + 1));
    serde_json::to_writer(&mut *bytes, value).map_err(|_| Failure::Unavailable)?;
    bytes.push(b'\n');
    Ok(bytes)
}
fn readable(fd: std::os::fd::BorrowedFd<'_>, until: Instant) -> Result<(), Failure> {
    let left = until
        .checked_duration_since(Instant::now())
        .ok_or(Failure::Unavailable)?;
    if org_socket::poll_readable(fd.as_raw_fd(), left).map_err(|_| Failure::Unavailable)? {
        Ok(())
    } else {
        Err(Failure::Unavailable)
    }
}
fn receive_error(error: ReceiveError, fatal: &dyn Fn()) -> Failure {
    if matches!(&error,ReceiveError::Fatal(e) if e.kind()!=io::ErrorKind::UnexpectedEof) {
        fatal();
    }
    Failure::Unavailable
}
impl HostControl {
    /// The verified gateway rendezvous supplies both descriptors. `fatal` must
    /// terminate the gateway on fatal ancillary framing (X-02 §5.2); it is not
    /// an authorization hook. No process is spawned by this crate.
    pub fn from_handoff(
        control: OwnedFd,
        transfer: OwnedFd,
        instance: String,
        fatal: Arc<dyn Fn() + Send + Sync>,
    ) -> io::Result<Arc<Self>> {
        let socket = org_socket::rights::connected_unix_stream(control)?;
        let transfer = org_socket::rights::unix_datagram(transfer)?;
        socket.set_nonblocking(false)?;
        socket.set_write_timeout(Some(DEADLINE))?;
        let writer = socket.try_clone()?;
        let receiver = StreamReceiver::new(socket.try_clone()?);
        let transfer = DatagramReceiver::new(transfer);
        let (jobs, pending) = mpsc::sync_channel::<Job>(32);
        let live = Arc::new(AtomicBool::new(true));
        let alive = Arc::clone(&live);
        thread::Builder::new()
            .name("gateway-host-control".into())
            .spawn(move || {
                actor(writer, receiver, transfer, pending, &alive, &*fatal);
                alive.store(false, Ordering::SeqCst);
            })?;
        let control = Arc::new(Self {
            instance,
            live,
            socket,
            jobs,
            next: std::sync::atomic::AtomicU64::new(1),
            channels: Mutex::new(BTreeMap::new()),
        });
        let hello = control
            .call::<org_host_control::Hello>(&Default::default(), None)
            .map_err(|_| io::Error::other("host hello failed"))?
            .0;
        if hello.api_version != org_host_control::API_VERSION
            || hello.instance_id.as_deref() != Some(control.instance())
        {
            return Err(io::Error::other("host scope mismatch"));
        }
        Ok(control)
    }
    pub fn instance(&self) -> &str {
        &self.instance
    }
    /// Configuration for the listener received on this admitted connection only.
    pub fn fetch_listener(&self) -> Result<ops::TlsFetchResult, Failure> {
        self.call::<ops::TlsFetch>(&Default::default(), None)
            .map(|r| r.0)
    }
    pub fn live(&self) -> bool {
        self.live.load(Ordering::SeqCst)
    }
    fn close(&self) {
        self.live.store(false, Ordering::SeqCst);
        let _ = self.socket.shutdown(Shutdown::Both);
    }
    fn call<C: Call>(
        &self,
        params: &C::Params,
        office: Option<String>,
    ) -> Result<(C::Result, Option<OwnedFd>), Failure> {
        #[derive(Serialize)]
        struct Request<'a, T> {
            id: u64,
            method: &'static str,
            params: &'a T,
        }
        if !self.live() {
            return Err(Failure::Unavailable);
        }
        let id = self
            .next
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < (1 << 53) - 1).then_some(n + 1)
            })
            .map_err(|_| Failure::Unavailable)?;
        let line = encode(&Request {
            id,
            method: C::METHOD,
            params,
        })?;
        let until = Instant::now() + DEADLINE;
        let (answer, receiver) = mpsc::sync_channel(1);
        self.jobs
            .try_send(Job {
                line,
                id,
                office,
                until,
                answer,
            })
            .map_err(|_| Failure::Unavailable)?;
        let reply = match receiver.recv_timeout(DEADLINE) {
            Ok(reply) => reply?,
            Err(_) => {
                self.close();
                return Err(Failure::Unavailable);
            }
        };
        Ok((decode(&reply.bytes, id)?, reply.descriptor))
    }
    pub fn verify(
        &self,
        login: &str,
        password: &Password,
    ) -> Result<ops::AccountVerifyResult, Failure> {
        #[derive(Serialize)]
        struct Params<'a> {
            login: &'a str,
            password: &'a Password,
        }
        // Borrow the password for typed direct encoding, without a raw Value.
        struct Verify;
        impl Call for Verify {
            const METHOD: &'static str = "account.verify";
            type Params = ();
            type Result = ops::AccountVerifyResult;
        }
        self.borrowed::<Verify, _>(&Params { login, password })
    }
    fn borrowed<C: Call, T: Serialize>(&self, params: &T) -> Result<C::Result, Failure> {
        #[derive(Serialize)]
        struct Request<'a, T> {
            id: u64,
            method: &'static str,
            params: &'a T,
        }
        if !self.live() {
            return Err(Failure::Unavailable);
        }
        let id = self
            .next
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < (1 << 53) - 1).then_some(n + 1)
            })
            .map_err(|_| Failure::Unavailable)?;
        let (answer, receiver) = mpsc::sync_channel(1);
        let until = Instant::now() + DEADLINE;
        self.jobs
            .try_send(Job {
                line: encode(&Request {
                    id,
                    method: C::METHOD,
                    params,
                })?,
                id,
                office: None,
                until,
                answer,
            })
            .map_err(|_| Failure::Unavailable)?;
        match receiver.recv_timeout(DEADLINE) {
            Ok(reply) => decode(&reply?.bytes, id),
            Err(_) => {
                self.close();
                Err(Failure::Unavailable)
            }
        }
    }
    pub fn renew(&self, handle: &SessionHandle) -> Result<(), Failure> {
        #[derive(Serialize)]
        struct Params<'a> {
            session: &'a SessionHandle,
        }
        self.borrowed::<ops::SessionRenew, _>(&Params { session: handle })
            .map(|_| ())
    }
    pub fn inspect(&self, handle: &SessionHandle) -> Result<ops::SessionInspectResult, Failure> {
        #[derive(Serialize)]
        struct Params<'a> {
            session: &'a SessionHandle,
        }
        self.borrowed::<ops::SessionInspect, _>(&Params { session: handle })
    }
    pub fn end(&self, handle: &SessionHandle) -> Result<(), Failure> {
        #[derive(Serialize)]
        struct Params<'a> {
            session: &'a SessionHandle,
        }
        let result = self.borrowed::<ops::SessionEnd, _>(&Params { session: handle });
        if let Ok(mut channels) = self.channels.lock() {
            let key: [u8; 32] = Sha256::digest(handle.expose().as_bytes()).into();
            channels.retain(|(k, _), _| *k != key);
        }
        result.map(|_| ())
    }
    pub fn office(
        &self,
        handle: &SessionHandle,
        scope: &OfficeScope,
    ) -> Result<OfficeAccess, Failure> {
        #[derive(Serialize)]
        struct Grant<'a> {
            session: &'a SessionHandle,
            office_id: &'a str,
            channel_id: &'a str,
        }
        if scope.instance_id != self.instance {
            return Err(Failure::Denied);
        }
        let key: [u8; 32] = Sha256::digest(handle.expose().as_bytes()).into();
        let mut channels = self.channels.lock().map_err(|_| Failure::Unavailable)?;
        let cache_key = (key, scope.office_id.clone());
        if !channels.contains_key(&cache_key) {
            if channels.len() >= 256 {
                return Err(Failure::Unavailable);
            }
            let opened = self.open(handle, &scope.office_id)?;
            let office = Arc::new(
                ApprovedOffice::from_descriptor(
                    opened.1,
                    self.instance.clone(),
                    scope.org_id.clone(),
                    scope.office_id.clone(),
                    opened.0.clone(),
                )
                .map_err(|_| Failure::Unavailable)?,
            );
            channels.insert(
                cache_key.clone(),
                Cached {
                    channel: opened.0,
                    office,
                },
            );
        }
        let cached = channels.get(&cache_key).ok_or(Failure::Unavailable)?;
        let grant = self.borrowed::<ops::SessionGrant, _>(&Grant {
            session: handle,
            office_id: &scope.office_id,
            channel_id: &cached.channel,
        })?;
        if grant.grant.claims.org_id != scope.org_id
            || grant.grant.claims.office_id != scope.office_id
            || grant.grant.claims.channel_id != cached.channel
        {
            self.close();
            return Err(Failure::Unavailable);
        }
        Ok(OfficeAccess {
            office: Arc::clone(&cached.office),
            principal: Principal { grant: grant.grant },
        })
    }
    fn open(&self, handle: &SessionHandle, office: &str) -> Result<(String, OwnedFd), Failure> {
        #[derive(Serialize)]
        struct Params<'a> {
            session: &'a SessionHandle,
            office_id: &'a str,
        }
        #[derive(Serialize)]
        struct Request<T> {
            id: u64,
            method: &'static str,
            params: T,
        }
        let id = self
            .next
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < (1 << 53) - 1).then_some(n + 1)
            })
            .map_err(|_| Failure::Unavailable)?;
        let (answer, receiver) = mpsc::sync_channel(1);
        self.jobs
            .try_send(Job {
                line: encode(&Request {
                    id,
                    method: ops::OfficeOpenChannel::METHOD,
                    params: Params {
                        session: handle,
                        office_id: office,
                    },
                })?,
                id,
                office: Some(office.into()),
                until: Instant::now() + DEADLINE,
                answer,
            })
            .map_err(|_| Failure::Unavailable)?;
        let reply = match receiver.recv_timeout(DEADLINE) {
            Ok(reply) => reply?,
            Err(_) => {
                self.close();
                return Err(Failure::Unavailable);
            }
        };
        let result: ops::OfficeOpenChannelResult = decode(&reply.bytes, id)?;
        let fd = reply.descriptor.ok_or(Failure::Unavailable)?;
        let stream =
            org_socket::rights::connected_unix_stream(fd).map_err(|_| Failure::Unavailable)?;
        self.call::<ops::FdsReceived>(
            &ops::FdsReceivedParams {
                of: result.channel_id.clone(),
            },
            None,
        )?;
        Ok((result.channel_id, stream.into()))
    }
}
impl Drop for HostControl {
    fn drop(&mut self) {
        self.close();
    }
}
fn actor(
    mut writer: UnixStream,
    mut receiver: StreamReceiver,
    mut transfer: DatagramReceiver,
    pending: mpsc::Receiver<Job>,
    live: &AtomicBool,
    fatal: &dyn Fn(),
) {
    while live.load(Ordering::SeqCst) {
        let job = match pending.recv_timeout(Duration::from_millis(100)) {
            Ok(job) => job,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if receiver.fd().is_none_or(|fd| {
                    org_socket::poll_readable(fd.as_raw_fd(), Duration::ZERO).unwrap_or(true)
                }) {
                    break;
                }
                continue;
            }
        };
        if Instant::now() >= job.until {
            let _ = job.answer.send(Err(Failure::Unavailable));
            continue;
        }
        let result = (|| {
            writer
                .write_all(&job.line)
                .map_err(|_| Failure::Unavailable)?;
            let mut bytes = Zeroizing::new(Vec::with_capacity(MAX_CONTROL_BYTES));
            loop {
                readable(receiver.fd().ok_or(Failure::Unavailable)?, job.until)?;
                let (b, _) = receiver
                    .receive_byte(0, &SPAWN_FENCE)
                    .map_err(|e| receive_error(e, fatal))?;
                if b == b'\n' {
                    break;
                }
                if bytes.len() == MAX_CONTROL_BYTES {
                    return Err(Failure::Unavailable);
                }
                bytes.push(b);
            }
            let mut descriptor = None;
            if let Some(office) = job.office {
                // Error responses carry no descriptor. A successful response is
                // followed by exactly its matching transfer datagram.
                if let Ok(result) = decode::<ops::OfficeOpenChannelResult>(&bytes, job.id) {
                    readable(transfer.fd().ok_or(Failure::Unavailable)?, job.until)?;
                    let mut header = vec![0; org_office_launch::v3::TRANSFER_HEADER_MAX_BYTES];
                    let received = transfer
                        .receive(&mut header, 1, &SPAWN_FENCE)
                        .map_err(|e| receive_error(e, fatal))?;
                    let record =
                        org_office_launch::v3::decode_transfer_record(&header[..received.bytes])
                            .map_err(|_| Failure::Unavailable)?;
                    if !matches!(record,org_office_launch::v3::TransferRecord::Channel {kind:org_office_launch::v3::ChannelKind::Gateway,office_id,channel_id} if office_id==office&&channel_id==result.channel_id)
                    {
                        return Err(Failure::Unavailable);
                    }
                    descriptor = received.descriptors.into_iter().next();
                }
            }
            Ok(Reply { bytes, descriptor })
        })();
        let failed = result.is_err();
        let _ = job.answer.send(result);
        if failed {
            break;
        }
    }
    live.store(false, Ordering::SeqCst);
    let _ = writer.shutdown(Shutdown::Both);
}

#[cfg(test)]
mod error_tests {
    use super::*;
    #[test]
    fn native_scope_refusal_is_distinct_from_session_end_and_transport_failure() {
        for (code, expected) in [
            ("not_found", Failure::NotFound),
            ("not_authorized", Failure::Denied),
            ("unavailable", Failure::Unavailable),
            ("storage_error", Failure::Unavailable),
        ] {
            let bytes = serde_json::to_vec(
                &serde_json::json!({"id":1,"error":{"code":code,"message":"sanitized"}}),
            )
            .unwrap();
            assert_eq!(
                decode::<ops::OfficeOpenChannelResult>(&bytes, 1).err(),
                Some(expected)
            );
        }
        assert_eq!(
            decode::<ops::OfficeOpenChannelResult>(
                br#"{"id":2,"error":{"code":"not_found","message":"sanitized"}}"#,
                1
            )
            .err(),
            Some(Failure::Unavailable)
        );
    }
}
