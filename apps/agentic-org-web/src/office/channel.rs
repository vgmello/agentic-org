//! One multiplexed office channel: negotiation, its read and write loops, round trips and eviction.
//!
//! Generic over any `AsyncRead` / `AsyncWrite` halves; concrete Unix socket conversion stays
//! consolidated in [`crate::transport::local_unix::LocalUnixOfficeStream`].

use std::collections::BTreeMap;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use org_proto::{
    ApiRange, Hello, HelloParams, HelloResult, Outcome, Request, Response, MAX_FRAME_BYTES,
};
use serde_json::Value;
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
};
use tokio::sync::{mpsc, oneshot};
use tokio::time;

use super::{MultiplexedChannel, PendingMap, CHANNEL_WRITE_QUEUE};
use crate::event_hub::{EventBus, HubEvent};
use crate::ports::OFFICE_DEADLINE;
use crate::transport::local_unix::LocalUnixOfficeStream;

impl MultiplexedChannel {
    pub(super) fn from_local_unix(
        office_id: String,
        channel_id: String,
        stream: LocalUnixOfficeStream,
        channels: Arc<Mutex<BTreeMap<String, Arc<Self>>>>,
        events: Arc<dyn EventBus>,
    ) -> io::Result<Arc<Self>> {
        Self::from_local_unix_with_permit(office_id, channel_id, stream, channels, events, None)
    }

    pub(super) fn from_local_unix_with_permit(
        office_id: String,
        channel_id: String,
        stream: LocalUnixOfficeStream,
        channels: Arc<Mutex<BTreeMap<String, Arc<Self>>>>,
        events: Arc<dyn EventBus>,
        slot_permit: Option<tokio::sync::OwnedSemaphorePermit>,
    ) -> io::Result<Arc<Self>> {
        let (read_half, write_half) = stream.into_async_split()?;
        Ok(Self::from_async_halves(
            office_id,
            channel_id,
            read_half,
            write_half,
            channels,
            events,
            slot_permit,
        ))
    }

    pub(super) fn from_async_halves<R, W>(
        office_id: String,
        channel_id: String,
        read_half: R,
        write_half: W,
        channels: Arc<Mutex<BTreeMap<String, Arc<Self>>>>,
        events: Arc<dyn EventBus>,
        slot_permit: Option<tokio::sync::OwnedSemaphorePermit>,
    ) -> Arc<Self>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (writer, write_rx) = mpsc::channel::<Vec<u8>>(CHANNEL_WRITE_QUEUE);
        let pending: PendingMap = Arc::new(Mutex::new(BTreeMap::new()));
        let channel = Arc::new(Self {
            channel_id: channel_id.clone(),
            writer,
            pending: Arc::clone(&pending),
        });
        tokio::spawn(write_loop(write_half, write_rx));
        tokio::spawn(read_loop(
            read_half,
            pending,
            channels,
            office_id,
            channel_id,
            events,
            slot_permit,
        ));
        channel
    }

    pub(super) async fn roundtrip_raw(&self, request: &Request, rpc_id: u64) -> io::Result<Value> {
        self.roundtrip_raw_with_timeout(request, rpc_id, OFFICE_DEADLINE)
            .await
    }

    pub(super) async fn roundtrip_raw_with_timeout(
        &self,
        request: &Request,
        rpc_id: u64,
        deadline: Duration,
    ) -> io::Result<Value> {
        let mut bytes = serde_json::to_vec(request).map_err(io::Error::other)?;
        if bytes.len() >= MAX_FRAME_BYTES {
            return Err(io::Error::other("office request exceeds frame limit"));
        }
        bytes.push(b'\n');
        let (tx, rx) = oneshot::channel();
        {
            let mut map = self
                .pending
                .lock()
                .map_err(|_| io::Error::other("office pending map poisoned"))?;
            map.insert(rpc_id, tx);
        }
        // From here on the entry is removed however the round trip ends without its reply:
        // a closed writer, a timeout, or the caller dropping this future while it waits for
        // a slot in the full write queue (`race_until_closed`, `TASK-174` AC #3).
        let mut entry = PendingEntry {
            pending: &self.pending,
            rpc_id,
            armed: true,
        };
        if self.writer.send(bytes).await.is_err() {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "office writer closed",
            ));
        }
        match time::timeout(deadline, rx).await {
            Ok(Ok(reply)) => {
                // The read loop removed the entry when it delivered the reply.
                entry.armed = false;
                Ok(reply)
            }
            _ => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "office reply unavailable",
            )),
        }
    }
}

/// A round trip's entry in the pending map, removed on drop unless its reply arrived, so a
/// cancelled or failed round trip never leaves a stale waiter behind.
struct PendingEntry<'a> {
    pending: &'a PendingMap,
    rpc_id: u64,
    armed: bool,
}

impl Drop for PendingEntry<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(&self.rpc_id);
        }
    }
}

pub(super) async fn negotiate(channel: &MultiplexedChannel) -> io::Result<()> {
    let mut params = HelloParams::default();
    params.api = Some(ApiRange { min: 2, max: 2 });
    params.client = Some(format!("agentic-org-web {}", env!("CARGO_PKG_VERSION")));
    let request = Request::new::<Hello>(1, &params).map_err(io::Error::other)?;
    let raw = channel.roundtrip_raw(&request, 1).await?;
    let reply: Response = serde_json::from_value(raw).map_err(io::Error::other)?;
    if reply.id != 1 {
        return Err(io::Error::other("office hello id mismatch"));
    }
    let result: HelloResult = match reply.outcome {
        Outcome::Result(value) => serde_json::from_value(value).map_err(io::Error::other)?,
        Outcome::Error(_) => return Err(io::Error::other("office refused hello")),
    };
    if result.api_version != 2 {
        return Err(io::Error::other("office negotiated unexpected version"));
    }
    Ok(())
}

pub(super) async fn write_loop<W: AsyncWrite + Unpin>(
    mut write_half: W,
    mut rx: mpsc::Receiver<Vec<u8>>,
) {
    while let Some(frame) = rx.recv().await {
        if write_half.write_all(&frame).await.is_err() {
            break;
        }
    }
    let _ = write_half.shutdown().await;
}

/// Maximum wire bytes read per newline-delimited frame (`MAX_FRAME_BYTES` payload + `\r\n`).
const MAX_FRAME_WIRE_BYTES: u64 = (MAX_FRAME_BYTES + 2) as u64;

/// Reads one newline-delimited office frame into `buf`, capping reads and allocation at
/// [`MAX_FRAME_WIRE_BYTES`] before scanning for `\n` so an unterminated or oversized stream
/// cannot allocate unbounded memory.
///
/// Returns `Ok(false)` on clean EOF, `Ok(true)` with trailing `\n` / `\r\n` removed when the
/// payload fits within [`MAX_FRAME_BYTES`], or `Err(InvalidData)` when the frame exceeds the
/// limit (whether terminated or unterminated).
pub(super) async fn read_bounded_frame<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    buf: &mut Vec<u8>,
) -> io::Result<bool> {
    buf.clear();
    let read = reader
        .take(MAX_FRAME_WIRE_BYTES)
        .read_until(b'\n', buf)
        .await?;
    if read == 0 {
        return Ok(false);
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
        if buf.last() == Some(&b'\r') {
            buf.pop();
        }
    }
    if buf.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "office frame exceeds maximum size",
        ));
    }
    Ok(true)
}

pub(super) async fn read_loop<R: AsyncRead + Unpin>(
    read_half: R,
    pending: PendingMap,
    channels: Arc<Mutex<BTreeMap<String, Arc<MultiplexedChannel>>>>,
    office_id: String,
    channel_id: String,
    events: Arc<dyn EventBus>,
    slot_permit: Option<tokio::sync::OwnedSemaphorePermit>,
) {
    let _slot_permit = slot_permit;
    let mut reader = BufReader::new(read_half);
    let mut line = Vec::with_capacity(4096);
    loop {
        match read_bounded_frame(&mut reader, &mut line).await {
            Ok(true) => {}
            Ok(false) | Err(_) => break,
        }
        let Ok(value) = serde_json::from_slice::<Value>(&line) else {
            break;
        };
        if let Some(id) = value.get("id").and_then(Value::as_u64) {
            let waiter = pending.lock().ok().and_then(|mut map| map.remove(&id));
            if let Some(tx) = waiter {
                let _ = tx.send(value);
            }
        } else if let Some(event) = value.get("event").and_then(Value::as_str) {
            forward_office_event(events.as_ref(), event, &value);
        }
    }
    if let Ok(mut map) = pending.lock() {
        map.clear();
    }
    evict_channel(&channels, &office_id, &channel_id);
}

pub(super) fn forward_office_event(events: &dyn EventBus, event: &str, frame: &Value) {
    let data = frame.get("data").cloned().unwrap_or(Value::Null);
    if let Some(org_id) = data.get("organization_id").and_then(Value::as_str) {
        let seq = frame.get("seq").and_then(Value::as_u64).unwrap_or(0);
        events.publish(HubEvent::OrganizationChanged {
            org_id: org_id.to_owned(),
            cursor: format!("org:{org_id}:1:{seq}"),
            event: event.to_owned(),
            data,
        });
    }
}

pub(super) fn evict_channel(
    channels: &Mutex<BTreeMap<String, Arc<MultiplexedChannel>>>,
    office_id: &str,
    channel_id: &str,
) {
    if let Ok(mut map) = channels.lock() {
        if map
            .get(office_id)
            .is_some_and(|c| c.channel_id == channel_id)
        {
            map.remove(office_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Poll};

    use tokio::io::ReadBuf;

    use super::*;
    use crate::event_hub::InMemoryEventBus;

    fn run_async(fut: impl std::future::Future<Output = ()>) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(fut);
    }

    /// `AsyncBufRead` stream returning a repeating byte (or finite slice) while recording every
    /// byte consumed by the caller.
    struct CountingBufStream {
        prefix: Vec<u8>,
        pos: usize,
        total_len: usize,
        chunk_buf: [u8; 4096],
        consumed: Arc<AtomicUsize>,
    }

    impl CountingBufStream {
        fn repeating(byte: u8, total_len: usize, consumed: Arc<AtomicUsize>) -> Self {
            Self {
                prefix: Vec::new(),
                pos: 0,
                total_len,
                chunk_buf: [byte; 4096],
                consumed,
            }
        }

        fn from_bytes(bytes: Vec<u8>, consumed: Arc<AtomicUsize>) -> Self {
            let total_len = bytes.len();
            Self {
                prefix: bytes,
                pos: 0,
                total_len,
                chunk_buf: [b'a'; 4096],
                consumed,
            }
        }
    }

    impl AsyncRead for CountingBufStream {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            let remaining = self.total_len.saturating_sub(self.pos);
            if remaining == 0 {
                return Poll::Ready(Ok(()));
            }
            let n = remaining.min(buf.remaining());
            if !self.prefix.is_empty() {
                buf.put_slice(&self.prefix[self.pos..self.pos + n]);
            } else {
                let mut left = n;
                while left > 0 {
                    let take = left.min(self.chunk_buf.len());
                    buf.put_slice(&self.chunk_buf[..take]);
                    left -= take;
                }
            }
            self.pos += n;
            self.consumed.fetch_add(n, Ordering::SeqCst);
            Poll::Ready(Ok(()))
        }
    }

    impl AsyncBufRead for CountingBufStream {
        fn poll_fill_buf(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
            let this = self.get_mut();
            let remaining = this.total_len.saturating_sub(this.pos);
            if remaining == 0 {
                return Poll::Ready(Ok(&[]));
            }
            if !this.prefix.is_empty() {
                Poll::Ready(Ok(&this.prefix[this.pos..]))
            } else {
                let n = remaining.min(this.chunk_buf.len());
                Poll::Ready(Ok(&this.chunk_buf[..n]))
            }
        }

        fn consume(self: Pin<&mut Self>, amt: usize) {
            let this = self.get_mut();
            let step = amt.min(this.total_len.saturating_sub(this.pos));
            this.pos += step;
            this.consumed.fetch_add(step, Ordering::SeqCst);
        }
    }

    #[test]
    fn read_bounded_frame_accepts_exact_max_frame_bytes_lf_and_crlf() {
        run_async(async {
            for suffix in [&b"\n"[..], &b"\r\n"[..]] {
                let mut raw = vec![b'x'; MAX_FRAME_BYTES];
                raw.extend_from_slice(suffix);
                let consumed = Arc::new(AtomicUsize::new(0));
                let mut reader = CountingBufStream::from_bytes(raw, Arc::clone(&consumed));
                let mut buf = Vec::new();
                let ok = read_bounded_frame(&mut reader, &mut buf)
                    .await
                    .expect("frame at exact MAX_FRAME_BYTES must succeed");
                assert!(ok);
                assert_eq!(buf.len(), MAX_FRAME_BYTES);
                assert_eq!(
                    consumed.load(Ordering::SeqCst),
                    MAX_FRAME_BYTES + suffix.len()
                );
                assert!(!read_bounded_frame(&mut reader, &mut buf)
                    .await
                    .expect("EOF after valid frame must succeed"));
            }
        });
    }

    #[test]
    fn read_bounded_frame_rejects_oversized_and_unterminated_streams_within_bound() {
        run_async(async {
            // Case 1: MAX_FRAME_BYTES + 1 followed by newline fails closed.
            let mut over = vec![b'a'; MAX_FRAME_BYTES + 1];
            over.push(b'\n');
            let consumed = Arc::new(AtomicUsize::new(0));
            let mut reader = CountingBufStream::from_bytes(over, Arc::clone(&consumed));
            let mut buf = Vec::new();
            let err = read_bounded_frame(&mut reader, &mut buf)
                .await
                .expect_err("MAX_FRAME_BYTES + 1 must fail closed");
            assert_eq!(err.kind(), io::ErrorKind::InvalidData);
            assert!(consumed.load(Ordering::SeqCst) <= MAX_FRAME_BYTES + 2);

            // Case 2: 16 MiB unterminated stream fails closed after reading at most MAX_FRAME_BYTES + 2.
            let consumed_huge = Arc::new(AtomicUsize::new(0));
            let mut huge =
                CountingBufStream::repeating(b'a', 16 * 1024 * 1024, Arc::clone(&consumed_huge));
            buf.clear();
            let err = read_bounded_frame(&mut huge, &mut buf)
                .await
                .expect_err("16 MiB unterminated frame must fail closed");
            assert_eq!(err.kind(), io::ErrorKind::InvalidData);
            assert_eq!(consumed_huge.load(Ordering::SeqCst), MAX_FRAME_BYTES + 2);
            assert!(buf.len() <= MAX_FRAME_BYTES + 2);
        });
    }

    #[test]
    fn read_loop_evicts_channel_and_stops_reading_unterminated_16mib_stream() {
        run_async(async {
            let consumed = Arc::new(AtomicUsize::new(0));
            let huge = CountingBufStream::repeating(b'a', 16 * 1024 * 1024, Arc::clone(&consumed));
            let pending: PendingMap = Arc::new(Mutex::new(BTreeMap::new()));
            let (tx, rx) = oneshot::channel();
            pending.lock().unwrap().insert(1, tx);
            let channels = Arc::new(Mutex::new(BTreeMap::new()));
            let (writer, _write_rx) = mpsc::channel(1);
            channels.lock().unwrap().insert(
                "office-1".to_owned(),
                Arc::new(MultiplexedChannel {
                    channel_id: "ch-1".to_owned(),
                    writer,
                    pending: Arc::clone(&pending),
                }),
            );
            let events: Arc<dyn EventBus> = Arc::new(InMemoryEventBus::default());
            read_loop(
                huge,
                Arc::clone(&pending),
                Arc::clone(&channels),
                "office-1".to_owned(),
                "ch-1".to_owned(),
                events,
                None,
            )
            .await;
            assert!(pending.lock().unwrap().is_empty());
            assert!(rx.await.is_err());
            assert!(!channels.lock().unwrap().contains_key("office-1"));
            // BufReader fills at most one 8 KiB chunk past MAX_FRAME_BYTES + 2.
            assert!(
                consumed.load(Ordering::SeqCst) <= MAX_FRAME_BYTES + 16 * 1024,
                "read_loop consumed {} bytes on 16 MiB stream",
                consumed.load(Ordering::SeqCst)
            );
        });
    }
}
