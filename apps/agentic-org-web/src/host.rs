//! One bounded actor owns the already-admitted private host connection. The
//! gateway transfer descriptor from the reviewed handoff is held for the
//! connection's lifetime; the web process opens no office channel on it and
//! sends no office channel request. No host.sock/path fallback, office key or
//! registry access. Browser sessions are not served here until FC-159.
use org_host_control::ops;
use org_proto::Call;
use org_socket::rights::{ReceiveError, StreamReceiver};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    io::{self, Write},
    net::Shutdown,
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::net::{UnixDatagram, UnixStream},
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
/// The actor writes a job only if at least this much of its deadline remains.
/// A job dequeued later stays unclaimed and is refused unwritten: a write with
/// almost no reply budget would expire in flight and close the connection.
const MIN_WRITE_BUDGET: Duration = Duration::from_millis(250);
/// Scheduling slack on top of the actor's own bound for a claimed job (its
/// deadline plus one write timeout) before the caller treats the actor as stuck.
const ACTOR_SLACK: Duration = Duration::from_secs(1);
const MAX_CONTROL_BYTES: usize = org_proto::MAX_FRAME_BYTES;
static SPAWN_FENCE: Mutex<()> = Mutex::new(());
#[cfg(test)]
mod tests;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    Denied,
    NotFound,
    Unavailable,
}
struct Reply {
    bytes: Zeroizing<Vec<u8>>,
}
impl Reply {
    fn new() -> Self {
        Self {
            bytes: Zeroizing::new(Vec::with_capacity(MAX_CONTROL_BYTES.min(64 * 1024))),
        }
    }
    fn push(&mut self, byte: u8) -> Result<(), Failure> {
        if self.bytes.len() == MAX_CONTROL_BYTES {
            return Err(Failure::Unavailable);
        }
        if self.bytes.len() == self.bytes.capacity() {
            let capacity = (self.bytes.capacity() * 2).min(MAX_CONTROL_BYTES);
            let mut bytes = Zeroizing::new(Vec::with_capacity(capacity));
            bytes.extend_from_slice(&self.bytes);
            // Replacing the wrapper zeroes the old allocation before releasing
            // it; Vec's ordinary growth could leave a secret copy behind.
            self.bytes = bytes;
        }
        self.bytes.push(byte);
        Ok(())
    }
}
struct Job {
    line: Zeroizing<Vec<u8>>,
    until: Instant,
    answer: SyncSender<Result<Reply, Failure>>,
    /// Claimed once, by the actor taking the job or by the caller abandoning
    /// it. An abandoned job is never written, so the stream stays in step.
    claimed: Arc<AtomicBool>,
}
pub struct HostControl {
    instance: String,
    live: Arc<AtomicBool>,
    socket: UnixStream,
    jobs: SyncSender<Job>,
    next: std::sync::atomic::AtomicU64,
    // Kept open so the host's transfer end stays valid; never read by this process.
    _transfer: UnixDatagram,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Response<T> {
    id: u64,
    result: Option<T>,
    error: Option<org_proto::ApiError>,
}
/// `desync` runs when the reply answers another request id: replies are no
/// longer paired with requests, so the connection must not be reused.
fn decode<T: DeserializeOwned>(bytes: &[u8], id: u64, desync: &dyn Fn()) -> Result<T, Failure> {
    if bytes.iter().find(|b| !b.is_ascii_whitespace()) != Some(&b'{') {
        return Err(Failure::Unavailable);
    }
    let reply: Response<T> = serde_json::from_slice(bytes).map_err(|_| Failure::Unavailable)?;
    if reply.id != id {
        desync();
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
        let (jobs, pending) = mpsc::sync_channel::<Job>(32);
        let live = Arc::new(AtomicBool::new(true));
        let alive = Arc::clone(&live);
        thread::Builder::new()
            .name("gateway-host-control".into())
            .spawn(move || {
                actor(writer, receiver, pending, &alive, &*fatal);
                alive.store(false, Ordering::SeqCst);
            })?;
        let control = Arc::new(Self {
            instance,
            live,
            socket,
            jobs,
            next: std::sync::atomic::AtomicU64::new(1),
            _transfer: transfer,
        });
        let hello = control
            .call::<org_host_control::Hello>(&Default::default())
            .map_err(|_| io::Error::other("host hello failed"))?;
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
        self.call::<ops::TlsFetch>(&Default::default())
    }
    /// The one `gateway.serving` notification (GW-01 r4): HTTP serving is
    /// established. The host keeps the gateway `starting` until it arrives.
    pub fn report_serving(&self) -> Result<(), Failure> {
        self.call::<ops::GatewayServing>(&Default::default())
            .map(|_| ())
    }
    pub fn live(&self) -> bool {
        self.live.load(Ordering::SeqCst)
    }
    fn close(&self) {
        self.live.store(false, Ordering::SeqCst);
        let _ = self.socket.shutdown(Shutdown::Both);
    }
    fn call<C: Call>(&self, params: &C::Params) -> Result<C::Result, Failure> {
        self.borrowed::<C, C::Params>(params)
    }
    /// Every call gets the full per-call deadline. After the actor took the
    /// request, the caller waits for the actor's own answer (bounded by the
    /// actor's deadline and write timeout); callers must never shorten this budget. Expiry while still queued, or
    /// dequeuing with less than [`MIN_WRITE_BUDGET`] left, abandons the unwritten
    /// request and keeps the connection.
    fn borrowed<C: Call, T: Serialize>(&self, params: &T) -> Result<C::Result, Failure> {
        self.borrowed_until::<C, T>(params, Instant::now() + DEADLINE)
    }
    fn borrowed_until<C: Call, T: Serialize>(
        &self,
        params: &T,
        until: Instant,
    ) -> Result<C::Result, Failure> {
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
        let claimed = Arc::new(AtomicBool::new(false));
        self.jobs
            .try_send(Job {
                line: encode(&Request {
                    id,
                    method: C::METHOD,
                    params,
                })?,
                until,
                answer,
                claimed: Arc::clone(&claimed),
            })
            .map_err(|_| Failure::Unavailable)?;
        match receiver.recv_timeout(until.saturating_duration_since(Instant::now())) {
            Ok(reply) => decode(&reply?.bytes, id, &|| self.close()),
            Err(_) => self.settle(&claimed, &receiver, id, until),
        }
    }
    /// The caller's deadline passed. Unclaimed: the actor will skip it unwritten.
    /// Claimed: the actor owns the stream for this job and always answers it;
    /// its work ends by `until` plus one write timeout. Wait for that answer:
    /// a reply keeps the stream in step, and only an actor failure or a reply
    /// for another id closes the connection. An actor that misses its own bound
    /// (or is gone) cannot be trusted with the stream, so that closes too.
    fn settle<T: DeserializeOwned>(
        &self,
        claimed: &AtomicBool,
        receiver: &mpsc::Receiver<Result<Reply, Failure>>,
        id: u64,
        until: Instant,
    ) -> Result<T, Failure> {
        if !claimed.swap(true, Ordering::SeqCst) {
            return Err(Failure::Unavailable);
        }
        let bound = until.saturating_duration_since(Instant::now()) + DEADLINE + ACTOR_SLACK;
        match receiver.recv_timeout(bound) {
            Ok(Ok(reply)) => decode(&reply.bytes, id, &|| self.close()),
            // The actor failed this job and is already ending the connection.
            Ok(Err(error)) => {
                self.close();
                Err(error)
            }
            Err(_) => {
                self.close();
                Err(Failure::Unavailable)
            }
        }
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
    pending: mpsc::Receiver<Job>,
    live: &AtomicBool,
    fatal: &dyn Fn(),
) {
    while live.load(Ordering::SeqCst) {
        let job = match pending.recv_timeout(Duration::from_millis(100)) {
            Ok(job) => job,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Readable while idle means EOF or an unsolicited frame: either
                // ends this connection.
                if receiver.fd().is_none_or(|fd| {
                    org_socket::poll_readable(fd.as_raw_fd(), Duration::ZERO).unwrap_or(true)
                }) {
                    break;
                }
                continue;
            }
        };
        // Expired or nearly expired jobs stay unclaimed and unwritten: a caller
        // resuming its timeout handler must not mistake them for in-flight I/O.
        if job.until.saturating_duration_since(Instant::now()) < MIN_WRITE_BUDGET
            || job.claimed.swap(true, Ordering::SeqCst)
        {
            let _ = job.answer.send(Err(Failure::Unavailable));
            continue;
        }
        let result = (|| {
            writer
                .write_all(&job.line)
                .map_err(|_| Failure::Unavailable)?;
            let mut reply = Reply::new();
            loop {
                readable(receiver.fd().ok_or(Failure::Unavailable)?, job.until)?;
                let (b, _) = receiver
                    .receive_byte(0, &SPAWN_FENCE)
                    .map_err(|e| receive_error(e, fatal))?;
                if b == b'\n' {
                    break;
                }
                reply.push(b)?;
            }
            Ok(reply)
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
