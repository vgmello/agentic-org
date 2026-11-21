//! End-to-end B6 integration tests covering live `subscribe`, `inbox.changed` hints,
//! `chat.events_poll`, `status.set`, `org-agent-cli` (`msg`, `channel`, `status`, `session attach`),
//! and `ACK-1` latency over a real SQLite-backed `org-office-server` Unix socket (`TASK-45`).

use std::io::{BufReader, Cursor};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use org_agent_cli::attach::run_attach_loop;
use org_agent_cli::office::{Binding, BoundOffice};
use org_office::{Office, TestOffice};
use org_proto::chat::{
    ChatChannelCreate, ChatChannelCreateParams, ChatEventsPoll, ChatEventsPollParams, ChatSend,
    ChatSendParams,
};
use org_proto::directory::{StatusSet, StatusSetParams};
use org_proto::frame;
use org_proto::msg::{Meta, MsgBind, MsgBindParams};
use org_proto::org::{TeamCreate, TeamCreateParams};
use org_proto::{ErrorCode, Outcome, Request, Response, Subscribe, SubscribeParams};
use serde_json::{json, Value};

use crate::{build_office_ports, inbox, live, org_feed, serve, start_session_manager};

fn uuidv7(seed: u16) -> String {
    // Matches `TestOffice`'s fixed clock (`CREATION_AT = 1_700_000_000_000` ms).
    let ms: u64 = 1_700_000_001_000u64 + u64::from(seed);
    let hi = (ms << 16) | (0x7 << 12) | u64::from(seed & 0x0fff);
    let lo = (0b10u64 << 62) | 0x0123_4567_89ab_0000u64 | u64::from(seed);
    let hex = format!("{hi:016x}{lo:016x}");
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

struct LiveOfficeFixture {
    _dir: tempfile::TempDir,
    socket: std::path::PathBuf,
    office: Arc<Office>,
    sessions: Arc<org_office_sessions::SessionManager>,
    shared: Arc<serve::Shared>,
    stop: Arc<AtomicBool>,
    server: Option<std::thread::JoinHandle<()>>,
    owner_id: String,
    hr_id: String,
}

impl LiveOfficeFixture {
    fn start() -> Self {
        let dir = tempfile::Builder::new().prefix("b6").tempdir().unwrap();
        let socket = dir.path().join("office.sock");
        let office = Arc::new(Office::for_test(TestOffice {
            engine: org_office::EngineChoice::Sqlite,
        }));
        let info = office.info().clone();
        let owner_id = office.owner_member_id().to_owned();
        let snap = office.org_snapshot().unwrap();
        let hr_id = snap
            .members
            .iter()
            .find(|m| m.handle.as_deref() == Some("hr"))
            .expect("seeded hr agent")
            .id
            .clone();

        let wake = Arc::new(org_feed::FeedWake::new());
        let live_wake = Arc::clone(&wake);
        let live = Arc::new(live::LiveEvents::for_owner(
            Box::new(move || live_wake.notify()),
            owner_id.clone(),
        ));
        let hq = inbox::Hq::for_office(Arc::clone(&office)).unwrap();
        let sessions = start_session_manager(Arc::clone(&office), &socket);
        let ports = build_office_ports(Arc::clone(&office), Arc::clone(&sessions), live, wake);
        let mut shared =
            serve::Shared::new(&info, &socket, ports, crate::test_support::grant_key());
        shared.register_inbox(hq, office.is_hq());
        shared.enable_inbox_hints().unwrap();
        assert!(
            shared
                .hello
                .capabilities
                .iter()
                .any(|c| c == "inbox.changed"),
            "B6 HQ must advertise inbox.changed"
        );
        let shared = Arc::new(shared);

        let listener = serve::bind(&socket).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let server_shared = Arc::clone(&shared);
        let server_stop = Arc::clone(&stop);
        let server = std::thread::spawn(move || {
            serve::run(&listener, &server_shared, &|| {
                server_stop.load(Ordering::SeqCst)
            });
        });

        Self {
            _dir: dir,
            socket,
            office,
            sessions,
            shared,
            stop,
            server: Some(server),
            owner_id,
            hr_id,
        }
    }

    fn issue_binding_for(&self, employee_id: &str) -> Binding {
        let token = self
            .sessions
            .bindings()
            .issue(
                employee_id,
                "run-b6-test",
                "sess-b6-test",
                std::process::id() as libc::pid_t,
            )
            .unwrap()
            .expose_token()
            .to_owned();
        Binding {
            socket: Some(self.socket.clone()),
            office_id: Some(self.office.info().office_id.clone()),
            runtime_dir: None,
            session_token: Some(token),
            agent_id: Some(employee_id.to_owned()),
        }
    }
}

impl Drop for LiveOfficeFixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.server.take() {
            let _ = handle.join();
        }
    }
}

fn read_frame_with_timeout(reader: &mut BufReader<UnixStream>, timeout: Duration) -> Option<Value> {
    reader.get_ref().set_read_timeout(Some(timeout)).ok()?;
    frame::read_message::<Value>(reader).ok().flatten()
}

fn wait_for_event(
    reader: &mut BufReader<UnixStream>,
    event_name: &str,
    timeout: Duration,
) -> Option<Value> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let rem = deadline.saturating_duration_since(Instant::now());
        let Some(frame) = read_frame_with_timeout(reader, rem.min(Duration::from_millis(200)))
        else {
            continue;
        };
        if frame.get("event").and_then(Value::as_str) == Some(event_name) {
            return Some(frame);
        }
    }
    None
}

fn assert_subscribe_org_changes_and_replay(
    fx: &LiveOfficeFixture,
    snap: &org_proto::OrgSnapshot,
    client_reader: &mut BufReader<UnixStream>,
) -> String {
    let cmd_id = uuidv7(10);
    let team_id = uuidv7(11);
    let team_req = Request::new::<TeamCreate>(
        10,
        &TeamCreateParams {
            command_id: cmd_id.clone(),
            team_id,
            name: "Platform".into(),
            owner_team_id: Some(snap.teams[0].id.clone()),
        },
    )
    .unwrap();
    let team_resp = fx.shared.answer_for_test(&team_req, &fx.owner_id, true);
    let Outcome::Result(accepted_val) = team_resp.outcome else {
        panic!("team.create failed: {team_resp:?}");
    };
    let ev = wait_for_event(client_reader, "org.changed", Duration::from_secs(2))
        .expect("org.changed delivered after team.create");
    assert_eq!(ev["data"]["command_id"], cmd_id);
    assert_eq!(
        ev["data"]["position"]["seq"],
        accepted_val["position"]["seq"]
    );

    let replay_resp = fx.shared.answer_for_test(&team_req, &fx.owner_id, true);
    assert!(matches!(replay_resp.outcome, Outcome::Result(_)));
    assert!(
        wait_for_event(client_reader, "org.changed", Duration::from_millis(300)).is_none(),
        "idempotent replay must not emit a duplicate org.changed"
    );
    cmd_id
}

fn assert_presence_delivery_and_inbox_hints(
    fx: &LiveOfficeFixture,
    client_reader: &mut BufReader<UnixStream>,
) -> String {
    let hr_binding = fx.issue_binding_for(&fx.hr_id);
    let hr_client = org_client::Client::connect(&fx.socket).unwrap();
    hr_client
        .call::<MsgBind>(&MsgBindParams::member_with_token(
            fx.hr_id.clone(),
            hr_binding.session_token.clone().unwrap(),
        ))
        .unwrap();
    let _ = hr_client
        .call::<StatusSet>(&StatusSetParams {
            text: Some("Triaging hires".into()),
            ttl_ms: Some(60_000),
        })
        .unwrap();
    let pres_ev = wait_for_event(
        client_reader,
        "member.presence_changed",
        Duration::from_secs(2),
    )
    .expect("member.presence_changed delivered after status.set");
    assert_eq!(pres_ev["data"]["member_id"], fx.hr_id);
    assert_eq!(pres_ev["data"]["status"], "Triaging hires");

    let secret_body = "CONFIDENTIAL-BODY-NEVER-IN-EVENTS";
    let msg_id = uuidv7(20);
    let sent = hr_client
        .call::<ChatSend>(&ChatSendParams {
            id: msg_id.clone(),
            to: fx.owner_id.clone(),
            kind: "msg".into(),
            priority: None,
            subject: Some("Hiring update".into()),
            body: secret_body.into(),
            meta: Meta::new(),
            reply_to: None,
            also_in_channel: false,
            reason: None,
        })
        .unwrap();
    assert_eq!(sent.id, msg_id);

    let (mut got_received, mut got_hint) = (None, None);
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline && (got_received.is_none() || got_hint.is_none()) {
        if let Some(frame) = read_frame_with_timeout(client_reader, Duration::from_millis(200)) {
            let text = frame.to_string();
            assert!(
                !text.contains(secret_body),
                "live event leaked body: {text}"
            );
            match frame.get("event").and_then(Value::as_str) {
                Some("message.received") => got_received = Some(frame),
                Some("inbox.changed") => got_hint = Some(frame),
                _ => {}
            }
        }
    }
    let received = got_received.expect("header-only message.received delivered");
    assert_eq!(received["data"]["header"]["id"], msg_id);
    assert!(received["data"]["header"].get("body").is_none());

    let hint = got_hint.expect("inbox.changed hint delivered within 1s");
    assert_eq!(hint["data"]["degraded"], json!([]));
    let inbox_page = fx
        .shared
        .inbox
        .as_ref()
        .unwrap()
        .list(org_proto::inbox::InboxListParams {
            state: org_proto::inbox::InboxState::Open,
            kinds: None,
            project: None,
            team: None,
            cursor: None,
            limit: None,
        })
        .unwrap();
    assert_eq!(
        hint["data"]["counts"],
        serde_json::to_value(&inbox_page.counts).unwrap(),
        "inbox.changed counts must match inbox.list"
    );

    assert_no_duplicate_or_unaddressed_hints(&hr_client, fx, &msg_id, secret_body, client_reader);
    msg_id
}

fn assert_no_duplicate_or_unaddressed_hints(
    hr_client: &org_client::Client,
    fx: &LiveOfficeFixture,
    msg_id: &str,
    secret_body: &str,
    client_reader: &mut BufReader<UnixStream>,
) {
    let _ = hr_client
        .call::<ChatSend>(&ChatSendParams {
            id: msg_id.to_owned(),
            to: fx.owner_id.clone(),
            kind: "msg".into(),
            priority: None,
            subject: Some("Hiring update".into()),
            body: secret_body.into(),
            meta: Meta::new(),
            reply_to: None,
            also_in_channel: false,
            reason: None,
        })
        .unwrap();
    assert!(
        wait_for_event(client_reader, "inbox.changed", Duration::from_millis(400)).is_none(),
        "idempotent replay must not emit a second inbox.changed hint"
    );

    let _ = hr_client
        .call::<ChatChannelCreate>(&ChatChannelCreateParams {
            id: Some(uuidv7(22)),
            name: "quiet-ops".into(),
            members: Vec::new(),
        })
        .unwrap();
    let _ = hr_client
        .call::<ChatSend>(&ChatSendParams {
            id: uuidv7(21),
            to: "#quiet-ops".into(),
            kind: "msg".into(),
            priority: None,
            subject: None,
            body: "No owner mention here.".into(),
            meta: Meta::new(),
            reply_to: None,
            also_in_channel: false,
            reason: None,
        })
        .unwrap();
    assert!(
        wait_for_event(client_reader, "inbox.changed", Duration::from_millis(400)).is_none(),
        "message not addressed to owner must not emit inbox.changed hint"
    );
}

#[test]
fn b6_subscribe_org_changes_presence_header_only_messages_and_inbox_hints() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let fx = LiveOfficeFixture::start();

    {
        let client = org_client::Client::connect(&fx.socket).unwrap();
        let err = client
            .call::<Subscribe>(&SubscribeParams {
                from_position: None,
                from_work_position: None,
            })
            .unwrap_err();
        assert!(
            matches!(err, org_client::ClientError::Api(ref api) if api.code == ErrorCode::NotAuthorized),
            "{err:?}"
        );
    }

    let (mut client_stream, server_stream) = UnixStream::pair().unwrap();
    let mut client_reader = BufReader::new(client_stream.try_clone().unwrap());
    let sub_stop = Arc::new(AtomicBool::new(false));
    let sub_thread = {
        let shared = Arc::clone(&fx.shared);
        let stop = Arc::clone(&sub_stop);
        let owner = fx.owner_id.clone();
        std::thread::spawn(move || {
            serve::serve_grant_bound_stream_for_test(server_stream, &shared, 9001, &stop, &owner);
        })
    };

    let snap = fx.office.org_snapshot().unwrap();
    let beyond = Request::new::<Subscribe>(
        1,
        &SubscribeParams {
            from_position: Some(org_proto::org::Position {
                epoch: snap.position.epoch,
                seq: snap.position.seq + 50,
            }),
            from_work_position: None,
        },
    )
    .unwrap();
    frame::write_message(&mut client_stream, &beyond).unwrap();
    let resp: Response = frame::read_message(&mut client_reader).unwrap().unwrap();
    assert!(
        matches!(resp.outcome, Outcome::Error(ref e) if e.code == ErrorCode::SnapshotRequired),
        "{resp:?}"
    );

    let sub_req = Request::new::<Subscribe>(
        2,
        &SubscribeParams {
            from_position: Some(snap.position),
            from_work_position: None,
        },
    )
    .unwrap();
    frame::write_message(&mut client_stream, &sub_req).unwrap();
    let sub_ack: Response = frame::read_message(&mut client_reader).unwrap().unwrap();
    assert!(matches!(sub_ack.outcome, Outcome::Result(_)), "{sub_ack:?}");

    let cmd_id = assert_subscribe_org_changes_and_replay(&fx, &snap, &mut client_reader);
    let msg_id = assert_presence_delivery_and_inbox_hints(&fx, &mut client_reader);

    let poll_req = Request::new::<ChatEventsPoll>(
        3,
        &ChatEventsPollParams {
            after_org_seq: Some(snap.position.seq),
            after_live_seq: Some(0),
            after_presence_seq: Some(0),
        },
    )
    .unwrap();
    let poll_resp = fx.shared.answer_for_test(&poll_req, &fx.owner_id, true);
    let Outcome::Result(poll_val) = poll_resp.outcome else {
        panic!("poll failed: {poll_resp:?}");
    };
    let poll_res: org_proto::chat::ChatEventsPollResult = serde_json::from_value(poll_val).unwrap();
    assert!(!poll_res.snapshot_required);
    assert!(poll_res.org_changes.iter().any(|c| c.command_id == cmd_id));
    assert!(poll_res
        .messages_received
        .iter()
        .any(|m| m.header.id == msg_id));
    assert!(poll_res
        .presence_changes
        .iter()
        .any(|p| p.member_id == fx.hr_id));

    sub_stop.store(true, Ordering::SeqCst);
    let _ = client_stream.shutdown(std::net::Shutdown::Both);
    let _ = sub_thread.join();
}

#[test]
fn b6_agent_os_commands_session_attach_and_ack1_latency_over_real_office() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let fx = LiveOfficeFixture::start();
    let hr_binding = fx.issue_binding_for(&fx.hr_id);

    // 1. Exercise `agent-os` one-shot CLI commands over the real office socket.
    let run_cli = |args: &[&str]| -> Value {
        let vec: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
        let out = org_agent_cli::execute(&vec, hr_binding.clone());
        assert_eq!(out.exit_code, 0, "args={args:?} out={}", out.stdout);
        serde_json::from_str(&out.stdout).unwrap()
    };

    let channels = run_cli(&["channel", "list"]);
    assert!(
        channels["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["name"] == "general"),
        "{channels}"
    );

    let set_status = run_cli(&["status", "set", "--text", "Working on B6"]);
    assert_eq!(set_status["data"]["status"], "Working on B6");
    let cleared = run_cli(&["status", "clear"]);
    assert!(cleared["data"].get("status").is_none());

    let sent = run_cli(&[
        "msg",
        "send",
        "--to",
        "owner",
        "--subject",
        "CLI check",
        "--body",
        "Hello owner via real office socket!",
    ]);
    assert_eq!(sent["meta"]["ack"], "committed", "{sent}");
    let sent_id = sent["data"]["id"].as_str().unwrap().to_owned();

    // Owner sends a handoff to HR so HR can test inbox, reply, read, accept, defer, done, wait.
    let handoff_id = uuidv7(50);
    let _ = fx
        .office
        .chat_send(
            &fx.owner_id,
            ChatSendParams {
                id: handoff_id.clone(),
                to: fx.hr_id.clone(),
                kind: "handoff".into(),
                priority: None,
                subject: Some("Handoff".into()),
                body: "Please take this task".into(),
                meta: Meta::new(),
                reply_to: None,
                also_in_channel: false,
                reason: None,
            },
        )
        .unwrap()
        .unwrap();

    let inbox_res = run_cli(&["msg", "inbox", "--because", "addressed"]);
    assert!(
        inbox_res["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["id"] == handoff_id && m["delivered_because"] == "addressed"),
        "{inbox_res}"
    );

    let read_res = run_cli(&["msg", "read", "--id", &handoff_id]);
    assert_eq!(read_res["data"]["body"], "Please take this task");

    let accepted = run_cli(&["msg", "accept", "--id", &handoff_id, "--reason", "Accepted"]);
    assert_eq!(accepted["data"]["accepted_by"], fx.hr_id);

    let replied = run_cli(&[
        "msg",
        "reply",
        "--id",
        &handoff_id,
        "--body",
        "Got it, taking ownership.",
    ]);
    assert_eq!(replied["data"]["thread_id"], handoff_id);

    let listed = run_cli(&["msg", "list", "--thread", &handoff_id]);
    assert!(listed["data"].as_array().unwrap().len() >= 2, "{listed}");
    assert!(!sent_id.is_empty());

    let deferred = run_cli(&["msg", "defer", "--id", &handoff_id, "--after-task"]);
    assert_eq!(deferred["data"]["state"], "snoozed");

    let done = run_cli(&["msg", "done", "--id", &handoff_id]);
    assert_eq!(done["data"]["state"], "done");

    let wait_res = run_cli(&["msg", "wait", "--timeout-ms", "20"]);
    assert_eq!(wait_res["data"]["woken"], false);

    // 2. Exercise `session attach` (binds ONCE, answers every line, measures ACK-1 latency).
    let mut office = BoundOffice::new(hr_binding.clone());
    office.ensure_connected().unwrap();
    let mut lines = String::new();
    let sample_count = 15usize;
    for i in 0..sample_count {
        lines.push_str(&format!(
            "{{\"command\":\"msg send\",\"input\":{{\"to\":\"#general\",\"body\":\"ack1 sample {i}\"}}}}\n"
        ));
        lines.push_str("{\"command\":\"msg inbox\",\"input\":{}}\n");
    }
    let start = Instant::now();
    let mut out = Vec::new();
    let exit = run_attach_loop("agent-os", Cursor::new(lines), &mut out, &mut office);
    let elapsed = start.elapsed();
    assert_eq!(exit, 0);
    let rendered = std::str::from_utf8(&out).unwrap();
    let out_lines: Vec<&str> = rendered.lines().collect();
    assert_eq!(out_lines.len(), sample_count * 2);
    for line in &out_lines {
        let parsed: Value = serde_json::from_str(line).unwrap();
        assert_eq!(parsed["ok"], true, "{line}");
    }
    let per_cmd_ms = elapsed.as_secs_f64() * 1000.0 / (out_lines.len() as f64);
    eprintln!(
        "ACK-1 macOS session attach measurement: {} commands in {:.2}ms (avg {:.2}ms/cmd)",
        out_lines.len(),
        elapsed.as_secs_f64() * 1000.0,
        per_cmd_ms
    );
    // ADR-0014 ACK-1 target: committed common commands well below 50ms/cmd on local macOS SSD.
    assert!(
        per_cmd_ms < 50.0,
        "ACK-1 average latency {per_cmd_ms:.2}ms exceeded 50ms budget"
    );
}

#[test]
fn b6_non_owner_grant_caller_cannot_read_other_members_private_messages() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let fx = LiveOfficeFixture::start();
    let snap = fx.office.org_snapshot().unwrap();
    let top_team = snap.teams[0].id.clone();

    // Add a non-owner person (`bob`).
    let bob_id = uuidv7(90);
    let add_bob = Request::new::<org_proto::org::MemberAddPerson>(
        90,
        &org_proto::org::MemberAddPersonParams {
            command_id: uuidv7(91),
            member_id: bob_id.clone(),
            name: "Bob".into(),
            contact: None,
            handle: Some("bob".into()),
            home_team_id: Some(top_team),
        },
    )
    .unwrap();
    let add_resp = fx.shared.answer_for_test(&add_bob, &fx.owner_id, true);
    assert!(
        matches!(add_resp.outcome, Outcome::Result(_)),
        "{add_resp:?}"
    );

    // Owner sends a private DM to HR.
    let secret_msg_id = uuidv7(92);
    let _ = fx
        .office
        .chat_send(
            &fx.owner_id,
            ChatSendParams {
                id: secret_msg_id.clone(),
                to: fx.hr_id.clone(),
                kind: "msg".into(),
                priority: None,
                subject: Some("Owner HR private".into()),
                body: "Private hiring note".into(),
                meta: Meta::new(),
                reply_to: None,
                also_in_channel: false,
                reason: None,
            },
        )
        .unwrap()
        .unwrap();

    // Non-owner grant caller (`bob`, via_grant = true) calls `msg.get` -> refused NotAuthorized/NotFound.
    let get_req = Request::new::<org_proto::msg::MsgGet>(
        93,
        &org_proto::msg::MsgGetParams {
            id: secret_msg_id.clone(),
        },
    )
    .unwrap();
    let bob_get = fx.shared.answer_for_test(&get_req, &bob_id, true);
    assert!(
        matches!(
            bob_get.outcome,
            Outcome::Error(ref e)
                if e.code == ErrorCode::NotAuthorized || e.code == ErrorCode::NotFound
        ),
        "non-owner grant caller must NOT read owner<->HR private message: {bob_get:?}"
    );

    // Non-owner grant caller (`bob`, via_grant = true) calls `msg.list` without participant ->
    // must NOT see owner<->HR private message.
    let list_req = Request::new::<org_proto::msg::MsgList>(
        94,
        &org_proto::msg::MsgListParams {
            participant: None,
            thread_id: None,
            unread_only: false,
            include_snoozed: true,
            cursor: None,
            limit: None,
        },
    )
    .unwrap();
    let bob_list = fx.shared.answer_for_test(&list_req, &bob_id, true);
    if let Outcome::Result(val) = &bob_list.outcome {
        let headers = val["headers"].as_array().unwrap();
        assert!(
            !headers.iter().any(|h| h["id"] == secret_msg_id),
            "non-owner grant caller saw private message in msg.list: {val}"
        );
    }

    // Owner grant caller CAN still read via `msg.get`.
    let owner_get = fx.shared.answer_for_test(&get_req, &fx.owner_id, true);
    assert!(
        matches!(owner_get.outcome, Outcome::Result(_)),
        "owner grant caller should read via msg.get: {owner_get:?}"
    );
}

#[test]
fn b7_msg_ask_short_hold_wait_answer_withdraw_and_live_inbox_surfacing() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let fx = LiveOfficeFixture::start();
    let hr_binding = fx.issue_binding_for(&fx.hr_id);

    let run_cli = |args: &[&str]| -> Value {
        let vec: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
        let out = org_agent_cli::execute(&vec, hr_binding.clone());
        assert_eq!(out.exit_code, 0, "args={args:?} out={}", out.stdout);
        serde_json::from_str(&out.stdout).unwrap()
    };

    // 1. HR agent asks owner a structured 2-question approval ask with a 30ms short hold.
    let ask_json = json!({
        "to": "owner",
        "subject": "Approve hire",
        "approval": true,
        "required": true,
        "wait_ms": 60_000,
        "hold_ms": 30,
        "questions": [
            {
                "id": "decision",
                "question_type": "choice",
                "prompt": "Approve hiring Ada?",
                "options": [
                    { "id": "approve", "label": "Approve" },
                    { "id": "reject", "label": "Reject" }
                ]
            },
            {
                "id": "note",
                "question_type": "free_text",
                "prompt": "Optional note",
                "optional": true
            }
        ]
    });
    let asked = run_cli(&["msg", "ask", "--json", &ask_json.to_string()]);
    assert_eq!(asked["ok"], true, "{asked}");
    assert_eq!(asked["data"]["status"], "waiting", "{asked}");
    let ask_id = asked["data"]["ask_id"].as_str().unwrap().to_owned();

    // 2. Live unified Inbox surfaces the open approval ask with counts.approvals == 1.
    let inbox_open = fx
        .shared
        .inbox
        .as_ref()
        .unwrap()
        .list(org_proto::inbox::InboxListParams {
            state: org_proto::inbox::InboxState::Open,
            kinds: None,
            project: None,
            team: None,
            cursor: None,
            limit: None,
        })
        .unwrap();
    assert_eq!(inbox_open.counts.approvals, 1, "{inbox_open:?}");
    assert!(inbox_open
        .items
        .iter()
        .any(|i| i.reference.ends_with(&format!(":msg:{ask_id}"))
            && i.kind == org_proto::inbox::InboxKind::Approval));

    // 3. Spawn concurrent `agent-os msg wait --ask <id> --timeout-ms 5000`, then answer as owner.
    let wait_binding = hr_binding.clone();
    let wait_ask_id = ask_id.clone();
    let waiter = std::thread::spawn(move || {
        let args = vec![
            "msg".to_owned(),
            "wait".to_owned(),
            "--ask".to_owned(),
            wait_ask_id,
            "--timeout-ms".to_owned(),
            "5000".to_owned(),
        ];
        let out = org_agent_cli::execute(&args, wait_binding);
        assert_eq!(out.exit_code, 0, "{}", out.stdout);
        serde_json::from_str::<Value>(&out.stdout).unwrap()
    });
    std::thread::sleep(Duration::from_millis(40));

    let answer_req = Request::new::<org_proto::ask::ChatAnswer>(
        201,
        &org_proto::ask::ChatAnswerParams {
            ask_id: ask_id.clone(),
            answers: vec![
                org_proto::ask::QuestionAnswer {
                    question_id: "decision".into(),
                    option_ids: vec!["approve".into()],
                    text: None,
                },
                org_proto::ask::QuestionAnswer {
                    question_id: "note".into(),
                    option_ids: Vec::new(),
                    text: Some("Welcome aboard".into()),
                },
            ],
            option_id: None,
            text: None,
        },
    )
    .unwrap();
    let answer_resp = fx.shared.answer_for_test(&answer_req, &fx.owner_id, true);
    assert!(
        matches!(answer_resp.outcome, Outcome::Result(_)),
        "{answer_resp:?}"
    );

    let waited = waiter.join().unwrap();
    assert_eq!(waited["data"]["woken"], true, "{waited}");
    assert_eq!(waited["data"]["status"], "answered", "{waited}");
    assert!(waited["data"]["answer"]
        .as_str()
        .unwrap()
        .contains("Welcome aboard"));

    // Inbox approval count immediately clears to 0!
    let inbox_after = fx
        .shared
        .inbox
        .as_ref()
        .unwrap()
        .list(org_proto::inbox::InboxListParams {
            state: org_proto::inbox::InboxState::Open,
            kinds: None,
            project: None,
            team: None,
            cursor: None,
            limit: None,
        })
        .unwrap();
    assert_eq!(inbox_after.counts.approvals, 0, "{inbox_after:?}");

    // 4. Send a second ask and withdraw it via `agent-os msg withdraw`.
    let ask2_json = json!({
        "to": "owner",
        "hold_ms": 0,
        "questions": [{
            "id": "q1",
            "question_type": "yes_no",
            "prompt": "Keep old branch?",
            "default_value": "no"
        }]
    });
    let asked2 = run_cli(&["msg", "ask", "--json", &ask2_json.to_string()]);
    let ask2_id = asked2["data"]["ask_id"].as_str().unwrap().to_owned();
    let withdrawn = run_cli(&["msg", "withdraw", "--id", &ask2_id, "--reason", "Resolved"]);
    assert_eq!(withdrawn["data"]["ask_id"], ask2_id);
    assert_eq!(withdrawn["data"]["reason"], "Resolved");
}

fn sample_yes_no_ask(id: String, to: &str, hold_ms: u64, addressed_to: Option<String>) -> Request {
    Request::new::<org_proto::ask::ChatAsk>(
        301,
        &org_proto::ask::ChatAskParams {
            id,
            to: to.into(),
            subject: Some("Ask check".into()),
            body: None,
            questions: vec![org_proto::ask::AskQuestion {
                id: "q1".into(),
                question_type: org_proto::ask::AskQuestionType::YesNo,
                prompt: "Proceed?".into(),
                options: Vec::new(),
                allow_multiple: false,
                optional: false,
                default_value: None,
            }],
            required: false,
            wait_ms: Some(60_000),
            hold_ms: Some(hold_ms),
            addressed_to,
            blocks: None,
            approval: false,
            meta: Default::default(),
        },
    )
    .unwrap()
}

#[test]
fn b7_ask_refuses_self_addressed_and_never_blocks_host_channel() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let fx = LiveOfficeFixture::start();

    let self_ask = sample_yes_no_ask(
        "0190a5d2-7c3e-7a01-8b2c-3d4e5f607901".into(),
        "owner",
        0,
        Some(fx.hr_id.clone()),
    );
    let self_resp = fx.shared.answer_for_test(&self_ask, &fx.hr_id, false);
    assert!(
        matches!(self_resp.outcome, Outcome::Error(ref e) if e.code == ErrorCode::InvalidRequest),
        "{self_resp:?}"
    );

    let grant_ask = sample_yes_no_ask(
        "0190a5d2-7c3e-7a01-8b2c-3d4e5f607902".into(),
        "hr",
        90_000,
        None,
    );
    let started = std::time::Instant::now();
    let grant_resp = fx.shared.answer_for_test(&grant_ask, &fx.owner_id, true);
    assert!(
        matches!(grant_resp.outcome, Outcome::Result(_)),
        "{grant_resp:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "via_grant chat.ask must never block host channel: {:?}",
        started.elapsed()
    );
}

#[test]
fn b7_open_ask_stays_in_owner_inbox_after_msg_read() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let fx = LiveOfficeFixture::start();
    let hr_binding = fx.issue_binding_for(&fx.hr_id);

    let owner_ask_json = json!({
        "to": "owner",
        "hold_ms": 0,
        "required": true,
        "approval": true,
        "questions": [{
            "id": "q1",
            "question_type": "yes_no",
            "prompt": "Approve?"
        }]
    });
    let out = org_agent_cli::execute(
        &[
            "msg".into(),
            "ask".into(),
            "--json".into(),
            owner_ask_json.to_string(),
        ],
        hr_binding,
    );
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    let parsed: Value = serde_json::from_str(&out.stdout).unwrap();
    let owner_ask_id = parsed["data"]["ask_id"].as_str().unwrap().to_owned();
    let read_req = Request::new::<org_proto::msg::MsgRead>(
        303,
        &org_proto::msg::MsgReadParams {
            id: Some(owner_ask_id.clone()),
        },
    )
    .unwrap();
    let read_resp = fx.shared.answer_for_test(&read_req, &fx.owner_id, true);
    assert!(
        matches!(read_resp.outcome, Outcome::Result(_)),
        "{read_resp:?}"
    );
    let inbox = fx
        .shared
        .inbox
        .as_ref()
        .unwrap()
        .list(org_proto::inbox::InboxListParams {
            state: org_proto::inbox::InboxState::Open,
            kinds: None,
            project: None,
            team: None,
            cursor: None,
            limit: None,
        })
        .unwrap();
    assert!(
        inbox
            .items
            .iter()
            .any(|i| i.reference.ends_with(&format!(":msg:{owner_ask_id}"))),
        "open ask must stay in owner Inbox after msg.read: {inbox:?}"
    );
}

fn free_text_ask_req(id: &str, to: &str, body: Option<&str>, prompt: String) -> Request {
    Request::new::<org_proto::ask::ChatAsk>(
        310,
        &org_proto::ask::ChatAskParams {
            id: id.into(),
            to: to.into(),
            subject: None,
            body: body.map(str::to_owned),
            questions: vec![org_proto::ask::AskQuestion {
                id: "q1".into(),
                question_type: org_proto::ask::AskQuestionType::FreeText,
                prompt,
                options: Vec::new(),
                allow_multiple: false,
                optional: false,
                default_value: None,
            }],
            required: false,
            wait_ms: Some(60_000),
            hold_ms: Some(0),
            addressed_to: None,
            blocks: None,
            approval: false,
            meta: Default::default(),
        },
    )
    .unwrap()
}

#[test]
fn b7_self_mention_does_not_self_address_and_oversized_card_or_answers_refused() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let fx = LiveOfficeFixture::start();
    let ask_id = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607910";

    // 1. Channel ask mentioning only @hr (the sender) stays open for owner to answer.
    let self_mention_ask = free_text_ask_req(ask_id, "#general", Some("cc @hr"), "Details?".into());
    let asked = fx
        .shared
        .answer_for_test(&self_mention_ask, &fx.hr_id, false);
    assert!(matches!(asked.outcome, Outcome::Result(_)), "{asked:?}");

    // 2. Oversized answer (> MAX_META_BYTES) is refused with TooLarge; normal answer succeeds.
    let make_ans = |text: String| {
        Request::new::<org_proto::ask::ChatAnswer>(
            311,
            &org_proto::ask::ChatAnswerParams {
                ask_id: ask_id.into(),
                answers: vec![org_proto::ask::QuestionAnswer {
                    question_id: "q1".into(),
                    option_ids: Vec::new(),
                    text: Some(text),
                }],
                option_id: None,
                text: None,
            },
        )
        .unwrap()
    };
    let big_resp = fx
        .shared
        .answer_for_test(&make_ans("x".repeat(70_000)), &fx.owner_id, true);
    assert!(
        matches!(big_resp.outcome, Outcome::Error(ref e) if e.code == ErrorCode::TooLarge),
        "{big_resp:?}"
    );
    let ok_resp = fx
        .shared
        .answer_for_test(&make_ans("All clear".into()), &fx.owner_id, true);
    assert!(matches!(ok_resp.outcome, Outcome::Result(_)), "{ok_resp:?}");

    // 3. Oversized AskCard (> MAX_META_BYTES) is refused with TooLarge.
    let huge_ask = free_text_ask_req(
        "0190a5d2-7c3e-7a01-8b2c-3d4e5f607911",
        "owner",
        None,
        "p".repeat(70_000),
    );
    let huge_resp = fx.shared.answer_for_test(&huge_ask, &fx.hr_id, false);
    assert!(
        matches!(huge_resp.outcome, Outcome::Error(ref e) if e.code == ErrorCode::TooLarge),
        "{huge_resp:?}"
    );
}

#[test]
fn c10_who_reports_available_presence_and_send_returns_presence_without_waking_ordinary_posts() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let fx = LiveOfficeFixture::start();

    // 1. AVAIL-4: idle active agent (HR) reports Available in `who` (never Offline/Asleep).
    let who_req =
        Request::new::<org_proto::directory::Who>(401, &org_proto::directory::WhoParams::default())
            .unwrap();
    let who_resp = fx.shared.answer_for_test(&who_req, &fx.owner_id, true);
    let Outcome::Result(who_val) = who_resp.outcome else {
        panic!("who failed: {who_resp:?}");
    };
    let who: org_proto::directory::WhoResult = serde_json::from_value(who_val).unwrap();
    let hr_card = who.members.iter().find(|c| c.id == fx.hr_id).unwrap();
    assert_eq!(
        hr_card.presence,
        org_proto::directory::MemberPresence::Available
    );

    // 2. AVAIL-3: ordinary post in #general does NOT wake HR.
    let post_req = Request::new::<ChatSend>(
        402,
        &org_proto::chat::ChatSendParams {
            id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f607920".into(),
            to: "#general".into(),
            kind: "message".into(),
            priority: None,
            subject: None,
            body: "Good morning team!".into(),
            meta: Default::default(),
            reply_to: None,
            also_in_channel: false,
            reason: None,
        },
    )
    .unwrap();
    let post_resp = fx.shared.answer_for_test(&post_req, &fx.owner_id, true);
    let Outcome::Result(post_val) = post_resp.outcome else {
        panic!("post failed: {post_resp:?}");
    };
    let post_res: org_proto::chat::ChatSendResult = serde_json::from_value(post_val).unwrap();
    assert!(
        post_res.woke_agents.is_empty(),
        "ordinary channel post must wake no agents"
    );

    // 3. AVAIL-5: DM to HR carries recipient_presence with Available.
    let dm_req = Request::new::<ChatSend>(
        403,
        &org_proto::chat::ChatSendParams {
            id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f607921".into(),
            to: "hr".into(),
            kind: "message".into(),
            priority: None,
            subject: None,
            body: "Please check capacity".into(),
            meta: Default::default(),
            reply_to: None,
            also_in_channel: false,
            reason: None,
        },
    )
    .unwrap();
    let dm_resp = fx.shared.answer_for_test(&dm_req, &fx.owner_id, true);
    let Outcome::Result(dm_val) = dm_resp.outcome else {
        panic!("dm failed: {dm_resp:?}");
    };
    let dm_res: org_proto::chat::ChatSendResult = serde_json::from_value(dm_val).unwrap();
    assert!(
        dm_res
            .recipient_presence
            .iter()
            .any(|p| p.member_id == fx.hr_id
                && p.presence == org_proto::directory::MemberPresence::Available),
        "DM must return recipient presence: {dm_res:?}"
    );
}
