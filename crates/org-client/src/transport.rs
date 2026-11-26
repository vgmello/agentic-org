//! The connection's plumbing: encoding and queueing requests, waiting for their answers,
//! and the writer and reader threads.

use std::io::{ErrorKind, Write};
use std::sync::mpsc::{self, TrySendError};

use org_proto::{frame, salvage_id, Request, ServerMessage, MAX_FRAME_BYTES};

use super::*;

pub(crate) fn encode_request(request: &Request) -> Result<Vec<u8>, ClientError> {
    let mut line = serde_json::to_vec(request).map_err(|e| ClientError::Protocol(e.to_string()))?;
    if line.len() > MAX_FRAME_BYTES {
        return Err(ClientError::RequestTooLarge);
    }
    line.push(b'\n');
    Ok(line)
}

pub(crate) fn enqueue_request(
    shared: &Shared,
    outbound: Option<&SyncSender<Outgoing>>,
    id: u64,
    message: Outgoing,
    deadline: Instant,
    sent: &mut bool,
) -> Result<(), ClientError> {
    let size = message.line.len();
    let mut waiting = Some(message);
    loop {
        let message = waiting
            .take()
            .expect("the message is put back before looping");
        let Some(outbound) = outbound.filter(|_| !shared.closed.load(Ordering::SeqCst)) else {
            lock(&shared.pending).remove(&id);
            return Err(ClientError::Disconnected);
        };
        let reserved = shared
            .queued_bytes
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |queued| {
                (queued + size <= MAX_QUEUED_BYTES).then_some(queued + size)
            })
            .is_ok();
        if !reserved {
            waiting = Some(message);
        } else {
            match outbound.try_send(message) {
                Ok(()) => {
                    *sent = true;
                    return Ok(());
                }
                Err(TrySendError::Full(message)) => {
                    shared.queued_bytes.fetch_sub(size, Ordering::SeqCst);
                    waiting = Some(message);
                }
                Err(TrySendError::Disconnected(_)) => {
                    shared.queued_bytes.fetch_sub(size, Ordering::SeqCst);
                    lock(&shared.pending).remove(&id);
                    return Err(ClientError::Disconnected);
                }
            }
        }
        if Instant::now() >= deadline {
            lock(&shared.pending).remove(&id);
            return Err(ClientError::Timeout);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

pub(crate) fn await_response(
    shared: &Shared,
    id: u64,
    answer: &Receiver<Result<Response, ClientError>>,
    deadline: Instant,
) -> Result<Result<Response, ClientError>, mpsc::RecvTimeoutError> {
    let mut outcome = answer.recv_timeout(deadline.saturating_duration_since(Instant::now()));
    let entry = lock(&shared.pending).remove(&id);
    if matches!(outcome, Err(mpsc::RecvTimeoutError::Timeout)) {
        let late = if entry.is_none() {
            answer.recv_timeout(Duration::from_millis(100)).ok()
        } else {
            answer.try_recv().ok()
        };
        if let Some(late) = late {
            outcome = Ok(late);
        }
    }
    outcome
}

pub(crate) fn finish_outcome(
    outcome: Result<Result<Response, ClientError>, mpsc::RecvTimeoutError>,
    deadline: Instant,
) -> Result<Response, ClientError> {
    match outcome {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(error)) => Err(error),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(ClientError::Timeout),
        Err(mpsc::RecvTimeoutError::Disconnected) if Instant::now() >= deadline => {
            Err(ClientError::Timeout)
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(ClientError::Disconnected),
    }
}

/// The writer thread: writes queued requests, each within its own deadline.
pub(crate) fn write_loop(shared: &Shared, mut stream: UnixStream, queue: Receiver<Outgoing>) {
    for message in queue {
        shared
            .queued_bytes
            .fetch_sub(message.line.len(), Ordering::SeqCst);
        if shared.closed.load(Ordering::SeqCst) {
            return;
        }
        // Too late already: the caller has given up, and nothing of it has been sent.
        if Instant::now() >= message.deadline {
            let _ = message.state.compare_exchange(
                QUEUED,
                ABANDONED,
                Ordering::SeqCst,
                Ordering::SeqCst,
            );
            continue;
        }
        // The caller may have withdrawn it in the meantime.
        if message
            .state
            .compare_exchange(QUEUED, WRITING, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            continue;
        }
        match write_before(&mut stream, &message.line, message.deadline) {
            Ok(Written::Whole) => message.state.store(WRITTEN, Ordering::SeqCst),
            // Never started after all: nothing is on the wire, so the stream is in step.
            Ok(Written::NotStarted) => message.state.store(ABANDONED, Ordering::SeqCst),
            // The write failed part-way: the stream can't be trusted.
            Err(_) => {
                shared.close();
                return;
            }
        }
    }
}

pub(crate) enum Written {
    Whole,
    NotStarted,
}

/// Writes all of `bytes`, unless `deadline` has already passed before the first byte
/// (`NotStarted`: nothing is on the wire, so the stream is still in step).
///
/// Once started, a request is FINISHED even if its deadline passes meanwhile: stopping would
/// leave half a frame on the wire and the connection could not be used again. A write that
/// is merely slow therefore costs the caller its timeout but not the connection. A write that
/// is stuck is cut by the caller (see [`Client::settle`]): after its deadline plus
/// [`WRITE_GRACE`] it closes the connection, which wakes the blocked `write()`. (A single
/// `write()` can outlast any deadline against a peer that takes a few bytes at a time.)
fn write_before(
    stream: &mut UnixStream,
    bytes: &[u8],
    deadline: Instant,
) -> std::io::Result<Written> {
    write_before_with(stream, bytes, deadline, Instant::now, |stream, timeout| {
        stream.set_write_timeout(Some(timeout))
    })
}

pub(crate) fn write_before_with<W: Write>(
    stream: &mut W,
    bytes: &[u8],
    deadline: Instant,
    mut now: impl FnMut() -> Instant,
    mut configure_timeout: impl FnMut(&mut W, Duration) -> std::io::Result<()>,
) -> std::io::Result<Written> {
    let mut rest = bytes;
    while !rest.is_empty() {
        let remaining = deadline.saturating_duration_since(now());
        if remaining.is_zero() && rest.len() == bytes.len() {
            return Ok(Written::NotStarted);
        }
        // A write that has started but is still not done `WRITE_GRACE` after the deadline is
        // given up here, which closes the connection (part of the request is on the wire).
        // This does not wait for a caller to notice: a caller whose call was answered early
        // (a server may answer before it has read the whole request) never will.
        if remaining.is_zero() && now() >= deadline + WRITE_GRACE {
            return Err(ErrorKind::TimedOut.into());
        }
        // Past the deadline, keep going in short steps; the check above (or the caller, in
        // `settle`) cuts it if it sticks.
        let step = if remaining.is_zero() {
            WRITE_GRACE
        } else {
            remaining
        };
        configure_timeout(stream, step)?;
        match stream.write(rest) {
            Ok(0) => return Err(ErrorKind::WriteZero.into()),
            Ok(n) => rest = &rest[n..],
            // No progress this step; try again (the caller cuts a write that stays stuck).
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(Written::Whole)
}

/// Makes sure the connection is closed however the reader ends, a panic included.
struct CloseOnExit<'a>(&'a Shared);

impl Drop for CloseOnExit<'_> {
    fn drop(&mut self) {
        self.0.close();
    }
}

/// Reads server messages until the connection ends, then fails everything still waiting.
pub(crate) fn read_loop(shared: &Shared, stream: UnixStream) {
    read_loop_with_poll(
        shared,
        stream,
        org_socket::READ_TICK,
        org_socket::poll_readable,
    );
}

pub(crate) fn read_loop_with_poll(
    shared: &Shared,
    stream: UnixStream,
    tick: Duration,
    poll: impl FnMut(std::os::fd::RawFd, Duration) -> std::io::Result<bool>,
) {
    let _close = CloseOnExit(shared);
    let mut reader =
        org_socket::Ticked::with_poll(stream, tick, || shared.closed.load(Ordering::SeqCst), poll);
    while let Ok(Some(line)) = frame::read_line(&mut reader) {
        match serde_json::from_slice::<ServerMessage>(&line) {
            Ok(ServerMessage::Response(response)) => {
                if let Some(waiting) = lock(&shared.pending).remove(&response.id) {
                    let _ = waiting.send(Ok(response));
                }
                // No one waiting: a late answer to a call that timed out, or id 0 (the
                // server couldn't read our request and had no id to answer under).
            }
            Ok(ServerMessage::Event(event)) => {
                let mut installed = lock(&shared.events);
                match installed.as_ref().map(|(_, events)| events.try_send(event)) {
                    Some(Err(TrySendError::Full(_))) => {
                        // The app isn't keeping up. Give the connection up rather than buffer.
                        drop(installed);
                        return;
                    }
                    // The receiver is gone: release the subscription.
                    Some(Err(TrySendError::Disconnected(_))) => *installed = None,
                    Some(Ok(())) | None => {}
                }
            }
            // A message shape from a newer server.
            Ok(ServerMessage::Unknown(_)) => {}
            Err(_) => {
                // A known shape that doesn't decode: fail just the call it answered, if
                // its id can be read; the stream itself is still in step.
                let waiting = salvage_id(&line).and_then(|id| lock(&shared.pending).remove(&id));
                if let Some(waiting) = waiting {
                    let _ = waiting.send(Err(ClientError::undecodable("the answer")));
                }
            }
        }
    }
}
