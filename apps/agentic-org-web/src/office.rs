//! Async multiplexed browser-to-office bridge (B2 Q8, TASK-16, TASK-232): opens host-issued
//! Web channels to offices, negotiates `org-proto` v2 `hello`, attaches per-request
//! HMAC grants minted by the host, and multiplexes concurrent reads and mutations
//! over Tokio stream tasks without touching the host `BlockingPool` or any
//! office database/key.
//!
//! [`OfficeBridge`] (`LocalUnixOfficeTransport`) is the sole production [`OfficeTransport`]
//! adapter. Concrete Unix socket conversion is isolated in [`crate::transport::local_unix`].

use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use org_host_control::{
    browser::{
        BrowserSessionGrant, BrowserSessionGrantParams, BrowserWebhookGrant,
        BrowserWebhookGrantParams,
    },
    organization::BrowserAuth,
};
use org_proto::{Outcome, Principal, Request, Response};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

pub use crate::ports::{OfficeCallOptions, OfficeFailure, OfficeTransport};
use crate::{
    event_hub::EventBus,
    host::{Failure, HostControl},
    ports::BoxFuture,
};

mod channel;

use channel::{evict_channel, negotiate};

const CHANNEL_WRITE_QUEUE: usize = 64;
const MAX_ISOLATED_COLLECT_CHANNELS: usize = 2;

/// The sole production [`OfficeTransport`] adapter over host-transferred local Unix channels.
pub type LocalUnixOfficeTransport = OfficeBridge;

type PendingMap = Arc<Mutex<BTreeMap<u64, oneshot::Sender<Value>>>>;

struct MultiplexedChannel {
    channel_id: String,
    writer: mpsc::Sender<Vec<u8>>,
    pending: PendingMap,
}

/// Manages async multiplexed host-opened Web channels to offices.
#[derive(Clone)]
pub struct OfficeBridge {
    host: Arc<HostControl>,
    channels: Arc<Mutex<BTreeMap<String, Arc<MultiplexedChannel>>>>,
    open_locks: Arc<Mutex<BTreeMap<String, Arc<tokio::sync::Mutex<()>>>>>,
    isolated_slots: Arc<tokio::sync::Semaphore>,
    events: Arc<dyn EventBus>,
    next_id: Arc<AtomicU64>,
}

impl OfficeBridge {
    pub fn new(host: Arc<HostControl>, events: Arc<dyn EventBus>) -> Self {
        Self {
            host,
            channels: Arc::new(Mutex::new(BTreeMap::new())),
            open_locks: Arc::new(Mutex::new(BTreeMap::new())),
            isolated_slots: Arc::new(tokio::sync::Semaphore::new(MAX_ISOLATED_COLLECT_CHANNELS)),
            events,
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
        let channel = MultiplexedChannel::from_local_unix(
            office_id.to_owned(),
            channel_id.clone(),
            stream,
            Arc::clone(&self.channels),
            Arc::clone(&self.events),
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

    async fn call_raw_inner(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
        method: &'static str,
        params: Value,
        options: OfficeCallOptions,
    ) -> Result<Value, OfficeFailure> {
        if options.isolated {
            return self
                .call_isolated_raw(auth, office_id, method, params)
                .await;
        }
        match self
            .call_multiplexed_once(auth, office_id, method, &params, options.deadline)
            .await
        {
            Err(OfficeFailure::Transport) if options.retry_on_transport => {
                self.call_multiplexed_once(auth, office_id, method, &params, options.deadline)
                    .await
            }
            other => other,
        }
    }

    async fn call_isolated_raw(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
        method: &'static str,
        params: Value,
    ) -> Result<Value, OfficeFailure> {
        let Ok(permit) = Arc::clone(&self.isolated_slots).try_acquire_owned() else {
            return Err(OfficeFailure::Refused(org_proto::ApiError::new(
                org_proto::ErrorCode::Conflict,
                "too many concurrent subscription usage collections in progress",
            )));
        };
        let bridge = self.clone();
        let auth = auth.clone();
        let office_id = office_id.to_owned();
        // Run the channel roundtrip in a spawned task so the permit stays held until the office
        // finishes even if the caller drops the HTTP request early.
        tokio::spawn(async move {
            bridge
                .call_isolated_inner(&auth, &office_id, method, params, permit)
                .await
        })
        .await
        .map_err(|_| OfficeFailure::Transport)?
    }

    async fn call_isolated_inner(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
        method: &'static str,
        params: Value,
        permit: tokio::sync::OwnedSemaphorePermit,
    ) -> Result<Value, OfficeFailure> {
        let (channel_id, stream) = self
            .host
            .open_office_channel_async(auth.clone(), office_id.to_owned())
            .await
            .map_err(OfficeFailure::BeforeDispatch)?;
        let isolated_channels = Arc::new(Mutex::new(BTreeMap::new()));
        // Move `permit` into the socket's read loop so the admission slot stays held until the
        // office physically closes its end of the channel (EOF), even on early reply timeout.
        let channel = MultiplexedChannel::from_local_unix_with_permit(
            office_id.to_owned(),
            channel_id.clone(),
            stream,
            isolated_channels,
            Arc::clone(&self.events),
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
        let request = build_request(rpc_id, method, params, &principal)?;
        let raw = channel
            .roundtrip_raw(&request, rpc_id)
            .await
            .map_err(|_| OfficeFailure::Transport)?;
        decode_response(raw)
    }

    async fn call_multiplexed_once(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
        method: &'static str,
        params: &Value,
        deadline: Duration,
    ) -> Result<Value, OfficeFailure> {
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
        let request = build_request(rpc_id, method, params.clone(), &principal)?;
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
        decode_response(raw)
    }

    async fn dispatch_mutation_inner(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
        method: &str,
        params: Value,
    ) -> Result<Option<Value>, OfficeFailure> {
        let (channel, principal) = self.channel_and_grant(auth, office_id).await?;
        let rpc_id = self.next_rpc_id();
        let request = build_request(rpc_id, method, params, &principal)?;
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
    async fn call_webhook_inner(
        &self,
        org_id: &str,
        method: &'static str,
        params: Value,
    ) -> Result<Value, OfficeFailure> {
        let (office_id, channel_id, stream) = self
            .host
            .open_webhook_channel_async(org_id.to_owned())
            .await
            .map_err(OfficeFailure::BeforeDispatch)?;
        let isolated = Arc::new(Mutex::new(BTreeMap::new()));
        let channel = MultiplexedChannel::from_local_unix(
            office_id.clone(),
            channel_id.clone(),
            stream,
            isolated,
            Arc::clone(&self.events),
        )
        .map_err(|_| OfficeFailure::BeforeDispatch(Failure::Unavailable))?;
        if negotiate(&channel).await.is_err() {
            return Err(OfficeFailure::BeforeDispatch(Failure::Unavailable));
        }
        let granted = self
            .host
            .request_async::<BrowserWebhookGrant>(&BrowserWebhookGrantParams {
                org_id: org_id.to_owned(),
                office_id: office_id.clone(),
                channel_id: channel_id.clone(),
            })
            .await
            .map_err(OfficeFailure::BeforeDispatch)?;
        let principal = Principal {
            grant: granted.grant,
        };
        let rpc_id = self.next_rpc_id();
        let request = build_request(rpc_id, method, params, &principal)?;
        let raw = channel
            .roundtrip_raw(&request, rpc_id)
            .await
            .map_err(|_| OfficeFailure::Transport)?;
        decode_response(raw)
    }
}

impl OfficeTransport for OfficeBridge {
    fn call_raw<'a>(
        &'a self,
        auth: &'a BrowserAuth,
        office_id: &'a str,
        method: &'static str,
        params: Value,
        options: OfficeCallOptions,
    ) -> BoxFuture<'a, Result<Value, OfficeFailure>> {
        Box::pin(self.call_raw_inner(auth, office_id, method, params, options))
    }

    fn dispatch_mutation_raw<'a>(
        &'a self,
        auth: &'a BrowserAuth,
        office_id: &'a str,
        method: &'a str,
        params: Value,
    ) -> BoxFuture<'a, Result<Option<Value>, OfficeFailure>> {
        Box::pin(self.dispatch_mutation_inner(auth, office_id, method, params))
    }

    fn call_webhook_raw<'a>(
        &'a self,
        org_id: &'a str,
        method: &'static str,
        params: Value,
    ) -> BoxFuture<'a, Result<Value, OfficeFailure>> {
        Box::pin(self.call_webhook_inner(org_id, method, params))
    }
}

fn build_request(
    rpc_id: u64,
    method: &str,
    params: Value,
    principal: &Principal,
) -> Result<Request, OfficeFailure> {
    Request {
        id: rpc_id,
        method: method.to_owned(),
        principal: None,
        params,
    }
    .with_principal(principal)
    .map_err(|_| OfficeFailure::BeforeDispatch(Failure::Unavailable))
}

fn decode_response(raw: Value) -> Result<Value, OfficeFailure> {
    let response: Response = serde_json::from_value(raw).map_err(|_| OfficeFailure::Transport)?;
    match response.outcome {
        Outcome::Result(value) => Ok(value),
        Outcome::Error(error) => Err(OfficeFailure::Refused(error)),
    }
}

#[cfg(test)]
mod tests;
