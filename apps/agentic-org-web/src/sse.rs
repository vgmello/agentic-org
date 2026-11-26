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
use org_proto::chat::{ChatEventsPoll, ChatEventsPollParams, ChatEventsPollResult};
use org_proto::web::{
    EventFrame, Feed, Milliseconds, ResponseScope, ResumeCursor, ResyncReason, Scope,
    SessionReason, StreamRequest, Subscription, SubscriptionCursor,
};
use serde_json::Value;
use tokio::sync::{broadcast, mpsc};

use crate::{
    host::Failure,
    office::{OfficeBridge, OfficeFailure},
};

/// Bounded broadcast capacity shared by all active SSE listeners.
const HUB_CAPACITY: usize = 256;
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

/// Internal live event published into [`EventHub`] or a per-stream office poller.
#[derive(Clone, Debug)]
pub enum HubEvent {
    SessionEnded {
        session_id: String,
    },
    InstanceStatus {
        event: String,
        data: Value,
    },
    OrganizationChanged {
        org_id: String,
        cursor: String,
        event: String,
        data: Value,
    },
    MessagingEvent {
        org_id: String,
        cursor: String,
        event: String,
        data: Value,
    },
    PresenceChanged {
        org_id: String,
        cursor: String,
        data: Value,
    },
    OrgResync {
        org_id: String,
        feed: Feed,
        reason: ResyncReason,
    },
}

/// One organization HQ office polled for a live SSE stream (`B6`).
#[derive(Clone)]
pub struct OfficePollTarget {
    pub org_id: String,
    pub office_id: String,
    pub auth: BrowserAuth,
    pub initial_org_seq: Option<u64>,
}

/// Broadcast hub connecting host/office events to active SSE streams.
#[derive(Clone)]
pub struct EventHub {
    sender: broadcast::Sender<HubEvent>,
}

impl Default for EventHub {
    fn default() -> Self {
        let (sender, _) = broadcast::channel(HUB_CAPACITY);
        Self { sender }
    }
}

impl EventHub {
    pub fn publish(&self, event: HubEvent) {
        let _ = self.sender.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<HubEvent> {
        self.sender.subscribe()
    }

    /// Translates a host-control event pushed on the admitted web connection into a
    /// hub broadcast when relevant to live SSE subscribers.
    pub fn observe_host_event(&self, name: &str, data: &Value) {
        use org_host_control::{
            browser::{SessionEnded, SESSION_ENDED},
            event,
        };
        if name == SESSION_ENDED {
            if let Ok(ended) = serde_json::from_value::<SessionEnded>(data.clone()) {
                self.publish(HubEvent::SessionEnded {
                    session_id: ended.session_id,
                });
            }
        } else if matches!(
            name,
            event::ORGANIZATION_CREATED
                | event::ORGANIZATION_CHANGED
                | event::OFFICE_STATUS_CHANGED
        ) {
            self.publish(HubEvent::InstanceStatus {
                event: name.to_owned(),
                data: data.clone(),
            });
        }
    }
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

async fn deliver_hub_event(
    tx: &mpsc::Sender<Bytes>,
    subs: &[Subscription],
    instance_id: &str,
    session_id: &str,
    seq: &mut u64,
    event: HubEvent,
) -> bool {
    match event {
        HubEvent::SessionEnded { session_id: ended } if ended == session_id => {
            let _ = send_frame(
                tx,
                &EventFrame::SessionEnded {
                    reason: Some(SessionReason::SessionEnded),
                },
            )
            .await;
            false
        }
        HubEvent::SessionEnded { .. } => true,
        HubEvent::InstanceStatus { event, data } => {
            *seq = seq.saturating_add(1);
            deliver_instance_event(tx, subs, instance_id, *seq, &event, &data)
                .await
                .is_ok()
        }
        HubEvent::OrganizationChanged {
            org_id,
            cursor,
            event,
            data,
        } => deliver_scoped_event(
            tx,
            subs,
            instance_id,
            &org_id,
            Feed::Organization,
            &cursor,
            &event,
            &data,
        )
        .await
        .is_ok(),
        HubEvent::MessagingEvent {
            org_id,
            cursor,
            event,
            data,
        } => deliver_scoped_event(
            tx,
            subs,
            instance_id,
            &org_id,
            Feed::Messaging,
            &cursor,
            &event,
            &data,
        )
        .await
        .is_ok(),
        HubEvent::PresenceChanged {
            org_id,
            cursor,
            data,
        } => deliver_scoped_event(
            tx,
            subs,
            instance_id,
            &org_id,
            Feed::Presence,
            &cursor,
            "member.presence_changed",
            &data,
        )
        .await
        .is_ok(),
        HubEvent::OrgResync {
            org_id,
            feed,
            reason,
        } => {
            let subscribed = subs.iter().any(|sub| {
                sub.feed == feed
                    && matches!(
                        &sub.scope,
                        Scope::Organization { instance_id: i, org_id: o }
                            if i == instance_id && o == &org_id
                    )
            });
            if !subscribed {
                return true;
            }
            send_frame(
                tx,
                &EventFrame::Resync {
                    scope: ResponseScope::Known(Scope::Organization {
                        instance_id: instance_id.to_owned(),
                        org_id,
                    }),
                    feed,
                    reason,
                },
            )
            .await
            .is_ok()
        }
    }
}

fn is_permanent_poll_denial(err: &OfficeFailure) -> bool {
    match err {
        OfficeFailure::BeforeDispatch(Failure::Denied | Failure::NotFound) => true,
        OfficeFailure::Refused(api) => matches!(
            api.code,
            org_proto::ErrorCode::NotAuthorized | org_proto::ErrorCode::NotBound
        ),
        _ => false,
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

async fn poll_office_loop(
    bridge: OfficeBridge,
    target: OfficePollTarget,
    tx: mpsc::Sender<StreamInput>,
) {
    let mut after_org = target.initial_org_seq;
    let mut after_live: Option<u64> = None;
    let mut after_presence: Option<u64> = None;
    let mut first = true;
    loop {
        if tx.is_closed() {
            break;
        }
        // First request bootstraps current cursors immediately; subsequent requests block on
        // the office's `FeedWake` condvar and wake within <1 ms on any committed change (`TASK-140`).
        let wait_ms = (!first).then_some(OFFICE_EVENT_WAIT_MS);
        first = false;
        let params = ChatEventsPollParams {
            after_org_seq: after_org,
            after_live_seq: after_live,
            after_presence_seq: after_presence,
            wait_ms,
        };
        let Some(call_res) = race_until_closed(
            &tx,
            bridge.call_typed_with_timeout::<ChatEventsPoll>(
                &target.auth,
                &target.office_id,
                &params,
                true,
                OFFICE_EVENT_RPC_TIMEOUT,
            ),
        )
        .await
        else {
            break;
        };
        let res = match call_res {
            Ok(res) => res,
            Err(err) => {
                if is_permanent_poll_denial(&err) {
                    break;
                }
                if race_until_closed(&tx, tokio::time::sleep(OFFICE_RETRY_BACKOFF))
                    .await
                    .is_none()
                {
                    break;
                }
                continue;
            }
        };
        after_org = if res.snapshot_required || res.org_changes.is_empty() {
            Some(res.org_head_seq)
        } else {
            res.org_changes.last().map(|c| c.position.seq)
        };
        after_live = Some(res.live_head_seq);
        after_presence = Some(res.presence_head_seq);
        if !emit_poll_result(&target.org_id, res, &tx).await {
            break;
        }
    }
}

async fn emit_poll_resyncs(
    org_id: &str,
    snapshot_required: bool,
    live_lost: bool,
    tx: &mpsc::Sender<StreamInput>,
) -> bool {
    if snapshot_required {
        let ev = HubEvent::OrgResync {
            org_id: org_id.to_owned(),
            feed: Feed::Organization,
            reason: ResyncReason::SnapshotRequired,
        };
        if tx.send(StreamInput::Event(ev)).await.is_err() {
            return false;
        }
    }
    if live_lost {
        let ev = HubEvent::OrgResync {
            org_id: org_id.to_owned(),
            feed: Feed::Messaging,
            reason: ResyncReason::SnapshotRequired,
        };
        if tx.send(StreamInput::Event(ev)).await.is_err() {
            return false;
        }
    }
    true
}

async fn emit_poll_messaging_events(
    org_id: &str,
    live_head_seq: u64,
    messages: Vec<org_proto::msg::MessageReceivedData>,
    states: Vec<org_proto::msg::MessageStateChangedData>,
    tx: &mpsc::Sender<StreamInput>,
) -> bool {
    for msg in messages {
        let data = serde_json::to_value(&msg).unwrap_or(Value::Null);
        let ev = HubEvent::MessagingEvent {
            org_id: org_id.to_owned(),
            cursor: format!("msg:{org_id}:{live_head_seq}"),
            event: "message.received".to_owned(),
            data,
        };
        if tx.send(StreamInput::Event(ev)).await.is_err() {
            return false;
        }
    }
    for state in states {
        let data = serde_json::to_value(&state).unwrap_or(Value::Null);
        let ev = HubEvent::MessagingEvent {
            org_id: org_id.to_owned(),
            cursor: format!("msg:{org_id}:{live_head_seq}"),
            event: "message.state_changed".to_owned(),
            data,
        };
        if tx.send(StreamInput::Event(ev)).await.is_err() {
            return false;
        }
    }
    true
}

async fn emit_poll_inbox_and_presence(
    org_id: &str,
    presence_head_seq: u64,
    inbox_changes: Vec<org_proto::inbox::InboxChangedData>,
    presence_changes: Vec<org_proto::directory::MemberPresenceChangedData>,
    tx: &mpsc::Sender<StreamInput>,
) -> bool {
    for inbox in inbox_changes {
        let data = serde_json::to_value(&inbox).unwrap_or(Value::Null);
        let cursor = format!("inbox:{org_id}:{}", inbox.as_of.seq);
        let ev_msg = HubEvent::MessagingEvent {
            org_id: org_id.to_owned(),
            cursor: cursor.clone(),
            event: "inbox.changed".to_owned(),
            data: data.clone(),
        };
        if tx.send(StreamInput::Event(ev_msg)).await.is_err() {
            return false;
        }
        let ev_org = HubEvent::OrganizationChanged {
            org_id: org_id.to_owned(),
            cursor,
            event: "inbox.changed".to_owned(),
            data,
        };
        if tx.send(StreamInput::Event(ev_org)).await.is_err() {
            return false;
        }
    }
    for presence in presence_changes {
        let data = serde_json::to_value(&presence).unwrap_or(Value::Null);
        let ev = HubEvent::PresenceChanged {
            org_id: org_id.to_owned(),
            cursor: format!("presence:{org_id}:{presence_head_seq}"),
            data,
        };
        if tx.send(StreamInput::Event(ev)).await.is_err() {
            return false;
        }
    }
    true
}

async fn emit_poll_result(
    org_id: &str,
    res: ChatEventsPollResult,
    tx: &mpsc::Sender<StreamInput>,
) -> bool {
    if !emit_poll_resyncs(org_id, res.snapshot_required, res.live_lost, tx).await {
        return false;
    }
    for change in res.org_changes {
        let cursor = format!(
            "org:{org_id}:{}:{}",
            change.position.epoch, change.position.seq
        );
        let data = serde_json::to_value(&change).unwrap_or(Value::Null);
        let ev = HubEvent::OrganizationChanged {
            org_id: org_id.to_owned(),
            cursor,
            event: "org.changed".to_owned(),
            data,
        };
        if tx.send(StreamInput::Event(ev)).await.is_err() {
            return false;
        }
    }
    if !emit_poll_messaging_events(
        org_id,
        res.live_head_seq,
        res.messages_received,
        res.message_states,
        tx,
    )
    .await
    {
        return false;
    }
    emit_poll_inbox_and_presence(
        org_id,
        res.presence_head_seq,
        res.inbox_changes,
        res.presence_changes,
        tx,
    )
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

async fn deliver_instance_event(
    tx: &mpsc::Sender<Bytes>,
    subs: &[Subscription],
    instance_id: &str,
    seq: u64,
    event: &str,
    data: &Value,
) -> Result<(), ()> {
    let subscribed = subs.iter().any(|sub| {
        sub.feed == Feed::InstanceStatus
            && matches!(&sub.scope, Scope::Instance { instance_id: id } if id == instance_id)
    });
    if !subscribed {
        return Ok(());
    }
    let Ok(cursor) = ResumeCursor::parse(format!("instance:{instance_id}:{seq}")) else {
        return Ok(());
    };
    send_frame(
        tx,
        &EventFrame::Change {
            scope: ResponseScope::Known(Scope::Instance {
                instance_id: instance_id.to_owned(),
            }),
            feed: Feed::InstanceStatus,
            cursor,
            event: event.to_owned(),
            data: data.clone(),
        },
    )
    .await
}

#[expect(
    clippy::too_many_arguments,
    reason = "scoped SSE delivery takes explicit channel, filter, cursor, and payload fields"
)]
async fn deliver_scoped_event(
    tx: &mpsc::Sender<Bytes>,
    subs: &[Subscription],
    instance_id: &str,
    org_id: &str,
    feed: Feed,
    cursor: &str,
    event: &str,
    data: &Value,
) -> Result<(), ()> {
    let subscribed = subs.iter().any(|sub| {
        sub.feed == feed
            && matches!(&sub.scope, Scope::Organization { instance_id: i, org_id: o } if i == instance_id && o == org_id)
    });
    if !subscribed {
        return Ok(());
    }
    let Ok(cursor) = ResumeCursor::parse(cursor.to_owned()) else {
        return Ok(());
    };
    send_frame(
        tx,
        &EventFrame::Change {
            scope: ResponseScope::Known(Scope::Organization {
                instance_id: instance_id.to_owned(),
                org_id: org_id.to_owned(),
            }),
            feed,
            cursor,
            event: event.to_owned(),
            data: data.clone(),
        },
    )
    .await
}

async fn deliver_slow_consumer(tx: &mpsc::Sender<Bytes>, subs: &[Subscription]) -> Result<(), ()> {
    for sub in subs {
        send_frame(
            tx,
            &EventFrame::Resync {
                scope: ResponseScope::Known(sub.scope.clone()),
                feed: sub.feed,
                reason: ResyncReason::SlowConsumer,
            },
        )
        .await?;
    }
    Ok(())
}

async fn send_frame(tx: &mpsc::Sender<Bytes>, frame: &EventFrame) -> Result<(), ()> {
    let encoded = format_sse_frame(frame).ok_or(())?;
    tx.send(encoded).await.map_err(|_| ())
}

fn format_sse_frame(frame: &EventFrame) -> Option<Bytes> {
    let json = serde_json::to_string(frame).ok()?;
    Some(Bytes::from(format!("event: frame\ndata: {json}\n\n")))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    #[test]
    fn sse_stream_emits_ready_live_org_change_and_closes_on_session_ended() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let hub = EventHub::default();
            let req = StreamRequest {
                subscriptions: vec![
                    Subscription {
                        scope: Scope::Organization {
                            instance_id: "inst-1".into(),
                            org_id: "org-1".into(),
                        },
                        feed: Feed::Organization,
                        cursor: None,
                    },
                    Subscription {
                        scope: Scope::Organization {
                            instance_id: "inst-1".into(),
                            org_id: "org-1".into(),
                        },
                        feed: Feed::Messaging,
                        cursor: None,
                    },
                    Subscription {
                        scope: Scope::Organization {
                            instance_id: "inst-1".into(),
                            org_id: "org-1".into(),
                        },
                        feed: Feed::Presence,
                        cursor: None,
                    },
                ],
            };
            let mut linked = BTreeSet::new();
            linked.insert("org-1".to_owned());
            assert!(authorize_subscriptions(&req, "inst-1", &linked));

            let response = open_stream(&hub, "inst-1".into(), "sess-1".into(), req);
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response.headers().get(CONTENT_TYPE).unwrap(),
                "text/event-stream; charset=utf-8"
            );
            let mut body = response.into_body();
            let ready = body.frame().await.unwrap().unwrap().into_data().unwrap();
            let ready_text = std::str::from_utf8(&ready).unwrap();
            assert!(ready_text.starts_with("event: frame\ndata: "));
            assert!(ready_text.contains("\"type\":\"ready\""), "{ready_text}");

            hub.publish(HubEvent::OrganizationChanged {
                org_id: "org-1".into(),
                cursor: "org:org-1:1:2".into(),
                event: "team.created".into(),
                data: serde_json::json!({ "operation_id": "op-1" }),
            });
            let change = body.frame().await.unwrap().unwrap().into_data().unwrap();
            let change_text = std::str::from_utf8(&change).unwrap();
            assert!(change_text.contains("\"type\":\"change\""), "{change_text}");
            assert!(change_text.contains("\"team.created\""), "{change_text}");

            hub.publish(HubEvent::MessagingEvent {
                org_id: "org-1".into(),
                cursor: "msg:org-1:1".into(),
                event: "message.received".into(),
                data: serde_json::json!({ "header": { "id": "m1" } }),
            });
            let msg_frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
            let msg_text = std::str::from_utf8(&msg_frame).unwrap();
            assert!(msg_text.contains("\"message.received\""), "{msg_text}");
            assert!(!msg_text.contains("\"body\""), "{msg_text}");

            hub.publish(HubEvent::PresenceChanged {
                org_id: "org-1".into(),
                cursor: "presence:org-1:1".into(),
                data: serde_json::json!({ "member_id": "e1", "presence": "working" }),
            });
            let pres_frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
            let pres_text = std::str::from_utf8(&pres_frame).unwrap();
            assert!(
                pres_text.contains("\"member.presence_changed\""),
                "{pres_text}"
            );

            // Another session ending does not close this stream.
            hub.publish(HubEvent::SessionEnded {
                session_id: "sess-other".into(),
            });
            // This session ending emits session_ended and closes the stream.
            hub.publish(HubEvent::SessionEnded {
                session_id: "sess-1".into(),
            });
            let ended = body.frame().await.unwrap().unwrap().into_data().unwrap();
            let ended_text = std::str::from_utf8(&ended).unwrap();
            assert!(
                ended_text.contains("\"type\":\"session_ended\""),
                "{ended_text}"
            );
            assert!(body.frame().await.is_none());
        });
    }

    #[test]
    fn sse_stream_filters_org_resync_to_subscribed_feeds() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let hub = EventHub::default();
            let req = StreamRequest {
                subscriptions: vec![Subscription {
                    scope: Scope::Organization {
                        instance_id: "inst-1".into(),
                        org_id: "org-1".into(),
                    },
                    feed: Feed::Messaging,
                    cursor: None,
                }],
            };
            let response = open_stream(&hub, "inst-1".into(), "sess-2".into(), req);
            let mut body = response.into_body();
            let _ready = body.frame().await.unwrap().unwrap();

            // Unsubscribed feed (Organization) must NOT emit a resync frame.
            hub.publish(HubEvent::OrgResync {
                org_id: "org-1".into(),
                feed: Feed::Organization,
                reason: ResyncReason::SnapshotRequired,
            });
            // Subscribed feed (Messaging) MUST emit a resync frame.
            hub.publish(HubEvent::OrgResync {
                org_id: "org-1".into(),
                feed: Feed::Messaging,
                reason: ResyncReason::SnapshotRequired,
            });
            let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
            let text = std::str::from_utf8(&frame).unwrap();
            assert!(text.contains("\"type\":\"resync\""), "{text}");
            assert!(text.contains("\"feed\":\"messaging\""), "{text}");
        });
    }
}
