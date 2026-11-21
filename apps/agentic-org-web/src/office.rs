//! Browser-to-office bridge (B2 Q8): opens host-issued Web channels to offices,
//! negotiates `org-proto` v2 `hello`, attaches per-request HMAC grants minted by
//! the host, and executes reads and strict mutations without any DB or key access.
use std::{
    collections::BTreeMap,
    io::{self, BufReader, Write},
    os::unix::net::UnixStream,
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
    frame, gateway::org_mutations::OrgMutation, ApiRange, Empty, Hello, HelloParams, HelloResult,
    OrgSnapshot, OrgSnapshotCall, Outcome, Principal, Request, Response,
};

use crate::host::{Failure, HostControl};

const OFFICE_DEADLINE: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub enum OfficeFailure {
    /// Failed before writing to the office channel (auth, missing office, or host/channel setup).
    BeforeDispatch(Failure),
    /// The office refused a read request with an `ApiError`.
    Refused(org_proto::ApiError),
    /// Transport or protocol failure on the office stream.
    Transport,
}

struct OfficeChannel {
    channel_id: String,
    stream: Mutex<UnixStream>,
}

/// Manages host-opened Web channels to offices, one negotiated channel per office.
pub struct OfficeBridge {
    host: Arc<HostControl>,
    channels: Mutex<BTreeMap<String, Arc<OfficeChannel>>>,
    next_id: AtomicU64,
}

impl OfficeBridge {
    pub fn new(host: Arc<HostControl>) -> Self {
        Self {
            host,
            channels: Mutex::new(BTreeMap::new()),
            next_id: AtomicU64::new(2),
        }
    }

    fn next_rpc_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    fn get_or_open(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
    ) -> Result<Arc<OfficeChannel>, OfficeFailure> {
        if let Ok(channels) = self.channels.lock() {
            if let Some(existing) = channels.get(office_id) {
                return Ok(Arc::clone(existing));
            }
        }
        let (channel_id, mut stream) = self
            .host
            .open_office_channel(auth.clone(), office_id)
            .map_err(OfficeFailure::BeforeDispatch)?;
        negotiate(&mut stream).map_err(|_| OfficeFailure::BeforeDispatch(Failure::Unavailable))?;
        let channel = Arc::new(OfficeChannel {
            channel_id,
            stream: Mutex::new(stream),
        });
        if let Ok(mut channels) = self.channels.lock() {
            channels.insert(office_id.to_owned(), Arc::clone(&channel));
        }
        Ok(channel)
    }

    fn evict(&self, office_id: &str, channel_id: &str) {
        if let Ok(mut channels) = self.channels.lock() {
            if channels
                .get(office_id)
                .is_some_and(|c| c.channel_id == channel_id)
            {
                channels.remove(office_id);
            }
        }
    }

    fn grant(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
        channel_id: &str,
    ) -> Result<Principal, Failure> {
        let granted = self
            .host
            .request::<BrowserSessionGrant>(&BrowserSessionGrantParams {
                auth: auth.clone(),
                office_id: office_id.to_owned(),
                channel_id: channel_id.to_owned(),
            })?;
        Ok(Principal {
            grant: granted.grant,
        })
    }

    /// Reads `org.snapshot` from `office_id`. If a cached channel went stale (for
    /// example after an office restart), evicts it and retries once on a fresh channel.
    pub fn org_snapshot(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
    ) -> Result<OrgSnapshot, OfficeFailure> {
        match self.org_snapshot_once(auth, office_id) {
            Err(OfficeFailure::Transport) => self.org_snapshot_once(auth, office_id),
            other => other,
        }
    }

    fn org_snapshot_once(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
    ) -> Result<OrgSnapshot, OfficeFailure> {
        let channel = self.get_or_open(auth, office_id)?;
        let principal = match self.grant(auth, office_id, &channel.channel_id) {
            Ok(principal) => principal,
            Err(Failure::Unavailable) => {
                // Channel slot may have died on host after an office restart.
                self.evict(office_id, &channel.channel_id);
                return Err(OfficeFailure::Transport);
            }
            Err(error) => return Err(OfficeFailure::BeforeDispatch(error)),
        };
        let rpc_id = self.next_rpc_id();
        let request = Request::new::<OrgSnapshotCall>(rpc_id, &Empty {})
            .and_then(|req| req.with_principal(&principal))
            .map_err(|_| OfficeFailure::BeforeDispatch(Failure::Unavailable))?;
        let response = match roundtrip_response(&channel.stream, &request, rpc_id) {
            Ok(response) => response,
            Err(_) => {
                self.evict(office_id, &channel.channel_id);
                return Err(OfficeFailure::Transport);
            }
        };
        match response.outcome {
            Outcome::Result(value) => {
                serde_json::from_value(value).map_err(|_| OfficeFailure::Transport)
            }
            Outcome::Error(error) => Err(OfficeFailure::Refused(error)),
        }
    }

    /// Dispatches one validated organization mutation. Returns `Err(BeforeDispatch)` only
    /// when nothing could have been written to the office socket; returns `Ok(None)` when
    /// transport fails after possible dispatch so the caller reports `504 unknown`.
    pub fn org_mutation(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
        mutation: &OrgMutation,
    ) -> Result<Option<serde_json::Value>, OfficeFailure> {
        let (channel, principal) = self.channel_and_grant(auth, office_id)?;
        let rpc_id = self.next_rpc_id();
        let request = mutation
            .native_request(rpc_id)
            .and_then(|req| req.with_principal(&principal))
            .map_err(|_| OfficeFailure::BeforeDispatch(Failure::Unavailable))?;
        match roundtrip_raw(&channel.stream, &request, rpc_id) {
            Ok(reply) => Ok(Some(reply)),
            Err(_) => {
                self.evict(office_id, &channel.channel_id);
                Ok(None)
            }
        }
    }

    /// Obtains a live channel + grant, retrying once if a cached channel died before
    /// any mutation bytes were written.
    fn channel_and_grant(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
    ) -> Result<(Arc<OfficeChannel>, Principal), OfficeFailure> {
        let channel = self.get_or_open(auth, office_id)?;
        match self.grant(auth, office_id, &channel.channel_id) {
            Ok(principal) => Ok((channel, principal)),
            Err(Failure::Unavailable) => {
                self.evict(office_id, &channel.channel_id);
                let fresh = self.get_or_open(auth, office_id)?;
                let principal = self
                    .grant(auth, office_id, &fresh.channel_id)
                    .map_err(OfficeFailure::BeforeDispatch)?;
                Ok((fresh, principal))
            }
            Err(error) => Err(OfficeFailure::BeforeDispatch(error)),
        }
    }
}

fn negotiate(stream: &mut UnixStream) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(OFFICE_DEADLINE))?;
    stream.set_write_timeout(Some(OFFICE_DEADLINE))?;
    let mut params = HelloParams::default();
    params.api = Some(ApiRange { min: 2, max: 2 });
    params.client = Some(format!("agentic-org-web {}", env!("CARGO_PKG_VERSION")));
    let request = Request::new::<Hello>(1, &params).map_err(io::Error::other)?;
    frame::write_message(stream, &request).map_err(io::Error::other)?;
    stream.flush()?;
    let mut reader = BufReader::new(&*stream);
    let reply: Response = frame::read_message(&mut reader)
        .map_err(io::Error::other)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "office closed on hello"))?;
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

fn roundtrip_response(
    stream: &Mutex<UnixStream>,
    request: &Request,
    rpc_id: u64,
) -> io::Result<Response> {
    let mut guard = stream
        .lock()
        .map_err(|_| io::Error::other("office stream poisoned"))?;
    frame::write_message(&mut *guard, request).map_err(io::Error::other)?;
    guard.flush()?;
    let mut reader = BufReader::new(&*guard);
    let reply: Response = frame::read_message(&mut reader)
        .map_err(io::Error::other)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "office closed stream"))?;
    if reply.id != rpc_id {
        return Err(io::Error::other("office response id mismatch"));
    }
    Ok(reply)
}

fn roundtrip_raw(
    stream: &Mutex<UnixStream>,
    request: &Request,
    rpc_id: u64,
) -> io::Result<serde_json::Value> {
    let mut guard = stream
        .lock()
        .map_err(|_| io::Error::other("office stream poisoned"))?;
    frame::write_message(&mut *guard, request).map_err(io::Error::other)?;
    guard.flush()?;
    let mut reader = BufReader::new(&*guard);
    let reply: serde_json::Value = frame::read_message(&mut reader)
        .map_err(io::Error::other)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "office closed stream"))?;
    if reply.get("id").and_then(serde_json::Value::as_u64) != Some(rpc_id) {
        return Err(io::Error::other("office response id mismatch"));
    }
    Ok(reply)
}
