use super::*;

fn context() -> HostChannel {
    let mut hello = HelloResult::host("test", "This Mac").with_instance_id("verified-instance");
    hello.api_version = 2;
    HostChannel {
        _channel_id: "opaque-channel".into(),
        _kind: org_office_launch::v3::ChannelKind::Local,
        hello,
        _revocations: Arc::default(),
    }
}
fn exchange(
    peer: &mut std::io::BufReader<UnixStream>,
    method: &str,
    params: serde_json::Value,
) -> serde_json::Value {
    frame::write_message(
        peer.get_mut(),
        &serde_json::json!({"id":1,"method":method,"params":params}),
    )
    .unwrap();
    frame::read_message(peer).unwrap().unwrap()
}
fn host_pair() -> (std::io::BufReader<UnixStream>, std::thread::JoinHandle<()>) {
    let (socket, peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let thread = std::thread::spawn(move || serve(socket, context(), &Default::default()));
    (std::io::BufReader::new(peer), thread)
}
#[test]
fn host_hello_checks_both_range_edges_and_malformed_params_before_version_lock() {
    for range in [
        serde_json::json!({"min":1,"max":1}),
        serde_json::json!({"min":3,"max":3}),
    ] {
        let (mut peer, thread) = host_pair();
        assert_eq!(
            exchange(&mut peer, "hello", serde_json::json!({"api":range}))["error"]["code"],
            "unsupported_version"
        );
        assert!(frame::read_line(&mut peer).unwrap().is_none());
        thread.join().unwrap();
    }
    let (mut peer, thread) = host_pair();
    assert_eq!(
        exchange(&mut peer, "hello", serde_json::json!({"api":[]}))["error"]["code"],
        "invalid_request"
    );
    assert_eq!(
        exchange(&mut peer, "work.list", serde_json::json!({}))["error"]["code"],
        "unsupported_version"
    );
    assert!(frame::read_line(&mut peer).unwrap().is_none());
    thread.join().unwrap();
    let (mut peer, thread) = host_pair();
    for range in [
        serde_json::json!({"min":1,"max":3}),
        serde_json::json!({"min":2,"max":2}),
    ] {
        let answer = exchange(&mut peer, "hello", serde_json::json!({"api":range}));
        assert_eq!(answer["result"]["api_version"], 2);
        assert_eq!(answer["result"]["instance_id"], "verified-instance");
    }
    assert_eq!(
        exchange(&mut peer, "hello", serde_json::json!({"api":null}))["error"]["code"],
        "unsupported_version"
    );
    assert!(frame::read_line(&mut peer).unwrap().is_none());
    thread.join().unwrap();
}
