use super::*;
use http_body_util::BodyExt;
use org_proto::web::ResyncReason;

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

fn org_sub(feed: Feed) -> Subscription {
    Subscription {
        scope: Scope::Organization {
            instance_id: "inst-1".into(),
            org_id: "org-1".into(),
        },
        feed,
        cursor: None,
    }
}

fn activity_data(member: &str) -> serde_json::Value {
    serde_json::json!({
        "member_id": member, "phase": "running_tool", "active_tool_name": "Bash",
        "supports_steer": true, "changed_at": "2026-11-27T10:00:00Z"
    })
}

/// Agents' live activity (`TASK-103`) reaches a stream through the `EventBus` seam as
/// `member.activity_changed` on its organization's `presence` feed only.
#[test]
fn the_presence_feed_carries_agent_activity_through_the_event_bus() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let bus = InMemoryEventBus::default();
        let req = StreamRequest {
            subscriptions: vec![org_sub(Feed::Presence)],
        };
        let response = open_stream_on_bus(&bus, "inst-1".into(), "s".into(), req, None, vec![]);
        let mut body = response.into_body();
        let _ready = body.frame().await.unwrap().unwrap();

        // Another organization's activity never reaches this stream.
        bus.publish(HubEvent::ActivityChanged {
            org_id: "org-2".into(),
            cursor: "presence:org-2:1".into(),
            data: activity_data("other-org-agent"),
        });
        bus.publish(HubEvent::ActivityChanged {
            org_id: "org-1".into(),
            cursor: "presence:org-1:4".into(),
            data: activity_data("ada"),
        });
        let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
        let text = std::str::from_utf8(&frame).unwrap();
        assert!(
            text.contains("\"event\":\"member.activity_changed\""),
            "{text}"
        );
        assert!(text.contains("\"feed\":\"presence\""), "{text}");
        assert!(text.contains("\"cursor\":\"presence:org-1:4\""), "{text}");
        assert!(text.contains("\"member_id\":\"ada\""), "{text}");
        assert!(!text.contains("other-org-agent"), "{text}");
    });
}

#[test]
fn a_stream_without_the_presence_feed_never_gets_agent_activity() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let bus = InMemoryEventBus::default();
        let req = StreamRequest {
            subscriptions: vec![org_sub(Feed::Messaging)],
        };
        let response = open_stream_on_bus(&bus, "inst-1".into(), "s".into(), req, None, vec![]);
        let mut body = response.into_body();
        let _ready = body.frame().await.unwrap().unwrap();
        bus.publish(HubEvent::ActivityChanged {
            org_id: "org-1".into(),
            cursor: "presence:org-1:4".into(),
            data: activity_data("ada"),
        });
        bus.publish(HubEvent::MessagingEvent {
            org_id: "org-1".into(),
            cursor: "msg:org-1:1".into(),
            event: "message.received".into(),
            data: serde_json::json!({ "header": { "id": "m1" } }),
        });
        let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
        let text = std::str::from_utf8(&frame).unwrap();
        assert!(text.contains("\"message.received\""), "{text}");
        assert!(!text.contains("activity_changed"), "{text}");
    });
}

/// An office poll's activity changes become presence-feed events at the poll's presence head,
/// exactly as the office scoped them (no tool detail added or kept back here).
#[test]
fn office_poll_activity_changes_become_presence_events() {
    use org_proto::sessions::{ActivityPhase, MemberActivityChangedData};

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let (tx, mut rx) = mpsc::channel::<StreamInput>(8);
        let res = org_proto::chat::ChatEventsPollResult {
            org_head_seq: 1,
            live_head_seq: 0,
            presence_head_seq: 7,
            snapshot_required: false,
            live_lost: false,
            org_changes: Vec::new(),
            messages_received: Vec::new(),
            message_states: Vec::new(),
            presence_changes: Vec::new(),
            activity_changes: vec![MemberActivityChangedData {
                member_id: "tess".into(),
                phase: Some(ActivityPhase::RunningTool),
                active_tool_name: None,
                active_tool_started_at: None,
                supports_steer: true,
                changed_at: "2026-11-27T10:00:00Z".into(),
            }],
            inbox_changes: Vec::new(),
        };
        assert!(office_poll::emit_poll_result("org-1", res, &tx).await);
        let Some(StreamInput::Event(HubEvent::ActivityChanged {
            org_id,
            cursor,
            data,
        })) = rx.recv().await
        else {
            panic!("expected an activity event");
        };
        assert_eq!(org_id, "org-1");
        assert_eq!(cursor, "presence:org-1:7");
        assert_eq!(data["phase"], "running_tool");
        assert!(data.get("active_tool_name").is_none());
    });
}
