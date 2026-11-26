use std::path::PathBuf;

use org_proto::msg::MsgBind;
use org_proto::ErrorCode;
use serde_json::Value;

use super::BoundOffice;
use crate::runtime::binding::Binding;
use crate::runtime::transport::TransportOffice;

#[test]
fn unknown_or_malformed_selected_office_refuses_immediately() {
    let office = BoundOffice::new(Binding {
        socket: None,
        office_id: Some("not-a-uuidv7".into()),
        runtime_dir: Some(PathBuf::from("/tmp")),
        session_token: Some("token".into()),
        agent_id: None,
        external_state: None,
    });
    let err = office.connect_client().err().expect("refused");
    assert_eq!(err.code, ErrorCode::InvalidRequest);
    assert!(err.message.contains("not-a-uuidv7"), "{}", err.message);

    let dir = tempfile::tempdir().unwrap();
    let missing_id = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071ff";
    let missing = BoundOffice::new(Binding {
        socket: None,
        office_id: Some(missing_id.into()),
        runtime_dir: Some(dir.path().to_path_buf()),
        session_token: Some("token".into()),
        agent_id: None,
        external_state: None,
    });
    let err = missing.connect_client().err().expect("refused");
    assert_eq!(err.code, ErrorCode::NotFound);
    assert!(err.message.contains(missing_id), "{}", err.message);
}

#[test]
fn bound_office_propagates_per_call_waiting_messages_without_stale_leaks() {
    use std::io::BufReader;
    use std::os::unix::net::UnixListener;

    use org_proto::msg::MsgBindResult;
    use org_proto::{frame, Request as WireRequest, Response as WireResponse};
    use serde_json::json;

    let dir = tempfile::Builder::new().prefix("oac-wm").tempdir().unwrap();
    let sock = dir.path().join("office.sock");
    let listener = UnixListener::bind(&sock).unwrap();

    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());

        // 1. `msg.bind` returns a non-zero waiting_messages hint that must NOT leak.
        let bind_req: WireRequest = frame::read_message(&mut reader).unwrap().unwrap();
        assert_eq!(bind_req.method, "msg.bind");
        let bind_resp = WireResponse::ok::<MsgBind>(
            bind_req.id,
            &MsgBindResult {
                member_id: Some("0193a5c0-0000-7000-8000-000000000003".into()),
                office_id: "0193a5c0-0000-7000-8000-000000000001".into(),
            },
        )
        .unwrap()
        .with_waiting_messages(99);
        frame::write_message(&mut stream, &bind_resp).unwrap();

        // 2. First command RPC returns `waiting_messages: Some(4)`.
        let req1: WireRequest = frame::read_message(&mut reader).unwrap().unwrap();
        assert_eq!(req1.method, "budget.agent_own");
        let resp1 = WireResponse {
            id: req1.id,
            outcome: org_proto::Outcome::Result(json!({
                "agent_id": "0193a5c0-0000-7000-8000-000000000003"
            })),
            waiting_messages: None,
        }
        .with_waiting_messages(4);
        frame::write_message(&mut stream, &resp1).unwrap();

        // 3. Second command RPC returns `waiting_messages: None`.
        let req2: WireRequest = frame::read_message(&mut reader).unwrap().unwrap();
        assert_eq!(req2.method, "budget.agent_own");
        let resp2 = WireResponse {
            id: req2.id,
            outcome: org_proto::Outcome::Result(json!({
                "agent_id": "0193a5c0-0000-7000-8000-000000000003"
            })),
            waiting_messages: None,
        };
        frame::write_message(&mut stream, &resp2).unwrap();
    });

    let mut office = BoundOffice::new(Binding {
        socket: Some(sock),
        office_id: None,
        runtime_dir: None,
        session_token: Some("tok-123".into()),
        agent_id: Some("0193a5c0-0000-7000-8000-000000000003".into()),
        external_state: None,
    });
    office.ensure_connected().unwrap();
    assert_eq!(office.waiting_messages, None);

    let args: Vec<String> = ["budget", "status", "--format", "json"]
        .into_iter()
        .map(str::to_owned)
        .collect();

    let first = crate::execute_with_office("agent-os", &args, &mut TransportOffice(&mut office));
    assert_eq!(first.exit_code, 0, "{}", first.stdout);
    let first_json: Value = serde_json::from_str(&first.stdout).unwrap();
    assert_eq!(first_json["meta"]["waiting_messages"], 4);

    let second = crate::execute_with_office("agent-os", &args, &mut TransportOffice(&mut office));
    assert_eq!(second.exit_code, 0, "{}", second.stdout);
    let second_json: Value = serde_json::from_str(&second.stdout).unwrap();
    assert!(second_json["meta"].get("waiting_messages").is_none());

    drop(office);
    server.join().unwrap();
}
