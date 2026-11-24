//! Real-process E2E proof for `TASK-42` (`C3`: session inspection, grants summary,
//! browser-driven CLI provider enrollment, wrapped credential custody, and revocation
//! over `/api/v1` through real `org-host`, `agentic-org-web`, and `agentic-org-office`).

use std::fs;
use std::net::SocketAddr;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::{
    browser_sessions::{credentials, pair, wait_serving},
    create_body, http, launch, lookup, post_until_available, remote, Launched, Seed,
};
use crate::web_process_tests::organizations::{stop, wait_offices_active};

fn wait_org_completed(
    address: SocketAddr,
    headers: &str,
    instance_id: &str,
    operation_id: &str,
) -> Value {
    let until = Instant::now() + Duration::from_secs(120);
    loop {
        let reply = lookup(address, headers, instance_id, operation_id);
        assert_eq!(reply.status, 200, "lookup: {}", reply.body);
        let outcome = &reply.body["result"];
        if outcome["outcome"] == "completed" {
            return outcome["result"].clone();
        }
        assert_eq!(outcome["outcome"], "accepted", "create: {}", reply.body);
        assert!(Instant::now() < until, "timed out waiting for org creation");
        thread::sleep(Duration::from_millis(50));
    }
}

fn post_api(
    address: SocketAddr,
    headers: &str,
    org_id: &str,
    path: &str,
    body: Value,
) -> super::Reply {
    http(
        address,
        "POST",
        &format!("/api/v1/organizations/{org_id}/{path}"),
        headers,
        &body.to_string(),
    )
}

fn get_api(address: SocketAddr, headers: &str, org_id: &str, path: &str) -> super::Reply {
    http(
        address,
        "GET",
        &format!("/api/v1/organizations/{org_id}/{path}"),
        headers,
        "",
    )
}

#[test]
fn c3_sessions_grants_and_cli_enrollment_e2e_over_real_web_and_office() {
    let Launched {
        host,
        address,
        socket,
        base,
        temp: _temp,
        ..
    } = launch(Seed::Administrator);
    wait_serving(&host, address);

    let sign_in = post_until_available(address, "/api/v1/session/sign-in", &credentials());
    assert_eq!(sign_in.status, 200, "sign-in: {}", sign_in.body);
    let headers = pair(&sign_in, "c3-admin").headers();
    let instance_id = sign_in.body["result"]["instance_id"]
        .as_str()
        .unwrap()
        .to_owned();

    let op_id = crate::ids::uuid_v7().unwrap();
    let create_req = create_body(
        &instance_id,
        &op_id,
        "Acme C3",
        "acme-c3",
        remote("acme-c3"),
    );
    let create = http(
        address,
        "POST",
        "/api/v1/organizations",
        &headers,
        &create_req,
    );
    assert!(
        create.status == 200 || create.status == 202,
        "org create: {}",
        create.body
    );
    let org = wait_org_completed(address, &headers, &instance_id, &op_id);
    let org_id = org["id"].as_str().unwrap().to_owned();
    let office_id = org["offices"][0]["id"].as_str().unwrap().to_owned();
    wait_offices_active(&host, 1);

    let snap = get_api(address, &headers, &org_id, "snapshot");
    assert_eq!(snap.status, 200, "snapshot: {}", snap.body);
    let top_team_id = snap.body["result"]["snapshot"]["teams"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // 1. Create provider connection + hire an agent.
    let conn_id = crate::ids::uuid_v7().unwrap();
    let profile_id = crate::ids::uuid_v7().unwrap();
    let conn_res = post_api(
        address,
        &headers,
        &org_id,
        "provider-connections",
        json!({
            "command_id": crate::ids::uuid_v7().unwrap(),
            "connection_id": conn_id,
            "default_profile_id": profile_id,
            "key": "claude-c3",
            "name": "Claude C3",
            "provider_type": "anthropic",
            "auth_mode": "api_key",
            "runtime_driver": "claude-code",
            "max_concurrent_turns": 2
        }),
    );
    assert_eq!(conn_res.status, 200, "provider conn: {}", conn_res.body);

    let agent_id = crate::ids::uuid_v7().unwrap();
    let hire_res = post_api(
        address,
        &headers,
        &org_id,
        "hires/direct",
        json!({
            "command_id": crate::ids::uuid_v7().unwrap(),
            "request_id": crate::ids::uuid_v7().unwrap(),
            "employee_id": agent_id,
            "home_team_id": top_team_id,
            "proposal": {
                "name": "Grace Hopper",
                "handle": "grace",
                "title": "Compiler Engineer",
                "model_profile_id": profile_id,
                "model_id": "claude-sonnet-4-6",
                "permission_preset": "workspace_edits"
            },
            "reason": "C3 E2E agent"
        }),
    );
    assert_eq!(hire_res.status, 200, "hire: {}", hire_res.body);

    // High #2: Hired agent appears in org.snapshot with office_id and AgentProfile.
    let snap_after_hire = get_api(address, &headers, &org_id, "snapshot");
    assert_eq!(snap_after_hire.status, 200);
    let hired_member = snap_after_hire.body["result"]["snapshot"]["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == agent_id)
        .expect("hired agent present in snapshot");
    assert_eq!(hired_member["office_id"], office_id);
    assert_eq!(hired_member["agent"]["status"], "active");
    assert_eq!(hired_member["agent"]["messaging_name"], "grace");

    // Medium #12: GET .../enroll/start is refused with 400 (does not match {login_id}).
    let bad_enroll_get = get_api(
        address,
        &headers,
        &org_id,
        &format!("provider-connections/{conn_id}/enroll/start"),
    );
    assert_eq!(
        bad_enroll_get.status, 400,
        "GET enroll/start must return 400: {}",
        bad_enroll_get.body
    );

    // High #2: Unbound/spoofed agent-os socket call against real office socket is refused.
    let runtime_dir = socket.parent().expect("host runtime dir");
    let sock_path = org_proto::location::selected_office_socket_in(runtime_dir, &office_id)
        .expect("selected office socket path");
    assert!(sock_path.exists(), "office socket exists at {sock_path:?}");
    let agent_os_bin = crate::test_programs::sibling_program("agent-os");
    assert!(
        agent_os_bin.exists(),
        "built agent-os binary expected at {agent_os_bin:?}"
    );
    let out = std::process::Command::new(&agent_os_bin)
        .args(["who"])
        .env("AGENT_OS_SOCKET", &sock_path)
        .env("AGENT_OS_AGENT_ID", &agent_id)
        .env("AGENT_OS_SESSION_TOKEN", "forged-token")
        .output()
        .expect("run agent-os");
    assert!(
        !out.status.success(),
        "forged agent-os socket bind must be refused; stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.contains("not_authorized"),
        "expected not_authorized refusal from real office socket, got: {combined}"
    );

    // 2. Inspect idle session & effective grants (CLI-9/10 & FC-102).
    let sess = get_api(
        address,
        &headers,
        &org_id,
        &format!("agents/{agent_id}/session"),
    );
    assert_eq!(sess.status, 200, "session inspect: {}", sess.body);
    assert_eq!(sess.body["result"]["session"]["state"], "idle");
    assert_eq!(
        sess.body["result"]["session"]["permission_preset"],
        "workspace_edits"
    );
    assert_eq!(sess.body["result"]["session"]["sandbox_enabled"], true);
    assert!(sess.body["result"]["session"]["same_user_isolation_notice"]
        .as_str()
        .unwrap()
        .contains("private 0700 home"));

    let grants = get_api(
        address,
        &headers,
        &org_id,
        &format!("agents/{agent_id}/grants"),
    );
    assert_eq!(grants.status, 200, "grants: {}", grants.body);
    assert_eq!(
        grants.body["result"]["permission_preset"],
        "workspace_edits"
    );
    assert_eq!(grants.body["result"]["sandbox_enabled"], true);
    assert!(!grants.body["result"]["writable_roots"]
        .as_array()
        .unwrap()
        .is_empty());

    // 3. Browser-driven CLI enrollment + wrapped credential custody + revoke (ENROLL-1..5).
    let enroll_start = post_api(
        address,
        &headers,
        &org_id,
        &format!("provider-connections/{conn_id}/enroll/start"),
        json!({ "command_id": crate::ids::uuid_v7().unwrap() }),
    );
    assert_eq!(
        enroll_start.status, 200,
        "enroll start: {}",
        enroll_start.body
    );
    let login_id = enroll_start.body["result"]["login_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(enroll_start.body["result"]["state"], "awaiting_code");

    // Second concurrent enrollment on same connection conflicts (ENROLL-4).
    let enroll_dup = post_api(
        address,
        &headers,
        &org_id,
        &format!("provider-connections/{conn_id}/enroll/start"),
        json!({ "command_id": crate::ids::uuid_v7().unwrap() }),
    );
    assert_eq!(enroll_dup.status, 409, "dup enroll: {}", enroll_dup.body);

    // Invalid code rejected (ENROLL-2).
    let bad_code = post_api(
        address,
        &headers,
        &org_id,
        &format!("provider-connections/{conn_id}/enroll/{login_id}/submit"),
        json!({
            "command_id": crate::ids::uuid_v7().unwrap(),
            "code": "bad code\nwith spaces"
        }),
    );
    assert_eq!(bad_code.status, 400, "bad code: {}", bad_code.body);

    // Valid code wraps credential under office secrets key; no plaintext hits disk (ENROLL-2/3).
    let raw_token = "sk-ant-oat-e2e-secret-token-778899";
    let submit = post_api(
        address,
        &headers,
        &org_id,
        &format!("provider-connections/{conn_id}/enroll/{login_id}/submit"),
        json!({
            "command_id": crate::ids::uuid_v7().unwrap(),
            "code": raw_token
        }),
    );
    assert_eq!(submit.status, 200, "submit code: {}", submit.body);
    assert_eq!(submit.body["result"]["state"], "completed");

    let cred_status = get_api(
        address,
        &headers,
        &org_id,
        &format!("provider-connections/{conn_id}/credential"),
    );
    assert_eq!(cred_status.status, 200, "cred status: {}", cred_status.body);
    assert_eq!(cred_status.body["result"]["enrolled"], true);
    assert!(!cred_status.body.to_string().contains(raw_token));

    let cred_file = base
        .join("instance")
        .join(&org_id)
        .join("durable")
        .join("local")
        .join("offices")
        .join(&office_id)
        .join("auth")
        .join(&conn_id)
        .join("credential.v1.json");
    let on_disk = fs::read_to_string(&cred_file).expect("wrapped credential file on disk");
    assert!(
        !on_disk.contains(raw_token),
        "plaintext secret leaked to disk"
    );

    // Revoke credential (ENROLL-5).
    let revoke = post_api(
        address,
        &headers,
        &org_id,
        &format!("provider-connections/{conn_id}/credential/revoke"),
        json!({ "command_id": crate::ids::uuid_v7().unwrap() }),
    );
    assert_eq!(revoke.status, 200, "revoke: {}", revoke.body);
    assert_eq!(revoke.body["result"]["enrolled"], false);
    assert!(!cred_file.exists());

    // 4. Clear-stall and stop endpoints respond cleanly.
    let clear = post_api(
        address,
        &headers,
        &org_id,
        &format!("agents/{agent_id}/clear-stall"),
        json!({ "command_id": crate::ids::uuid_v7().unwrap() }),
    );
    assert_eq!(clear.status, 200, "clear stall: {}", clear.body);

    stop(host, &base);
}
