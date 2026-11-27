//! Delivering hub events to one stream: authorization scope, cursors, slow consumers and SSE
//! framing.

use hyper::body::Bytes;
use org_proto::web::{
    EventFrame, Feed, ResponseScope, ResumeCursor, ResyncReason, Scope, SessionReason, Subscription,
};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::event_hub::HubEvent;

pub(super) async fn deliver_hub_event(
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
        HubEvent::ActivityChanged {
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
            "member.activity_changed",
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

pub(super) async fn deliver_instance_event(
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
pub(super) async fn deliver_scoped_event(
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

pub(super) async fn deliver_slow_consumer(
    tx: &mpsc::Sender<Bytes>,
    subs: &[Subscription],
) -> Result<(), ()> {
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

pub(super) async fn send_frame(tx: &mpsc::Sender<Bytes>, frame: &EventFrame) -> Result<(), ()> {
    let encoded = format_sse_frame(frame).ok_or(())?;
    tx.send(encoded).await.map_err(|_| ())
}

pub(super) fn format_sse_frame(frame: &EventFrame) -> Option<Bytes> {
    let json = serde_json::to_string(frame).ok()?;
    Some(Bytes::from(format!("event: frame\ndata: {json}\n\n")))
}
