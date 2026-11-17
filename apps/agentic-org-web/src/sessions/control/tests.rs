//! Control actor framing, scope and EOF on a real socket pair.
use super::*;
use std::{io::BufReader, os::unix::net::UnixDatagram, thread::JoinHandle};

const INSTANCE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

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
        assert_eq!(request["method"], "session.inspect");
        stalled.recv().unwrap();
    });
    let control = HostControl::from_handoff(
        gateway.into(),
        transfer.into(),
        INSTANCE.into(),
        Arc::new(|| panic!("unexpected ancillary failure")),
    )
    .unwrap();
    let handle = SessionHandle::new("synthetic native handle".into()).unwrap();
    let started = Instant::now();
    let error = control.inspect(&handle).err();
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
            decode::<ops::SessionInspectResult>(&bytes, 1).err(),
            Some(expected)
        );
    }
    for bytes in [
        &br#"{"id":2,"error":{"code":"not_authorized","message":"sanitized"}}"#[..],
        br#"{"id":1,"result":{},"extra":true}"#,
        br#"[1]"#,
    ] {
        assert_eq!(
            decode::<ops::SessionInspectResult>(bytes, 1).err(),
            Some(Failure::Unavailable)
        );
    }
}
