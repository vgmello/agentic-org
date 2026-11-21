//! Real-binary integration tests for `agent-os` on `org-command`.

use std::io::BufReader;
use std::os::unix::net::UnixListener;
use std::process::Command;
use std::thread;

use org_proto::directory::{DirectoryCard, MemberPresence, Who, WhoResult};
use org_proto::org::{Member, MemberKind, OrgSnapshot, OrgSnapshotCall, Position, Team};
use org_proto::{frame, Hello, HelloResult, MemberKind as ProtoMemberKind, Request, Response};
use serde_json::Value;

fn bin() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_agent-os"));
    cmd.env_remove("AGENT_OS_SOCKET")
        .env_remove("AGENT_OS_OFFICE")
        .env_remove("AGENT_OS_RUNTIME_DIR")
        .env_remove("ORG_OFFICE_HOME")
        .env_remove("AGENT_OS_SESSION_TOKEN")
        .env_remove("AGENT_OS_AGENT_ID");
    cmd
}

#[test]
fn help_schema_and_version_run_without_binding() {
    let help = bin().arg("--help").output().unwrap();
    assert!(help.status.success());
    let stdout = String::from_utf8(help.stdout).unwrap();
    assert!(
        stdout.contains("org") && stdout.contains("team"),
        "{stdout}"
    );

    let schema = bin().args(["team", "list", "--schema"]).output().unwrap();
    assert!(schema.status.success());
    let env: Value = serde_json::from_slice(&schema.stdout).unwrap();
    assert_eq!(env["ok"], true);
    assert!(env["data"]["output"].is_object());

    let ver = bin().arg("--version").output().unwrap();
    assert!(ver.status.success());
    assert!(String::from_utf8(ver.stdout)
        .unwrap()
        .starts_with("agent-os "));
}

#[test]
fn invalid_cli_exits_2_and_unbound_call_refuses_with_not_authorized() {
    let bad = bin()
        .args(["team", "list", "extra-positional"])
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(2));
    let env: Value = serde_json::from_slice(&bad.stdout).unwrap();
    assert_eq!(env["ok"], false);
    assert_eq!(env["error"]["code"], "invalid_request");

    let unbound = bin().args(["team", "list"]).output().unwrap();
    assert_eq!(unbound.status.code(), Some(1));
    let env: Value = serde_json::from_slice(&unbound.stdout).unwrap();
    assert_eq!(env["ok"], false);
    assert_eq!(env["error"]["code"], "not_authorized");
}

#[test]
fn bound_agent_os_reads_teams_and_directory_over_office_socket() {
    let dir = tempfile::Builder::new().prefix("aos").tempdir().unwrap();
    let sock = dir.path().join("office.sock");
    let listener = UnixListener::bind(&sock).unwrap();

    let server = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            while let Ok(Some(line)) = frame::read_line(&mut reader) {
                let req: Request = serde_json::from_slice(&line).unwrap();
                let resp = if req.is::<Hello>() {
                    let hello: HelloResult = serde_json::from_value(serde_json::json!({
                        "api_version": 2,
                        "server_version": "test",
                        "capabilities": []
                    }))
                    .unwrap();
                    Response::ok::<Hello>(req.id, &hello).unwrap()
                } else if req.is::<OrgSnapshotCall>() {
                    Response::ok::<OrgSnapshotCall>(
                        req.id,
                        &OrgSnapshot {
                            position: Position { epoch: 1, seq: 3 },
                            teams: vec![Team {
                                id: "0193a5c0-0000-7000-8000-000000000001".into(),
                                name: "Core".into(),
                                owner_team_id: None,
                                lead_member_id: None,
                                version: 1,
                            }],
                            members: vec![Member {
                                id: "0193a5c0-0000-7000-8000-000000000002".into(),
                                kind: MemberKind::Person,
                                name: "Owner".into(),
                                handle: Some("owner".into()),
                                home_team_id: Some("0193a5c0-0000-7000-8000-000000000001".into()),
                                title_in_team: None,
                                contact: None,
                                owner: true,
                                office_id: None,
                                agent: None,
                                reports_to_member_id: None,
                                created_at: None,
                                version: 1,
                            }],
                            projects: vec![],
                            project_teams: vec![],
                        },
                    )
                    .unwrap()
                } else if req.is::<Who>() {
                    Response::ok::<Who>(
                        req.id,
                        &WhoResult {
                            members: vec![DirectoryCard {
                                id: "0193a5c0-0000-7000-8000-000000000002".into(),
                                name: "Owner".into(),
                                handle: Some("owner".into()),
                                kind: ProtoMemberKind::Person,
                                owner: true,
                                agent_status: None,
                                role_title: None,
                                teams: vec![],
                                reports_to: None,
                                office: None,
                                presence: MemberPresence::Offline,
                                last_seen_at: None,
                                status: None,
                                status_expires_at: None,
                                current_task_id: None,
                                managed_home: None,
                            }],
                        },
                    )
                    .unwrap()
                } else {
                    panic!("unexpected method: {}", req.method);
                };
                frame::write_message(&mut stream, &resp).unwrap();
            }
        }
    });

    let teams = bin()
        .args(["team", "list"])
        .env("AGENT_OS_SOCKET", &sock)
        .env("AGENT_OS_SESSION_TOKEN", "session-token-fixture")
        .output()
        .unwrap();
    assert!(
        teams.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&teams.stdout),
        String::from_utf8_lossy(&teams.stderr)
    );
    let env: Value = serde_json::from_slice(&teams.stdout).unwrap();
    assert_eq!(env["ok"], true);
    assert_eq!(env["data"][0]["name"], "Core");

    let person = bin()
        .args(["person", "inspect", "--member", "owner"])
        .env("AGENT_OS_SOCKET", &sock)
        .env("AGENT_OS_SESSION_TOKEN", "session-token-fixture")
        .output()
        .unwrap();
    assert!(person.status.success());
    let env: Value = serde_json::from_slice(&person.stdout).unwrap();
    assert_eq!(env["ok"], true);
    assert_eq!(env["data"]["handle"], "owner");

    server.join().unwrap();
}

#[test]
fn selected_office_uuid_addressing_and_unknown_office_or_agent_refusals() {
    let dir = tempfile::Builder::new()
        .prefix("aos-sel")
        .tempdir()
        .unwrap();
    let office_id = "0193a5c0-0000-7000-8000-0000000000aa";
    let missing_office_id = "0193a5c0-0000-7000-8000-0000000000bb";
    let sock = org_proto::location::selected_office_socket_in(dir.path(), office_id).unwrap();
    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&sock).unwrap();

    // 1. Unknown office ID is refused before any RPC, naming the requested office.
    let missing = bin()
        .args(["team", "list"])
        .env("AGENT_OS_OFFICE", missing_office_id)
        .env("AGENT_OS_RUNTIME_DIR", dir.path())
        .env("AGENT_OS_SESSION_TOKEN", "session-token-fixture")
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(1));
    let env: Value = serde_json::from_slice(&missing.stdout).unwrap();
    assert_eq!(env["ok"], false);
    assert_eq!(env["error"]["code"], "not_found");
    assert!(
        env["error"]["message"]
            .as_str()
            .unwrap()
            .contains(missing_office_id),
        "{env}"
    );

    // 2. Malformed office ID is refused as invalid_request.
    let malformed = bin()
        .args(["team", "list"])
        .env("AGENT_OS_OFFICE", "../escape")
        .env("AGENT_OS_RUNTIME_DIR", dir.path())
        .env("AGENT_OS_SESSION_TOKEN", "session-token-fixture")
        .output()
        .unwrap();
    assert_eq!(malformed.status.code(), Some(1));
    let env: Value = serde_json::from_slice(&malformed.stdout).unwrap();
    assert_eq!(env["error"]["code"], "invalid_request");

    // 3. Valid selected office UUIDv7 connects via `<runtime>/<last-12-hex>.sock`, verifies
    //    `hello.office.office.id == office_id`, and refuses unknown agent bind with details.
    let server = thread::spawn(move || {
        for call_idx in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            while let Ok(Some(line)) = frame::read_line(&mut reader) {
                let req: Request = serde_json::from_slice(&line).unwrap();
                let resp = if req.is::<Hello>() {
                    let hello: HelloResult = serde_json::from_value(serde_json::json!({
                        "api_version": 1,
                        "server_version": "test",
                        "host": "test",
                        "capabilities": [],
                        "office": {
                            "office": {
                                "id": office_id,
                                "organization_id": "0193a5c0-0000-7000-8000-000000000001",
                                "name": "HQ",
                                "role": "hq",
                                "host": "test",
                                "status": "active"
                            },
                            "organization_name": "Example"
                        }
                    }))
                    .unwrap();
                    Response::ok::<Hello>(req.id, &hello).unwrap()
                } else if call_idx == 0 && req.is::<OrgSnapshotCall>() {
                    Response::ok::<OrgSnapshotCall>(
                        req.id,
                        &OrgSnapshot {
                            position: Position { epoch: 1, seq: 1 },
                            teams: vec![Team {
                                id: "0193a5c0-0000-7000-8000-000000000001".into(),
                                name: "Selected".into(),
                                owner_team_id: None,
                                lead_member_id: None,
                                version: 1,
                            }],
                            members: vec![],
                            projects: vec![],
                            project_teams: vec![],
                        },
                    )
                    .unwrap()
                } else {
                    Response::error(
                        req.id,
                        org_proto::ApiError::new(
                            org_proto::ErrorCode::NotAuthorized,
                            "unknown agent",
                        ),
                    )
                };
                frame::write_message(&mut stream, &resp).unwrap();
            }
        }
    });

    let ok = bin()
        .args(["team", "list"])
        .env("AGENT_OS_OFFICE", office_id)
        .env("AGENT_OS_RUNTIME_DIR", dir.path())
        .env("AGENT_OS_SESSION_TOKEN", "session-token-fixture")
        .output()
        .unwrap();
    assert!(
        ok.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&ok.stdout),
        String::from_utf8_lossy(&ok.stderr)
    );
    let env: Value = serde_json::from_slice(&ok.stdout).unwrap();
    assert_eq!(env["ok"], true);
    assert_eq!(env["data"][0]["name"], "Selected");

    let bad_agent = bin()
        .args(["team", "list"])
        .env("AGENT_OS_OFFICE", office_id)
        .env("AGENT_OS_RUNTIME_DIR", dir.path())
        .env("AGENT_OS_SESSION_TOKEN", "session-token-fixture")
        .env("AGENT_OS_AGENT_ID", "0193a5c0-0000-7000-8000-0000000000ff")
        .output()
        .unwrap();
    assert_eq!(bad_agent.status.code(), Some(1));
    let env: Value = serde_json::from_slice(&bad_agent.stdout).unwrap();
    assert_eq!(env["ok"], false);
    assert_eq!(env["error"]["code"], "not_authorized");
    let msg = env["error"]["message"].as_str().unwrap();
    assert!(
        msg.contains(office_id) && msg.contains("0193a5c0-0000-7000-8000-0000000000ff"),
        "{msg}"
    );

    server.join().unwrap();
}

#[test]
fn c2_cli14_roles_and_hr_hire_authorized_and_unauthorized() {
    use org_proto::hiring::{
        HireOpResult, HrHire, RoleInspect, RoleList, RoleListResult, RoleRecord,
    };
    use org_proto::msg::{MsgBind, MsgBindResult};

    let dir = tempfile::Builder::new().prefix("aos-c2").tempdir().unwrap();
    let sock = dir.path().join("office.sock");
    let listener = UnixListener::bind(&sock).unwrap();

    let authorized_agent = "0193a5c0-0000-7000-8000-0000000000a1";
    let unauthorized_agent = "0193a5c0-0000-7000-8000-0000000000a2";

    let server = thread::spawn(move || {
        for _ in 0..4 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut bound: Option<String> = None;
            while let Ok(Some(line)) = frame::read_line(&mut reader) {
                let req: Request = serde_json::from_slice(&line).unwrap();
                let resp = if req.is::<Hello>() {
                    let hello: HelloResult = serde_json::from_value(serde_json::json!({
                        "api_version": 2,
                        "server_version": "test",
                        "capabilities": []
                    }))
                    .unwrap();
                    Response::ok::<Hello>(req.id, &hello).unwrap()
                } else if req.is::<MsgBind>() {
                    let p: org_proto::msg::MsgBindParams =
                        serde_json::from_value(req.params.clone()).unwrap();
                    bound = p.member.clone();
                    Response::ok::<MsgBind>(
                        req.id,
                        &MsgBindResult {
                            member_id: bound.clone(),
                            office_id: "0193a5c0-0000-7000-8000-0000000000f1".into(),
                        },
                    )
                    .unwrap()
                } else if req.is::<RoleList>() {
                    Response::ok::<RoleList>(
                        req.id,
                        &RoleListResult {
                            roles: vec![RoleRecord {
                                id: "0193a5c0-0000-7000-8000-000000000040".into(),
                                key: Some("backend-engineer".into()),
                                name: "Backend Engineer".into(),
                                body: "Builds Rust services.".into(),
                                revision: 1,
                                created_at: None,
                                updated_at: None,
                            }],
                        },
                    )
                    .unwrap()
                } else if req.is::<RoleInspect>() {
                    Response::ok::<RoleInspect>(
                        req.id,
                        &RoleRecord {
                            id: "0193a5c0-0000-7000-8000-000000000040".into(),
                            key: Some("backend-engineer".into()),
                            name: "Backend Engineer".into(),
                            body: "Builds Rust services.".into(),
                            revision: 1,
                            created_at: None,
                            updated_at: None,
                        },
                    )
                    .unwrap()
                } else if req.is::<HrHire>() {
                    if bound.as_deref() == Some(authorized_agent) {
                        Response::ok::<HrHire>(
                            req.id,
                            &HireOpResult {
                                request_id: "0193a5c0-0000-7000-8000-000000000050".into(),
                                state: "hired".into(),
                                employee_id: Some("0193a5c0-0000-7000-8000-000000000051".into()),
                                failed_step: None,
                                revision: 1,
                                generation: 5,
                            },
                        )
                        .unwrap()
                    } else {
                        Response::error(
                            req.id,
                            org_proto::ApiError::new(
                                org_proto::ErrorCode::NotAuthorized,
                                "You don't have access to hire. Ask your team lead.",
                            ),
                        )
                    }
                } else {
                    panic!("unexpected method: {}", req.method);
                };
                frame::write_message(&mut stream, &resp).unwrap();
            }
        }
    });

    // 1. role list & role inspect
    let list = bin()
        .args(["role", "list"])
        .env("AGENT_OS_SOCKET", &sock)
        .env("AGENT_OS_SESSION_TOKEN", "session-token-fixture")
        .env("AGENT_OS_AGENT_ID", authorized_agent)
        .output()
        .unwrap();
    assert!(
        list.status.success(),
        "{}",
        String::from_utf8_lossy(&list.stdout)
    );
    let env: Value = serde_json::from_slice(&list.stdout).unwrap();
    assert_eq!(env["data"][0]["key"], "backend-engineer");

    let inspect = bin()
        .args(["role", "inspect", "--role", "backend-engineer"])
        .env("AGENT_OS_SOCKET", &sock)
        .env("AGENT_OS_SESSION_TOKEN", "session-token-fixture")
        .env("AGENT_OS_AGENT_ID", authorized_agent)
        .output()
        .unwrap();
    assert!(inspect.status.success());
    let env: Value = serde_json::from_slice(&inspect.stdout).unwrap();
    assert_eq!(env["data"]["name"], "Backend Engineer");

    // 2. Authorized agent hr hire succeeds.
    let ok_hire = bin()
        .args([
            "hr",
            "hire",
            "--team",
            "0193a5c0-0000-7000-8000-000000000001",
            "--name",
            "Turing",
            "--handle",
            "turing",
            "--reason",
            "Need backend capacity",
        ])
        .env("AGENT_OS_SOCKET", &sock)
        .env("AGENT_OS_SESSION_TOKEN", "session-token-fixture")
        .env("AGENT_OS_AGENT_ID", authorized_agent)
        .output()
        .unwrap();
    assert!(
        ok_hire.status.success(),
        "{}",
        String::from_utf8_lossy(&ok_hire.stdout)
    );
    let env: Value = serde_json::from_slice(&ok_hire.stdout).unwrap();
    assert_eq!(env["data"]["state"], "hired");

    // 3. Unauthorized agent hr hire refuses with ADR-0029 message and hint.
    let denied_hire = bin()
        .args([
            "hr",
            "hire",
            "--team",
            "0193a5c0-0000-7000-8000-000000000001",
            "--name",
            "Turing",
            "--handle",
            "turing",
            "--reason",
            "Unauthorized attempt",
        ])
        .env("AGENT_OS_SOCKET", &sock)
        .env("AGENT_OS_SESSION_TOKEN", "session-token-fixture")
        .env("AGENT_OS_AGENT_ID", unauthorized_agent)
        .output()
        .unwrap();
    assert_eq!(denied_hire.status.code(), Some(1));
    let env: Value = serde_json::from_slice(&denied_hire.stdout).unwrap();
    assert_eq!(env["ok"], false);
    assert_eq!(env["error"]["code"], "not_authorized");
    assert_eq!(
        env["error"]["message"],
        "You don't have access to hire. Ask your team lead."
    );
    assert_eq!(env["error"]["hint"], "Ask your team lead.");

    server.join().unwrap();
}

#[test]
fn session_inspect_grant_list_who_and_run_binding_refusals() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("c3-office.sock");
    let listener = UnixListener::bind(&sock).unwrap();

    let live_agent = "0193a5c0-0000-7000-8000-000000000051";
    let offboarded_agent = "0193a5c0-0000-7000-8000-000000000052";
    let expected_token = "live-run-session-token-0123456789abcdef";

    let server = thread::spawn(move || {
        for _ in 0..5 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut bound_token_ok = false;
            while let Ok(Some(req)) = frame::read_message::<Request>(&mut reader) {
                let resp = match req.method.as_str() {
                    "hello" => Response::ok::<org_proto::Hello>(
                        req.id,
                        &org_proto::HelloResult::office(
                            "0.1.0",
                            "test-host",
                            org_proto::OfficeHello {
                                organization_name: "Acme".into(),
                                office: org_proto::OfficeInfo {
                                    id: "0193a5c0-0000-7000-8000-000000000000".into(),
                                    organization_id: "0193a5c0-0000-7000-8000-000000000099".into(),
                                    name: "HQ".into(),
                                    role: org_proto::OfficeRole::Hq,
                                    host: "test-host".into(),
                                    endpoint: None,
                                    status: org_proto::OfficeStatus::Active,
                                    status_reason: None,
                                    next_restart_at: None,
                                },
                            },
                        ),
                    )
                    .unwrap(),
                    "msg.bind" => {
                        let member = req.params["member"].as_str().unwrap_or("");
                        let token = req.params["session_token"].as_str().unwrap_or("");
                        if member == live_agent && token == expected_token {
                            bound_token_ok = true;
                            Response {
                                id: req.id,
                                outcome: org_proto::Outcome::Result(serde_json::json!({
                                    "member_id": live_agent,
                                    "office_id": "0193a5c0-0000-7000-8000-000000000000"
                                })),
                            }
                        } else {
                            Response::error(
                                req.id,
                                org_proto::ApiError::new(
                                    org_proto::ErrorCode::NotAuthorized,
                                    "this office cannot bind this member",
                                ),
                            )
                        }
                    }
                    "member.session_inspect" if bound_token_ok => Response {
                        id: req.id,
                        outcome: org_proto::Outcome::Result(serde_json::json!({
                            "session": {
                                "employee_id": live_agent,
                                "office_id": "0193a5c0-0000-7000-8000-000000000000",
                                "state": "running",
                                "run_id": "run-1",
                                "session_id": "sess-1",
                                "runtime_id": "claude",
                                "private_home": "/tmp/agent/home",
                                "same_user_isolation_notice": "same-user notice",
                                "permission_preset": "workspace_edits",
                                "sandbox_enabled": true,
                                "consecutive_stalls": 0,
                                "stall_locked": false,
                                "recovery_required": false,
                                "usage": {
                                    "input_tokens": 120,
                                    "output_tokens": 45,
                                    "cache_read_tokens": 0,
                                    "subagent_invocations": 1
                                }
                            },
                            "accepted_commands_count": 2,
                            "spec_hash": "0123456789abcdef"
                        })),
                    },
                    "member.session_grants" if bound_token_ok => Response {
                        id: req.id,
                        outcome: org_proto::Outcome::Result(serde_json::json!({
                            "employee_id": live_agent,
                            "permission_preset": "workspace_edits",
                            "permission_mode": "accept_edits",
                            "sandbox_enabled": true,
                            "writable_roots": ["/tmp/agent/home"],
                            "read_only_dirs": [],
                            "denied_write_patterns": ["/tmp/agent/home/.git/hooks/**"],
                            "denied_read_patterns": [],
                            "rules": [{"effect": "allow", "tool": "Read", "scope": "any"}]
                        })),
                    },
                    "who" if bound_token_ok => Response {
                        id: req.id,
                        outcome: org_proto::Outcome::Result(serde_json::json!({
                            "members": [{
                                "id": live_agent,
                                "name": "Ada",
                                "handle": "ada",
                                "kind": "agent",
                                "owner": false,
                                "agent_status": "active",
                                "role_title": "Engineer",
                                "presence": "working",
                                "teams": []
                            }]
                        })),
                    },
                    other => Response::error(
                        req.id,
                        org_proto::ApiError::new(
                            org_proto::ErrorCode::UnknownMethod,
                            format!("unexpected {other}"),
                        ),
                    ),
                };
                frame::write_message(&mut stream, &resp).unwrap();
            }
        }
    });

    // 0. Unbound agent-os refuses locally without connecting (CLI-12).
    let unbound = bin()
        .args(["session", "inspect"])
        .env("AGENT_OS_SOCKET", &sock)
        .env("AGENT_OS_AGENT_ID", live_agent)
        .env_remove("AGENT_OS_SESSION_TOKEN")
        .output()
        .unwrap();
    assert_eq!(unbound.status.code(), Some(1));
    let env: Value = serde_json::from_slice(&unbound.stdout).unwrap();
    assert_eq!(env["ok"], false);
    assert_eq!(env["error"]["code"], "not_authorized");

    // 1. CLI-9: session inspect
    let inspect = bin()
        .args(["session", "inspect"])
        .env("AGENT_OS_SOCKET", &sock)
        .env("AGENT_OS_SESSION_TOKEN", expected_token)
        .env("AGENT_OS_AGENT_ID", live_agent)
        .output()
        .unwrap();
    assert!(
        inspect.status.success(),
        "{}",
        String::from_utf8_lossy(&inspect.stdout)
    );
    let env: Value = serde_json::from_slice(&inspect.stdout).unwrap();
    assert_eq!(env["ok"], true);
    assert_eq!(env["data"]["session"]["state"], "running");
    assert_eq!(env["data"]["accepted_commands_count"], 2);

    // 2. CLI-10: grant list
    let grants = bin()
        .args(["grant", "list"])
        .env("AGENT_OS_SOCKET", &sock)
        .env("AGENT_OS_SESSION_TOKEN", expected_token)
        .env("AGENT_OS_AGENT_ID", live_agent)
        .output()
        .unwrap();
    assert!(
        grants.status.success(),
        "{}",
        String::from_utf8_lossy(&grants.stdout)
    );
    let env: Value = serde_json::from_slice(&grants.stdout).unwrap();
    assert_eq!(env["data"]["permission_preset"], "workspace_edits");
    assert_eq!(env["data"]["sandbox_enabled"], true);

    // 3. B2 Q9: agent-os who
    let who = bin()
        .args(["who"])
        .env("AGENT_OS_SOCKET", &sock)
        .env("AGENT_OS_SESSION_TOKEN", expected_token)
        .env("AGENT_OS_AGENT_ID", live_agent)
        .output()
        .unwrap();
    assert!(
        who.status.success(),
        "{}",
        String::from_utf8_lossy(&who.stdout)
    );
    let env: Value = serde_json::from_slice(&who.stdout).unwrap();
    assert_eq!(env["data"][0]["handle"], "ada");

    // 4. CLI-12: wrong session token refused on office bind
    let wrong_tok = bin()
        .args(["who"])
        .env("AGENT_OS_SOCKET", &sock)
        .env("AGENT_OS_SESSION_TOKEN", "wrong-token")
        .env("AGENT_OS_AGENT_ID", live_agent)
        .output()
        .unwrap();
    assert_eq!(wrong_tok.status.code(), Some(1));
    let env: Value = serde_json::from_slice(&wrong_tok.stdout).unwrap();
    assert_eq!(env["error"]["code"], "not_authorized");

    // 5. B2 Q9: offboarded agent bind refused
    let offboarded = bin()
        .args(["who"])
        .env("AGENT_OS_SOCKET", &sock)
        .env("AGENT_OS_SESSION_TOKEN", expected_token)
        .env("AGENT_OS_AGENT_ID", offboarded_agent)
        .output()
        .unwrap();
    assert_eq!(offboarded.status.code(), Some(1));
    let env: Value = serde_json::from_slice(&offboarded.stdout).unwrap();
    assert_eq!(env["error"]["code"], "not_authorized");

    server.join().unwrap();
}
