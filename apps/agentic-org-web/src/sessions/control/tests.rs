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
