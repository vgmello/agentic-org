//! Wire-shape tests for `chat.*` types.

use super::*;

#[test]
fn chat_wire_shapes_round_trip_and_redact_body_in_debug() {
    let send = ChatSendParams {
        id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a0".into(),
        to: "#general".into(),
        kind: "msg".into(),
        priority: None,
        subject: Some("Standup".into()),
        body: "super-secret-body-text".into(),
        meta: Meta::new(),
        reply_to: None,
        also_in_channel: false,
        reason: None,
        attachment_ids: Vec::new(),
    };
    let dbg = format!("{send:?}");
    assert!(!dbg.contains("super-secret-body-text"), "{dbg}");
    let encoded = serde_json::to_value(&send).unwrap();
    let decoded: ChatSendParams = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded, send);

    let item = ChatInboxItem {
        id: send.id.clone(),
        conversation_id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a1".into(),
        thread_id: send.id.clone(),
        reply_to: None,
        from_member_id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a2".into(),
        from_display_name: "Ada".into(),
        kind: "msg".into(),
        priority: MessagePriority::Normal,
        urgent: false,
        subject: None,
        created_at: "2026-11-21T12:00:00.000Z".into(),
        state: MessageLifecycleState::Unread,
        triage_gen: 0,
        snoozed_until: None,
        snooze_after_task: false,
        snooze_note: None,
        read_at: None,
        delivered_because: DeliveredBecause::Mention,
        related_message_ids: vec![],
        reason: None,
        local_seq: 1,
        attachments: Vec::new(),
    };
    let encoded = serde_json::to_value(&item).unwrap();
    assert_eq!(encoded["delivered_because"], "mention");
    assert_eq!(
        serde_json::from_value::<ChatInboxItem>(encoded).unwrap(),
        item
    );
}

#[test]
fn catchup_wire_fields_are_additive_and_compact() {
    // Older peers' payloads (no catch-up fields) still decode, with zero/absent defaults.
    let summary: ConversationSummary = serde_json::from_value(serde_json::json!({
        "id": "c1", "kind": "channel", "archived": false, "revision": 1,
        "created_at": "2026-11-21T12:00:00.000Z", "members": [], "unread_count": 2
    }))
    .unwrap();
    assert_eq!((summary.new_message_count, summary.unread_count), (0, 2));
    assert!(summary.has_unread());
    let params: ChatMessagesParams =
        serde_json::from_value(serde_json::json!({ "conversation": "#ops" })).unwrap();
    assert!(!params.is_delta() && !params.headers_only && !params.top_level_only);
    let delta = ChatMessagesParams {
        after_id: Some("0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a0".into()),
        ..params
    };
    assert!(delta.is_delta());
    // The page cursor is a separate, additive field: absent unless a later page is read.
    let wire = serde_json::to_value(&delta).unwrap();
    assert!(wire.get("page_after_id").is_none(), "{wire}");
    let paged: ChatMessagesParams = serde_json::from_value(serde_json::json!({
        "conversation": "#ops", "after_id": "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a0",
        "page_after_id": "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a1"
    }))
    .unwrap();
    assert_eq!(paged.after_id, delta.after_id, "the anchor is kept apart");
    assert_eq!(
        paged.page_after_id.as_deref(),
        Some("0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a1")
    );

    // A headers-only view carries no body key; zero counts are left out entirely.
    let view: ChatMessageView = serde_json::from_value(serde_json::json!({
        "id": "m1", "conversation_id": "c1", "thread_id": "m1", "sender_kind": "employee",
        "sender_display_name": "Ada", "kind": "msg", "priority": "normal",
        "snippet": "hi", "body_bytes": 2, "created_at": "2026-11-21T12:00:00.000Z",
        "origin_sequence": 1
    }))
    .unwrap();
    assert_eq!((view.body.as_deref(), view.body_bytes), (None, Some(2)));
    let encoded = serde_json::to_value(&view).unwrap();
    for absent in ["body", "reply_count", "attachment_count", "latest_reply_at"] {
        assert!(encoded.get(absent).is_none(), "{absent}: {encoded}");
    }
    assert!(
        !format!("{view:?}").contains("hi"),
        "Debug redacts the snippet"
    );

    // `chat.mark_read` and `chat.channel_summarize` stay compatible with question-only callers.
    let mark: ChatMarkReadParams =
        serde_json::from_value(serde_json::json!({ "conversation_id": "#ops" })).unwrap();
    assert!(!mark.dry_run && mark.last_read_message_id.is_none());
    let sum: ChatChannelSummarizeParams =
        serde_json::from_value(serde_json::json!({ "channel": "#ops", "since_read": true }))
            .unwrap();
    assert!(sum.since_read && sum.question.is_empty());
    let wire = serde_json::to_value(&sum).unwrap();
    assert!(wire.get("question").is_none(), "{wire}");
}
