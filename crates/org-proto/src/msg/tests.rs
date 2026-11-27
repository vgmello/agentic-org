use std::fmt::Debug;

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

use super::*;
use crate::calls::Call;
use crate::events::EventKind;
use crate::{ApiError, CallError, ErrorCode, Request, Response, MAX_FRAME_BYTES};

/// Asserts `value` serializes to exactly `expected` and decodes back to `value`.
fn check<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: &T, expected: Value) {
    assert_eq!(serde_json::to_value(value).unwrap(), expected);
    assert_eq!(&serde_json::from_value::<T>(expected).unwrap(), value);
}

/// Asserts `{}` decodes as the default.
fn empty_decodes<T: DeserializeOwned + Default + PartialEq + Debug>() {
    assert_eq!(
        serde_json::from_value::<T>(json!({})).unwrap(),
        T::default()
    );
}

const BODY: &str = "zebra-crossing-body-7731";
const META_VALUE: &str = "meta-secret-4482";

fn meta() -> Meta {
    Meta::from([("ticket".to_string(), json!(META_VALUE))])
}

fn header() -> MessageHeader {
    MessageHeader {
        id: "01M6WRN71MBX8FHS08MB0Q9Z24".into(),
        thread_id: "01M6WRN71MBX8FHS08MB0Q9Z24".into(),
        reply_to: None,
        from_member_id: "01M6WA".into(),
        from_display_name: "ada".into(),
        to_member_id: "01M6WB".into(),
        to_display_name: "remy".into(),
        kind: KIND_REQUEST.into(),
        priority: MessagePriority::Urgent,
        urgent: true,
        subject: Some("Quick check".into()),
        created_at: "2026-11-08T05:03:51.348Z".into(),
        expires_at: Some("2026-11-08T05:05:51.348Z".into()),
        ttl_ms: Some(120_000),
        body_bytes: 26,
        meta_bytes: 0,
        local_seq: 42,
        state: MessageLifecycleState::Unread,
        triage_gen: 0,
        snoozed_until: None,
        snooze_after_task: false,
        snooze_note: None,
        read_at: None,
        expired_waiting_person: false,
    }
}

fn header_json() -> Value {
    json!({
        "id": "01M6WRN71MBX8FHS08MB0Q9Z24",
        "thread_id": "01M6WRN71MBX8FHS08MB0Q9Z24",
        "from_member_id": "01M6WA",
        "from_display_name": "ada",
        "to_member_id": "01M6WB",
        "to_display_name": "remy",
        "kind": "request",
        "priority": "urgent",
        "urgent": true,
        "subject": "Quick check",
        "created_at": "2026-11-08T05:03:51.348Z",
        "expires_at": "2026-11-08T05:05:51.348Z",
        "ttl_ms": 120000,
        "body_bytes": 26,
        "meta_bytes": 0,
        "local_seq": 42,
        "state": "unread",
        "triage_gen": 0
    })
}

fn message() -> Message {
    Message {
        header: header(),
        body: BODY.into(),
        meta: meta(),
    }
}

fn message_json() -> Value {
    json!({"header": header_json(), "body": BODY, "meta": {"ticket": META_VALUE}})
}

fn token_bytes() -> [u8; 32] {
    let mut bytes = [0u8; 32];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = (i as u8).wrapping_mul(29).wrapping_add(0x3c);
    }
    bytes
}

fn token_hex() -> String {
    token_bytes().iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn method_and_event_names_are_exact_and_unique() {
    let names = [
        MsgBind::METHOD,
        MsgSend::METHOD,
        MsgReply::METHOD,
        MsgAsk::METHOD,
        MsgDefer::METHOD,
        MsgUnsnooze::METHOD,
        MsgDone::METHOD,
        MsgNotify::METHOD,
        MsgWait::METHOD,
        MsgRead::METHOD,
        MsgInbox::METHOD,
        MsgList::METHOD,
        MsgGet::METHOD,
        MsgHumanNeedsYou::METHOD,
        MsgThreads::METHOD,
        crate::directory::Who::METHOD,
        crate::directory::StatusSet::METHOD,
        MessageReceived::NAME,
        MessageStateChanged::NAME,
        MessageDeliveryFailed::NAME,
        crate::directory::MemberPresenceChanged::NAME,
    ];
    assert_eq!(
        names,
        [
            "msg.bind",
            "msg.send",
            "msg.reply",
            "msg.ask",
            "msg.defer",
            "msg.unsnooze",
            "msg.done",
            "msg.notify",
            "msg.wait",
            "msg.read",
            "msg.inbox",
            "msg.list",
            "msg.get",
            "msg.human_needs_you",
            "msg.threads",
            "who",
            "status.set",
            "message.received",
            "message.state_changed",
            "message.delivery_failed",
            "member.presence_changed",
        ]
    );
    let unique: std::collections::BTreeSet<_> = names.iter().collect();
    assert_eq!(unique.len(), names.len());
}

#[test]
fn header_and_message_wire_shapes() {
    check(&header(), header_json());
    check(&message(), message_json());

    // Every optional field set.
    let mut full = header();
    full.reply_to = Some("01M6WP".into());
    full.meta_bytes = 12;
    full.triage_gen = 2;
    full.state = MessageLifecycleState::Snoozed;
    full.snoozed_until = Some("2026-11-08T07:00:00.000Z".into());
    full.snooze_after_task = true;
    full.snooze_note = Some("after the build".into());
    full.read_at = Some("2026-11-08T05:04:00.000Z".into());
    full.expired_waiting_person = true;
    let mut expected = header_json();
    let obj = expected.as_object_mut().unwrap();
    obj.insert("reply_to".into(), json!("01M6WP"));
    obj.insert("meta_bytes".into(), json!(12));
    obj.insert("triage_gen".into(), json!(2));
    obj.insert("state".into(), json!("snoozed"));
    obj.insert("snoozed_until".into(), json!("2026-11-08T07:00:00.000Z"));
    obj.insert("snooze_after_task".into(), json!(true));
    obj.insert("snooze_note".into(), json!("after the build"));
    obj.insert("read_at".into(), json!("2026-11-08T05:04:00.000Z"));
    obj.insert("expired_waiting_person".into(), json!(true));
    check(&full, expected);

    // A message without meta omits it, and `meta_bytes`/`triage_gen` may be missing.
    let bare = Message {
        meta: Meta::new(),
        ..message()
    };
    check(&bare, json!({"header": header_json(), "body": BODY}));
    let mut old = header_json();
    old.as_object_mut().unwrap().remove("meta_bytes");
    old.as_object_mut().unwrap().remove("triage_gen");
    assert_eq!(
        serde_json::from_value::<MessageHeader>(old).unwrap(),
        header()
    );
}

#[test]
fn bind_wire_shape() {
    check(
        &MsgBindParams::member("ac-software-engineer-remy"),
        json!({"member": "ac-software-engineer-remy"}),
    );
    // The retired `app_token` is refused on presence, whatever its value, and the error
    // never quotes it.
    for value in [json!(token_hex()), json!(null), json!("not hex"), json!(7)] {
        let err =
            serde_json::from_value::<MsgBindParams>(json!({"member": "owner", "app_token": value}))
                .unwrap_err()
                .to_string();
        assert!(err.contains(crate::retired::APP_TOKEN_RETIRED), "{err}");
        assert!(!err.contains(&token_hex()[..8]), "{err}");
    }
    empty_decodes::<MsgBindParams>();
    // Both names at once don't decode (a server answers `invalid_request`).
    assert!(serde_json::from_value::<MsgBindParams>(json!({"member": "a", "agent": "b"})).is_err());
    // `agent` (the ADR's name) is read as `member`; `member` is what's written.
    assert_eq!(
        serde_json::from_value::<MsgBindParams>(json!({"agent": "x"})).unwrap(),
        MsgBindParams::member("x")
    );
    check(
        &MsgBindResult {
            member_id: Some("01M6WB".into()),
            office_id: "01M6O".into(),
        },
        json!({"member_id": "01M6WB", "office_id": "01M6O"}),
    );
    check(
        &MsgBindResult {
            member_id: None,
            office_id: "01M6O".into(),
        },
        json!({"office_id": "01M6O"}),
    );
}

fn send_params() -> MsgSendParams {
    MsgSendParams {
        id: "01M6WS".into(),
        to: "remy".into(),
        kind: KIND_MSG.into(),
        priority: None,
        subject: None,
        body: BODY.into(),
        meta: Meta::new(),
        reply_to: None,
        attachment_ids: Vec::new(),
    }
}

#[test]
fn send_has_no_deadline_fields() {
    // Only `msg.ask` has a deadline; an old client's `ttl_ms` on a send is ignored.
    let p: MsgSendParams = serde_json::from_value(json!({
        "id": "01M6WS", "to": "remy", "kind": "msg", "body": BODY, "ttl_ms": 60000,
        "expires_at": "2026-11-08T05:02:00.000Z"
    }))
    .unwrap();
    assert_eq!(p, send_params());
    let s: SpooledSend = serde_json::from_value(json!({
        "v": 1, "office_id": "01M6O", "from": "ac-software-engineer-remy",
        "id": "01M6WS", "to": "ada", "kind": "msg", "priority": "normal",
        "body": BODY, "meta": {"ticket": META_VALUE},
        "expires_at": "2026-11-08T05:02:00.000Z", "ttl_ms": 120000,
        "created_at": "2026-11-08T05:00:00.000Z"
    }))
    .unwrap();
    assert_eq!(s, spooled());
}

#[test]
fn send_wire_shape() {
    check(
        &send_params(),
        json!({"id": "01M6WS", "to": "remy", "kind": "msg", "body": BODY}),
    );
    let full = MsgSendParams {
        priority: Some(MessagePriority::High),
        subject: Some("Build".into()),
        meta: meta(),
        reply_to: Some("01M6WP".into()),
        ..send_params()
    };
    check(
        &full,
        json!({
            "id": "01M6WS", "to": "remy", "kind": "msg", "priority": "high",
            "subject": "Build", "body": BODY, "meta": {"ticket": META_VALUE},
            "reply_to": "01M6WP"
        }),
    );
    check(
        &SendResult {
            id: "01M6WS".into(),
            thread_id: "01M6WP".into(),
            local_seq: 43,
            origin_seq: 7,
        },
        json!({"id": "01M6WS", "thread_id": "01M6WP", "local_seq": 43, "origin_seq": 7}),
    );
}

fn reply_params() -> MsgReplyParams {
    MsgReplyParams {
        id: "01M6WR".into(),
        to_id: "01M6WRN71MBX8FHS08MB0Q9Z24".into(),
        priority: None,
        subject: None,
        body: BODY.into(),
        meta: meta(),
        attachment_ids: Vec::new(),
    }
}

#[test]
fn reply_wire_shape() {
    check(
        &reply_params(),
        json!({
            "id": "01M6WR", "to_id": "01M6WRN71MBX8FHS08MB0Q9Z24", "body": BODY,
            "meta": {"ticket": META_VALUE}
        }),
    );
    let full = MsgReplyParams {
        priority: Some(MessagePriority::Normal),
        subject: Some("Re: Quick check".into()),
        meta: Meta::new(),
        ..reply_params()
    };
    check(
        &full,
        json!({
            "id": "01M6WR", "to_id": "01M6WRN71MBX8FHS08MB0Q9Z24", "priority": "normal",
            "subject": "Re: Quick check", "body": BODY
        }),
    );
}

fn ask_params() -> MsgAskParams {
    MsgAskParams {
        id: "01M6WQ".into(),
        to: "owner".into(),
        priority: None,
        subject: Some("Approve?".into()),
        body: BODY.into(),
        meta: meta(),
        ttl_ms: None,
        wait_ms: 3_600_000,
        attachment_ids: Vec::new(),
    }
}

#[test]
fn ask_wire_shape() {
    check(
        &ask_params(),
        json!({
            "id": "01M6WQ", "to": "owner", "subject": "Approve?", "body": BODY,
            "meta": {"ticket": META_VALUE}, "wait_ms": 3600000
        }),
    );
    let full = MsgAskParams {
        priority: Some(MessagePriority::High),
        ttl_ms: Some(86_400_000),
        ..ask_params()
    };
    assert_eq!(serde_json::to_value(&full).unwrap()["ttl_ms"], 86_400_000);
    assert_eq!(serde_json::to_value(&full).unwrap()["priority"], "high");
    assert!(serde_json::from_value::<MsgAskParams>(json!({
        "id": "01M6WQ", "to": "owner", "body": "x"
    }))
    .is_err());

    check(
        &MsgAskResult {
            reply: None,
            expires_at: "2026-11-09T05:03:51.348Z".into(),
        },
        json!({"expires_at": "2026-11-09T05:03:51.348Z"}),
    );
    check(
        &MsgAskResult {
            reply: Some(message()),
            expires_at: "2026-11-09T05:03:51.348Z".into(),
        },
        json!({"reply": message_json(), "expires_at": "2026-11-09T05:03:51.348Z"}),
    );
}

#[test]
fn defer_wire_shape_and_snooze_targets() {
    check(
        &MsgDeferParams {
            id: "01M6WQ".into(),
            until: SnoozeTarget::Until {
                at: "2026-11-08T10:00:00.000Z".into(),
            },
            note: Some("after lunch".into()),
        },
        json!({
            "id": "01M6WQ",
            "until": {"type": "until", "at": "2026-11-08T10:00:00.000Z"},
            "note": "after lunch"
        }),
    );
    check(
        &MsgDeferParams {
            id: "01M6WQ".into(),
            until: SnoozeTarget::AfterTask,
            note: None,
        },
        json!({"id": "01M6WQ", "until": {"type": "after_task"}}),
    );
    check(
        &MsgDeferResult {
            id: "01M6WQ".into(),
            triage_gen: 1,
            snoozed_until: Some("2026-11-08T10:00:00.000Z".into()),
            snooze_after_task: false,
            snooze_note: Some("after lunch".into()),
        },
        json!({
            "id": "01M6WQ", "triage_gen": 1, "snoozed_until": "2026-11-08T10:00:00.000Z",
            "snooze_note": "after lunch"
        }),
    );
    check(
        &MsgDeferResult {
            id: "01M6WQ".into(),
            triage_gen: 2,
            snoozed_until: None,
            snooze_after_task: true,
            snooze_note: None,
        },
        json!({"id": "01M6WQ", "triage_gen": 2, "snooze_after_task": true}),
    );
}

#[test]
fn done_wire_shape() {
    check(
        &MsgDoneParams {
            id: "01M6WQ".into(),
            note: None,
        },
        json!({"id": "01M6WQ"}),
    );
    check(
        &MsgDoneParams {
            id: "01M6WQ".into(),
            note: Some("handled in standup".into()),
        },
        json!({"id": "01M6WQ", "note": "handled in standup"}),
    );
    check(
        &MsgDoneResult {
            id: "01M6WQ".into(),
            resolved_at: "2026-11-08T06:00:00.000Z".into(),
        },
        json!({"id": "01M6WQ", "resolved_at": "2026-11-08T06:00:00.000Z"}),
    );
}

fn notify_result() -> NotifyResult {
    NotifyResult {
        v: NOTIFY_VERSION,
        agent: "agy-senior-dev".into(),
        urgent: true,
        count: 1,
        urgent_count: 1,
        items: vec![NotifyItem {
            id: "01M6WRN71MBX8FHS08MB0Q9Z24".into(),
            from: "ac-staff-engineer-ada".into(),
            from_name: "Ada".into(),
            subject: "Quick check".into(),
            kind: "request".into(),
            urgent: true,
            priority: MessagePriority::Urgent,
            triage_gen: 0,
            created_at: "2026-11-08T05:03:51.348Z".into(),
            reason: NotifyReason::New,
        }],
        more: 0,
        text: "1 new message(s), 1 urgent".into(),
    }
}

#[test]
fn notify_wire_shape() {
    check(
        &MsgNotifyParams {
            session_id: Some("sess-1".into()),
            after_task: true,
            urgent_only: true,
            sweep_after_ms: Some(300_000),
        },
        json!({
            "session_id": "sess-1", "after_task": true, "urgent_only": true,
            "sweep_after_ms": 300000
        }),
    );
    check(&MsgNotifyParams::default(), json!({}));
    empty_decodes::<MsgNotifyParams>();
    check(
        &notify_result(),
        json!({
            "v": 1, "agent": "agy-senior-dev", "urgent": true, "count": 1,
            "urgent_count": 1,
            "items": [{
                "id": "01M6WRN71MBX8FHS08MB0Q9Z24", "from": "ac-staff-engineer-ada",
                "from_name": "Ada", "subject": "Quick check", "kind": "request",
                "urgent": true, "priority": "urgent", "triage_gen": 0,
                "created_at": "2026-11-08T05:03:51.348Z", "reason": "new"
            }],
            "more": 0, "text": "1 new message(s), 1 urgent"
        }),
    );
}

#[test]
fn notify_result_keys_keep_the_locked_order() {
    assert_eq!(
        serde_json::to_string(&notify_result()).unwrap(),
        concat!(
            r#"{"v":1,"agent":"agy-senior-dev","urgent":true,"count":1,"urgent_count":1,"#,
            r#""items":[{"id":"01M6WRN71MBX8FHS08MB0Q9Z24","from":"ac-staff-engineer-ada","#,
            r#""from_name":"Ada","subject":"Quick check","kind":"request","urgent":true,"#,
            r#""priority":"urgent","triage_gen":0,"created_at":"2026-11-08T05:03:51.348Z","#,
            r#""reason":"new"}],"more":0,"text":"1 new message(s), 1 urgent"}"#,
        )
    );
}

/// Asserts each value's wire name both ways. `Unknown` re-serializes as `"unknown"`.
fn wire_names<T: Serialize + DeserializeOwned + PartialEq + Debug + Copy>(table: &[(T, &str)]) {
    for &(value, name) in table {
        assert_eq!(serde_json::to_value(value).unwrap(), name, "{value:?}");
        assert_eq!(serde_json::from_value::<T>(name.into()).unwrap(), value);
    }
}

#[test]
fn enum_wire_names() {
    wire_names(&[
        (MessagePriority::Normal, "normal"),
        (MessagePriority::High, "high"),
        (MessagePriority::Urgent, "urgent"),
        (MessagePriority::Unknown, "unknown"),
    ]);
    wire_names(&[
        (MessageLifecycleState::Unread, "unread"),
        (MessageLifecycleState::Snoozed, "snoozed"),
        (MessageLifecycleState::Read, "read"),
        (MessageLifecycleState::Done, "done"),
        (MessageLifecycleState::Expired, "expired"),
        (MessageLifecycleState::DeadLetter, "dead_letter"),
        (MessageLifecycleState::Unknown, "unknown"),
    ]);
    wire_names(&[
        (NotifyReason::New, "new"),
        (NotifyReason::Due, "due"),
        (NotifyReason::AfterTask, "after_task"),
        (NotifyReason::Sweep, "sweep"),
        (NotifyReason::Unknown, "unknown"),
    ]);
    wire_names(&[
        (DeliveryFailedReason::UnknownRecipient, "unknown_recipient"),
        (DeliveryFailedReason::Expired, "expired"),
        (DeliveryFailedReason::Unknown, "unknown"),
    ]);
    assert_eq!(
        serde_json::to_value(SnoozeTarget::Unknown).unwrap(),
        json!({"type": "unknown"})
    );
}

#[test]
fn constants_are_pinned() {
    assert_eq!(
        [
            KIND_MSG,
            KIND_REQUEST,
            KIND_REPLY,
            KIND_DELIVERY_FAILED,
            OFFICE_SENDER
        ],
        ["msg", "request", "reply", "delivery_failed", "office"]
    );
    assert_eq!(
        [
            MAX_BODY_BYTES,
            MAX_META_BYTES,
            MAX_SUBJECT_BYTES,
            MAX_NOTE_BYTES,
            MAX_ID_BYTES,
            MAX_HANDLE_BYTES,
            MAX_HELD_CALLS_PER_CONNECTION,
            NOTIFY_MAX_LINES,
            NOTIFY_MAX_ITEMS,
            NOTIFY_SUBJECT_MAX_CHARS,
            NOTIFY_FROM_NAME_MAX_CHARS,
        ],
        [262_144, 65_536, 1024, 1024, 128, 258, 16, 10, 100, 80, 48]
    );
    assert_eq!(
        [
            MAX_WAIT_MS,
            MAX_TTL_MS,
            DEFAULT_ASK_TTL_MS_AGENT,
            DEFAULT_ASK_TTL_MS_PERSON,
            DEFAULT_SWEEP_AFTER_MS,
        ],
        [3_600_000, 86_400_000, 120_000, 86_400_000, 300_000]
    );
    assert_eq!([MAX_PAGE_LIMIT, DEFAULT_PAGE_LIMIT], [100, 50]);
    assert_eq!([NOTIFY_VERSION, SPOOL_VERSION], [1, 1]);
}

#[test]
fn wait_and_read_wire_shapes() {
    check(
        &MsgWaitParams {
            timeout_ms: 60_000,
            from: None,
            urgent_only: false,
        },
        json!({"timeout_ms": 60000}),
    );
    check(
        &MsgWaitParams {
            timeout_ms: 60_000,
            from: Some("ada".into()),
            urgent_only: true,
        },
        json!({"timeout_ms": 60000, "from": "ada", "urgent_only": true}),
    );
    assert!(serde_json::from_value::<MsgWaitParams>(json!({})).is_err());
    check(&MsgWaitResult { message: None }, json!({}));
    check(
        &MsgWaitResult {
            message: Some(message()),
        },
        json!({"message": message_json()}),
    );

    check(&MsgReadParams::default(), json!({}));
    empty_decodes::<MsgReadParams>();
    check(
        &MsgReadParams {
            id: Some("01M6WQ".into()),
        },
        json!({"id": "01M6WQ"}),
    );
    check(&MsgReadResult { message: None }, json!({}));
    check(
        &MsgReadResult {
            message: Some(message()),
        },
        json!({"message": message_json()}),
    );
}

#[test]
fn inbox_and_list_wire_shapes() {
    check(&MsgInboxParams::default(), json!({}));
    empty_decodes::<MsgInboxParams>();
    check(
        &MsgInboxParams {
            include_all: true,
            from: Some("ada".into()),
            thread_id: Some("01M6WP".into()),
            cursor: Some(40),
            limit: Some(20),
        },
        json!({
            "include_all": true, "from": "ada", "thread_id": "01M6WP", "cursor": 40,
            "limit": 20
        }),
    );
    check(
        &HeaderPage {
            headers: vec![header()],
            next_cursor: Some(42),
        },
        json!({"headers": [header_json()], "next_cursor": 42}),
    );
    check(
        &HeaderPage {
            headers: vec![],
            next_cursor: None,
        },
        json!({"headers": []}),
    );

    check(&MsgListParams::default(), json!({}));
    empty_decodes::<MsgListParams>();
    check(
        &MsgListParams {
            thread_id: Some("01M6WP".into()),
            participant: Some("owner".into()),
            unread_only: true,
            include_snoozed: true,
            cursor: Some(9),
            limit: Some(100),
        },
        json!({
            "thread_id": "01M6WP", "participant": "owner", "unread_only": true,
            "include_snoozed": true, "cursor": 9, "limit": 100
        }),
    );
}

#[test]
fn get_wire_shape() {
    check(
        &MsgGetParams {
            id: "01M6WQ".into(),
        },
        json!({"id": "01M6WQ"}),
    );
    check(
        &MsgGetResult { message: message() },
        json!({"message": message_json()}),
    );
}

#[test]
fn human_needs_you_wire_shape() {
    check(&MsgHumanNeedsYouParams::default(), json!({}));
    empty_decodes::<MsgHumanNeedsYouParams>();
    check(
        &MsgHumanNeedsYouParams {
            member_id: Some("01M6WO".into()),
            limit: Some(5),
        },
        json!({"member_id": "01M6WO", "limit": 5}),
    );
    check(
        &HumanNeedsYouResult {
            items: vec![header()],
            total_urgent_unread: 3,
        },
        json!({"items": [header_json()], "total_urgent_unread": 3}),
    );
}

#[test]
fn threads_wire_shape() {
    check(&MsgThreadsParams::default(), json!({}));
    empty_decodes::<MsgThreadsParams>();
    let cursor = ThreadCursor {
        last_msg_at: "2026-11-08T05:03:51.348Z".into(),
        thread_id: "01M6WP".into(),
    };
    let cursor_json = json!({"last_msg_at": "2026-11-08T05:03:51.348Z", "thread_id": "01M6WP"});
    check(
        &MsgThreadsParams {
            participant: Some("owner".into()),
            cursor: Some(cursor.clone()),
            limit: Some(25),
        },
        json!({"participant": "owner", "cursor": cursor_json, "limit": 25}),
    );
    let summary = ThreadSummary {
        thread_id: "01M6WP".into(),
        subject: None,
        created_at: "2026-11-08T05:00:00.000Z".into(),
        last_msg_id: "01M6WQ".into(),
        last_msg_at: "2026-11-08T05:03:51.348Z".into(),
        message_count: 4,
        participants: vec!["01M6WA".into(), "01M6WO".into()],
    };
    check(
        &ThreadPage {
            threads: vec![summary],
            next_cursor: Some(cursor),
        },
        json!({
            "threads": [{
                "thread_id": "01M6WP", "created_at": "2026-11-08T05:00:00.000Z",
                "last_msg_id": "01M6WQ", "last_msg_at": "2026-11-08T05:03:51.348Z",
                "message_count": 4, "participants": ["01M6WA", "01M6WO"]
            }],
            "next_cursor": cursor_json
        }),
    );
    check(
        &ThreadPage {
            threads: vec![],
            next_cursor: None,
        },
        json!({"threads": []}),
    );
}

#[test]
fn event_wire_shapes() {
    check(
        &MessageReceivedData { header: header() },
        json!({"header": header_json()}),
    );
    check(
        &MessageStateChangedData {
            id: "01M6WQ".into(),
            thread_id: "01M6WP".into(),
            to_member_id: "01M6WB".into(),
            state: MessageLifecycleState::Done,
            triage_gen: 1,
            snoozed_until: None,
            snooze_after_task: false,
            reason: Some("handled".into()),
            expired_waiting_person: false,
            updated_at: "2026-11-08T06:00:00.000Z".into(),
        },
        json!({
            "id": "01M6WQ", "thread_id": "01M6WP", "to_member_id": "01M6WB",
            "state": "done", "triage_gen": 1, "reason": "handled",
            "updated_at": "2026-11-08T06:00:00.000Z"
        }),
    );
    // A person's ask past its deadline: still unread, flagged.
    check(
        &MessageStateChangedData {
            id: "01M6WQ".into(),
            thread_id: "01M6WQ".into(),
            to_member_id: "01M6WO".into(),
            state: MessageLifecycleState::Unread,
            triage_gen: 0,
            snoozed_until: None,
            snooze_after_task: false,
            reason: None,
            expired_waiting_person: true,
            updated_at: "2026-11-09T05:03:51.348Z".into(),
        },
        json!({
            "id": "01M6WQ", "thread_id": "01M6WQ", "to_member_id": "01M6WO",
            "state": "unread", "triage_gen": 0, "expired_waiting_person": true,
            "updated_at": "2026-11-09T05:03:51.348Z"
        }),
    );
    check(
        &MessageDeliveryFailedData {
            id: "01M6WF".into(),
            original_id: "01M6WQ".into(),
            thread_id: "01M6WP".into(),
            from_member_id: "01M6WA".into(),
            recipient_id: "01M6WB".into(),
            recipient_name: "remy".into(),
            reason: DeliveryFailedReason::Expired,
        },
        json!({
            "id": "01M6WF", "original_id": "01M6WQ", "thread_id": "01M6WP",
            "from_member_id": "01M6WA", "recipient_id": "01M6WB",
            "recipient_name": "remy", "reason": "expired"
        }),
    );
    let ev =
        crate::Event::new::<MessageReceived>(1, &MessageReceivedData { header: header() }).unwrap();
    assert_eq!(
        ev.data::<MessageReceived>().unwrap().unwrap().header,
        header()
    );
}

fn spooled() -> SpooledSend {
    SpooledSend {
        v: SPOOL_VERSION,
        office_id: "01M6O".into(),
        from: "ac-software-engineer-remy".into(),
        id: "01M6WS".into(),
        to: "ada".into(),
        kind: KIND_MSG.into(),
        priority: MessagePriority::Normal,
        subject: None,
        body: BODY.into(),
        meta: meta(),
        reply_to: None,
        created_at: "2026-11-08T05:00:00.000Z".into(),
    }
}

#[test]
fn spooled_send_wire_shape() {
    check(
        &spooled(),
        json!({
            "v": 1, "office_id": "01M6O", "from": "ac-software-engineer-remy",
            "id": "01M6WS", "to": "ada", "kind": "msg", "priority": "normal",
            "body": BODY, "meta": {"ticket": META_VALUE},
            "created_at": "2026-11-08T05:00:00.000Z"
        }),
    );
    let full = SpooledSend {
        subject: Some("Hi".into()),
        meta: Meta::new(),
        reply_to: Some("01M6WP".into()),
        ..spooled()
    };
    check(
        &full,
        json!({
            "v": 1, "office_id": "01M6O", "from": "ac-software-engineer-remy",
            "id": "01M6WS", "to": "ada", "kind": "msg", "priority": "normal",
            "subject": "Hi", "body": BODY, "reply_to": "01M6WP",
            "created_at": "2026-11-08T05:00:00.000Z"
        }),
    );
}

#[test]
fn unknown_values_decode_as_unknown_and_unknown_fields_are_ignored() {
    assert_eq!(
        serde_json::from_value::<MessagePriority>(json!("critical")).unwrap(),
        MessagePriority::Unknown
    );
    assert_eq!(
        serde_json::from_value::<MessageLifecycleState>(json!("archived")).unwrap(),
        MessageLifecycleState::Unknown
    );
    assert_eq!(
        serde_json::from_value::<NotifyReason>(json!("reminder")).unwrap(),
        NotifyReason::Unknown
    );
    assert_eq!(
        serde_json::from_value::<DeliveryFailedReason>(json!("blocked")).unwrap(),
        DeliveryFailedReason::Unknown
    );
    assert_eq!(
        serde_json::from_value::<SnoozeTarget>(json!({"type": "next_week", "x": 1})).unwrap(),
        SnoozeTarget::Unknown
    );

    let mut h = header_json();
    h["priority"] = json!("critical");
    h["state"] = json!("archived");
    h["reactions"] = json!(["+1"]);
    let decoded: MessageHeader = serde_json::from_value(h).unwrap();
    assert_eq!(decoded.priority, MessagePriority::Unknown);
    assert_eq!(decoded.state, MessageLifecycleState::Unknown);

    let mut m = message_json();
    m["attachments"] = json!([]);
    assert_eq!(serde_json::from_value::<Message>(m).unwrap(), message());
    let p: MsgNotifyParams =
        serde_json::from_value(json!({"after_task": true, "format": "json"})).unwrap();
    assert!(p.after_task);
}

#[test]
fn priority_defaults_to_normal_and_snooze_target_needs_its_fields() {
    assert_eq!(MessagePriority::default(), MessagePriority::Normal);
    assert!(serde_json::from_value::<SnoozeTarget>(json!({"type": "until"})).is_err());
    assert!(serde_json::from_value::<SnoozeTarget>(json!({"at": "x"})).is_err());
}

#[test]
fn debug_never_prints_bodies_meta_or_tokens() {
    let bind = MsgBindParams::member("owner");
    let shown = [
        format!("{:?}", message()),
        format!(
            "{:?}",
            MsgSendParams {
                meta: meta(),
                ..send_params()
            }
        ),
        format!("{:?}", reply_params()),
        format!("{:?}", ask_params()),
        format!("{:?}", spooled()),
        format!(
            "{:?}",
            MsgReadResult {
                message: Some(message())
            }
        ),
        format!("{bind:?}"),
    ];
    let hex = token_hex();
    for s in &shown {
        assert!(!s.contains(BODY), "{s}");
        assert!(!s.contains(META_VALUE), "{s}");
        assert!(!s.contains(&hex[..16]), "{s}");
    }
    for s in &shown[..6] {
        assert!(s.contains(&format!("<{} bytes>", BODY.len())), "{s}");
        assert!(s.contains("<1 keys>"), "{s}");
    }
    // A bind carries no secret: the retired token field has no values.
    assert!(shown[6].contains("retired_token: None"), "{}", shown[6]);
}

#[test]
fn json_string_len_matches_serde_json() {
    let mut cases: Vec<String> = vec![
        String::new(),
        "plain".into(),
        "\"quoted\"".into(),
        "back\\slash".into(),
        "\u{7f}".into(),
        "é漢字🦀".into(),
        "\u{2028}\u{2029}".into(),
        "/<>&'".into(),
    ];
    for c in 0u8..0x20 {
        cases.push(format!("a{}b", c as char));
    }
    cases.push((0u8..0x20).map(char::from).collect());
    for s in &cases {
        assert_eq!(
            json_string_len(s),
            serde_json::to_string(s).unwrap().len(),
            "{s:?}"
        );
    }
}

#[test]
fn urgency_follows_kind_and_priority() {
    use MessagePriority::*;
    for (kind, priority, urgent) in [
        (KIND_MSG, Normal, false),
        (KIND_MSG, High, true),
        (KIND_MSG, Urgent, true),
        (KIND_MSG, Unknown, false),
        (KIND_REQUEST, Normal, true),
        (KIND_REQUEST, Unknown, true),
        (KIND_REPLY, Normal, false),
        (KIND_REPLY, High, true),
        ("custom", Urgent, true),
        ("custom", Normal, false),
    ] {
        assert_eq!(is_urgent(kind, priority), urgent, "{kind} {priority:?}");
    }
}

#[test]
fn a_message_at_every_limit_fits_in_a_frame() {
    // Quotes double on the wire, so the body is at its limit with half as many characters.
    let body = "\"".repeat((MAX_BODY_BYTES - 2) / 2);
    assert_eq!(json_string_len(&body), MAX_BODY_BYTES);
    let meta = Meta::from([("k".to_string(), json!("m".repeat(MAX_META_BYTES - 8)))]);
    assert_eq!(serde_json::to_string(&meta).unwrap().len(), MAX_META_BYTES);
    let id = max_id();

    let params = MsgSendParams {
        id: id.clone(),
        to: max_handle(),
        kind: id.clone(),
        priority: Some(MessagePriority::Urgent),
        subject: Some("s".repeat(MAX_SUBJECT_BYTES - 2)),
        body: body.clone(),
        meta: meta.clone(),
        reply_to: Some(id),
        attachment_ids: Vec::new(),
    };
    let line = serde_json::to_string(&Request::new::<MsgSend>(u64::MAX, &params).unwrap()).unwrap();
    assert!(line.len() <= MAX_FRAME_BYTES, "{}", line.len());

    let result = MsgReadResult {
        message: Some(Message {
            header: header_at_limits(),
            body,
            meta,
        }),
    };
    let line = serde_json::to_string(&Response::ok::<MsgRead>(u64::MAX, &result).unwrap()).unwrap();
    assert!(line.len() <= MAX_FRAME_BYTES, "{}", line.len());
}

#[test]
fn new_error_codes_round_trip() {
    for (code, name) in [
        (ErrorCode::Timeout, "timeout"),
        (ErrorCode::TooLarge, "too_large"),
        (ErrorCode::NotBound, "not_bound"),
    ] {
        assert_eq!(serde_json::to_value(code).unwrap(), name);
        assert_eq!(
            serde_json::from_value::<ErrorCode>(name.into()).unwrap(),
            code
        );
        let line = serde_json::to_string(&Response::error(9, ApiError::new(code, "x"))).unwrap();
        match serde_json::from_str::<Response>(&line)
            .unwrap()
            .into_result::<MsgSend>()
        {
            Err(CallError::Api(e)) => assert_eq!(e.code, code),
            other => panic!("expected an api error, got {other:?}"),
        }
    }
}

#[test]
fn typed_request_and_response_round_trip() {
    let req = Request::new::<MsgAsk>(3, &ask_params()).unwrap();
    assert!(req.is::<MsgAsk>());
    assert_eq!(req.params::<MsgAsk>().unwrap(), ask_params());
    let result = MsgAskResult {
        reply: Some(message()),
        expires_at: "2026-11-09T05:03:51.348Z".into(),
    };
    let line = serde_json::to_string(&Response::ok::<MsgAsk>(3, &result).unwrap()).unwrap();
    let back: Response = serde_json::from_str(&line).unwrap();
    assert_eq!(back.into_result::<MsgAsk>().unwrap(), result);
}

/// An id or kind at [`MAX_ID_BYTES`].
fn max_id() -> String {
    let id = "i".repeat(MAX_ID_BYTES - 2);
    assert_eq!(json_string_len(&id), MAX_ID_BYTES);
    id
}

/// A handle or display name at [`MAX_HANDLE_BYTES`]: 64 four-byte characters.
fn max_handle() -> String {
    let handle = "\u{1F980}".repeat(64);
    assert_eq!(json_string_len(&handle), MAX_HANDLE_BYTES);
    handle
}

/// A header with every string field at its limit.
fn header_at_limits() -> MessageHeader {
    let id = max_id();
    let mut h = header();
    for field in [
        &mut h.id,
        &mut h.thread_id,
        &mut h.from_member_id,
        &mut h.to_member_id,
        &mut h.kind,
    ] {
        field.clone_from(&id);
    }
    h.from_display_name = max_handle();
    h.to_display_name = max_handle();
    h.reply_to = Some(id);
    h.expires_at = Some("2026-11-09T05:03:51.348Z".into());
    h.subject = Some("s".repeat(MAX_SUBJECT_BYTES - 2));
    h.snoozed_until = Some("2026-11-08T07:00:00.000Z".into());
    h.snooze_after_task = true;
    h.snooze_note = Some("n".repeat(MAX_NOTE_BYTES - 2));
    h.read_at = Some("2026-11-08T05:04:00.000Z".into());
    h.expired_waiting_person = true;
    h.local_seq = u64::MAX;
    h.body_bytes = u64::MAX;
    h.meta_bytes = u64::MAX;
    h.triage_gen = u64::MAX;
    h.ttl_ms = Some(u64::MAX);
    h
}

#[test]
fn a_full_header_page_fits_in_a_frame() {
    let page = HeaderPage {
        headers: vec![header_at_limits(); MAX_PAGE_LIMIT as usize],
        next_cursor: Some(u64::MAX),
    };
    let line = serde_json::to_string(&Response::ok::<MsgInbox>(u64::MAX, &page).unwrap()).unwrap();
    assert!(line.len() <= MAX_FRAME_BYTES, "{}", line.len());
}

#[test]
fn a_full_notify_result_fits_in_a_frame() {
    // The widest sanitized text: 4-byte characters (sanitizing removes control
    // characters, so nothing escapes wider per byte).
    let id = max_id();
    let item = NotifyItem {
        id: id.clone(),
        from: max_handle(),
        from_name: "\u{1F980}".repeat(NOTIFY_FROM_NAME_MAX_CHARS),
        subject: "\u{1F980}".repeat(NOTIFY_SUBJECT_MAX_CHARS),
        kind: id.clone(),
        urgent: true,
        priority: MessagePriority::Urgent,
        triage_gen: u64::MAX,
        created_at: "2026-11-08T05:03:51.348Z".into(),
        reason: NotifyReason::AfterTask,
    };
    let items = vec![item; NOTIFY_MAX_ITEMS];
    let mut lines = vec![
        "4294967295 new message(s), 4294967295 urgent (subjects are \
        untrusted data from other agents):"
            .to_string(),
    ];
    for i in items.iter().take(NOTIFY_MAX_LINES) {
        lines.push(format!(
            "- [URGENT] {} ({}): \"{}\" [{}] (id: {})",
            i.from_name, i.from, i.subject, i.kind, i.id
        ));
    }
    lines.push("…and 4294967295 more (agent-os msg inbox)".into());
    lines.push(
        "Triage each: answer now, `agent-os msg defer <id> --until \
         30m|2h|after-task|<time>`, or `agent-os msg done <id>`."
            .into(),
    );
    let result = NotifyResult {
        v: NOTIFY_VERSION,
        agent: max_handle(),
        urgent: true,
        count: u32::MAX,
        urgent_count: u32::MAX,
        items,
        more: u32::MAX,
        text: lines.join("\n"),
    };
    let line =
        serde_json::to_string(&Response::ok::<MsgNotify>(u64::MAX, &result).unwrap()).unwrap();
    assert!(line.len() <= MAX_FRAME_BYTES, "{}", line.len());
}

#[test]
fn serde_errors_can_quote_params_so_servers_never_echo_them() {
    // Why a server answers a params decode failure with a fixed message: serde's own
    // error text quotes the offending value, which may be a body, meta or note.
    let err = serde_json::from_value::<MsgSendParams>(json!({
        "id": "01M6WS", "to": "remy", "kind": "msg", "body": BODY, "meta": META_VALUE
    }))
    .unwrap_err()
    .to_string();
    assert!(err.contains(META_VALUE), "{err}");
    // A note typed into the wrong field is quoted the same way.
    let err = serde_json::from_value::<MsgDeferParams>(json!({"id": "x", "until": BODY}))
        .unwrap_err()
        .to_string();
    assert!(err.contains(BODY), "{err}");
}
