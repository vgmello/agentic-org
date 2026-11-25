//! One bounded actor owns the already-admitted private host connection. The
//! web process transfer descriptor from the reviewed handoff receives
//! host-opened Web office channels (`browser.open_channel` + `fds.received`).
//! No host.sock/path fallback, office key or registry access.
//!
//! The actor is the connection's one ordered reader (A3 B5): host replies and
//! host events (`session.ended`) are applied in arrival order on its thread,
//! also while idle. A call may carry a hook that runs on that thread with its
//! reply before the next frame is read, so a cache insert from a validate reply
//! is always followed by any `session.ended` the host wrote after it.
use org_host_control::{
    browser::{BrowserOpenChannel, BrowserOpenChannelParams},
    ops,
    organization::BrowserAuth,
};
use org_office_launch::{
    decode_transfer_record, ChannelKind, TransferRecord, TRANSFER_HEADER_MAX_BYTES,
};
use org_proto::Call;
use org_socket::rights::{DatagramReceiver, ReceiveError, StreamReceiver};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    Denied,
    NotFound,
    Unavailable,
    /// A host refusal a route may show the browser: an input rule (`invalid_request`
    /// with `field`/`rule`), a rate limit with its delay, or a create refusal
    /// (`already_exists`, `operation_reused`, `expired`, `clock_skew`).
    Refused(Box<org_proto::ApiError>),
}
/// Applies a host event (`name`, `data`) on the reader thread, in arrival order.
pub type EventSink = Arc<dyn Fn(&str, &serde_json::Value) + Send + Sync>;
/// Runs on the reader thread with a call's successful reply bytes and id.
type Hook = Box<dyn FnOnce(u64, &[u8]) + Send>;
/// Typed reply hook passed to async host calls.
pub type ReplyHook<T> = Box<dyn FnOnce(&Result<T, Failure>) + Send>;
/// Each admitted connection gets a new generation; a cache entry is valid only
/// for the generation whose reader inserted it.
static GENERATIONS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
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
enum Answer {
    Sync(SyncSender<Result<Reply, Failure>>),
    Async(tokio::sync::oneshot::Sender<Result<Reply, Failure>>),
}
impl Answer {
    fn send(self, result: Result<Reply, Failure>) {
        match self {
            Self::Sync(tx) => {
                let _ = tx.send(result);
            }
            Self::Async(tx) => {
                let _ = tx.send(result);
            }
        }
    }
}
struct Job {
    line: Zeroizing<Vec<u8>>,
    until: Instant,
    answer: Answer,
    id: u64,
    hook: Option<Hook>,
    /// Claimed once, by the actor taking the job or by the caller abandoning
    /// it. An abandoned job is never written, so the stream stays in step.
    claimed: Arc<AtomicBool>,
}
pub struct HostControl {
    instance: String,
    generation: u64,
    live: Arc<AtomicBool>,
    socket: UnixStream,
    jobs: SyncSender<Job>,
    next: std::sync::atomic::AtomicU64,
    transfer: Mutex<DatagramReceiver>,
    fatal: Arc<dyn Fn() + Send + Sync>,
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
        (None, Some(e))
            if matches!(
                e.code,
                org_proto::ErrorCode::InvalidRequest
                    | org_proto::ErrorCode::RateLimited
                    | org_proto::ErrorCode::AlreadyExists
                    | org_proto::ErrorCode::Conflict
                    | org_proto::ErrorCode::NotReady
                    | org_proto::ErrorCode::OperationReused
                    | org_proto::ErrorCode::Expired
                    | org_proto::ErrorCode::ClockSkew
            ) =>
        {
            Err(Failure::Refused(Box::new(e)))
        }
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
fn receive_office_channel(
    receiver: &mut DatagramReceiver,
    expected_office: &str,
    expected_channel: &str,
    until: Instant,
    fatal: &dyn Fn(),
) -> Result<UnixStream, Failure> {
    let mut payload = [0u8; TRANSFER_HEADER_MAX_BYTES];
    let received = loop {
        match receiver.receive(&mut payload, 1, &SPAWN_FENCE) {
            Ok(received) => break received,
            Err(ReceiveError::WouldBlock) => {
                let fd = receiver.fd().ok_or(Failure::Unavailable)?;
                readable(fd, until)?;
            }
            Err(error) => return Err(receive_error(error, fatal)),
        }
    };
    let mut descriptors = received.descriptors;
    if descriptors.len() != 1 {
        return Err(Failure::Unavailable);
    }
    let fd = descriptors.pop().ok_or(Failure::Unavailable)?;
    let record =
        decode_transfer_record(&payload[..received.bytes]).map_err(|_| Failure::Unavailable)?;
    match record {
        TransferRecord::Channel {
            kind: ChannelKind::Web,
            office_id,
            channel_id,
        } if office_id == expected_office && channel_id == expected_channel => {
            org_socket::rights::connected_unix_stream(fd).map_err(|_| Failure::Unavailable)
        }
        _ => Err(Failure::Unavailable),
    }
}

fn receive_error(error: ReceiveError, fatal: &dyn Fn()) -> Failure {
    if matches!(&error,ReceiveError::Fatal(e) if e.kind()!=io::ErrorKind::UnexpectedEof) {
        fatal();
    }
    Failure::Unavailable
}
impl HostControl {
    /// The verified web process rendezvous supplies both descriptors. `fatal` must
    /// terminate the web process on fatal ancillary framing (X-02 §5.2); it is not
    /// an authorization hook. No process is spawned by this crate.
    pub fn from_handoff(
        control: OwnedFd,
        transfer: OwnedFd,
        instance: String,
        fatal: Arc<dyn Fn() + Send + Sync>,
        events: EventSink,
    ) -> io::Result<Arc<Self>> {
        let socket = org_socket::rights::connected_unix_stream(control)?;
        let transfer = org_socket::rights::unix_datagram(transfer)?;
        transfer.set_nonblocking(true)?;
        socket.set_nonblocking(false)?;
        socket.set_write_timeout(Some(DEADLINE))?;
        let writer = socket.try_clone()?;
        let receiver = StreamReceiver::new(socket.try_clone()?);
        let (jobs, pending) = mpsc::sync_channel::<Job>(32);
        let live = Arc::new(AtomicBool::new(true));
        let (active_tx, active_rx) = mpsc::sync_channel::<ActiveCall>(1);
        let (reply_tx, reply_rx) = mpsc::sync_channel::<Result<Reply, Failure>>(1);
        let reader_live = Arc::clone(&live);
        let reader_fatal = Arc::clone(&fatal);
        let reader_shutdown = socket.try_clone()?;
        thread::Builder::new()
            .name("web-host-reader".into())
            .spawn(move || {
                reader_loop(
                    receiver,
                    active_rx,
                    reply_tx,
                    &reader_live,
                    &*reader_fatal,
                    &*events,
                );
                reader_live.store(false, Ordering::SeqCst);
                let _ = reader_shutdown.shutdown(Shutdown::Both);
            })?;
        let writer_live = Arc::clone(&live);
        thread::Builder::new()
            .name("web-host-writer".into())
            .spawn(move || {
                writer_loop(writer, pending, active_tx, reply_rx, &writer_live);
                writer_live.store(false, Ordering::SeqCst);
            })?;
        let control = Arc::new(Self {
            instance,
            generation: GENERATIONS.fetch_add(1, Ordering::SeqCst),
            live,
            socket,
            jobs,
            next: std::sync::atomic::AtomicU64::new(1),
            transfer: Mutex::new(DatagramReceiver::new(transfer)),
            fatal,
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
    /// This connection's generation (see [`GENERATIONS`]).
    pub fn generation(&self) -> u64 {
        self.generation
    }
    /// A host call; `C` must be an op the host serves on the web connection.
    pub fn request<C: Call>(&self, params: &C::Params) -> Result<C::Result, Failure> {
        self.call::<C>(params)
    }
    /// Non-blocking async host call that awaits the control reader's oneshot directly
    /// on Tokio without occupying any `BlockingPool` worker.
    pub async fn request_async<C: Call>(&self, params: &C::Params) -> Result<C::Result, Failure>
    where
        C::Result: 'static,
    {
        self.request_then_async::<C>(params, None).await
    }
    /// Async version of [`request_then`](Self::request_then).
    pub async fn request_then_async<C: Call>(
        &self,
        params: &C::Params,
        hook: Option<ReplyHook<C::Result>>,
    ) -> Result<C::Result, Failure>
    where
        C::Result: 'static,
    {
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
        let until = Instant::now() + DEADLINE;
        let (answer, receiver) = tokio::sync::oneshot::channel();
        let claimed = Arc::new(AtomicBool::new(false));
        let hook: Option<Hook> = hook.map(|f| {
            Box::new(move |reply_id: u64, bytes: &[u8]| {
                f(&decode::<C::Result>(bytes, reply_id, &|| {}));
            }) as Hook
        });
        self.jobs
            .try_send(Job {
                line: encode(&Request {
                    id,
                    method: C::METHOD,
                    params,
                })?,
                until,
                answer: Answer::Async(answer),
                id,
                hook,
                claimed: Arc::clone(&claimed),
            })
            .map_err(|_| Failure::Unavailable)?;
        let wait = until.saturating_duration_since(Instant::now());
        match tokio::time::timeout(wait, receiver).await {
            Ok(Ok(reply)) => decode(&reply?.bytes, id, &|| self.close()),
            Ok(Err(_)) => Err(Failure::Unavailable),
            Err(_) => {
                if !claimed.swap(true, Ordering::SeqCst) {
                    return Err(Failure::Unavailable);
                }
                self.close();
                Err(Failure::Unavailable)
            }
        }
    }
    /// Async wrapper for [`open_office_channel`](Self::open_office_channel) used by the
    /// async multiplexed office bridge when establishing a new channel.
    pub async fn open_office_channel_async(
        self: &Arc<Self>,
        auth: BrowserAuth,
        office_id: String,
    ) -> Result<(String, UnixStream), Failure> {
        let host = Arc::clone(self);
        tokio::task::spawn_blocking(move || host.open_office_channel(auth, &office_id))
            .await
            .map_err(|_| Failure::Unavailable)?
    }
    /// Like [`request`](Self::request), and `hook` runs on the reader thread
    /// with the decoded reply before the next host frame is applied. It does
    /// not run when the call is refused unwritten or the connection fails.
    pub fn request_then<C: Call>(
        &self,
        params: &C::Params,
        hook: impl FnOnce(&Result<C::Result, Failure>) + Send + 'static,
    ) -> Result<C::Result, Failure> {
        let hook: Hook = Box::new(move |id, bytes| hook(&decode::<C::Result>(bytes, id, &|| {})));
        self.hooked::<C, C::Params>(params, Instant::now() + DEADLINE, Some(hook))
    }
    /// Configuration for the listener received on this admitted connection only.
    pub fn fetch_listener(&self) -> Result<ops::TlsFetchResult, Failure> {
        self.call::<ops::TlsFetch>(&Default::default())
    }
    /// The one `web.serving` notification (GW-01 r4): HTTP serving is
    /// established. The host keeps the web process `starting` until it arrives.
    pub fn report_serving(&self) -> Result<(), Failure> {
        self.call::<ops::WebServing>(&Default::default())
            .map(|_| ())
    }
    /// Opens an authenticated Web channel to `office_id` through the host,
    /// receives the descriptor on the transfer socket, validates its header and
    /// socket type, and acknowledges receipt with `fds.received`.
    pub fn open_office_channel(
        &self,
        auth: BrowserAuth,
        office_id: &str,
    ) -> Result<(String, UnixStream), Failure> {
        // One open in flight at a time so each transfer datagram matches its reply
        // and `fds.received` unblocks the host writer immediately.
        let mut transfer = self.transfer.lock().map_err(|_| Failure::Unavailable)?;
        let opened = self.request::<BrowserOpenChannel>(&BrowserOpenChannelParams {
            auth,
            office_id: office_id.to_owned(),
        })?;
        let until = Instant::now() + DEADLINE;
        let stream = match receive_office_channel(
            &mut transfer,
            office_id,
            &opened.channel_id,
            until,
            &*self.fatal,
        ) {
            Ok(stream) => stream,
            Err(error) => {
                // Host write_loop is awaiting `fds.received`; closing ends the
                // connection cleanly rather than leaving it desynchronized.
                self.close();
                return Err(error);
            }
        };
        self.request::<ops::FdsReceived>(&ops::FdsReceivedParams {
            of: opened.channel_id.clone(),
        })?;
        Ok((opened.channel_id, stream))
    }
    pub fn live(&self) -> bool {
        self.live.load(Ordering::SeqCst)
    }
    /// Ends the connection: [`live`](Self::live) turns false, which ends serving.
    /// Also called when the host's stdin pipe closes (the host is gone).
    pub(crate) fn close(&self) {
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
        self.hooked::<C, T>(params, until, None)
    }
    fn hooked<C: Call, T: Serialize>(
        &self,
        params: &T,
        until: Instant,
        hook: Option<Hook>,
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
                answer: Answer::Sync(answer),
                id,
                hook,
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
/// Reads one frame (up to `\n`) by `until`.
fn read_frame(
    receiver: &mut StreamReceiver,
    until: Instant,
    fatal: &dyn Fn(),
) -> Result<Reply, Failure> {
    let mut reply = Reply::new();
    loop {
        readable(receiver.fd().ok_or(Failure::Unavailable)?, until)?;
        let (b, _) = receiver
            .receive_byte(0, &SPAWN_FENCE)
            .map_err(|e| receive_error(e, fatal))?;
        if b == b'\n' {
            return Ok(reply);
        }
        reply.push(b)?;
    }
}
/// A host event frame: `{"event", "seq", "data"}` with no `id`.
fn event(bytes: &[u8]) -> Option<(String, serde_json::Value)> {
    #[derive(Deserialize)]
    struct Frame {
        event: String,
        #[serde(rename = "seq")]
        _seq: u64,
        #[serde(default)]
        data: serde_json::Value,
    }
    let frame: Frame = serde_json::from_slice(bytes).ok()?;
    Some((frame.event, frame.data))
}
struct ActiveCall {
    id: u64,
    hook: Option<Hook>,
}

/// Dedicated event-driven host reader thread: blocks natively on `poll_readable`
/// (zero 100 ms idle job-polling loop), applies host events in arrival order as
/// soon as they arrive, runs each reply hook on this thread before reading the next
/// frame, and delivers replies to the writer thread.
fn reader_loop(
    mut receiver: StreamReceiver,
    active: mpsc::Receiver<ActiveCall>,
    replies: SyncSender<Result<Reply, Failure>>,
    live: &AtomicBool,
    fatal: &dyn Fn(),
    events: &dyn Fn(&str, &serde_json::Value),
) {
    let mut in_flight: Option<ActiveCall> = None;
    while live.load(Ordering::SeqCst) {
        let Some(fd) = receiver.fd() else {
            break;
        };
        match org_socket::poll_readable(fd.as_raw_fd(), Duration::from_secs(1)) {
            Ok(false) => continue,
            Ok(true) => {}
            Err(_) => break,
        }
        let Ok(frame) = read_frame(&mut receiver, Instant::now() + DEADLINE, fatal) else {
            break;
        };
        if let Some((name, data)) = event(&frame.bytes) {
            events(&name, &data);
            continue;
        }
        // Writer registers `ActiveCall` before writing the request, so any reply
        // frame has its call waiting on `active`.
        if in_flight.is_none() {
            in_flight = active.try_recv().ok();
        }
        let Some(call) = in_flight.take() else {
            break;
        };
        if let Some(hook) = call.hook {
            hook(call.id, &frame.bytes);
        }
        if replies.send(Ok(frame)).is_err() {
            break;
        }
    }
    let _ = replies.try_send(Err(Failure::Unavailable));
}

/// Dedicated event-driven host writer thread: blocks natively on `pending.recv()`
/// with zero idle polling, writes one request at a time, and pairs it with the
/// reader thread's reply within the job's deadline.
fn writer_loop(
    mut writer: UnixStream,
    pending: mpsc::Receiver<Job>,
    active: SyncSender<ActiveCall>,
    replies: mpsc::Receiver<Result<Reply, Failure>>,
    live: &AtomicBool,
) {
    while live.load(Ordering::SeqCst) {
        let Ok(job) = pending.recv() else {
            break;
        };
        if !live.load(Ordering::SeqCst) {
            job.answer.send(Err(Failure::Unavailable));
            break;
        }
        // Expired or nearly expired jobs stay unclaimed and unwritten: a caller
        // resuming its timeout handler must not mistake them for in-flight I/O.
        if job.until.saturating_duration_since(Instant::now()) < MIN_WRITE_BUDGET
            || job.claimed.swap(true, Ordering::SeqCst)
        {
            job.answer.send(Err(Failure::Unavailable));
            continue;
        }
        if active
            .send(ActiveCall {
                id: job.id,
                hook: job.hook,
            })
            .is_err()
        {
            job.answer.send(Err(Failure::Unavailable));
            break;
        }
        let result = if writer.write_all(&job.line).is_err() {
            Err(Failure::Unavailable)
        } else {
            let wait = job.until.saturating_duration_since(Instant::now());
            match replies.recv_timeout(wait) {
                Ok(reply) => reply,
                Err(_) => Err(Failure::Unavailable),
            }
        };
        let failed = result.is_err();
        job.answer.send(result);
        if failed {
            break;
        }
    }
    live.store(false, Ordering::SeqCst);
    let _ = writer.shutdown(Shutdown::Both);
}
