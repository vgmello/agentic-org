use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use org_proto::directory::MemberPresence;

use super::*;

fn presence(member: &str, status: &str) -> MemberPresenceChangedData {
    MemberPresenceChangedData {
        member_id: member.to_owned(),
        presence: MemberPresence::Offline,
        last_seen_at: "2026-11-09T12:00:00.000Z".to_owned(),
        status: Some(status.to_owned()),
        status_expires_at: None,
        current_task_id: None,
    }
}

fn state(id: &str) -> Live {
    Live::StateChanged(MessageStateChangedData {
        id: id.to_owned(),
        thread_id: "T".to_owned(),
        to_member_id: "M".to_owned(),
        state: org_proto::msg::MessageLifecycleState::Read,
        triage_gen: 0,
        snoozed_until: None,
        snooze_after_task: false,
        reason: None,
        expired_waiting_person: false,
        updated_at: "2026-11-09T12:00:00.000Z".to_owned(),
    })
}

#[test]
fn mailbox_change_counter_ignores_secondary_notifications_presence_and_its_own_hint() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let live = LiveEvents::new(Box::new(|| {}));
    let Live::StateChanged(data) = state("M1") else {
        unreachable!()
    };
    live.publish(MessageEvent::StateChanged(data));
    assert_eq!(live.mailbox_head(), 1);
    live.publish(MessageEvent::DeliveryFailed(
        org_proto::msg::MessageDeliveryFailedData {
            id: "notice".into(),
            original_id: "M1".into(),
            thread_id: "T".into(),
            from_member_id: "owner".into(),
            recipient_id: "agent".into(),
            recipient_name: "Agent".into(),
            reason: org_proto::msg::DeliveryFailedReason::Expired,
        },
    ));
    live.publish_presence(presence("agent", "working"));
    live.publish_live(Live::InboxChanged(org_proto::inbox::InboxChangedData {
        counts: Default::default(),
        as_of: org_proto::inbox::InboxPosition {
            epoch: "instance".into(),
            seq: 1,
        },
        degraded: vec![],
    }));
    assert_eq!(
        live.mailbox_head(),
        1,
        "one transition, no recursive hint trigger"
    );
}

#[test]
fn a_subscriber_sees_only_what_is_published_after_its_head_and_each_publish_wakes() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let woken = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&woken);
    let live = LiveEvents::new(Box::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    }));
    live.publish_live(state("a"));
    live.publish_presence(presence("A", "x"));
    let start = live.head();
    assert_eq!(
        start,
        Head {
            messages: 1,
            presence: 1
        }
    );
    let none = live.since(start.messages, start.messages);
    assert_eq!((none.events.len(), none.next, none.lost), (0, 1, false));
    live.publish_live(state("b"));
    live.publish_live(state("c"));
    live.publish_presence(presence("A", "y"));
    assert_eq!(woken.load(Ordering::SeqCst), 5);
    let head = live.head();
    let got = live.since(start.messages, head.messages);
    assert_eq!(got.events, [state("b"), state("c")]);
    assert_eq!((got.next, got.lost), (3, false));
    assert_eq!(
        live.presence_since(start.presence, head.presence),
        (vec![presence("A", "y")], 2)
    );
    // `upto` holds back what was published after it.
    let held = live.since(start.messages, 2);
    assert_eq!((held.events, held.next), (vec![state("b")], 2));
}

#[test]
fn a_flood_of_presence_cannot_push_message_events_out() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let live = LiveEvents::new(Box::new(|| {}));
    live.publish_live(state("urgent"));
    for n in 0..CAPACITY * 3 {
        live.publish_presence(presence("A", &n.to_string()));
    }
    let head = live.head();
    let got = live.since(0, head.messages);
    assert_eq!(got.events, [state("urgent")]);
    assert!(!got.lost);
    let (presences, _) = live.presence_since(0, head.presence);
    assert_eq!(presences.len(), 1, "one slot per member");
    assert_eq!(
        presences[0].status.as_deref(),
        Some(&*(CAPACITY * 3 - 1).to_string())
    );
}

#[test]
fn a_reader_that_fell_behind_the_ring_is_told_it_lost_events() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let live = LiveEvents::new(Box::new(|| {}));
    for n in 0..CAPACITY + 10 {
        live.publish_live(state(&n.to_string()));
    }
    let head = live.head();
    let late = live.since(0, head.messages);
    assert!(late.lost, "the first ten are gone");
    assert_eq!(late.events.len(), CAPACITY);
    assert_eq!(late.events[0], state("10"));
    // Exactly one evicted: the reader's next event (11) is still there, but 10 is gone only
    // for a reader at 9.
    assert!(live.since(9, head.messages).lost);
    assert!(!live.since(10, head.messages).lost);
    // One that is just inside the ring lost nothing.
    let ok = live.since(10, head.messages);
    assert!(!ok.lost);
    assert_eq!(ok.events.len(), CAPACITY);
    // And one that is caught up reads nothing.
    let caught_up = live.since(head.messages, head.messages);
    assert!(!caught_up.lost && caught_up.events.is_empty());
}
