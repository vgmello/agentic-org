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
