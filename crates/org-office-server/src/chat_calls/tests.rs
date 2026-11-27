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

use crate::federation_runtime::{build_sync_wake_fn, HqFederationSlot};
use crate::federation_stream::SatSyncWake;
use crate::office_ports::{build_messaging_ports, build_office_ports};
use crate::office_runtime::start_session_manager;
use crate::{live, org_feed, serve};

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
        let hq = crate::office_runtime::hq_inbox(Arc::clone(&office)).unwrap();
        let sessions = start_session_manager(Arc::clone(&office), &socket);
        let channel_watcher = Arc::new(crate::watcher::ChannelWatcher::production(
            Arc::clone(&office),
            Some(Arc::clone(&sessions)),
        ));
        let sat_wake = Arc::new(SatSyncWake::default());
        let hq_fed_slot: HqFederationSlot = Arc::default();
        let sync_wake = build_sync_wake_fn(&office, sat_wake, Arc::clone(&hq_fed_slot));
        let messaging = build_messaging_ports(
            &office,
            &live,
            &wake,
            &sessions,
            &channel_watcher,
            Arc::clone(&sync_wake),
        );
        let ports = build_office_ports(
            Arc::clone(&office),
            Arc::clone(&sessions),
            channel_watcher,
            live,
            &wake,
            messaging,
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            hq_fed_slot,
            sync_wake,
        );
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
                None,
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
            external_state: None,
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
            wait_ms: None,
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

fn open_owner_inbox_count(fx: &LiveOfficeFixture) -> u64 {
    let req = Request::new::<org_proto::inbox::InboxList>(
        599,
        &org_proto::inbox::InboxListParams {
            state: org_proto::inbox::InboxState::Open,
            kinds: None,
            project: None,
            team: None,
            cursor: None,
            limit: None,
        },
    )
    .unwrap();
    let resp = fx.shared.answer_for_test(&req, &fx.owner_id, true);
    let Outcome::Result(val) = resp.outcome else {
        panic!("inbox.list failed: {resp:?}");
    };
    let page: org_proto::inbox::InboxListResult = serde_json::from_value(val).unwrap();
    page.counts.total
}

fn chat_inbox_ids(
    fx: &LiveOfficeFixture,
    member: &str,
    is_owner: bool,
    state: Option<org_proto::msg::MessageLifecycleState>,
) -> Vec<String> {
    let req = Request::new::<org_proto::chat::ChatInbox>(
        598,
        &org_proto::chat::ChatInboxParams {
            state,
            ..Default::default()
        },
    )
    .unwrap();
    let resp = fx.shared.answer_for_test(&req, member, is_owner);
    let Outcome::Result(val) = resp.outcome else {
        panic!("chat.inbox failed: {resp:?}");
    };
    let page: org_proto::chat::ChatInboxResult = serde_json::from_value(val).unwrap();
    page.items.into_iter().map(|i| i.id).collect()
}

fn future_rfc3339(offset_ms: u64) -> String {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    org_office::format_rfc3339_ms(now_ms.saturating_add(offset_ms))
}

fn wait_for_state_and_inbox_hint(
    reader: &mut BufReader<UnixStream>,
    expected_state: &str,
    expected_total: u64,
) {
    let (mut got_state, mut got_hint) = (false, false);
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline && (!got_state || !got_hint) {
        if let Some(frame) = read_frame_with_timeout(reader, Duration::from_millis(200)) {
            match frame.get("event").and_then(Value::as_str) {
                Some("message.state_changed")
                    if frame["data"]["state"].as_str() == Some(expected_state) =>
                {
                    got_state = true;
                }
                Some("inbox.changed")
                    if frame["data"]["counts"]["total"].as_u64() == Some(expected_total) =>
                {
                    got_hint = true;
                }
                _ => {}
            }
        }
    }
    assert!(got_state, "missing message.state_changed={expected_state}");
    assert!(got_hint, "missing inbox.changed total={expected_total}");
}

#[test]
fn task81_snooze_unsnooze_resurface_access_and_live_hints() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let fx = LiveOfficeFixture::start();

    let (mut sub_stream, server_stream) = UnixStream::pair().unwrap();
    let mut reader = BufReader::new(sub_stream.try_clone().unwrap());
    let sub_stop = Arc::new(AtomicBool::new(false));
    let sub_thread = {
        let shared = Arc::clone(&fx.shared);
        let stop = Arc::clone(&sub_stop);
        let owner = fx.owner_id.clone();
        std::thread::spawn(move || {
            serve::serve_grant_bound_stream_for_test(server_stream, &shared, 9002, &stop, &owner);
        })
    };
    let snap = fx.office.org_snapshot().unwrap();
    let sub_req = Request::new::<Subscribe>(
        1,
        &SubscribeParams {
            from_position: Some(snap.position),
            from_work_position: None,
        },
    )
    .unwrap();
    frame::write_message(&mut sub_stream, &sub_req).unwrap();
    let ack: Response = frame::read_message(&mut reader).unwrap().unwrap();
    assert!(matches!(ack.outcome, Outcome::Result(_)), "{ack:?}");

    // 1. HR sends DM to owner; owner inbox count is 1.
    let msg_id = uuidv7(810);
    let send_req = Request::new::<ChatSend>(
        501,
        &ChatSendParams {
            id: msg_id.clone(),
            to: "owner".into(),
            kind: "msg".into(),
            priority: None,
            subject: Some("Snooze E2E".into()),
            body: "Review budget".into(),
            meta: Meta::new(),
            reply_to: None,
            also_in_channel: false,
            reason: None,
        },
    )
    .unwrap();
    assert!(matches!(
        fx.shared
            .answer_for_test(&send_req, &fx.hr_id, false)
            .outcome,
        Outcome::Result(_)
    ));
    assert_eq!(open_owner_inbox_count(&fx), 1);

    // 2. Unauthorized caller (HR) cannot snooze or un-snooze owner's message; past timestamp rejected.
    let defer_req = |id_num: u64, at: String| {
        Request::new::<org_proto::msg::MsgDefer>(
            id_num,
            &org_proto::msg::MsgDeferParams {
                id: msg_id.clone(),
                until: org_proto::msg::SnoozeTarget::Until { at },
                note: Some("later".into()),
            },
        )
        .unwrap()
    };
    let unauth =
        fx.shared
            .answer_for_test(&defer_req(502, future_rfc3339(60_000)), &fx.hr_id, false);
    assert!(matches!(
        unauth.outcome,
        Outcome::Error(ref e) if e.code == ErrorCode::NotFound
    ));
    let past = fx.shared.answer_for_test(
        &defer_req(503, "2020-01-01T00:00:00.000Z".into()),
        &fx.owner_id,
        true,
    );
    assert!(matches!(
        past.outcome,
        Outcome::Error(ref e) if e.code == ErrorCode::InvalidRequest
    ));

    // 3. Owner snoozes: hidden from active Inbox & badge count, visible in Snoozed view, live event emitted.
    let deferred =
        fx.shared
            .answer_for_test(&defer_req(504, future_rfc3339(600_000)), &fx.owner_id, true);
    assert!(
        matches!(deferred.outcome, Outcome::Result(_)),
        "{deferred:?}"
    );
    assert_eq!(open_owner_inbox_count(&fx), 0);
    assert!(chat_inbox_ids(&fx, &fx.owner_id, true, None).is_empty());
    assert_eq!(
        chat_inbox_ids(
            &fx,
            &fx.owner_id,
            true,
            Some(org_proto::msg::MessageLifecycleState::Snoozed),
        ),
        vec![msg_id.clone()]
    );
    wait_for_state_and_inbox_hint(&mut reader, "snoozed", 0);

    // 4. Early un-snooze restores to active Inbox & badge count and emits state_changed + inbox.changed.
    let unsnooze_req = Request::new::<org_proto::msg::MsgUnsnooze>(
        505,
        &org_proto::msg::MsgUnsnoozeParams { id: msg_id.clone() },
    )
    .unwrap();
    let unauth_u = fx.shared.answer_for_test(&unsnooze_req, &fx.hr_id, false);
    assert!(matches!(
        unauth_u.outcome,
        Outcome::Error(ref e) if e.code == ErrorCode::NotFound
    ));
    let unsnoozed = fx.shared.answer_for_test(&unsnooze_req, &fx.owner_id, true);
    assert!(
        matches!(unsnoozed.outcome, Outcome::Result(_)),
        "{unsnoozed:?}"
    );
    assert_eq!(open_owner_inbox_count(&fx), 1);
    assert_eq!(
        chat_inbox_ids(&fx, &fx.owner_id, true, None),
        vec![msg_id.clone()]
    );
    wait_for_state_and_inbox_hint(&mut reader, "unread", 1);

    // 5. Short snooze resurfaces automatically via release_all_due_snoozes once due.
    let short_defer =
        fx.shared
            .answer_for_test(&defer_req(506, future_rfc3339(600)), &fx.owner_id, true);
    assert!(
        matches!(short_defer.outcome, Outcome::Result(_)),
        "{short_defer:?}"
    );
    assert_eq!(open_owner_inbox_count(&fx), 0);
    wait_for_state_and_inbox_hint(&mut reader, "snoozed", 0);
    std::thread::sleep(Duration::from_millis(650));
    let released = fx
        .shared
        .mailbox()
        .unwrap()
        .release_all_due_snoozes()
        .unwrap();
    assert_eq!(released, 1);
    assert_eq!(open_owner_inbox_count(&fx), 1);
    assert_eq!(chat_inbox_ids(&fx, &fx.owner_id, true, None), vec![msg_id]);
    wait_for_state_and_inbox_hint(&mut reader, "unread", 1);

    sub_stop.store(true, Ordering::SeqCst);
    drop(sub_stream);
    drop(reader);
    let _ = sub_thread.join();
}

fn seed_process_doc(
    fx: &LiveOfficeFixture,
    seed: u16,
    update: Option<(String, u64)>,
    scope: org_proto::processes::ProcessScopeWire,
    key: &str,
    title: &str,
    body: String,
) -> String {
    let (document_id, expected_revision) = match update {
        Some((id, rev)) => (Some(id), Some(rev)),
        None => (None, None),
    };
    let params = org_proto::processes::ProcessWriteParams {
        command_id: uuidv7(seed),
        document_id,
        kind: org_proto::processes::DocumentKindWire::Process,
        scope,
        key: Some(key.into()),
        title: title.into(),
        status: org_proto::processes::DocumentStatusWire::Current,
        owner_employee_id: Some(fx.owner_id.clone()),
        overridable: true,
        body_markdown: body,
        expected_revision,
    };
    let req = Request::new::<org_proto::processes::ProcessWrite>(u64::from(seed), &params).unwrap();
    let resp = fx.shared.answer_for_test(&req, &fx.owner_id, true);
    let Outcome::Result(val) = resp.outcome else {
        panic!("process.write failed: {resp:?}");
    };
    val["document_id"].as_str().unwrap().to_owned()
}

fn assert_process_inspect_continuation_guards(
    fx: &LiveOfficeFixture,
    hr_binding: &Binding,
    root_team_id: &str,
    team_doc_id: &str,
    stale_rev: u64,
    next_offset: u64,
) {
    use org_proto::processes::ProcessScopeWire;

    // Updating the document invalidates stale pinned continuation reads with `conflict`.
    seed_process_doc(
        fx,
        603,
        Some((team_doc_id.to_owned(), stale_rev)),
        ProcessScopeWire::Team {
            team_id: root_team_id.to_owned(),
        },
        "big-proc",
        "Root Team Big Process v2",
        "éx updated body".into(),
    );
    let stale_out = org_agent_cli::execute(
        &[
            "process".into(),
            "inspect".into(),
            "--id".into(),
            team_doc_id.to_owned(),
            "--revision".into(),
            stale_rev.to_string(),
            "--offset".into(),
            next_offset.to_string(),
        ],
        hr_binding.clone(),
    );
    assert_eq!(stale_out.exit_code, 1, "{}", stale_out.stdout);
    let stale_env: Value = serde_json::from_str(&stale_out.stdout).unwrap();
    assert_eq!(stale_env["error"]["code"], "conflict");
    let hint = stale_env["error"]["hint"].as_str().unwrap();
    assert!(
        hint.contains("offset 0") && !hint.contains("--if-match"),
        "unexpected conflict hint: {hint}"
    );

    // Pinning --revision with --key instead of --id fails closed to prevent cross-document swaps.
    let key_pin = org_agent_cli::execute(
        &[
            "process".into(),
            "inspect".into(),
            "--key".into(),
            "big-proc".into(),
            "--revision".into(),
            stale_rev.to_string(),
            "--offset".into(),
            next_offset.to_string(),
        ],
        hr_binding.clone(),
    );
    assert_eq!(key_pin.exit_code, 1, "{}", key_pin.stdout);
    let key_pin_env: Value = serde_json::from_str(&key_pin.stdout).unwrap();
    assert_eq!(key_pin_env["error"]["code"], "invalid_request");

    // Non-UTF-8-boundary offset inside multi-byte `é` fails closed with `invalid_request`.
    let mid_char = org_agent_cli::execute(
        &[
            "process".into(),
            "inspect".into(),
            "--id".into(),
            team_doc_id.to_owned(),
            "--offset".into(),
            "1".into(),
        ],
        hr_binding.clone(),
    );
    assert_eq!(mid_char.exit_code, 1, "{}", mid_char.stdout);
    let mid_env: Value = serde_json::from_str(&mid_char.stdout).unwrap();
    assert_eq!(mid_env["error"]["code"], "invalid_request");
}

fn assert_large_why_and_metadata_only_rpcs(
    fx: &LiveOfficeFixture,
    hr_binding: &Binding,
    root_team_id: &str,
) {
    // Run `--why` while BOTH the 500 KiB parent and 512 KiB escaped winner are live
    // (would exceed the 1 MiB socket frame if `body_markdown` were not stripped).
    let why_out = org_agent_cli::execute(
        &[
            "process".into(),
            "inspect".into(),
            "--key".into(),
            "big-proc".into(),
            "--why".into(),
        ],
        hr_binding.clone(),
    );
    assert_eq!(why_out.exit_code, 0, "{}", why_out.stdout);
    let why_env: Value = serde_json::from_str(&why_out.stdout).unwrap();
    let chain = why_env["data"]["chain"].as_array().unwrap();
    assert_eq!(chain.len(), 2);
    assert_eq!(chain[0]["document"]["level"], "organization");
    assert_eq!(chain[1]["document"]["team_id"], root_team_id);
    assert!(chain[1]["document"]["scope_name"].as_str().is_some());
    assert_eq!(why_env["data"]["winner"]["team_id"], root_team_id);

    // Direct RPC check: `process.why` and `process.show` return metadata-only documents.
    let raw_why = fx
        .office
        .process_why(
            &fx.hr_id,
            org_proto::processes::ProcessWhyParams {
                key: "big-proc".into(),
                employee_id: None,
                r#for: None,
            },
        )
        .unwrap()
        .unwrap();
    assert!(raw_why
        .chain
        .iter()
        .all(|l| l.document.body_markdown.is_empty()));
    assert!(raw_why.winner.unwrap().body_markdown.is_empty());
    let raw_show = fx
        .office
        .process_show(
            &fx.hr_id,
            org_proto::processes::ProcessShowParams {
                key: Some("big-proc".into()),
                document_id: None,
                employee_id: None,
                r#for: None,
                offset: Some(0),
                revision: None,
            },
        )
        .unwrap()
        .unwrap();
    assert!(raw_show.document.body_markdown.is_empty());
    assert!(raw_show.parent.unwrap().body_markdown.is_empty());
}

#[test]
fn task95_process_inspect_paginates_large_escaped_doc_and_preserves_why_over_real_socket() {
    use org_proto::processes::ProcessScopeWire;

    let fx = LiveOfficeFixture::start();
    let root_team_id = fx.office.org_snapshot().unwrap().teams[0].id.clone();

    // 512 KiB of JSON-escapable quotes/newlines (> 1 MiB if JSON-escaped in one frame!)
    // plus a large parent doc; must paginate cleanly over the real 1 MiB framed socket.
    let parent_body = "p".repeat(500 * 1024);
    let big_body = "\"\n".repeat(256 * 1024);
    assert_eq!(big_body.len(), 512 * 1024);

    seed_process_doc(
        &fx,
        601,
        None,
        ProcessScopeWire::Organization,
        "big-proc",
        "Org Big Process",
        parent_body,
    );
    let team_doc_id = seed_process_doc(
        &fx,
        602,
        None,
        ProcessScopeWire::Team {
            team_id: root_team_id.clone(),
        },
        "big-proc",
        "Root Team Big Process",
        big_body.clone(),
    );

    let hr_binding = fx.issue_binding_for(&fx.hr_id);
    let mut assembled = String::new();
    let mut offset: Option<u64> = None;
    let mut pinned: Option<(String, u64)> = None;
    let mut first_next_offset = 0u64;
    let mut pages = 0usize;
    loop {
        let mut args = vec!["process".into(), "inspect".into()];
        if let (Some((id, rev)), Some(off)) = (&pinned, offset) {
            args.extend([
                "--id".into(),
                id.clone(),
                "--revision".into(),
                rev.to_string(),
                "--offset".into(),
                off.to_string(),
            ]);
        } else {
            args.extend(["--key".into(), "big-proc".into()]);
        }
        let out = org_agent_cli::execute(&args, hr_binding.clone());
        assert_eq!(out.exit_code, 0, "page {pages} failed: {}", out.stdout);
        let env: Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(env["data"]["total_bytes"], big_body.len());
        assert_eq!(env["data"]["parent"]["level"], "organization");
        assert_eq!(env["data"]["document"]["team_id"], root_team_id);
        if pages == 0 {
            let id = env["data"]["document"]["id"].as_str().unwrap().to_owned();
            let rev = env["data"]["document"]["revision"].as_u64().unwrap();
            first_next_offset = env["data"]["next_offset"].as_u64().unwrap();
            pinned = Some((id, rev));
        }
        assembled.push_str(env["data"]["content"].as_str().unwrap());
        pages += 1;
        offset = env["data"]["next_offset"].as_u64();
        if offset.is_none() {
            break;
        }
        assert!(pages <= 4, "unexpected excessive pagination: {pages}");
    }
    assert!(
        pages >= 2,
        "expected escaped 512 KiB doc to span >= 2 frames"
    );
    assert_eq!(assembled, big_body);

    assert_large_why_and_metadata_only_rpcs(&fx, &hr_binding, &root_team_id);

    let (_, pinned_rev) = pinned.unwrap();
    assert_process_inspect_continuation_guards(
        &fx,
        &hr_binding,
        &root_team_id,
        &team_doc_id,
        pinned_rev,
        first_next_offset,
    );
}

#[test]
fn task95_real_office_dry_run_forwarded_and_refused_writes_leave_store_unchanged() {
    struct OwnerServerTransport<'a> {
        fx: &'a LiveOfficeFixture,
        next_id: u64,
    }
    impl org_agent_cli::dispatch::OfficeTransport for OwnerServerTransport<'_> {
        fn call_method(
            &mut self,
            method: &str,
            params: Value,
        ) -> Result<Value, org_agent_cli::dispatch::Refusal> {
            self.next_id += 1;
            let req = Request {
                id: self.next_id,
                method: method.to_owned(),
                params,
                principal: None,
            };
            match self
                .fx
                .shared
                .answer_for_test(&req, &self.fx.owner_id, true)
                .outcome
            {
                Outcome::Result(v) => Ok(v),
                Outcome::Error(err) => Err(org_agent_cli::dispatch::Refusal::new(
                    err.code,
                    err.message,
                    "Check office error.",
                )),
            }
        }
    }

    let fx = LiveOfficeFixture::start();
    let before = fx.office.org_snapshot().unwrap();
    let mut office = org_agent_cli::dispatch::TransportOffice(OwnerServerTransport {
        fx: &fx,
        next_id: 700,
    });

    for args in [
        vec!["team", "create", "--name", "DryTeam", "--dry-run"],
        vec!["person", "add", "--name", "DryPerson", "--dry-run"],
        vec!["project", "create", "--name", "DryProj", "--dry-run"],
    ] {
        let owned: Vec<String> = args.into_iter().map(str::to_owned).collect();
        let out = org_agent_cli::execute_with_office("agent-os", &owned, &mut office);
        assert_eq!(out.exit_code, 0, "{owned:?} failed: {}", out.stdout);
        let env: Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(env["ok"], Value::Bool(true));
        assert_eq!(env["meta"]["dry_run"], Value::Bool(true));
        assert_eq!(env["data"]["status"], "checked");
    }

    // Refused msg write over real socket produces zero inbox items.
    let hr_binding = fx.issue_binding_for(&fx.hr_id);
    let msg_out = org_agent_cli::execute(
        &[
            "msg".into(),
            "send".into(),
            "--to".into(),
            "owner".into(),
            "--body".into(),
            "must not send".into(),
            "--dry-run".into(),
        ],
        hr_binding,
    );
    assert_eq!(msg_out.exit_code, 1, "{}", msg_out.stdout);
    let msg_env: Value = serde_json::from_str(&msg_out.stdout).unwrap();
    assert_eq!(msg_env["error"]["code"], "unsupported");
    assert_eq!(msg_env["meta"]["dry_run"], Value::Bool(true));

    let after = fx.office.org_snapshot().unwrap();
    assert_eq!(before.position, after.position);
    assert_eq!(before.teams.len(), after.teams.len());
    assert_eq!(before.members.len(), after.members.len());
    assert_eq!(before.projects.len(), after.projects.len());
    assert_eq!(open_owner_inbox_count(&fx), 0);
}

#[test]
fn task115_agent_os_waiting_messages_meta_and_active_ask_waiter_tracking() {
    let fx = LiveOfficeFixture::start();
    let hr_binding = fx.issue_binding_for(&fx.hr_id);

    // 1. With 0 waiting messages, meta.waiting_messages MUST be omitted completely.
    let clean_out = org_agent_cli::execute(&["org".into(), "inspect".into()], hr_binding.clone());
    assert_eq!(clean_out.exit_code, 0, "{}", clean_out.stdout);
    let clean_env: Value = serde_json::from_str(&clean_out.stdout).unwrap();
    assert!(
        clean_env["meta"].get("waiting_messages").is_none(),
        "zero waiting_messages must be omitted: {clean_env}"
    );

    // 2. When the per-employee in-memory AtomicU32 is non-zero, agent-os attaches meta.waiting_messages.
    let counter = fx.sessions.waiting_messages_counter(&fx.hr_id);
    counter.store(3, std::sync::atomic::Ordering::Relaxed);
    let hinted_out = org_agent_cli::execute(&["org".into(), "inspect".into()], hr_binding);
    assert_eq!(hinted_out.exit_code, 0, "{}", hinted_out.stdout);
    let hinted_env: Value = serde_json::from_str(&hinted_out.stdout).unwrap();
    assert_eq!(hinted_env["meta"]["waiting_messages"], Value::from(3));
    counter.store(0, std::sync::atomic::Ordering::Relaxed);

    // 3. Human grant calls never carry waiting_messages even if a counter is non-zero.
    let owner_counter = fx.sessions.waiting_messages_counter(&fx.owner_id);
    owner_counter.store(5, std::sync::atomic::Ordering::Relaxed);
    let req = Request::new::<org_proto::OrgSnapshotCall>(901, &org_proto::Empty {}).unwrap();
    let owner_resp = fx.shared.answer_for_test(&req, &fx.owner_id, true);
    assert_eq!(owner_resp.waiting_messages, None);
    owner_counter.store(0, std::sync::atomic::Ordering::Relaxed);
}

#[test]
fn task140_chat_events_poll_wakes_immediately_on_commit_and_respects_timeout() {
    let fx = LiveOfficeFixture::start();
    let owner_id = fx.owner_id.clone();

    // 1. Bootstrap initial cursors with wait_ms = None (returns immediately).
    let boot_req = Request::new::<ChatEventsPoll>(
        910,
        &ChatEventsPollParams {
            after_org_seq: None,
            after_live_seq: None,
            after_presence_seq: None,
            wait_ms: None,
        },
    )
    .unwrap();
    let Outcome::Result(boot_val) = fx
        .shared
        .answer_for_test(&boot_req, &owner_id, true)
        .outcome
    else {
        panic!("bootstrap chat.events_poll failed");
    };
    let boot: org_proto::chat::ChatEventsPollResult = serde_json::from_value(boot_val).unwrap();

    // 2. Idle wait_ms with no events returns cleanly after the requested window.
    let idle_req = Request::new::<ChatEventsPoll>(
        911,
        &ChatEventsPollParams {
            after_org_seq: Some(boot.org_head_seq),
            after_live_seq: Some(boot.live_head_seq),
            after_presence_seq: Some(boot.presence_head_seq),
            wait_ms: Some(120),
        },
    )
    .unwrap();
    let idle_start = Instant::now();
    let Outcome::Result(idle_val) = fx
        .shared
        .answer_for_test(&idle_req, &owner_id, true)
        .outcome
    else {
        panic!("idle chat.events_poll failed");
    };
    let idle: org_proto::chat::ChatEventsPollResult = serde_json::from_value(idle_val).unwrap();
    assert!(
        idle_start.elapsed() >= Duration::from_millis(100),
        "idle wait returned too early: {:?}",
        idle_start.elapsed()
    );
    assert!(idle.org_changes.is_empty() && idle.messages_received.is_empty());

    // 3. Long wait_ms (3000 ms) wakes via FeedWake condvar in < 400 ms on committed org change.
    let shared_poll = Arc::clone(&fx.shared);
    let owner_poll = owner_id.clone();
    let long_req = Request::new::<ChatEventsPoll>(
        912,
        &ChatEventsPollParams {
            after_org_seq: Some(boot.org_head_seq),
            after_live_seq: Some(boot.live_head_seq),
            after_presence_seq: Some(boot.presence_head_seq),
            wait_ms: Some(3_000),
        },
    )
    .unwrap();
    let started = Instant::now();
    let waiter = std::thread::spawn(move || {
        let resp = shared_poll.answer_for_test(&long_req, &owner_poll, true);
        (started.elapsed(), resp)
    });
    std::thread::sleep(Duration::from_millis(40));

    let root_team = fx.office.org_snapshot().unwrap().teams[0].id.clone();
    let create_req = Request::new::<TeamCreate>(
        913,
        &TeamCreateParams {
            command_id: uuidv7(91),
            team_id: uuidv7(92),
            name: "PushWakeTeam".into(),
            owner_team_id: Some(root_team),
        },
    )
    .unwrap();
    assert!(matches!(
        fx.shared
            .answer_for_test(&create_req, &owner_id, true)
            .outcome,
        Outcome::Result(_)
    ));

    let (elapsed, woke_resp) = waiter.join().unwrap();
    let Outcome::Result(woke_val) = woke_resp.outcome else {
        panic!("push-woken chat.events_poll failed: {woke_resp:?}");
    };
    let woke: org_proto::chat::ChatEventsPollResult = serde_json::from_value(woke_val).unwrap();
    assert_eq!(woke.org_changes.len(), 1);
    assert!(
        elapsed < Duration::from_millis(600),
        "chat.events_poll must wake promptly via FeedWake, took {elapsed:?}"
    );
}

fn events_poll_as(
    fx: &LiveOfficeFixture,
    member: &str,
    params: &ChatEventsPollParams,
) -> org_proto::chat::ChatEventsPollResult {
    let req = Request::new::<ChatEventsPoll>(930, params).unwrap();
    let Outcome::Result(val) = fx.shared.answer_for_test(&req, member, true).outcome else {
        panic!("chat.events_poll failed for {member}");
    };
    serde_json::from_value(val).unwrap()
}

/// Agents' coarse live activity streams over `chat.events_poll` (the web's `presence` feed):
/// coalesced to the latest per agent, carrying a phase and a tool's name only, and the Chief
/// of Staff's tool detail only for the owner (`TASK-103`).
#[test]
fn task103_events_poll_streams_coalesced_agent_activity_and_keeps_tess_detail_for_the_owner() {
    use org_office_sessions::{
        SessionActivity, SessionStreamEvent, SessionStreamFrame, SessionStreamKey,
    };
    use org_proto::sessions::ActivityPhase;

    let _spawn = crate::transfer::tests::without_process_spawn();
    let fx = LiveOfficeFixture::start();
    let tess = fx
        .office
        .chief_of_staff()
        .unwrap()
        .expect("Tess")
        .id
        .to_string();
    let top_team = fx.office.org_snapshot().unwrap().teams[0].id.clone();
    let bob = uuidv7(93);
    let add_bob = Request::new::<org_proto::org::MemberAddPerson>(
        93,
        &org_proto::org::MemberAddPersonParams {
            command_id: uuidv7(94),
            member_id: bob.clone(),
            name: "Bob".into(),
            contact: None,
            handle: Some("bob".into()),
            home_team_id: Some(top_team),
        },
    )
    .unwrap();
    assert!(matches!(
        fx.shared
            .answer_for_test(&add_bob, &fx.owner_id, true)
            .outcome,
        Outcome::Result(_)
    ));
    let boot = events_poll_as(&fx, &fx.owner_id, &ChatEventsPollParams::default());
    let cursors = ChatEventsPollParams {
        after_org_seq: Some(boot.org_head_seq),
        after_live_seq: Some(boot.live_head_seq),
        after_presence_seq: Some(boot.presence_head_seq),
        wait_ms: None,
    };

    let publish = |employee_id: &str, partition: Option<&str>, activity: SessionActivity| {
        fx.sessions.stream_hub().publish(&SessionStreamFrame {
            key: SessionStreamKey {
                org_id: fx.office.info().org_id.clone(),
                employee_id: employee_id.to_owned(),
                partition: partition.map(str::to_owned),
            },
            at: "2026-11-27T10:00:00Z".into(),
            event: SessionStreamEvent::Activity(activity),
        });
    };
    let running = |tool: &str| SessionActivity {
        phase: Some(ActivityPhase::RunningTool),
        active_tool_name: Some(tool.into()),
        active_tool_started_at: Some("2026-11-27T10:00:00Z".into()),
        supports_steer: false,
    };
    for n in 0..2_000 {
        let tool = if n % 2 == 0 { "Bash" } else { "Read" };
        publish(&fx.hr_id, None, running(tool));
    }
    publish(
        &tess,
        Some(&format!("tess:ask:{}", fx.owner_id)),
        running("Bash"),
    );

    let owner = events_poll_as(&fx, &fx.owner_id, &cursors);
    assert!(!owner.live_lost, "activity never evicts message events");
    assert_eq!(
        owner.activity_changes.len(),
        2,
        "{:?}",
        owner.activity_changes
    );
    let hr = &owner.activity_changes[0];
    assert_eq!(hr.member_id, fx.hr_id);
    assert_eq!(hr.active_tool_name.as_deref(), Some("Read"), "the latest");
    let tess_for_owner = &owner.activity_changes[1];
    assert_eq!(tess_for_owner.member_id, tess);
    assert_eq!(tess_for_owner.active_tool_name.as_deref(), Some("Bash"));

    let other = events_poll_as(&fx, &bob, &cursors);
    assert_eq!(other.activity_changes.len(), 2);
    assert_eq!(
        other.activity_changes[0].active_tool_name.as_deref(),
        Some("Read")
    );
    assert_eq!(other.activity_changes[1].member_id, tess);
    assert_eq!(
        other.activity_changes[1].phase,
        Some(ActivityPhase::RunningTool)
    );
    assert_eq!(other.activity_changes[1].active_tool_name, None);
    assert_eq!(other.activity_changes[1].active_tool_started_at, None);
    let wire = serde_json::to_value(&other).unwrap();
    assert!(wire["activity_changes"][1]
        .get("active_tool_name")
        .is_none());

    // A poll after the latest change sees nothing new.
    let again = events_poll_as(
        &fx,
        &fx.owner_id,
        &ChatEventsPollParams {
            after_presence_seq: Some(owner.presence_head_seq),
            ..cursors
        },
    );
    assert!(again.activity_changes.is_empty());
}
