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
