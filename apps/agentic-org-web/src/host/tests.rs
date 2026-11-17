//! Control actor framing, scope and EOF on a real socket pair.
use super::*;
use std::{io::BufReader, os::unix::net::UnixDatagram, thread::JoinHandle};

const INSTANCE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

/// A test-only host call: the actor's framing, deadlines and pairing do not
/// depend on the method, so one neutral method exercises all of them.
struct Probe;
impl Call for Probe {
    const METHOD: &'static str = "test.probe";
    type Params = serde_json::Value;
    type Result = serde_json::Value;
}
/// A second neutral method, distinguishable in the host's method log.
struct Other;
impl Call for Other {
    const METHOD: &'static str = "test.other";
    type Params = serde_json::Value;
    type Result = serde_json::Value;
}
fn probe(control: &HostControl) -> Result<serde_json::Value, Failure> {
    control.call::<Probe>(&serde_json::json!({}))
}
fn other(control: &HostControl) -> Result<serde_json::Value, Failure> {
    control.call::<Other>(&serde_json::json!({}))
}

fn host(instance: &'static str, peer: UnixStream) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut reader = BufReader::new(peer);
        while let Ok(Some(request)) =
            org_proto::frame::read_message::<serde_json::Value>(&mut reader)
        {
            let result = serde_json::to_value(
                org_proto::HelloResult::host("test", "disposable").with_instance_id(instance),
            )
            .unwrap();
            let reply = serde_json::json!({"id":request["id"],"result":result});
            if org_proto::frame::write_message(reader.get_mut(), &reply).is_err() {
                break;
            }
        }
    })
}
fn connect(instance: &'static str) -> (io::Result<Arc<HostControl>>, UnixStream, JoinHandle<()>) {
    let (gateway, peer) = UnixStream::pair().unwrap();
    let stop = peer.try_clone().unwrap();
    let (transfer, _host_transfer) = UnixDatagram::pair().unwrap();
    let thread = host(instance, peer);
    let control = HostControl::from_handoff(
        gateway.into(),
        transfer.into(),
        INSTANCE.into(),
        Arc::new(|| panic!("unexpected ancillary failure")),
    );
    (control, stop, thread)
}

#[test]
fn host_scope_must_match_the_launch_instance() {
    let (control, stop, thread) = connect("01ARZ3NDEKTSV4RRFFQ69G5FAW");
    assert!(control.is_err());
    // The refused control already closed its end; the peer may be disconnected.
    let _ = stop.shutdown(Shutdown::Both);
    thread.join().unwrap();
}

#[test]
fn control_eof_marks_the_connection_dead_and_refuses_calls() {
    let (control, stop, thread) = connect(INSTANCE);
    let control = control.unwrap();
    assert!(control.live());
    stop.shutdown(Shutdown::Both).unwrap();
    thread.join().unwrap();
    let until = Instant::now() + Duration::from_secs(2);
    while control.live() && Instant::now() < until {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(!control.live());
    assert!(control.fetch_listener().is_err());
}

#[test]
fn large_replies_keep_the_control_connection_in_step() {
    let large: Vec<String> = (0..700).map(|i| format!("{i:0128}")).collect();
    let expected = serde_json::json!({ "items": large });
    let sent = expected.clone();
    let (control, host) = scripted(move |request| {
        let reply = serde_json::json!({"id":request["id"],"result":sent});
        let bytes = serde_json::to_vec(&reply).unwrap();
        assert!(bytes.len() > 64 * 1024);
        assert!(bytes.len() < org_proto::MAX_FRAME_BYTES);
        Some(reply)
    });
    assert_eq!(probe(&control).unwrap(), expected);
    assert_eq!(other(&control).unwrap(), expected);
    assert!(control.live());
    drop(control);
    assert_eq!(host.join().unwrap(), ["hello", "test.probe", "test.other"]);
}

#[test]
fn control_replies_accept_the_native_frame_limit_and_refuse_larger_frames() {
    let mut reply = Reply::new();
    let header = br#"{"id":1,"result":{}}"#;
    for &byte in header {
        reply.push(byte).unwrap();
    }
    // JSON trailing whitespace counts toward the native frame limit;
    // the delimiting newline is handled separately by the actor.
    for _ in header.len()..org_proto::MAX_FRAME_BYTES {
        reply.push(b' ').unwrap();
    }
    assert_eq!(reply.bytes.len(), org_proto::MAX_FRAME_BYTES);
    decode::<org_proto::Empty>(&reply.bytes, 1, &|| {}).unwrap();
    assert_eq!(reply.push(b' '), Err(Failure::Unavailable));
    assert_eq!(reply.bytes.len(), org_proto::MAX_FRAME_BYTES);
}

#[test]
fn a_call_that_outlives_the_full_deadline_closes_the_connection() {
    let (gateway, peer) = UnixStream::pair().unwrap();
    let (transfer, _host_transfer) = UnixDatagram::pair().unwrap();
    let (release, stalled) = mpsc::sync_channel(1);
    let host = thread::spawn(move || {
        let mut reader = BufReader::new(peer);
        let hello: serde_json::Value = org_proto::frame::read_message(&mut reader)
            .unwrap()
            .unwrap();
        let result = serde_json::to_value(
            org_proto::HelloResult::host("test", "disposable").with_instance_id(INSTANCE),
        )
        .unwrap();
        org_proto::frame::write_message(
            reader.get_mut(),
            &serde_json::json!({"id":hello["id"],"result":result}),
        )
        .unwrap();
        let request: serde_json::Value = org_proto::frame::read_message(&mut reader)
            .unwrap()
            .unwrap();
        assert_eq!(request["method"], "test.probe");
        stalled.recv().unwrap();
    });
    let control = HostControl::from_handoff(
        gateway.into(),
        transfer.into(),
        INSTANCE.into(),
        Arc::new(|| panic!("unexpected ancillary failure")),
    )
    .unwrap();
    let started = Instant::now();
    let error = probe(&control).err();
    let elapsed = started.elapsed();
    release.send(()).unwrap();
    host.join().unwrap();
    // A stalled reply may still arrive later, so the stream is out of step:
    // the full per-call deadline is never shortened, and its expiry is fatal.
    assert_eq!(error, Some(Failure::Unavailable));
    assert!(
        elapsed >= DEADLINE - Duration::from_millis(50),
        "waited {elapsed:?}"
    );
    assert!(
        elapsed < DEADLINE + Duration::from_secs(2),
        "waited {elapsed:?}"
    );
    assert!(!control.live());
}

#[test]
fn host_errors_map_to_denied_not_found_or_unavailable_by_matching_id() {
    for (code, expected) in [
        ("not_found", Failure::NotFound),
        ("not_authorized", Failure::Denied),
        ("unavailable", Failure::Unavailable),
        ("storage_error", Failure::Unavailable),
    ] {
        let bytes = serde_json::to_vec(
            &serde_json::json!({"id":1,"error":{"code":code,"message":"sanitized"}}),
        )
        .unwrap();
        assert_eq!(
            decode::<serde_json::Value>(&bytes, 1, &|| {}).err(),
            Some(expected)
        );
    }
    for bytes in [
        &br#"{"id":2,"error":{"code":"not_authorized","message":"sanitized"}}"#[..],
        br#"{"id":1,"result":{},"extra":true}"#,
        br#"[1]"#,
    ] {
        assert_eq!(
            decode::<serde_json::Value>(bytes, 1, &|| {}).err(),
            Some(Failure::Unavailable)
        );
    }
}

#[test]
fn a_call_that_expires_while_queued_is_never_written_and_keeps_the_connection() {
    let (gateway, peer) = UnixStream::pair().unwrap();
    let (transfer, _host_transfer) = UnixDatagram::pair().unwrap();
    let (release, stalled) = mpsc::sync_channel::<()>(1);
    let (started, observed) = mpsc::sync_channel(1);
    let host = thread::spawn(move || {
        let mut reader = BufReader::new(peer);
        let mut methods = Vec::new();
        while let Ok(Some(request)) =
            org_proto::frame::read_message::<serde_json::Value>(&mut reader)
        {
            let method = request["method"].as_str().unwrap().to_owned();
            if method == "test.probe" && !methods.iter().any(|m| m == "test.probe") {
                // Stall the earlier call within its deadline.
                started.send(()).unwrap();
                stalled.recv().unwrap();
            }
            let result = if method != "hello" {
                serde_json::json!({})
            } else {
                serde_json::to_value(
                    org_proto::HelloResult::host("test", "disposable").with_instance_id(INSTANCE),
                )
                .unwrap()
            };
            methods.push(method);
            let reply = serde_json::json!({"id":request["id"],"result":result});
            if org_proto::frame::write_message(reader.get_mut(), &reply).is_err() {
                break;
            }
        }
        methods
    });
    let control = HostControl::from_handoff(
        gateway.into(),
        transfer.into(),
        INSTANCE.into(),
        Arc::new(|| panic!("unexpected ancillary failure")),
    )
    .unwrap();
    let earlier = {
        let control = Arc::clone(&control);
        thread::spawn(move || probe(&control).map(|_| ()))
    };
    observed.recv_timeout(Duration::from_secs(2)).unwrap();
    // Queued behind the stalled call, this one expires before it is written.
    let queued = control.borrowed_until::<Other, _>(
        &serde_json::json!({}),
        Instant::now() + Duration::from_millis(200),
    );
    assert_eq!(queued.err(), Some(Failure::Unavailable));
    assert!(control.live(), "an unwritten expiry closed the connection");

    // Also cover the caller being paused after recv_timeout returns but before
    // its timeout handler claims the job: the actor sees this expiry first.
    let (answer, waiting) = mpsc::sync_channel(1);
    let claimed = Arc::new(AtomicBool::new(false));
    control
        .jobs
        .try_send(Job {
            line: encode(&serde_json::json!({
                "id":99,"method":"test.other","params":{}
            }))
            .unwrap(),
            until: Instant::now(),
            answer,
            claimed: Arc::clone(&claimed),
        })
        .unwrap();
    assert!(matches!(
        waiting.recv_timeout(Duration::ZERO),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    release.send(()).unwrap();
    assert_eq!(earlier.join().unwrap(), Ok(()));
    assert_eq!(
        waiting.recv_timeout(Duration::from_secs(2)).unwrap().err(),
        Some(Failure::Unavailable)
    );
    // Resume the caller's timeout handler after the actor skipped its job.
    if claimed.swap(true, Ordering::SeqCst) {
        control.close();
    }
    assert!(control.live(), "an unwritten expiry closed the connection");
    // Later calls still work on the same in-step connection.
    assert_eq!(probe(&control), Ok(serde_json::json!({})));
    assert!(control.live());
    drop(control);
    // Neither expired test.other call was sent to the host.
    assert_eq!(host.join().unwrap(), ["hello", "test.probe", "test.probe"]);
}

/// A host on a real socket pair that answers `hello`, records every method and
/// answers the rest through `answer` (`None`: no reply is ever written).
fn scripted(
    answer: impl Fn(&serde_json::Value) -> Option<serde_json::Value> + Send + 'static,
) -> (Arc<HostControl>, JoinHandle<Vec<String>>) {
    let (gateway, peer) = UnixStream::pair().unwrap();
    let (transfer, host_transfer) = UnixDatagram::pair().unwrap();
    let host = thread::spawn(move || {
        let _transfer = host_transfer;
        let mut reader = BufReader::new(peer);
        let mut methods = Vec::new();
        while let Ok(Some(request)) =
            org_proto::frame::read_message::<serde_json::Value>(&mut reader)
        {
            let method = request["method"].as_str().unwrap().to_owned();
            let reply = if method == "hello" {
                let result = serde_json::to_value(
                    org_proto::HelloResult::host("test", "disposable").with_instance_id(INSTANCE),
                )
                .unwrap();
                Some(serde_json::json!({"id":request["id"],"result":result}))
            } else {
                answer(&request)
            };
            methods.push(method);
            if let Some(reply) = reply {
                if org_proto::frame::write_message(reader.get_mut(), &reply).is_err() {
                    break;
                }
            }
        }
        methods
    });
    let control = HostControl::from_handoff(
        gateway.into(),
        transfer.into(),
        INSTANCE.into(),
        Arc::new(|| panic!("unexpected ancillary failure")),
    )
    .unwrap();
    (control, host)
}

#[test]
fn a_job_dequeued_below_the_write_floor_is_refused_unwritten_and_keeps_the_connection() {
    // A written `test.other` would never be answered within its budget.
    let (control, host) = scripted(|request| {
        (request["method"] == "test.probe")
            .then(|| serde_json::json!({"id":request["id"],"result":{}}))
    });
    let near = control.borrowed_until::<Other, _>(
        &serde_json::json!({}),
        Instant::now() + Duration::from_millis(100),
    );
    assert_eq!(near.err(), Some(Failure::Unavailable));
    assert!(control.live());
    assert_eq!(probe(&control), Ok(serde_json::json!({})));
    drop(control);
    assert_eq!(host.join().unwrap(), ["hello", "test.probe"]);
}

#[test]
fn a_reply_delivered_as_the_caller_deadline_passes_is_used_without_closing() {
    let (control, host) = scripted(|_| None);
    let (answer, receiver) = mpsc::sync_channel(1);
    let mut reply = Reply::new();
    for byte in br#"{"id":7,"result":{}}"# {
        reply.push(*byte).unwrap();
    }
    answer.send(Ok(reply)).unwrap();
    // The actor had claimed the job and answered it as the caller's wait ended.
    let claimed = AtomicBool::new(true);
    let result = control.settle::<serde_json::Value>(&claimed, &receiver, 7, Instant::now());
    assert_eq!(result, Ok(serde_json::json!({})));
    assert!(control.live());
    drop(control);
    host.join().unwrap();
}

#[test]
fn a_claimed_job_waits_for_an_actor_delayed_after_reading_its_reply() {
    let (control, host) = scripted(|_| None);
    let (answer, receiver) = mpsc::sync_channel(1);
    // The actor read the complete reply by the deadline but is descheduled
    // before handing it over; its answer arrives well after the old grace.
    let actor = thread::spawn(move || {
        let mut reply = Reply::new();
        for byte in br#"{"id":7,"result":{}}"# {
            reply.push(*byte).unwrap();
        }
        thread::sleep(Duration::from_millis(300));
        answer.send(Ok(reply)).unwrap();
    });
    let claimed = AtomicBool::new(true);
    let result = control.settle::<serde_json::Value>(&claimed, &receiver, 7, Instant::now());
    actor.join().unwrap();
    assert_eq!(result, Ok(serde_json::json!({})));
    assert!(control.live());
    drop(control);
    host.join().unwrap();
}

#[test]
fn a_reply_with_another_id_desynchronizes_and_closes_the_connection() {
    let (control, host) = scripted(|request| {
        let id = request["id"].as_u64().unwrap() + 1;
        Some(serde_json::json!({"id":id,"result":{}}))
    });
    assert_eq!(probe(&control), Err(Failure::Unavailable));
    assert!(!control.live());
    assert_eq!(probe(&control), Err(Failure::Unavailable));
    drop(control);
    assert_eq!(host.join().unwrap(), ["hello", "test.probe"]);
}
