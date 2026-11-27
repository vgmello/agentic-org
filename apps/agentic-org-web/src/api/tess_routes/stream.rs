//! HTTP/1.1 SSE streaming for `GET /api/v1/organizations/{org}/tess/turns/{id}/stream`
//! (`TASK-80.4` Part B).
//!
//! Bridges `tess.turn_events` over [`OfficeTransport`] and browser session revocation over
//! [`EventBus`] with `Last-Event-ID` resume, 15 s keepalive heartbeats, and clean close on
//! `turn.done`, `turn.error`, or `HubEvent::SessionEnded`.

use std::future::Future;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use hyper::body::Bytes;
use hyper::header::{HeaderName, HeaderValue, CACHE_CONTROL, CONTENT_TYPE};
use hyper::{Response, StatusCode};
use org_host_control::organization::BrowserAuth;
use org_proto::tess::{
    TessTurnEvent, TessTurnEventKind, TessTurnEvents, TessTurnEventsParams, TessTurnEventsResult,
};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::api::hq_context::{resolve_hq_office, revalidate_and_resolve_hq};
use crate::api::{failure, unavailable, CallerAuth, SessionApi};
use crate::event_hub::{EventBus, EventRecvError, HubEvent};
use crate::host::Failure;
use crate::office::OfficeFailure;
use crate::ports::OfficeTransport;
use crate::service::WebReply;
use crate::sse::WebBody;

use super::map_office_failure;

const STREAM_QUEUE: usize = 32;
const TESS_POLL_WAIT_MS: u64 = 10_000;
const TESS_POLL_TIMEOUT: Duration = Duration::from_secs(15);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
const CLOSE_CHECK_INTERVAL: Duration = Duration::from_millis(200);
const RETRY_BACKOFF: Duration = Duration::from_millis(250);

enum TurnStreamMsg {
    Batch(TessTurnEventsResult),
    SessionEnded,
    Closed,
}

pub(super) async fn execute_turn_stream(
    api: &SessionApi,
    caller: CallerAuth,
    org_id: String,
    turn_id: String,
    last_event_id: Option<u64>,
) -> WebReply {
    let session = match api.validate_async(caller).await {
        Ok(session) => session,
        Err(error) => return WebReply::Json(failure(error)),
    };
    let mut office_id = match resolve_hq_office(api, caller, &org_id).await {
        Ok(id) => id,
        Err(Failure::Refused(_)) => return WebReply::Json(unavailable()),
        Err(error) => return WebReply::Json(failure(error)),
    };
    let auth = api.auth(&caller);
    let initial_params = TessTurnEventsParams {
        turn_id: turn_id.clone(),
        after_seq: last_event_id,
        wait_ms: Some(0),
    };
    let mut initial = api
        .offices
        .call_typed::<TessTurnEvents>(&auth, &office_id, &initial_params, true)
        .await;
    if matches!(
        &initial,
        Err(OfficeFailure::Refused(e)) if e.code == org_proto::ErrorCode::NotAuthorized
    ) {
        office_id = match revalidate_and_resolve_hq(api, caller, &org_id).await {
            Ok(id) => id,
            Err(Failure::Refused(_)) => return WebReply::Json(unavailable()),
            Err(error) => return WebReply::Json(failure(error)),
        };
        initial = api
            .offices
            .call_typed::<TessTurnEvents>(&auth, &office_id, &initial_params, true)
            .await;
    }
    let initial = match initial {
        Ok(res) => res,
        Err(err) => return WebReply::Json(map_office_failure(err)),
    };

    WebReply::Stream(open_turn_sse(
        api.events.as_ref(),
        Arc::clone(&api.offices),
        session.session_id,
        auth,
        office_id,
        turn_id,
        initial,
    ))
}

fn open_turn_sse(
    bus: &dyn EventBus,
    offices: Arc<dyn OfficeTransport>,
    session_id: String,
    auth: BrowserAuth,
    office_id: String,
    turn_id: String,
    initial: TessTurnEventsResult,
) -> Response<WebBody> {
    let (body_tx, body_rx) = mpsc::channel::<Bytes>(STREAM_QUEUE);
    let (msg_tx, msg_rx) = mpsc::channel::<TurnStreamMsg>(STREAM_QUEUE);

    if !initial.done {
        let mut events = bus.subscribe();
        let rev_tx = msg_tx.clone();
        let watched_session = session_id;
        tokio::spawn(async move {
            watch_session_revocation(&mut events, &watched_session, rev_tx).await;
        });

        let poll_tx = msg_tx.clone();
        let start_seq = initial.head_seq;
        tokio::spawn(async move {
            poll_turn_loop(offices, auth, office_id, turn_id, start_seq, poll_tx).await;
        });
    }
    drop(msg_tx);

    tokio::spawn(async move {
        write_sse_loop(body_tx, msg_rx, initial).await;
    });

    sse_response(body_rx)
}

async fn watch_session_revocation(
    events: &mut crate::event_hub::EventSubscription,
    watched_session: &str,
    tx: mpsc::Sender<TurnStreamMsg>,
) {
    loop {
        if tx.is_closed() {
            break;
        }
        match tokio::time::timeout(CLOSE_CHECK_INTERVAL, events.recv()).await {
            Err(_) => continue,
            Ok(Ok(HubEvent::SessionEnded { session_id })) if session_id == watched_session => {
                let _ = tx.send(TurnStreamMsg::SessionEnded).await;
                break;
            }
            Ok(Ok(_)) | Ok(Err(EventRecvError::Lagged(_))) => continue,
            Ok(Err(EventRecvError::Closed)) => {
                let _ = tx.send(TurnStreamMsg::Closed).await;
                break;
            }
        }
    }
}

async fn poll_turn_loop(
    offices: Arc<dyn OfficeTransport>,
    auth: BrowserAuth,
    office_id: String,
    turn_id: String,
    mut after_seq: u64,
    tx: mpsc::Sender<TurnStreamMsg>,
) {
    loop {
        if tx.is_closed() {
            break;
        }
        let params = TessTurnEventsParams {
            turn_id: turn_id.clone(),
            after_seq: Some(after_seq),
            wait_ms: Some(TESS_POLL_WAIT_MS),
        };
        let Some(outcome) = race_until_closed(
            &tx,
            offices.call_typed_with_timeout::<TessTurnEvents>(
                &auth,
                &office_id,
                &params,
                true,
                TESS_POLL_TIMEOUT,
            ),
        )
        .await
        else {
            break;
        };
        match outcome {
            Ok(batch) => {
                let done = batch.done;
                after_seq = batch.head_seq.max(after_seq);
                if tx.send(TurnStreamMsg::Batch(batch)).await.is_err() || done {
                    break;
                }
            }
            Err(err) => {
                if is_permanent_turn_denial(&err) {
                    let _ = tx.send(TurnStreamMsg::Closed).await;
                    break;
                }
                if race_until_closed(&tx, tokio::time::sleep(RETRY_BACKOFF))
                    .await
                    .is_none()
                {
                    break;
                }
            }
        }
    }
}

async fn write_sse_loop(
    body_tx: mpsc::Sender<Bytes>,
    mut msg_rx: mpsc::Receiver<TurnStreamMsg>,
    initial: TessTurnEventsResult,
) {
    if !emit_batch(&body_tx, initial.events).await || initial.done {
        return;
    }
    let mut last_heartbeat = tokio::time::Instant::now();
    loop {
        if body_tx.is_closed() {
            break;
        }
        match tokio::time::timeout(CLOSE_CHECK_INTERVAL, msg_rx.recv()).await {
            Err(_) => {
                if last_heartbeat.elapsed() >= HEARTBEAT_INTERVAL {
                    last_heartbeat = tokio::time::Instant::now();
                    if body_tx
                        .send(Bytes::from_static(b": keepalive\n\n"))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
            Ok(Some(TurnStreamMsg::Batch(batch))) => {
                last_heartbeat = tokio::time::Instant::now();
                let keep_going = emit_batch(&body_tx, batch.events).await;
                if !keep_going || batch.done {
                    break;
                }
            }
            Ok(Some(TurnStreamMsg::SessionEnded)) => {
                let _ = body_tx
                    .send(Bytes::from_static(
                        b"event: session.ended\ndata: {\"reason\":\"session_ended\"}\n\n",
                    ))
                    .await;
                break;
            }
            Ok(Some(TurnStreamMsg::Closed)) | Ok(None) => break,
        }
    }
}

async fn emit_batch(body_tx: &mpsc::Sender<Bytes>, events: Vec<TessTurnEvent>) -> bool {
    for ev in events {
        let terminal = ev.event.is_terminal();
        if let Some(frame) = format_turn_sse(&ev) {
            if body_tx.send(frame).await.is_err() {
                return false;
            }
        }
        if terminal {
            return false;
        }
    }
    true
}

fn format_turn_sse(ev: &TessTurnEvent) -> Option<Bytes> {
    let event_name = ev.event.event_name();
    let data: Value = match &ev.event {
        TessTurnEventKind::TurnStarted { turn_id } => json!({
            "turn_id": turn_id,
            "at": ev.at,
        }),
        TessTurnEventKind::ToolProgress {
            id,
            label,
            state,
            phase,
            started_at,
            duration_ms,
        } => json!({
            "id": id,
            "label": label,
            "state": state,
            "phase": phase,
            "started_at": started_at,
            "duration_ms": duration_ms,
            "at": ev.at,
        }),
        TessTurnEventKind::TextDelta { delta } => json!({
            "delta": delta,
            "at": ev.at,
        }),
        TessTurnEventKind::UiBlock { ui_block } => json!({
            "ui_block": ui_block,
            "at": ev.at,
        }),
        TessTurnEventKind::SuggestionCreated {
            suggestion,
            ui_block,
        } => json!({
            "suggestion": suggestion,
            "ui_block": ui_block,
            "at": ev.at,
        }),
        TessTurnEventKind::TurnDone {
            turn_id,
            outcome,
            follow_ups,
        } => json!({
            "turn_id": turn_id,
            "outcome": outcome,
            "follow_ups": follow_ups,
            "at": ev.at,
        }),
        TessTurnEventKind::TurnError { message } => json!({
            "message": message,
            "at": ev.at,
        }),
        _ => return None,
    };
    let encoded = serde_json::to_string(&data).ok()?;
    Some(Bytes::from(format!(
        "id: {}\nevent: {event_name}\ndata: {encoded}\n\n",
        ev.seq
    )))
}

fn is_permanent_turn_denial(err: &OfficeFailure) -> bool {
    match err {
        OfficeFailure::BeforeDispatch(Failure::Denied | Failure::NotFound) => true,
        OfficeFailure::Refused(api) => matches!(
            api.code,
            org_proto::ErrorCode::NotAuthorized
                | org_proto::ErrorCode::NotBound
                | org_proto::ErrorCode::NotFound
        ),
        _ => false,
    }
}

fn sse_response(rx: mpsc::Receiver<Bytes>) -> Response<WebBody> {
    let mut response = Response::new(WebBody::channel(rx));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream; charset=utf-8"),
    );
    response.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static("no-cache, no-store"),
    );
    response.headers_mut().insert(
        HeaderName::from_static("x-accel-buffering"),
        HeaderValue::from_static("no"),
    );
    response
}

async fn race_until_closed<T>(
    tx: &mpsc::Sender<TurnStreamMsg>,
    fut: impl Future<Output = T>,
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
