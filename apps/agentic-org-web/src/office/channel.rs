//! One multiplexed office channel: negotiation, its read and write loops, round trips and eviction.

use super::MultiplexedChannel;
use super::PendingMap;
use super::CHANNEL_WRITE_QUEUE;
use super::OFFICE_DEADLINE;
use crate::event_hub::EventHub;
use crate::event_hub::HubEvent;
use org_proto::ApiRange;
use org_proto::Hello;
use org_proto::HelloParams;
use org_proto::HelloResult;
use org_proto::Outcome;
use org_proto::Request;
use org_proto::Response;
use org_proto::MAX_FRAME_BYTES;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io;
use std::os::unix::net::UnixStream as StdUnixStream;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::time;

impl MultiplexedChannel {
    pub(super) fn from_std(
        office_id: String,
        channel_id: String,
        stream: StdUnixStream,
        channels: Arc<Mutex<BTreeMap<String, Arc<Self>>>>,
        hub: EventHub,
    ) -> io::Result<Arc<Self>> {
        Self::from_std_with_permit(office_id, channel_id, stream, channels, hub, None)
    }

    pub(super) fn from_std_with_permit(
        office_id: String,
        channel_id: String,
        stream: StdUnixStream,
        channels: Arc<Mutex<BTreeMap<String, Arc<Self>>>>,
        hub: EventHub,
        slot_permit: Option<tokio::sync::OwnedSemaphorePermit>,
    ) -> io::Result<Arc<Self>> {
        stream.set_nonblocking(true)?;
        let stream = UnixStream::from_std(stream)?;
        let (read_half, write_half) = stream.into_split();
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
            hub,
            slot_permit,
        ));
        Ok(channel)
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

pub(super) async fn write_loop(
    mut write_half: tokio::net::unix::OwnedWriteHalf,
    mut rx: mpsc::Receiver<Vec<u8>>,
) {
    while let Some(frame) = rx.recv().await {
        if write_half.write_all(&frame).await.is_err() {
            break;
        }
    }
    let _ = write_half.shutdown().await;
}

pub(super) async fn read_loop(
    read_half: tokio::net::unix::OwnedReadHalf,
    pending: PendingMap,
    channels: Arc<Mutex<BTreeMap<String, Arc<MultiplexedChannel>>>>,
    office_id: String,
    channel_id: String,
    hub: EventHub,
    slot_permit: Option<tokio::sync::OwnedSemaphorePermit>,
) {
    let _slot_permit = slot_permit;
    let mut reader = BufReader::new(read_half);
    let mut line = Vec::with_capacity(4096);
    loop {
        line.clear();
        let Ok(read) = reader.read_until(b'\n', &mut line).await else {
            break;
        };
        if read == 0 || line.len() > MAX_FRAME_BYTES + 1 {
            break;
        }
        if line.last() == Some(&b'\n') {
            line.pop();
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
            forward_office_event(&hub, event, &value);
        }
    }
    if let Ok(mut map) = pending.lock() {
        map.clear();
    }
    evict_channel(&channels, &office_id, &channel_id);
}

pub(super) fn forward_office_event(hub: &EventHub, event: &str, frame: &Value) {
    let data = frame.get("data").cloned().unwrap_or(Value::Null);
    if let Some(org_id) = data.get("organization_id").and_then(Value::as_str) {
        let seq = frame.get("seq").and_then(Value::as_u64).unwrap_or(0);
        hub.publish(HubEvent::OrganizationChanged {
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
