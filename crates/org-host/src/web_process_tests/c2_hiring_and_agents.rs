//! Real-process E2E proof for `TASK-41` (`C2`: Hire an agent through HR — role catalog,
//! provider connection + auto-generated model profile, two-step draft/confirm with simulated
//! failure & retry, private `0700` home directory creation only after confirm, `may_hire` /
//! `max_members` enforcement, agent lifecycle, and projection rebuild from durable files).

use std::fs;
use std::net::SocketAddr;
use std::os::unix::fs::PermissionsExt;
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

fn post_c2(
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

fn get_c2(address: SocketAddr, headers: &str, org_id: &str, path: &str) -> super::Reply {
    http(
        address,
        "GET",
        &format!("/api/v1/organizations/{org_id}/{path}"),
        headers,
        "",
    )
}

fn verify_hiring_flow(
    address: SocketAddr,
    headers: &str,
    org_id: &str,
    office_id: &str,
    top_team_id: &str,
    durable_root: &std::path::Path,
) -> (String, String, String) {
    let role_id = crate::ids::uuid_v7().unwrap();
    let conn_id = crate::ids::uuid_v7().unwrap();
    let profile_id = crate::ids::uuid_v7().unwrap();
    let req_id = crate::ids::uuid_v7().unwrap();
    let agent_id = crate::ids::uuid_v7().unwrap();

    // 1. Role catalog create + list + update.
    let role_res = post_c2(
        address,
        headers,
        org_id,
        "roles",
        json!({
            "command_id": crate::ids::uuid_v7().unwrap(),
            "role_id": role_id,
            "key": "backend-engineer",
            "name": "Backend Engineer",
            "body": "Builds Rust services."
        }),
    );
    assert_eq!(role_res.status, 200, "role create: {}", role_res.body);
    let roles = get_c2(address, headers, org_id, "roles");
    assert_eq!(roles.status, 200, "role list: {}", roles.body);
    assert_eq!(roles.body["result"]["roles"].as_array().unwrap().len(), 1);

    // 2. Provider connection -> auto-generated default ModelProfile.
    let conn_res = post_c2(
        address,
        headers,
        org_id,
        "provider-connections",
        json!({
            "command_id": crate::ids::uuid_v7().unwrap(),
            "connection_id": conn_id,
            "default_profile_id": profile_id,
            "key": "claude-main",
            "name": "Claude Main",
            "provider_type": "anthropic",
            "auth_mode": "api_key",
            "runtime_driver": "claude-code",
            "max_concurrent_turns": 2
        }),
    );
    assert_eq!(conn_res.status, 200, "provider conn: {}", conn_res.body);
    let profiles = get_c2(address, headers, org_id, "model-profiles");
    assert_eq!(profiles.status, 200, "model profiles: {}", profiles.body);
    assert_eq!(profiles.body["result"]["profiles"][0]["id"], profile_id);

    // 3. Draft hire -> NO agent, NO private home directory on disk!
    let draft = post_c2(
        address,
        headers,
        org_id,
        "hires",
        json!({
            "command_id": crate::ids::uuid_v7().unwrap(),
            "request_id": req_id,
            "home_team_id": top_team_id,
            "role_id": role_id,
            "proposal": {
                "name": "Turing",
                "handle": "turing",
                "title": "Backend Agent",
                "model_profile_id": profile_id,
                "model_id": "claude-sonnet-4-6",
                "permission_preset": "workspace_edits",
                "tools_config": ["read", "edit"],
                "system_prompt_markdown": "Ship clean code."
            },
            "reason": "Need backend engineer"
        }),
    );
    assert_eq!(draft.status, 200, "draft: {}", draft.body);
    assert_eq!(draft.body["result"]["state"], "drafting");

    let agent_home = durable_root
        .join("local")
        .join("offices")
        .join(office_id)
        .join("agents")
        .join(&agent_id)
        .join("home");
    assert!(
        !agent_home.exists(),
        "private home must NOT exist while drafting: {}",
        agent_home.display()
    );

    // 4. Confirm with simulated failure -> state = hiring + failed_step, still NO private home!
    let failed = post_c2(
        address,
        headers,
        org_id,
        "hires/confirm",
        json!({
            "command_id": crate::ids::uuid_v7().unwrap(),
            "request_id": req_id,
            "employee_id": agent_id,
            "expected_version": 1,
            "fail_at_step": "provision_home"
        }),
    );
    assert_eq!(failed.status, 200, "confirm fail: {}", failed.body);
    assert_eq!(failed.body["result"]["state"], "hiring");
    assert_eq!(failed.body["result"]["failed_step"], "provision_home");
    assert!(!agent_home.exists());

    // 5. Retry confirm ("Try again") -> state = hired, private home created 0700!
    let hired = post_c2(
        address,
        headers,
        org_id,
        "hires/confirm",
        json!({
            "command_id": crate::ids::uuid_v7().unwrap(),
            "request_id": req_id,
            "employee_id": agent_id,
            "expected_version": 2
        }),
    );
    assert_eq!(hired.status, 200, "confirm retry: {}", hired.body);
    assert_eq!(hired.body["result"]["state"], "hired");
    assert_eq!(hired.body["result"]["employee_id"], agent_id);
    assert!(agent_home.is_dir(), "private home created after confirm");
    assert_eq!(
        fs::metadata(&agent_home).unwrap().permissions().mode() & 0o777,
        0o700
    );

    let exec = get_c2(
        address,
        headers,
        org_id,
        &format!("agents/{agent_id}/execution"),
    );
    assert_eq!(exec.status, 200, "agent exec get: {}", exec.body);
    assert_eq!(exec.body["result"]["model_id"], "claude-sonnet-4-6");

    (role_id, profile_id, agent_id)
}

fn verify_limits_and_lifecycle(
    address: SocketAddr,
    headers: &str,
    org_id: &str,
    top_team_id: &str,
    agent_id: &str,
) {
    // Set team limit to 3 (Owner + HR + Turing = 3) -> next hire refused!
    let lim = post_c2(
        address,
        headers,
        org_id,
        "teams/set-max-members",
        json!({
            "command_id": crate::ids::uuid_v7().unwrap(),
            "team_id": top_team_id,
            "expected_version": 1,
            "max_members": 3
        }),
    );
    assert_eq!(lim.status, 200, "set max members: {}", lim.body);

    let over = post_c2(
        address,
        headers,
        org_id,
        "hires/direct",
        json!({
            "command_id": crate::ids::uuid_v7().unwrap(),
            "request_id": crate::ids::uuid_v7().unwrap(),
            "employee_id": crate::ids::uuid_v7().unwrap(),
            "home_team_id": top_team_id,
            "proposal": { "name": "Extra", "handle": "extra" },
            "reason": "Should hit team limit"
        }),
    );
    assert_eq!(over.status, 400, "over limit refused: {}", over.body);

    // Grant may_hire, update agent, pause, resume, and offboard.
    let mh = post_c2(
        address,
        headers,
        org_id,
        "members/set-may-hire",
        json!({
            "command_id": crate::ids::uuid_v7().unwrap(),
            "member_id": agent_id,
            "expected_version": 1,
            "may_hire": true
        }),
    );
    assert_eq!(mh.status, 200, "set may_hire: {}", mh.body);

    let upd = post_c2(
        address,
        headers,
        org_id,
        "agents/update",
        json!({
            "command_id": crate::ids::uuid_v7().unwrap(),
            "member_id": agent_id,
            "expected_version": 2,
            "title": "Staff Backend Agent"
        }),
    );
    assert_eq!(upd.status, 200, "agent update: {}", upd.body);

    let pause = post_c2(
        address,
        headers,
        org_id,
        "agents/set-status",
        json!({
            "command_id": crate::ids::uuid_v7().unwrap(),
            "member_id": agent_id,
            "expected_version": 3,
            "paused": true
        }),
    );
    assert_eq!(pause.status, 200, "agent pause: {}", pause.body);

    let resume = post_c2(
        address,
        headers,
        org_id,
        "agents/set-status",
        json!({
            "command_id": crate::ids::uuid_v7().unwrap(),
            "member_id": agent_id,
            "expected_version": 4,
            "paused": false
        }),
    );
    assert_eq!(resume.status, 200, "agent resume: {}", resume.body);

    let off = post_c2(
        address,
        headers,
        org_id,
        "agents/offboard",
        json!({
            "command_id": crate::ids::uuid_v7().unwrap(),
            "member_id": agent_id,
            "expected_version": 5
        }),
    );
    assert_eq!(off.status, 200, "agent offboard: {}", off.body);
}

#[test]
#[ignore = "real-process E2E: run with --ignored or cargo test --workspace -- --include-ignored"]
fn actual_c2_hr_hiring_roles_limits_lifecycle_and_rebuild_over_http() {
    let Launched {
        temp: _temp,
        base,
        address,
        host,
        ..
    } = launch(Seed::Administrator);
    wait_serving(&host, address);

    let sign_in = post_until_available(address, "/api/v1/session/sign-in", &credentials());
    assert_eq!(sign_in.status, 200, "sign-in: {}", sign_in.body);
    let headers = pair(&sign_in, "c2-admin").headers();
    let instance_id = sign_in.body["result"]["instance_id"]
        .as_str()
        .unwrap()
        .to_owned();

    let op_id = crate::ids::uuid_v7().unwrap();
    let create_req = create_body(
        &instance_id,
        &op_id,
        "C2 Hiring Org",
        "c2-hiring",
        remote("c2-hiring"),
    );
    let created_http = http(
        address,
        "POST",
        "/api/v1/organizations",
        &headers,
        &create_req,
    );
    assert!(
        created_http.status == 200 || created_http.status == 202,
        "create org: {}",
        created_http.body
    );
    let created = wait_org_completed(address, &headers, &instance_id, &op_id);
    let org_id = created["id"].as_str().unwrap().to_owned();
    let office_id = created["offices"][0]["id"].as_str().unwrap().to_owned();
    wait_offices_active(&host, 1);

    let snap = get_c2(address, &headers, &org_id, "snapshot");
    assert_eq!(snap.status, 200, "snapshot: {}", snap.body);
    let top_team_id = snap.body["result"]["snapshot"]["teams"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let durable_root = base.join("instance").join(&org_id).join("durable");
    let (_role_id, _profile_id, agent_id) = verify_hiring_flow(
        address,
        &headers,
        &org_id,
        &office_id,
        &top_team_id,
        &durable_root,
    );
    verify_limits_and_lifecycle(address, &headers, &org_id, &top_team_id, &agent_id);

    // Stop host, delete runtime projection folder, restart host -> verify rebuild!
    stop(host, &base);
    let runtime_root = base.join("instance").join(&org_id).join("runtime");
    assert!(
        runtime_root.exists(),
        "runtime root should exist before rebuild"
    );
    fs::remove_dir_all(&runtime_root).unwrap();
    fs::create_dir_all(&runtime_root).unwrap();
    org_platform::fs::set_owner_only_dir(&runtime_root).unwrap();

    let restarted = super::organizations::restart(&base, address);
    wait_serving(&restarted, address);
    let sign_in2 = post_until_available(address, "/api/v1/session/sign-in", &credentials());
    assert_eq!(sign_in2.status, 200);
    let headers2 = pair(&sign_in2, "c2-admin-2").headers();
    wait_offices_active(&restarted, 1);

    let roles_after = get_c2(address, &headers2, &org_id, "roles");
    assert_eq!(
        roles_after.status, 200,
        "roles after rebuild: {}",
        roles_after.body
    );
    assert_eq!(
        roles_after.body["result"]["roles"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let hires_after = get_c2(address, &headers2, &org_id, "hires");
    assert_eq!(
        hires_after.status, 200,
        "hires after rebuild: {}",
        hires_after.body
    );
    assert_eq!(
        hires_after.body["result"]["hires"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let exec_after = get_c2(
        address,
        &headers2,
        &org_id,
        &format!("agents/{agent_id}/execution"),
    );
    assert_eq!(
        exec_after.status, 200,
        "exec after rebuild: {}",
        exec_after.body
    );
    assert_eq!(exec_after.body["result"]["model_id"], "claude-sonnet-4-6");

    stop(restarted, &base);
}
