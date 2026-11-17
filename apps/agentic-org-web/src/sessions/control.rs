//! One bounded actor owns the already-admitted private host connection. The
//! gateway transfer descriptor from the reviewed handoff is held for the
//! connection's lifetime; F4 opens no office channel on it and sends no office
//! channel request. No host.sock/path fallback, office key or registry access.
use org_host_control::{ops, Password, SessionHandle};
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
const MAX_CONTROL_BYTES: usize = 64 * 1024;
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
struct Job {
    line: Zeroizing<Vec<u8>>,
    until: Instant,
    answer: SyncSender<Result<Reply, Failure>>,
}
pub struct HostControl {
    instance: String,
    live: Arc<AtomicBool>,
    socket: UnixStream,
    jobs: SyncSender<Job>,
    next: std::sync::atomic::AtomicU64,
    // Kept open so the host's gateway transfer end stays valid; never read in F4.
    _transfer: UnixDatagram,
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
        let until = until.min(Instant::now() + DEADLINE);
        if !self.live() || Instant::now() >= until {
            return Err(Failure::Unavailable);
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
                    method: C::METHOD,
                    params,
                })?,
                until,
                answer,
            })
            .map_err(|_| Failure::Unavailable)?;
        match receiver.recv_timeout(until.saturating_duration_since(Instant::now())) {
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
        self.inspect_until(handle, Instant::now() + DEADLINE)
    }
    pub(super) fn inspect_until(
        &self,
        handle: &SessionHandle,
        until: Instant,
    ) -> Result<ops::SessionInspectResult, Failure> {
        #[derive(Serialize)]
        struct Params<'a> {
            session: &'a SessionHandle,
        }
        self.borrowed_until::<ops::SessionInspect, _>(&Params { session: handle }, until)
    }
    pub fn end(&self, handle: &SessionHandle) -> Result<(), Failure> {
        #[derive(Serialize)]
        struct Params<'a> {
            session: &'a SessionHandle,
        }
        self.borrowed::<ops::SessionEnd, _>(&Params { session: handle })
            .map(|_| ())
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
                // ends this connection, and with it every host session on it.
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
            Ok(Reply { bytes })
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
