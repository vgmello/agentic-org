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
//!
//! Concrete Unix-domain socket I/O lives in [`crate::transport::local_unix`];
//! [`HostControl`] (`LocalUnixHostTransport`) implements [`ControlPlaneTransport`].

use std::{
    io,
    os::fd::OwnedFd,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, SyncSender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use org_host_control::{
    browser::{
        BrowserOpenChannel, BrowserOpenChannelParams, BrowserWebhookOpenChannel,
        BrowserWebhookOpenChannelParams,
    },
    ops,
    organization::BrowserAuth,
};
use org_proto::Call;
use serde::Deserialize;
#[cfg(test)]
use serde::{de::DeserializeOwned, Serialize};
use zeroize::Zeroizing;

#[cfg(test)]
use crate::ports::decode_control_frame as decode;
use crate::ports::{
    encode_control_params, encode_control_request_frame, BoxFuture, ControlPayload,
    ControlPayloadRef, RawReplyHook,
};
pub use crate::ports::{ControlPlaneTransport, EventSink, Failure, ReplyHook};
use crate::transport::local_unix::{
    ControlReplyBuffer as Reply, LocalUnixControlSocket, LocalUnixFrameReader,
    LocalUnixOfficeStream, LocalUnixTransferChannel,
};

const DEADLINE: Duration = Duration::from_secs(5);
/// The actor writes a job only if at least this much of its deadline remains.
/// A job dequeued later stays unclaimed and is refused unwritten: a write with
/// almost no reply budget would expire in flight and close the connection.
const MIN_WRITE_BUDGET: Duration = Duration::from_millis(250);
/// Scheduling slack on top of the actor's own bound for a claimed job (its
/// deadline plus one write timeout) before the caller treats the actor as stuck.
const ACTOR_SLACK: Duration = Duration::from_secs(1);

#[cfg(test)]
mod tests;

/// Each admitted connection gets a new generation; a cache entry is valid only
/// for the generation whose reader inserted it.
static GENERATIONS: AtomicU64 = AtomicU64::new(1);

/// The sole production [`ControlPlaneTransport`] adapter over the local Unix handoff socket.
pub type LocalUnixHostTransport = HostControl;

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
    hook: Option<RawReplyHook>,
    /// Claimed once, by the actor taking the job or by the caller abandoning
    /// it. An abandoned job is never written, so the stream stays in step.
    claimed: Arc<AtomicBool>,
}

pub struct HostControl {
    instance: String,
    generation: u64,
    live: Arc<AtomicBool>,
    socket: LocalUnixControlSocket,
    jobs: SyncSender<Job>,
    next: AtomicU64,
    transfer: LocalUnixTransferChannel,
    fatal: Arc<dyn Fn() + Send + Sync>,
}

#[cfg(test)]
fn encode<T: Serialize>(value: &T) -> Result<Zeroizing<Vec<u8>>, Failure> {
    let mut bytes = encode_control_params(value)?;
    if bytes.len() >= crate::ports::MAX_CONTROL_BYTES {
        return Err(Failure::Unavailable);
    }
    bytes.push(b'\n');
    Ok(bytes)
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
        let socket = LocalUnixControlSocket::from_handoff_fd(control, DEADLINE)?;
        let transfer = LocalUnixTransferChannel::from_handoff_fd(transfer)?;
        let writer = socket.try_clone()?;
        let receiver = socket.try_clone()?.into_frame_reader();
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
                reader_shutdown.shutdown();
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
            next: AtomicU64::new(1),
            transfer,
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
        let raw_hook = hook.map(|f| -> RawReplyHook {
            Box::new(move |payload| {
                let decoded = match payload {
                    Ok(inner) => inner.decode::<C::Result>(&|| {}),
                    Err(error) => Err(error.clone()),
                };
                f(&decoded);
            })
        });
        let encoded = encode_control_params(params)?;
        let payload = self
            .call_raw_async_inner(C::METHOD, encoded, raw_hook)
            .await?;
        payload.decode(&|| self.close())
    }

    /// Async wrapper for [`open_office_channel`](Self::open_office_channel) used by the
    /// async multiplexed office bridge when establishing a new local-Unix channel.
    pub(crate) async fn open_office_channel_async(
        self: &Arc<Self>,
        auth: BrowserAuth,
        office_id: String,
    ) -> Result<(String, LocalUnixOfficeStream), Failure> {
        let host = Arc::clone(self);
        tokio::task::spawn_blocking(move || host.open_office_channel(auth, &office_id))
            .await
            .map_err(|_| Failure::Unavailable)?
    }

    /// Async wrapper for [`open_webhook_channel`](Self::open_webhook_channel).
    pub(crate) async fn open_webhook_channel_async(
        self: &Arc<Self>,
        org_id: String,
    ) -> Result<(String, String, LocalUnixOfficeStream), Failure> {
        let host = Arc::clone(self);
        tokio::task::spawn_blocking(move || host.open_webhook_channel(&org_id))
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
        let raw_hook: RawReplyHook = Box::new(move |payload| {
            let decoded = match payload {
                Ok(inner) => inner.decode::<C::Result>(&|| {}),
                Err(error) => Err(error.clone()),
            };
            hook(&decoded);
        });
        let encoded = encode_control_params(params)?;
        let payload = self.call_raw_until(
            C::METHOD,
            encoded,
            Instant::now() + DEADLINE,
            Some(raw_hook),
        )?;
        payload.decode(&|| self.close())
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
    pub(crate) fn open_office_channel(
        &self,
        auth: BrowserAuth,
        office_id: &str,
    ) -> Result<(String, LocalUnixOfficeStream), Failure> {
        // One open in flight at a time so each transfer datagram matches its reply
        // and `fds.received` unblocks the host writer immediately.
        let mut transfer = self.transfer.lock()?;
        let opened = self.request::<BrowserOpenChannel>(&BrowserOpenChannelParams {
            auth,
            office_id: office_id.to_owned(),
        })?;
        let until = Instant::now() + DEADLINE;
        let stream = match transfer.receive_office_channel(
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

    /// Opens a webhook-scoped Web channel to `org_id`'s HQ office without a browser session.
    pub(crate) fn open_webhook_channel(
        &self,
        org_id: &str,
    ) -> Result<(String, String, LocalUnixOfficeStream), Failure> {
        let mut transfer = self.transfer.lock()?;
        let opened =
            self.request::<BrowserWebhookOpenChannel>(&BrowserWebhookOpenChannelParams {
                org_id: org_id.to_owned(),
            })?;
        let until = Instant::now() + DEADLINE;
        let stream = match transfer.receive_office_channel(
            &opened.office_id,
            &opened.channel_id,
            until,
            &*self.fatal,
        ) {
            Ok(stream) => stream,
            Err(error) => {
                self.close();
                return Err(error);
            }
        };
        self.request::<ops::FdsReceived>(&ops::FdsReceivedParams {
            of: opened.channel_id.clone(),
        })?;
        Ok((opened.office_id, opened.channel_id, stream))
    }

    pub fn live(&self) -> bool {
        self.live.load(Ordering::SeqCst)
    }

    /// Ends the connection: [`live`](Self::live) turns false, which ends serving.
    /// Also called when the host's stdin pipe closes (the host is gone).
    pub(crate) fn close(&self) {
        self.live.store(false, Ordering::SeqCst);
        self.socket.shutdown();
    }

    fn next_request_id(&self) -> Result<u64, Failure> {
        self.next
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < (1 << 53) - 1).then_some(n + 1)
            })
            .map_err(|_| Failure::Unavailable)
    }

    fn call<C: Call>(&self, params: &C::Params) -> Result<C::Result, Failure> {
        let encoded = encode_control_params(params)?;
        let payload = self.call_raw_until(C::METHOD, encoded, Instant::now() + DEADLINE, None)?;
        payload.decode(&|| self.close())
    }

    #[cfg(test)]
    fn borrowed_until<C: Call, T: Serialize>(
        &self,
        params: &T,
        until: Instant,
    ) -> Result<C::Result, Failure> {
        let encoded = encode_control_params(params)?;
        let payload = self.call_raw_until(C::METHOD, encoded, until, None)?;
        payload.decode(&|| self.close())
    }

    fn call_raw_until(
        &self,
        method: &'static str,
        params_json: Zeroizing<Vec<u8>>,
        until: Instant,
        hook: Option<RawReplyHook>,
    ) -> Result<ControlPayload, Failure> {
        if !self.live() {
            return Err(Failure::Unavailable);
        }
        let id = self.next_request_id()?;
        let line = encode_control_request_frame(id, method, params_json)?;
        let (answer, receiver) = mpsc::sync_channel(1);
        let claimed = Arc::new(AtomicBool::new(false));
        self.jobs
            .try_send(Job {
                line,
                until,
                answer: Answer::Sync(answer),
                id,
                hook,
                claimed: Arc::clone(&claimed),
            })
            .map_err(|_| Failure::Unavailable)?;
        match receiver.recv_timeout(until.saturating_duration_since(Instant::now())) {
            Ok(reply) => Ok(ControlPayload::Frame {
                id,
                bytes: reply?.bytes,
            }),
            Err(_) => self.settle_raw(&claimed, &receiver, id, until),
        }
    }

    async fn call_raw_async_inner(
        &self,
        method: &'static str,
        params_json: Zeroizing<Vec<u8>>,
        hook: Option<RawReplyHook>,
    ) -> Result<ControlPayload, Failure> {
        if !self.live() {
            return Err(Failure::Unavailable);
        }
        let id = self.next_request_id()?;
        let until = Instant::now() + DEADLINE;
        let line = encode_control_request_frame(id, method, params_json)?;
        let (answer, receiver) = tokio::sync::oneshot::channel();
        let claimed = Arc::new(AtomicBool::new(false));
        self.jobs
            .try_send(Job {
                line,
                until,
                answer: Answer::Async(answer),
                id,
                hook,
                claimed: Arc::clone(&claimed),
            })
            .map_err(|_| Failure::Unavailable)?;
        let wait = until.saturating_duration_since(Instant::now());
        match tokio::time::timeout(wait, receiver).await {
            Ok(Ok(reply)) => Ok(ControlPayload::Frame {
                id,
                bytes: reply?.bytes,
            }),
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

    /// The caller's deadline passed. Unclaimed: the actor will skip it unwritten.
    /// Claimed: the actor owns the stream for this job and always answers it;
    /// its work ends by `until` plus one write timeout. Wait for that answer:
    /// a reply keeps the stream in step, and only an actor failure or a reply
    /// for another id closes the connection. An actor that misses its own bound
    /// (or is gone) cannot be trusted with the stream, so that closes too.
    fn settle_raw(
        &self,
        claimed: &AtomicBool,
        receiver: &mpsc::Receiver<Result<Reply, Failure>>,
        id: u64,
        until: Instant,
    ) -> Result<ControlPayload, Failure> {
        if !claimed.swap(true, Ordering::SeqCst) {
            return Err(Failure::Unavailable);
        }
        let bound = until.saturating_duration_since(Instant::now()) + DEADLINE + ACTOR_SLACK;
        match receiver.recv_timeout(bound) {
            Ok(Ok(reply)) => Ok(ControlPayload::Frame {
                id,
                bytes: reply.bytes,
            }),
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

    #[cfg(test)]
    fn settle<T: DeserializeOwned>(
        &self,
        claimed: &AtomicBool,
        receiver: &mpsc::Receiver<Result<Reply, Failure>>,
        id: u64,
        until: Instant,
    ) -> Result<T, Failure> {
        let payload = self.settle_raw(claimed, receiver, id, until)?;
        payload.decode(&|| self.close())
    }
}

impl ControlPlaneTransport for HostControl {
    fn instance(&self) -> &str {
        HostControl::instance(self)
    }

    fn generation(&self) -> u64 {
        HostControl::generation(self)
    }

    fn live(&self) -> bool {
        HostControl::live(self)
    }

    fn close(&self) {
        HostControl::close(self);
    }

    fn call_raw(
        &self,
        method: &'static str,
        params_json: Zeroizing<Vec<u8>>,
        hook: Option<RawReplyHook>,
    ) -> Result<ControlPayload, Failure> {
        self.call_raw_until(method, params_json, Instant::now() + DEADLINE, hook)
    }

    fn call_raw_async(
        &self,
        method: &'static str,
        params_json: Zeroizing<Vec<u8>>,
        hook: Option<RawReplyHook>,
    ) -> BoxFuture<'_, Result<ControlPayload, Failure>> {
        Box::pin(self.call_raw_async_inner(method, params_json, hook))
    }
}

impl Drop for HostControl {
    fn drop(&mut self) {
        self.close();
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
    hook: Option<RawReplyHook>,
}

/// Dedicated event-driven host reader thread: blocks natively on `poll_readable`
/// (zero 100 ms idle job-polling loop), applies host events in arrival order as
/// soon as they arrive, runs each reply hook on this thread before reading the next
/// frame, and delivers replies to the writer thread.
fn reader_loop(
    mut receiver: LocalUnixFrameReader,
    active: mpsc::Receiver<ActiveCall>,
    replies: SyncSender<Result<Reply, Failure>>,
    live: &AtomicBool,
    fatal: &dyn Fn(),
    events: &dyn Fn(&str, &serde_json::Value),
) {
    let mut in_flight: Option<ActiveCall> = None;
    while live.load(Ordering::SeqCst) {
        match receiver.poll_readable(Duration::from_secs(1)) {
            Ok(Some(false)) => continue,
            Ok(Some(true)) => {}
            Ok(None) | Err(_) => break,
        }
        let Ok(frame) = receiver.read_frame(Instant::now() + DEADLINE, fatal) else {
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
            hook(&Ok(ControlPayloadRef::Frame {
                id: call.id,
                bytes: &frame.bytes,
            }));
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
    mut writer: LocalUnixControlSocket,
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
    writer.shutdown();
}
