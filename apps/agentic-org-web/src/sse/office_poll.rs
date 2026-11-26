//! Polling HQ offices for live chat events and emitting their frames.

use org_proto::chat::{ChatEventsPoll, ChatEventsPollParams, ChatEventsPollResult};
use org_proto::web::{Feed, ResyncReason};
use serde_json::Value;
use tokio::sync::mpsc;

use super::{
    OfficePollTarget, StreamInput, OFFICE_EVENT_RPC_TIMEOUT, OFFICE_EVENT_WAIT_MS,
    OFFICE_RETRY_BACKOFF,
};
use crate::event_hub::HubEvent;
use crate::host::Failure;
use crate::office::{OfficeBridge, OfficeFailure};
use crate::sse::race_until_closed;

pub(super) fn is_permanent_poll_denial(err: &OfficeFailure) -> bool {
    match err {
        OfficeFailure::BeforeDispatch(Failure::Denied | Failure::NotFound) => true,
        OfficeFailure::Refused(api) => matches!(
            api.code,
            org_proto::ErrorCode::NotAuthorized | org_proto::ErrorCode::NotBound
        ),
        _ => false,
    }
}

pub(super) async fn poll_office_loop(
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

pub(super) async fn emit_poll_resyncs(
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

pub(super) async fn emit_poll_messaging_events(
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

pub(super) async fn emit_poll_inbox_and_presence(
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

pub(super) async fn emit_poll_result(
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
