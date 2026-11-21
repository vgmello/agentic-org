//! Async SSE live-event hub and streaming HTTP body for `/api/v1/events/stream`
//! (web-api-v1 §6, TASK-16). Runs directly on the Tokio runtime with bounded per-stream
//! channels and never occupies the host `BlockingPool`.
use std::{
    collections::BTreeSet,
    convert::Infallible,
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
use org_proto::gateway::{
    EventFrame, Feed, Milliseconds, ResponseScope, ResumeCursor, ResyncReason, Scope,
    SessionReason, StreamRequest, Subscription, SubscriptionCursor,
};
use serde_json::Value;
use tokio::sync::{broadcast, mpsc};

/// Bounded broadcast capacity shared by all active SSE listeners.
const HUB_CAPACITY: usize = 256;
/// Bounded per-client frame queue; slow consumers receive `resync(slow_consumer)`.
const STREAM_QUEUE: usize = 32;
/// Periodic keepalive heartbeat interval for open SSE streams.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);

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

/// Internal live event published into [`EventHub`] by the host control reader or
/// multiplexed office channels.
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

/// Spawns an SSE stream task for an authenticated browser session and returns its
/// `text/event-stream` HTTP response immediately.
pub fn open_stream(
    hub: &EventHub,
    instance_id: String,
    session_id: String,
    request: StreamRequest,
) -> Response<WebBody> {
    let (tx, rx) = mpsc::channel::<Bytes>(STREAM_QUEUE);
    let mut events = hub.subscribe();
    let subs = request.subscriptions;
    tokio::spawn(async move {
        let initial_cursors = subs
            .iter()
            .filter_map(|sub| initial_cursor(sub, &instance_id))
            .collect();
        if send_frame(
            &tx,
            &EventFrame::Ready {
                subscriptions: initial_cursors,
            },
        )
        .await
        .is_err()
        {
            return;
        }
        let mut seq: u64 = 0;
        loop {
            if tx.is_closed() {
                break;
            }
            match tokio::time::timeout(HEARTBEAT_INTERVAL, events.recv()).await {
                Err(_) => {
                    if let Ok(ms) = Milliseconds::new(now_ms()) {
                        if send_frame(&tx, &EventFrame::Heartbeat { server_time_ms: ms })
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
                Ok(Ok(HubEvent::SessionEnded { session_id: ended })) if ended == session_id => {
                    let _ = send_frame(
                        &tx,
                        &EventFrame::SessionEnded {
                            reason: Some(SessionReason::SessionEnded),
                        },
                    )
                    .await;
                    break;
                }
                Ok(Ok(HubEvent::SessionEnded { .. })) => {}
                Ok(Ok(HubEvent::InstanceStatus { event, data })) => {
                    seq = seq.saturating_add(1);
                    if deliver_instance_event(&tx, &subs, &instance_id, seq, &event, &data)
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(Ok(HubEvent::OrganizationChanged {
                    org_id,
                    cursor,
                    event,
                    data,
                })) => {
                    if deliver_org_event(&tx, &subs, &instance_id, &org_id, &cursor, &event, &data)
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(Err(broadcast::error::RecvError::Lagged(_))) => {
                    if deliver_slow_consumer(&tx, &subs).await.is_err() {
                        break;
                    }
                }
                Ok(Err(broadcast::error::RecvError::Closed)) => break,
            }
        }
    });

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
        } => id == instance_id && linked_orgs.contains(org_id) && sub.feed == Feed::Organization,
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

async fn deliver_org_event(
    tx: &mpsc::Sender<Bytes>,
    subs: &[Subscription],
    instance_id: &str,
    org_id: &str,
    cursor: &str,
    event: &str,
    data: &Value,
) -> Result<(), ()> {
    let subscribed = subs.iter().any(|sub| {
        sub.feed == Feed::Organization
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
            feed: Feed::Organization,
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
                subscriptions: vec![Subscription {
                    scope: Scope::Organization {
                        instance_id: "inst-1".into(),
                        org_id: "org-1".into(),
                    },
                    feed: Feed::Organization,
                    cursor: None,
                }],
            };
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
}
