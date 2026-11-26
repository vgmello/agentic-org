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
    org::{ProjectCreate, ProjectCreateParams},
    projects::{
        ProjectInspect, ProjectInspectParams, ProjectList, ProjectListParams, ProjectListResult,
        ProjectRecord,
    },
    web::org_mutations::OrgMutation,
    ApiRange, Empty, Hello, HelloParams, HelloResult, OperationGet, OperationGetParams,
    OperationKind as NativeOperationKind, OperationOutcome, OrgSnapshot, OrgSnapshotCall, Outcome,
    Principal, Request, Response, MAX_FRAME_BYTES,
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

mod channel;

use channel::{evict_channel, negotiate};

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

const MAX_ISOLATED_COLLECT_CHANNELS: usize = 2;

/// Manages async multiplexed host-opened Web channels to offices.
#[derive(Clone)]
pub struct OfficeBridge {
    host: Arc<HostControl>,
    channels: Arc<Mutex<BTreeMap<String, Arc<MultiplexedChannel>>>>,
    open_locks: Arc<Mutex<BTreeMap<String, Arc<tokio::sync::Mutex<()>>>>>,
    isolated_slots: Arc<tokio::sync::Semaphore>,
    hub: EventHub,
    next_id: Arc<AtomicU64>,
}

impl OfficeBridge {
    pub fn new(host: Arc<HostControl>, hub: EventHub) -> Self {
        Self {
            host,
            channels: Arc::new(Mutex::new(BTreeMap::new())),
            open_locks: Arc::new(Mutex::new(BTreeMap::new())),
            isolated_slots: Arc::new(tokio::sync::Semaphore::new(MAX_ISOLATED_COLLECT_CHANNELS)),
            hub,
            next_id: Arc::new(AtomicU64::new(2)),
        }
    }

    fn next_rpc_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    fn open_lock_for(&self, office_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.open_locks.lock().unwrap_or_else(|e| e.into_inner());
        Arc::clone(
            locks
                .entry(office_id.to_owned())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
        )
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
        // Coalesce concurrent cold channel opens per office so parallel callers share one Web slot.
        let office_lock = self.open_lock_for(office_id);
        let _guard = office_lock.lock().await;
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

    /// Sends one typed `org-proto` RPC over the authenticated host channel to `office_id`.
    /// Reads may pass `retry_on_transport = true` to retry once after evicting a stale channel;
    /// mutations pass `false` so an uncertain transport failure is never auto-retried blindly.
    pub async fn call_typed<C: org_proto::Call>(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
        params: &C::Params,
        retry_on_transport: bool,
    ) -> Result<C::Result, OfficeFailure> {
        self.call_typed_with_timeout::<C>(
            auth,
            office_id,
            params,
            retry_on_transport,
            OFFICE_DEADLINE,
        )
        .await
    }

    /// Sends one typed `org-proto` RPC with an explicit reply deadline (used by push-woken
    /// `chat.events_poll` waiters so long-held waits do not expire at `OFFICE_DEADLINE`, `TASK-140`).
    pub async fn call_typed_with_timeout<C: org_proto::Call>(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
        params: &C::Params,
        retry_on_transport: bool,
        deadline: Duration,
    ) -> Result<C::Result, OfficeFailure> {
        match self
            .call_typed_once::<C>(auth, office_id, params, deadline)
            .await
        {
            Err(OfficeFailure::Transport) if retry_on_transport => {
                self.call_typed_once::<C>(auth, office_id, params, deadline)
                    .await
            }
            other => other,
        }
    }

    /// Sends one typed `org-proto` RPC over a bounded dedicated one-shot host channel (capped at
    /// `MAX_ISOLATED_COLLECT_CHANNELS = 2` so ordinary Web channels always retain capacity).
    /// Runs the channel roundtrip in a spawned task so the permit stays held until the office
    /// finishes even if the caller drops the HTTP request early.
    pub async fn call_typed_isolated<C>(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
        params: &C::Params,
    ) -> Result<C::Result, OfficeFailure>
    where
        C: org_proto::Call,
        C::Params: Clone + Send + Sync + 'static,
        C::Result: Send + 'static,
    {
        let Ok(permit) = Arc::clone(&self.isolated_slots).try_acquire_owned() else {
            return Err(OfficeFailure::Refused(org_proto::ApiError::new(
                org_proto::ErrorCode::Conflict,
                "too many concurrent subscription usage collections in progress",
            )));
        };
        let bridge = self.clone();
        let auth = auth.clone();
        let office_id = office_id.to_owned();
        let params = params.clone();
        tokio::spawn(async move {
            bridge
                .call_typed_isolated_inner::<C>(&auth, &office_id, &params, permit)
                .await
        })
        .await
        .map_err(|_| OfficeFailure::Transport)?
    }

    async fn call_typed_isolated_inner<C: org_proto::Call>(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
        params: &C::Params,
        permit: tokio::sync::OwnedSemaphorePermit,
    ) -> Result<C::Result, OfficeFailure> {
        let (channel_id, stream) = self
            .host
            .open_office_channel_async(auth.clone(), office_id.to_owned())
            .await
            .map_err(OfficeFailure::BeforeDispatch)?;
        let isolated_channels = Arc::new(Mutex::new(BTreeMap::new()));
        // Move `permit` into the socket's read loop so the admission slot stays held until the
        // office physically closes its end of the channel (EOF), even on early reply timeout.
        let channel = MultiplexedChannel::from_std_with_permit(
            office_id.to_owned(),
            channel_id.clone(),
            stream,
            isolated_channels,
            self.hub.clone(),
            Some(permit),
        )
        .map_err(|_| OfficeFailure::BeforeDispatch(Failure::Unavailable))?;
        if negotiate(&channel).await.is_err() {
            return Err(OfficeFailure::BeforeDispatch(Failure::Unavailable));
        }
        let principal = match self.grant(auth, office_id, &channel.channel_id).await {
            Ok(principal) => principal,
            Err(Failure::Unavailable) => return Err(OfficeFailure::Transport),
            Err(error) => return Err(OfficeFailure::BeforeDispatch(error)),
        };
        let rpc_id = self.next_rpc_id();
        let request = Request::new::<C>(rpc_id, params)
            .and_then(|req| req.with_principal(&principal))
            .map_err(|_| OfficeFailure::BeforeDispatch(Failure::Unavailable))?;
        let raw = channel
            .roundtrip_raw(&request, rpc_id)
            .await
            .map_err(|_| OfficeFailure::Transport)?;
        let response: Response =
            serde_json::from_value(raw).map_err(|_| OfficeFailure::Transport)?;
        match response.outcome {
            Outcome::Result(value) => {
                serde_json::from_value(value).map_err(|_| OfficeFailure::Transport)
            }
            Outcome::Error(error) => Err(OfficeFailure::Refused(error)),
        }
    }

    async fn call_typed_once<C: org_proto::Call>(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
        params: &C::Params,
        deadline: Duration,
    ) -> Result<C::Result, OfficeFailure> {
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
        let request = Request::new::<C>(rpc_id, params)
            .and_then(|req| req.with_principal(&principal))
            .map_err(|_| OfficeFailure::BeforeDispatch(Failure::Unavailable))?;
        let raw = match channel
            .roundtrip_raw_with_timeout(&request, rpc_id, deadline)
            .await
        {
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

    /// Reads `operation.get` (`kind: org`) asynchronously over the multiplexed office channel.
    /// If a cached channel went stale (e.g., office restart), evicts and retries once.
    pub async fn org_operation_get(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
        operation_id: &str,
    ) -> Result<OperationOutcome, OfficeFailure> {
        match self
            .org_operation_get_once(auth, office_id, operation_id)
            .await
        {
            Err(OfficeFailure::Transport) => {
                self.org_operation_get_once(auth, office_id, operation_id)
                    .await
            }
            other => other,
        }
    }

    async fn org_operation_get_once(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
        operation_id: &str,
    ) -> Result<OperationOutcome, OfficeFailure> {
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
        let params = OperationGetParams {
            kind: NativeOperationKind::Org,
            id: operation_id.to_owned(),
        };
        let request = Request::new::<OperationGet>(rpc_id, &params)
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

    /// Lists projects asynchronously over the multiplexed office channel (`project.list`).
    pub async fn projects_list(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
        include_archived: bool,
    ) -> Result<ProjectListResult, OfficeFailure> {
        match self
            .projects_list_once(auth, office_id, include_archived)
            .await
        {
            Err(OfficeFailure::Transport) => {
                self.projects_list_once(auth, office_id, include_archived)
                    .await
            }
            other => other,
        }
    }

    async fn projects_list_once(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
        include_archived: bool,
    ) -> Result<ProjectListResult, OfficeFailure> {
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
        let params = ProjectListParams { include_archived };
        let request = Request::new::<ProjectList>(rpc_id, &params)
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

    /// Inspects one project by ID, `P-<number>`, or slug (`project.inspect`).
    pub async fn project_inspect(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
        project: &str,
    ) -> Result<ProjectRecord, OfficeFailure> {
        match self.project_inspect_once(auth, office_id, project).await {
            Err(OfficeFailure::Transport) => {
                self.project_inspect_once(auth, office_id, project).await
            }
            other => other,
        }
    }

    async fn project_inspect_once(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
        project: &str,
    ) -> Result<ProjectRecord, OfficeFailure> {
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
        let params = ProjectInspectParams {
            project: project.to_owned(),
        };
        let request = Request::new::<ProjectInspect>(rpc_id, &params)
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

    /// Dispatches `project.create` asynchronously over the multiplexed office channel.
    pub async fn project_create(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
        params: &ProjectCreateParams,
    ) -> Result<Option<Value>, OfficeFailure> {
        let (channel, principal) = self.channel_and_grant(auth, office_id).await?;
        let rpc_id = self.next_rpc_id();
        let request = Request::new::<ProjectCreate>(rpc_id, params)
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

#[cfg(test)]
mod tests;
