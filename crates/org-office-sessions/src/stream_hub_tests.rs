//! `SessionStreamHub`: filtering, bounded non-blocking delivery, unsubscribe, and coarse tool
//! names (`TASK-103`).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use org_proto::sessions::ActivityPhase;

use super::{
    coarse_tool_name, SessionActivity, SessionStreamEvent, SessionStreamFrame, SessionStreamHub,
    SessionStreamKey, StreamFilter, StreamPublisher, MAX_SUBSCRIBERS, MAX_TOOL_NAME_BYTES,
    SUBSCRIBER_QUEUE,
};

fn key(employee_id: &str, partition: Option<&str>) -> SessionStreamKey {
    SessionStreamKey {
        org_id: "org-1".into(),
        employee_id: employee_id.into(),
        partition: partition.map(str::to_owned),
    }
}

fn frame(employee_id: &str, partition: Option<&str>, text: &str) -> SessionStreamFrame {
    SessionStreamFrame {
        key: key(employee_id, partition),
        at: "2026-11-27T10:00:00Z".into(),
        event: SessionStreamEvent::Delta { text: text.into() },
    }
}

fn texts(sub: &super::SessionStreamSubscription) -> Vec<String> {
    let mut out = Vec::new();
    while let Some(frame) = sub.try_recv() {
        if let SessionStreamEvent::Delta { text } = frame.event {
            out.push(text);
        }
    }
    out
}

#[test]
fn filters_route_frames_by_agent_and_partition() {
    let hub = SessionStreamHub::new();
    let all = hub.subscribe(StreamFilter::All).unwrap();
    let ada = hub.subscribe(StreamFilter::Employee("ada".into())).unwrap();
    let bob_ask = hub
        .subscribe(StreamFilter::Partition {
            employee_id: "tess".into(),
            partition: "tess:ask:bob".into(),
        })
        .unwrap();
    hub.publish(&frame("ada", None, "a1"));
    hub.publish(&frame("tess", Some("tess:ask:owner"), "owner-private"));
    hub.publish(&frame("tess", Some("tess:ask:bob"), "bob-private"));
    assert_eq!(texts(&all), ["a1", "owner-private", "bob-private"]);
    assert_eq!(texts(&ada), ["a1"]);
    // A caller's partition stream never carries another caller's partition.
    assert_eq!(texts(&bob_ask), ["bob-private"]);
    assert!(key("tess", Some("tess:ask:bob")).is_private());
    assert!(!key("ada", None).is_private());
}

#[test]
fn a_full_queue_loses_frames_without_blocking_and_reports_the_lag_once() {
    let hub = SessionStreamHub::new();
    let slow = hub.subscribe(StreamFilter::All).unwrap();
    for n in 0..SUBSCRIBER_QUEUE + 10 {
        hub.publish(&frame("ada", None, &n.to_string()));
    }
    assert!(slow.take_lagged());
    assert!(!slow.take_lagged(), "the lag flag clears once read");
    let got = texts(&slow);
    assert_eq!(got.len(), SUBSCRIBER_QUEUE);
    assert_eq!(got[0], "0", "the oldest queued frames are kept, in order");
    assert!(slow.recv_timeout(Duration::from_millis(1)).is_none());
}

#[test]
fn a_dropped_subscription_is_forgotten_on_the_next_publish() {
    let hub = SessionStreamHub::new();
    let kept = hub.subscribe(StreamFilter::All).unwrap();
    drop(hub.subscribe(StreamFilter::All).unwrap());
    assert_eq!(hub.subscriber_count(), 2);
    hub.publish(&frame("ada", None, "x"));
    assert_eq!(hub.subscriber_count(), 1);
    assert_eq!(texts(&kept), ["x"]);
}

#[test]
fn sinks_run_outside_the_lock_and_unsubscribe_by_returning_false() {
    let hub = SessionStreamHub::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    let reentrant = hub.clone();
    assert!(
        hub.add_sink(StreamFilter::Employee("ada".into()), move |frame| {
            // Reading the hub from a sink must not deadlock: the lock is released.
            let _ = reentrant.subscriber_count();
            assert_eq!(frame.key.employee_id, "ada");
            seen.fetch_add(1, Ordering::SeqCst) < 1
        })
    );
    hub.publish(&frame("grace", None, "not-mine"));
    hub.publish(&frame("ada", None, "one"));
    assert_eq!(hub.subscriber_count(), 1);
    hub.publish(&frame("ada", None, "two"));
    assert_eq!(
        hub.subscriber_count(),
        0,
        "the sink returned false and is gone"
    );
    hub.publish(&frame("ada", None, "three"));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn the_hub_refuses_subscribers_beyond_its_bound() {
    let hub = SessionStreamHub::new();
    let held: Vec<_> = (0..MAX_SUBSCRIBERS)
        .map(|_| hub.subscribe(StreamFilter::All).unwrap())
        .collect();
    assert!(hub.subscribe(StreamFilter::All).is_none());
    assert!(!hub.add_sink(StreamFilter::All, |_| true));
    drop(held);
    hub.publish(&frame("ada", None, "gc"));
    assert!(hub.subscribe(StreamFilter::All).is_some());
}

#[test]
fn a_publisher_stamps_its_run_key() {
    let hub = SessionStreamHub::new();
    let sub = hub.subscribe(StreamFilter::All).unwrap();
    let publisher = StreamPublisher::new(hub.clone(), key("ada", None));
    let busy = SessionActivity {
        supports_steer: true,
        ..SessionActivity::default()
    }
    .in_phase(ActivityPhase::Thinking);
    publisher.publish("t".into(), SessionStreamEvent::Activity(busy.clone()));
    let got = sub.try_recv().unwrap();
    assert_eq!(got.key, key("ada", None));
    assert_eq!(got.event, SessionStreamEvent::Activity(busy.clone()));
    assert_eq!(busy.idle().phase, None);
    assert!(busy.idle().supports_steer);
}

#[test]
fn coarse_tool_names_are_bounded_single_line_and_never_split_a_character() {
    assert_eq!(coarse_tool_name("Bash").as_deref(), Some("Bash"));
    assert_eq!(
        coarse_tool_name("mcp__agent-os__msg_send").as_deref(),
        Some("mcp__agent-os__msg_send")
    );
    assert_eq!(
        coarse_tool_name("  Read\n\u{1b}[31mfile\t").as_deref(),
        Some("Read [31mfile")
    );
    assert_eq!(coarse_tool_name(" \n\t\u{7}"), None);
    // Multibyte text right at the byte budget is cut on a character boundary.
    let crabs = "🦀".repeat(40);
    let cut = coarse_tool_name(&crabs).unwrap();
    assert!(cut.len() <= MAX_TOOL_NAME_BYTES);
    assert_eq!(cut, "🦀".repeat(MAX_TOOL_NAME_BYTES / 4));
    let long = format!("{} tail", "x".repeat(MAX_TOOL_NAME_BYTES + 5));
    assert_eq!(coarse_tool_name(&long).unwrap().len(), MAX_TOOL_NAME_BYTES);
}
