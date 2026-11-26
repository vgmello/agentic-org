//! Async SSE live-event hub and streaming HTTP body for `/api/v1/events/stream`
//! (web-api-v1 §6, TASK-16, TASK-45 B6). Runs directly on the Tokio runtime with bounded
//! per-stream channels and never occupies the host `BlockingPool`.
use std::{
    collections::BTreeSet,
    convert::Infallible,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use http_body_util::Full;
use hyper::{
    body::{Body, Bytes, Frame, SizeHint},
    header::{HeaderValue, CACHE_CONTROL, CONTENT_TYPE},
    Response, StatusCode,
};
use org_host_control::organization::BrowserAuth;
use org_proto::web::{
    EventFrame, Feed, Milliseconds, ResponseScope, ResumeCursor, Scope, StreamRequest,
    Subscription, SubscriptionCursor,
};
use tokio::sync::{broadcast, mpsc};

pub use crate::event_hub::{EventHub, HubEvent};
use crate::office::OfficeBridge;

mod deliver;
mod office_poll;

use deliver::{deliver_hub_event, deliver_slow_consumer, send_frame};
use office_poll::poll_office_loop;

/// Bounded per-client frame queue; slow consumers receive `resync(slow_consumer)`.
const STREAM_QUEUE: usize = 32;
/// Periodic keepalive heartbeat interval for open SSE streams.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
/// Server-side condvar wait window for push-woken `chat.events_poll` streams (`TASK-140`).
const OFFICE_EVENT_WAIT_MS: u64 = 10_000;
/// RPC deadline for push-woken `chat.events_poll` (exceeds `OFFICE_EVENT_WAIT_MS` with headroom).
const OFFICE_EVENT_RPC_TIMEOUT: Duration = Duration::from_secs(15);
/// Backoff after a transient office channel error before re-subscribing.
const OFFICE_RETRY_BACKOFF: Duration = Duration::from_millis(250);
/// Poll slice used to notice closed client streams promptly without waiting a full heartbeat.
const CLOSE_CHECK_INTERVAL: Duration = Duration::from_millis(200);

/// Unified HTTP response body for `agentic-org-web`: fixed JSON/static bodies and
/// async SSE event streams, with zero heap-boxing overhead.
pub enum WebBody {
    Full(Full<Bytes>),
    Stream(SseBody),
}

impl From<Full<Bytes>> for WebBody {
    fn from(full: Full<Bytes>) -> Self {
        Self::Full(full)
    }
}

impl Body for WebBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        match self.get_mut() {
            Self::Full(full) => Pin::new(full).poll_frame(cx),
            Self::Stream(stream) => Pin::new(stream).poll_frame(cx),
        }
    }

    fn is_end_stream(&self) -> bool {
        match self {
            Self::Full(full) => full.is_end_stream(),
            Self::Stream(stream) => stream.is_end_stream(),
        }
    }

    fn size_hint(&self) -> SizeHint {
        match self {
            Self::Full(full) => full.size_hint(),
            Self::Stream(stream) => stream.size_hint(),
        }
    }
}

/// Bounded async SSE body backed by a Tokio `mpsc::Receiver<Bytes>`.
pub struct SseBody {
    receiver: mpsc::Receiver<Bytes>,
}

impl SseBody {
    fn new(receiver: mpsc::Receiver<Bytes>) -> Self {
        Self { receiver }
    }
}

impl Body for SseBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        match self.receiver.poll_recv(cx) {
            Poll::Ready(Some(chunk)) => Poll::Ready(Some(Ok(Frame::data(chunk)))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// One organization HQ office polled for a live SSE stream (`B6`).
#[derive(Clone)]
pub struct OfficePollTarget {
    pub org_id: String,
    pub office_id: String,
    pub auth: BrowserAuth,
    pub initial_org_seq: Option<u64>,
}

/// Parses an organization resume cursor (`org:<org_id>:<epoch>:<seq>`) into its `seq`.
pub fn parse_org_cursor_seq(cursor: &ResumeCursor, org_id: &str) -> Option<u64> {
    let parts: Vec<&str> = cursor.as_str().split(':').collect();
    if parts.len() == 4 && parts[0] == "org" && parts[1] == org_id {
        parts[3].parse::<u64>().ok()
    } else {
        None
    }
}

/// Spawns an SSE stream task for an authenticated browser session without office polling.
pub fn open_stream(
    hub: &EventHub,
    instance_id: String,
    session_id: String,
    request: StreamRequest,
) -> Response<WebBody> {
    open_stream_with_offices(hub, instance_id, session_id, request, None, Vec::new())
}

/// Spawns an SSE stream task with optional live office polling (`chat.events_poll`).
pub fn open_stream_with_offices(
    hub: &EventHub,
    instance_id: String,
    session_id: String,
    request: StreamRequest,
    offices: Option<OfficeBridge>,
    targets: Vec<OfficePollTarget>,
) -> Response<WebBody> {
    let (tx, rx) = mpsc::channel::<Bytes>(STREAM_QUEUE);
    let mut events = hub.subscribe();
    let subs = request.subscriptions;
    let (merged_tx, merged_rx) = mpsc::channel::<StreamInput>(STREAM_QUEUE);
    let btx = merged_tx.clone();
    tokio::spawn(async move {
        loop {
            if btx.is_closed() {
                break;
            }
            match tokio::time::timeout(CLOSE_CHECK_INTERVAL, events.recv()).await {
                Err(_) => continue,
                Ok(Ok(ev)) => {
                    if btx.send(StreamInput::Event(ev)).await.is_err() {
                        break;
                    }
                }
                Ok(Err(broadcast::error::RecvError::Lagged(_))) => {
                    if btx.send(StreamInput::Lagged).await.is_err() {
                        break;
                    }
                }
                Ok(Err(broadcast::error::RecvError::Closed)) => break,
            }
        }
    });
    if let Some(bridge) = offices {
        for target in targets {
            let b = bridge.clone();
            let ptx = merged_tx.clone();
            tokio::spawn(async move { poll_office_loop(b, target, ptx).await });
        }
    }
    drop(merged_tx);
    tokio::spawn(async move {
        run_stream_loop(tx, merged_rx, subs, instance_id, session_id).await;
    });
    sse_response(rx)
}

enum StreamInput {
    Event(HubEvent),
    Lagged,
}

fn sse_response(rx: mpsc::Receiver<Bytes>) -> Response<WebBody> {
    let mut response = Response::new(WebBody::Stream(SseBody::new(rx)));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        hyper::header::HeaderName::from_static("x-accel-buffering"),
        HeaderValue::from_static("no"),
    );
    response
}

async fn run_stream_loop(
    tx: mpsc::Sender<Bytes>,
    mut incoming: mpsc::Receiver<StreamInput>,
    subs: Vec<Subscription>,
    instance_id: String,
    session_id: String,
) {
    let initial_cursors = subs
        .iter()
        .filter_map(|sub| initial_cursor(sub, &instance_id))
        .collect();
    let ready = EventFrame::Ready {
        subscriptions: initial_cursors,
    };
    if send_frame(&tx, &ready).await.is_err() {
        return;
    }
    let mut seq: u64 = 0;
    let mut last_heartbeat = tokio::time::Instant::now();
    loop {
        if tx.is_closed() {
            break;
        }
        match tokio::time::timeout(CLOSE_CHECK_INTERVAL, incoming.recv()).await {
            Err(_) => {
                if last_heartbeat.elapsed() >= HEARTBEAT_INTERVAL {
                    last_heartbeat = tokio::time::Instant::now();
                    if let Ok(ms) = Milliseconds::new(now_ms()) {
                        if send_frame(&tx, &EventFrame::Heartbeat { server_time_ms: ms })
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
            Ok(Some(StreamInput::Event(ev))) => {
                if !deliver_hub_event(&tx, &subs, &instance_id, &session_id, &mut seq, ev).await {
                    break;
                }
            }
            Ok(Some(StreamInput::Lagged)) => {
                if deliver_slow_consumer(&tx, &subs).await.is_err() {
                    break;
                }
            }
            Ok(None) => break,
        }
    }
}

async fn race_until_closed<T>(
    tx: &mpsc::Sender<StreamInput>,
    fut: impl std::future::Future<Output = T>,
) -> Option<T> {
    let mut closed = std::pin::pin!(tx.closed());
    let mut fut = std::pin::pin!(fut);
    std::future::poll_fn(|cx| {
        if closed.as_mut().poll(cx).is_ready() {
            return Poll::Ready(None);
        }
        if let Poll::Ready(out) = fut.as_mut().poll(cx) {
            return Poll::Ready(Some(out));
        }
        Poll::Pending
    })
    .await
}

/// Validates that every requested subscription names this instance and an organization
/// linked to the authenticated account.
pub fn authorize_subscriptions(
    request: &StreamRequest,
    instance_id: &str,
    linked_orgs: &BTreeSet<String>,
) -> bool {
    if request.subscriptions.is_empty() || request.subscriptions.len() > 32 {
        return false;
    }
    request.subscriptions.iter().all(|sub| match &sub.scope {
        Scope::Instance { instance_id: id } => {
            id == instance_id && sub.feed == Feed::InstanceStatus
        }
        Scope::Organization {
            instance_id: id,
            org_id,
        } => {
            id == instance_id
                && linked_orgs.contains(org_id)
                && matches!(
                    sub.feed,
                    Feed::Organization | Feed::Messaging | Feed::Presence
                )
        }
        Scope::Office {
            instance_id: id,
            org_id,
            ..
        } => id == instance_id && linked_orgs.contains(org_id),
    })
}

fn initial_cursor(sub: &Subscription, instance_id: &str) -> Option<SubscriptionCursor> {
    let cursor = sub.cursor.clone().or_else(|| {
        let label = match &sub.scope {
            Scope::Instance { .. } => format!("instance:{instance_id}:0"),
            Scope::Organization { org_id, .. } => format!("org:{org_id}:live"),
            Scope::Office { office_id, .. } => format!("office:{office_id}:live"),
        };
        ResumeCursor::parse(label).ok()
    })?;
    Some(SubscriptionCursor {
        scope: ResponseScope::Known(sub.scope.clone()),
        feed: sub.feed,
        cursor,
    })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
