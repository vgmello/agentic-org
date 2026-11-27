//! Consumer-owned transport and event-bus ports for `agentic-org-web` (`TASK-232`).
//!
//! HTTP route handlers (`api/*`), process lifecycle orchestration (`process`), and live SSE
//! streams (`sse/*`) depend strictly on [`ControlPlaneTransport`], [`OfficeTransport`], and
//! [`EventBus`] — never on concrete Unix sockets or `tokio::sync::broadcast::Sender` internals.
//!
//! The sole production adapters are the local-Unix host/office transports
//! ([`crate::host::LocalUnixHostTransport`], [`crate::office::LocalUnixOfficeTransport`]) and
//! [`InMemoryEventBus`] wrapping [`EventHub`].

use std::future::Future;
use std::io::{self, Write};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use org_host_control::{ops, organization::BrowserAuth};
use org_proto::{
    org::{ProjectCreate, ProjectCreateParams},
    projects::{
        ProjectInspect, ProjectInspectParams, ProjectList, ProjectListParams, ProjectListResult,
        ProjectRecord,
    },
    web::org_mutations::OrgMutation,
    Call, Empty, OperationGet, OperationGetParams, OperationKind as NativeOperationKind,
    OperationOutcome, OrgSnapshot, OrgSnapshotCall,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use zeroize::Zeroizing;

pub use crate::event_hub::{
    observe_host_event_on_bus, EventBus, EventHub, EventRecvError, EventSubscription, HubEvent,
    InMemoryEventBus,
};

#[cfg(test)]
#[path = "ports_tests.rs"]
mod tests;

/// Maximum wire frame size for control-plane requests and replies.
pub(crate) const MAX_CONTROL_BYTES: usize = org_proto::MAX_FRAME_BYTES;

/// Default deadline for an authenticated browser-to-office RPC.
pub const OFFICE_DEADLINE: Duration = Duration::from_secs(5);

/// Object-safe boxed future returned by async port methods.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Control-plane call failure mapped from host errors or transport loss.
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

/// Typed reply hook passed to async control-plane calls.
pub type ReplyHook<T> = Box<dyn FnOnce(&Result<T, Failure>) + Send>;

/// Raw zero-copy reply payload passed to a control-plane reader-thread hook.
pub enum ControlPayloadRef<'a> {
    /// Complete control-plane wire frame (`{"id", "result" | "error"}`).
    Frame { id: u64, bytes: &'a [u8] },
    /// Standalone JSON `result` bytes (for in-memory test doubles).
    Result(&'a [u8]),
}

impl ControlPayloadRef<'_> {
    pub(crate) fn decode<T: DeserializeOwned>(&self, desync: &dyn Fn()) -> Result<T, Failure> {
        match self {
            Self::Frame { id, bytes } => decode_control_frame(bytes, *id, desync),
            Self::Result(bytes) => serde_json::from_slice(bytes).map_err(|_| Failure::Unavailable),
        }
    }
}

/// Raw reply hook executed on the control-plane reader thread before the next frame is applied.
pub type RawReplyHook = Box<dyn FnOnce(&Result<ControlPayloadRef<'_>, Failure>) + Send>;

/// Zeroizing control-plane call payload returned by [`ControlPlaneTransport`].
pub enum ControlPayload {
    /// Complete control-plane wire frame (`{"id", "result" | "error"}`) in a zeroizing buffer.
    Frame { id: u64, bytes: Zeroizing<Vec<u8>> },
    /// Standalone JSON `result` bytes in a zeroizing buffer (for in-memory test doubles).
    Result(Zeroizing<Vec<u8>>),
}

impl ControlPayload {
    /// Wraps a serializable JSON result value in a zeroizing payload buffer.
    pub fn from_result<T: Serialize>(value: &T) -> Result<Self, Failure> {
        encode_control_params(value).map(Self::Result)
    }

    pub(crate) fn decode<T: DeserializeOwned>(&self, desync: &dyn Fn()) -> Result<T, Failure> {
        match self {
            Self::Frame { id, bytes } => decode_control_frame(bytes, *id, desync),
            Self::Result(bytes) => serde_json::from_slice(bytes).map_err(|_| Failure::Unavailable),
        }
    }
}

/// Consumer-owned port for the admitted web-to-host control-plane connection (`TASK-232`).
///
/// The sole production adapter is [`crate::host::LocalUnixHostTransport`] (`HostControl`);
/// test doubles may implement this trait cleanly in memory without creating Unix sockets.
pub trait ControlPlaneTransport: Send + Sync {
    /// Instance ID authenticated during handoff.
    fn instance(&self) -> &str;
    /// Monotonic connection generation used to scope cached browser sessions.
    fn generation(&self) -> u64;
    /// Whether the underlying host connection is still live.
    fn live(&self) -> bool;
    /// Ends the connection: `live()` becomes false and serving stops.
    fn close(&self);
    /// Sends one synchronous control-plane call with zeroizing JSON `params_json` bytes.
    /// When `hook` is `Some`, it runs in frame arrival order before subsequent host events.
    fn call_raw(
        &self,
        method: &'static str,
        params_json: Zeroizing<Vec<u8>>,
        hook: Option<RawReplyHook>,
    ) -> Result<ControlPayload, Failure>;
    /// Sends one non-blocking async control-plane call with zeroizing JSON `params_json` bytes.
    fn call_raw_async(
        &self,
        method: &'static str,
        params_json: Zeroizing<Vec<u8>>,
        hook: Option<RawReplyHook>,
    ) -> BoxFuture<'_, Result<ControlPayload, Failure>>;
}

impl dyn ControlPlaneTransport + '_ {
    /// Sends one typed synchronous host call.
    pub fn request<C: Call>(&self, params: &C::Params) -> Result<C::Result, Failure> {
        let encoded = encode_control_params(params)?;
        let payload = self.call_raw(C::METHOD, encoded, None)?;
        payload.decode(&|| self.close())
    }

    /// Sends one typed synchronous host call with a reader-thread reply hook.
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
        let payload = self.call_raw(C::METHOD, encoded, Some(raw_hook))?;
        payload.decode(&|| self.close())
    }

    /// Sends one typed non-blocking async host call.
    pub async fn request_async<C: Call>(&self, params: &C::Params) -> Result<C::Result, Failure>
    where
        C::Result: 'static,
    {
        self.request_then_async::<C>(params, None).await
    }

    /// Sends one typed non-blocking async host call with an optional reader-thread reply hook.
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
        let payload = self.call_raw_async(C::METHOD, encoded, raw_hook).await?;
        payload.decode(&|| self.close())
    }

    /// Configuration for the HTTP/HTTPS listener fetched on the admitted connection.
    pub fn fetch_listener(&self) -> Result<ops::TlsFetchResult, Failure> {
        self.request::<ops::TlsFetch>(&Default::default())
    }

    /// The one `web.serving` readiness notification sent after HTTP setup.
    pub fn report_serving(&self) -> Result<(), Failure> {
        self.request::<ops::WebServing>(&Default::default())
            .map(|_| ())
    }
}

/// Failure from an authenticated browser-to-office RPC.
#[derive(Debug)]
pub enum OfficeFailure {
    /// Failed before writing to the office channel (auth, missing office, or host/channel setup).
    BeforeDispatch(Failure),
    /// The office refused a request with an `ApiError`.
    Refused(org_proto::ApiError),
    /// Transport or protocol failure on the office stream.
    Transport,
}

/// Execution options for an [`OfficeTransport::call_raw`] RPC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfficeCallOptions {
    /// Retry once after evicting a stale cached channel on transport loss (reads only).
    pub retry_on_transport: bool,
    /// Per-attempt reply deadline.
    pub deadline: Duration,
    /// Use a bounded dedicated one-shot channel rather than the shared multiplexed channel.
    pub isolated: bool,
}

impl OfficeCallOptions {
    pub const fn multiplexed(retry_on_transport: bool, deadline: Duration) -> Self {
        Self {
            retry_on_transport,
            deadline,
            isolated: false,
        }
    }

    pub const fn isolated() -> Self {
        Self {
            retry_on_transport: false,
            deadline: OFFICE_DEADLINE,
            isolated: true,
        }
    }
}

/// Consumer-owned port for authenticated browser-to-office RPCs (`TASK-232`).
///
/// The sole production adapter is [`crate::office::LocalUnixOfficeTransport`] (`OfficeBridge`);
/// test doubles may implement these two methods cleanly in memory without Unix sockets.
pub trait OfficeTransport: Send + Sync {
    /// Sends one authenticated office RPC (`method`, `params`) and returns its JSON `result`.
    fn call_raw<'a>(
        &'a self,
        auth: &'a BrowserAuth,
        office_id: &'a str,
        method: &'static str,
        params: Value,
        options: OfficeCallOptions,
    ) -> BoxFuture<'a, Result<Value, OfficeFailure>>;

    /// Dispatches one office mutation (`method`, `params`) without auto-retry on transport loss.
    ///
    /// Returns `Err(OfficeFailure::BeforeDispatch(_))` only when nothing could have been written;
    /// returns `Ok(None)` on transport failure after possible dispatch (`504 unknown`); returns
    /// `Ok(Some(raw_response_envelope))` when the office answered.
    fn dispatch_mutation_raw<'a>(
        &'a self,
        auth: &'a BrowserAuth,
        office_id: &'a str,
        method: &'a str,
        params: Value,
    ) -> BoxFuture<'a, Result<Option<Value>, OfficeFailure>>;
}

impl dyn OfficeTransport + '_ {
    /// Sends one typed `org-proto` RPC with the default office deadline.
    pub async fn call_typed<C: Call>(
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

    /// Sends one typed `org-proto` RPC with an explicit reply deadline.
    pub async fn call_typed_with_timeout<C: Call>(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
        params: &C::Params,
        retry_on_transport: bool,
        deadline: Duration,
    ) -> Result<C::Result, OfficeFailure> {
        let params = serde_json::to_value(params)
            .map_err(|_| OfficeFailure::BeforeDispatch(Failure::Unavailable))?;
        let value = self
            .call_raw(
                auth,
                office_id,
                C::METHOD,
                params,
                OfficeCallOptions::multiplexed(retry_on_transport, deadline),
            )
            .await?;
        serde_json::from_value(value).map_err(|_| OfficeFailure::Transport)
    }

    /// Sends one typed `org-proto` RPC over a bounded dedicated one-shot office channel.
    pub async fn call_typed_isolated<C: Call>(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
        params: &C::Params,
    ) -> Result<C::Result, OfficeFailure> {
        let params = serde_json::to_value(params)
            .map_err(|_| OfficeFailure::BeforeDispatch(Failure::Unavailable))?;
        let value = self
            .call_raw(
                auth,
                office_id,
                C::METHOD,
                params,
                OfficeCallOptions::isolated(),
            )
            .await?;
        serde_json::from_value(value).map_err(|_| OfficeFailure::Transport)
    }

    /// Reads `org.snapshot` over the office transport (retries once on stale channel).
    pub async fn org_snapshot(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
    ) -> Result<OrgSnapshot, OfficeFailure> {
        self.call_typed::<OrgSnapshotCall>(auth, office_id, &Empty {}, true)
            .await
    }

    /// Reads `operation.get` (`kind: org`) over the office transport.
    pub async fn org_operation_get(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
        operation_id: &str,
    ) -> Result<OperationOutcome, OfficeFailure> {
        let params = OperationGetParams {
            kind: NativeOperationKind::Org,
            id: operation_id.to_owned(),
        };
        self.call_typed::<OperationGet>(auth, office_id, &params, true)
            .await
    }

    /// Lists projects (`project.list`) over the office transport.
    pub async fn projects_list(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
        include_archived: bool,
    ) -> Result<ProjectListResult, OfficeFailure> {
        self.call_typed::<ProjectList>(
            auth,
            office_id,
            &ProjectListParams { include_archived },
            true,
        )
        .await
    }

    /// Inspects one project (`project.inspect`) over the office transport.
    pub async fn project_inspect(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
        project: &str,
    ) -> Result<ProjectRecord, OfficeFailure> {
        let params = ProjectInspectParams {
            project: project.to_owned(),
        };
        self.call_typed::<ProjectInspect>(auth, office_id, &params, true)
            .await
    }

    /// Dispatches `project.create` over the office transport.
    pub async fn project_create(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
        params: &ProjectCreateParams,
    ) -> Result<Option<Value>, OfficeFailure> {
        let params = serde_json::to_value(params)
            .map_err(|_| OfficeFailure::BeforeDispatch(Failure::Unavailable))?;
        self.dispatch_mutation_raw(auth, office_id, ProjectCreate::METHOD, params)
            .await
    }

    /// Dispatches one validated organization mutation over the office transport.
    pub async fn org_mutation(
        &self,
        auth: &BrowserAuth,
        office_id: &str,
        mutation: &OrgMutation,
    ) -> Result<Option<Value>, OfficeFailure> {
        let request = mutation
            .native_request(0)
            .map_err(|_| OfficeFailure::BeforeDispatch(Failure::Unavailable))?;
        self.dispatch_mutation_raw(auth, office_id, &request.method, request.params)
            .await
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ControlResponse<T> {
    id: u64,
    result: Option<T>,
    error: Option<org_proto::ApiError>,
}

/// Decodes one control-plane response frame (`{"id", "result" | "error"}`).
/// `desync` runs when the reply answers another request id: replies are no
/// longer paired with requests, so the connection must not be reused.
pub(crate) fn decode_control_frame<T: DeserializeOwned>(
    bytes: &[u8],
    id: u64,
    desync: &dyn Fn(),
) -> Result<T, Failure> {
    if bytes.iter().find(|b| !b.is_ascii_whitespace()) != Some(&b'{') {
        return Err(Failure::Unavailable);
    }
    let reply: ControlResponse<T> =
        serde_json::from_slice(bytes).map_err(|_| Failure::Unavailable)?;
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

struct FrameCounter(usize);

impl Write for FrameCounter {
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

/// Serializes control-plane parameters directly into a bounded zeroizing JSON buffer.
pub(crate) fn encode_control_params<T: Serialize>(
    value: &T,
) -> Result<Zeroizing<Vec<u8>>, Failure> {
    let mut counter = FrameCounter(0);
    serde_json::to_writer(&mut counter, value).map_err(|_| Failure::Unavailable)?;
    let mut bytes = Zeroizing::new(Vec::with_capacity(counter.0));
    serde_json::to_writer(&mut *bytes, value).map_err(|_| Failure::Unavailable)?;
    Ok(bytes)
}

/// Assembles one newline-delimited JSON-RPC control frame from pre-encoded zeroizing params.
pub(crate) fn encode_control_request_frame(
    id: u64,
    method: &str,
    params_json: Zeroizing<Vec<u8>>,
) -> Result<Zeroizing<Vec<u8>>, Failure> {
    let method_json = serde_json::to_string(method).map_err(|_| Failure::Unavailable)?;
    let id_str = id.to_string();
    let json_len = 6usize
        .checked_add(id_str.len())
        .and_then(|n| n.checked_add(10))
        .and_then(|n| n.checked_add(method_json.len()))
        .and_then(|n| n.checked_add(10))
        .and_then(|n| n.checked_add(params_json.len()))
        .and_then(|n| n.checked_add(1))
        .filter(|n| *n < MAX_CONTROL_BYTES)
        .ok_or(Failure::Unavailable)?;
    let mut bytes = Zeroizing::new(Vec::with_capacity(json_len + 1));
    bytes.extend_from_slice(br#"{"id":"#);
    bytes.extend_from_slice(id_str.as_bytes());
    bytes.extend_from_slice(br#","method":"#);
    bytes.extend_from_slice(method_json.as_bytes());
    bytes.extend_from_slice(br#","params":"#);
    bytes.extend_from_slice(&params_json);
    bytes.extend_from_slice(b"}\n");
    debug_assert_eq!(bytes.len(), json_len + 1);
    Ok(bytes)
}
