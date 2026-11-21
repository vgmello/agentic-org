//! Async multiplexed browser-to-office bridge (B2 Q8, TASK-16): opens host-issued
//! Web channels to offices, negotiates `org-proto` v2 `hello`, attaches per-request
//! HMAC grants minted by the host, and multiplexes concurrent reads and mutations
//! over Tokio `UnixStream` tasks without touching the host `BlockingPool` or any
//! office database/key.
use std::{
    collections::BTreeMap,
    io,
    os::unix::net::UnixStream as StdUnixStream,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use org_host_control::{
    browser::{BrowserSessionGrant, BrowserSessionGrantParams},
    organization::BrowserAuth,
};
use org_proto::{
    gateway::org_mutations::OrgMutation, ApiRange, Empty, Hello, HelloParams, HelloResult,
    OrgSnapshot, OrgSnapshotCall, Outcome, Principal, Request, Response, MAX_FRAME_BYTES,
};
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    sync::{mpsc, oneshot},
    time,
};

use crate::{
    host::{Failure, HostControl},
    sse::{EventHub, HubEvent},
};

const OFFICE_DEADLINE: Duration = Duration::from_secs(5);
const CHANNEL_WRITE_QUEUE: usize = 64;

#[derive(Debug)]
pub enum OfficeFailure {
    /// Failed before writing to the office channel (auth, missing office, or host/channel setup).
    BeforeDispatch(Failure),
    /// The office refused a read request with an `ApiError`.
    Refused(org_proto::ApiError),
    /// Transport or protocol failure on the office stream.
    Transport,
}

type PendingMap = Arc<Mutex<BTreeMap<u64, oneshot::Sender<Value>>>>;

struct MultiplexedChannel {
    channel_id: String,
    writer: mpsc::Sender<Vec<u8>>,
    pending: PendingMap,
}

impl MultiplexedChannel {
    fn from_std(
        office_id: String,
        channel_id: String,
        stream: StdUnixStream,
        channels: Arc<Mutex<BTreeMap<String, Arc<Self>>>>,
        hub: EventHub,
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
            read_half, pending, channels, office_id, channel_id, hub,
        ));
        Ok(channel)
    }

    async fn roundtrip_raw(&self, request: &Request, rpc_id: u64) -> io::Result<Value> {
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
        if self.writer.send(bytes).await.is_err() {
            if let Ok(mut map) = self.pending.lock() {
                map.remove(&rpc_id);
            }
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "office writer closed",
            ));
        }
        match time::timeout(OFFICE_DEADLINE, rx).await {
            Ok(Ok(reply)) => Ok(reply),
            _ => {
                if let Ok(mut map) = self.pending.lock() {
                    map.remove(&rpc_id);
                }
                Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "office reply unavailable",
                ))
            }
        }
    }
}

/// Manages async multiplexed host-opened Web channels to offices.
pub struct OfficeBridge {
    host: Arc<HostControl>,
    channels: Arc<Mutex<BTreeMap<String, Arc<MultiplexedChannel>>>>,
    hub: EventHub,
    next_id: AtomicU64,
}

impl OfficeBridge {
    pub fn new(host: Arc<HostControl>, hub: EventHub) -> Self {
        Self {
            host,
            channels: Arc::new(Mutex::new(BTreeMap::new())),
            hub,
            next_id: AtomicU64::new(2),
        }
    }

    fn next_rpc_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    async fn get_or_open(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
    ) -> Result<Arc<MultiplexedChannel>, OfficeFailure> {
        if let Ok(channels) = self.channels.lock() {
            if let Some(existing) = channels.get(office_id) {
                return Ok(Arc::clone(existing));
            }
        }
        let (channel_id, stream) = self
            .host
            .open_office_channel_async(auth.clone(), office_id.to_owned())
            .await
            .map_err(OfficeFailure::BeforeDispatch)?;
        let channel = MultiplexedChannel::from_std(
            office_id.to_owned(),
            channel_id.clone(),
            stream,
            Arc::clone(&self.channels),
            self.hub.clone(),
        )
        .map_err(|_| OfficeFailure::BeforeDispatch(Failure::Unavailable))?;
        if negotiate(&channel).await.is_err() {
            return Err(OfficeFailure::BeforeDispatch(Failure::Unavailable));
        }
        if let Ok(mut channels) = self.channels.lock() {
            channels.insert(office_id.to_owned(), Arc::clone(&channel));
        }
        Ok(channel)
    }

    fn evict(&self, office_id: &str, channel_id: &str) {
        evict_channel(&self.channels, office_id, channel_id);
    }

    async fn grant(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
        channel_id: &str,
    ) -> Result<Principal, Failure> {
        let granted = self
            .host
            .request_async::<BrowserSessionGrant>(&BrowserSessionGrantParams {
                auth: auth.clone(),
                office_id: office_id.to_owned(),
                channel_id: channel_id.to_owned(),
            })
            .await?;
        Ok(Principal {
            grant: granted.grant,
        })
    }

    /// Reads `org.snapshot` asynchronously over the multiplexed office channel.
    /// If a cached channel went stale (e.g., office restart), evicts and retries once.
    pub async fn org_snapshot(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
    ) -> Result<OrgSnapshot, OfficeFailure> {
        match self.org_snapshot_once(auth, office_id).await {
            Err(OfficeFailure::Transport) => self.org_snapshot_once(auth, office_id).await,
            other => other,
        }
    }

    async fn org_snapshot_once(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
    ) -> Result<OrgSnapshot, OfficeFailure> {
        let channel = self.get_or_open(auth, office_id).await?;
        let principal = match self.grant(auth, office_id, &channel.channel_id).await {
            Ok(principal) => principal,
            Err(Failure::Unavailable) => {
                self.evict(office_id, &channel.channel_id);
                return Err(OfficeFailure::Transport);
            }
            Err(error) => return Err(OfficeFailure::BeforeDispatch(error)),
        };
        let rpc_id = self.next_rpc_id();
        let request = Request::new::<OrgSnapshotCall>(rpc_id, &Empty {})
            .and_then(|req| req.with_principal(&principal))
            .map_err(|_| OfficeFailure::BeforeDispatch(Failure::Unavailable))?;
        let raw = match channel.roundtrip_raw(&request, rpc_id).await {
            Ok(raw) => raw,
            Err(_) => {
                self.evict(office_id, &channel.channel_id);
                return Err(OfficeFailure::Transport);
            }
        };
        let response: Response =
            serde_json::from_value(raw).map_err(|_| OfficeFailure::Transport)?;
        match response.outcome {
            Outcome::Result(value) => {
                serde_json::from_value(value).map_err(|_| OfficeFailure::Transport)
            }
            Outcome::Error(error) => Err(OfficeFailure::Refused(error)),
        }
    }

    /// Dispatches one validated organization mutation asynchronously. Returns
    /// `Err(BeforeDispatch)` only when nothing could have been written; returns
    /// `Ok(None)` on transport failure after possible dispatch (`504 unknown`).
    pub async fn org_mutation(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
        mutation: &OrgMutation,
    ) -> Result<Option<Value>, OfficeFailure> {
        let (channel, principal) = self.channel_and_grant(auth, office_id).await?;
        let rpc_id = self.next_rpc_id();
        let request = mutation
            .native_request(rpc_id)
            .and_then(|req| req.with_principal(&principal))
            .map_err(|_| OfficeFailure::BeforeDispatch(Failure::Unavailable))?;
        match channel.roundtrip_raw(&request, rpc_id).await {
            Ok(reply) => Ok(Some(reply)),
            Err(_) => {
                self.evict(office_id, &channel.channel_id);
                Ok(None)
            }
        }
    }

    async fn channel_and_grant(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
    ) -> Result<(Arc<MultiplexedChannel>, Principal), OfficeFailure> {
        let channel = self.get_or_open(auth, office_id).await?;
        match self.grant(auth, office_id, &channel.channel_id).await {
            Ok(principal) => Ok((channel, principal)),
            Err(Failure::Unavailable) => {
                self.evict(office_id, &channel.channel_id);
                let fresh = self.get_or_open(auth, office_id).await?;
                let principal = self
                    .grant(auth, office_id, &fresh.channel_id)
                    .await
                    .map_err(OfficeFailure::BeforeDispatch)?;
                Ok((fresh, principal))
            }
            Err(error) => Err(OfficeFailure::BeforeDispatch(error)),
        }
    }
}

async fn negotiate(channel: &MultiplexedChannel) -> io::Result<()> {
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

async fn write_loop(
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

async fn read_loop(
    read_half: tokio::net::unix::OwnedReadHalf,
    pending: PendingMap,
    channels: Arc<Mutex<BTreeMap<String, Arc<MultiplexedChannel>>>>,
    office_id: String,
    channel_id: String,
    hub: EventHub,
) {
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

fn forward_office_event(hub: &EventHub, event: &str, frame: &Value) {
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

fn evict_channel(
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
    use super::*;
    use std::io::{BufRead, Write};

    #[test]
    fn multiplexed_office_channel_correlates_out_of_order_concurrent_replies() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let (client, server) = StdUnixStream::pair().unwrap();
            let peer = std::thread::spawn(move || {
                let mut reader = std::io::BufReader::new(server.try_clone().unwrap());
                let mut writer = server;
                let mut lines = Vec::new();
                for _ in 0..2 {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    let req: Value = serde_json::from_str(&line).unwrap();
                    lines.push(req["id"].as_u64().unwrap());
                }
                // Reply out of order (second request first) to prove async correlation.
                for id in lines.into_iter().rev() {
                    let reply = serde_json::json!({ "id": id, "result": { "echo": id } });
                    writeln!(writer, "{reply}").unwrap();
                }
                writer.flush().unwrap();
            });
            let channels = Arc::new(Mutex::new(BTreeMap::new()));
            let channel = MultiplexedChannel::from_std(
                "office-1".into(),
                "ch-1".into(),
                client,
                channels,
                EventHub::default(),
            )
            .unwrap();
            let ch_a = Arc::clone(&channel);
            let ch_b = Arc::clone(&channel);
            let a = tokio::spawn(async move {
                let req = Request::new::<OrgSnapshotCall>(10, &Empty {}).unwrap();
                ch_a.roundtrip_raw(&req, 10).await.unwrap()
            });
            let b = tokio::spawn(async move {
                let req = Request::new::<OrgSnapshotCall>(11, &Empty {}).unwrap();
                ch_b.roundtrip_raw(&req, 11).await.unwrap()
            });
            assert_eq!(a.await.unwrap()["result"]["echo"], 10);
            assert_eq!(b.await.unwrap()["result"]["echo"], 11);
            peer.join().unwrap();
        });
    }
}
