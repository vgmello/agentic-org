//! Real-process E2E proof for `TASK-39` (`R1`: open an existing organization from Git
//! repository files — FC-167, `doc-91`, `doc-61` D25).

use std::fs;
use std::net::SocketAddr;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::json;

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
) -> serde_json::Value {
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

fn org_mutation(
    address: SocketAddr,
    headers: &str,
    instance_id: &str,
    org_id: &str,
    path: &str,
    params: serde_json::Value,
) -> super::Reply {
    let op_id = org_host::ids::uuid_v7().unwrap();
    let body = json!({
        "operation_id": op_id,
        "scope": { "kind": "organization", "instance_id": instance_id, "org_id": org_id },
        "params": params,
    })
    .to_string();
    http(
        address,
        "POST",
        &format!("/api/v1/organizations/{org_id}/{path}"),
        headers,
        &body,
    )
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let target = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir_recursive(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

fn run_git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.test")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.test")
        .args(args)
        .output()
        .expect("git invocation");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

fn publish_state_to_bare_repo(
    state_src: &Path,
    bare_repo: &Path,
    corrupt_team_rel: &str,
) -> String {
    fs::create_dir_all(bare_repo).unwrap();
    run_git(bare_repo, &["init", "--bare"]);

    let work = bare_repo.parent().unwrap().join("work");
    fs::create_dir_all(&work).unwrap();
    run_git(&work, &["init"]);
    run_git(&work, &["checkout", "-b", "org/state"]);
    copy_dir_recursive(state_src, &work);
    run_git(&work, &["add", "."]);
    run_git(&work, &["commit", "-m", "publish organization state"]);
    run_git(
        &work,
        &["remote", "add", "origin", bare_repo.to_str().unwrap()],
    );
    run_git(&work, &["push", "origin", "org/state"]);

    // Second branch with one invalid team file to verify AC #2 names the offending file.
    run_git(&work, &["checkout", "-b", "org/corrupted"]);
    fs::write(work.join(corrupt_team_rel), b"schema_version: 999\n").unwrap();
    run_git(&work, &["add", "."]);
    run_git(&work, &["commit", "-m", "corrupt team record"]);
    run_git(&work, &["push", "origin", "org/corrupted"]);

    format!("file://{}", bare_repo.display())
}

fn seed_source_org_into_git(bare_repo: &Path) -> (String, String, String, String, String, String) {
    let Launched {
        temp: _temp,
        base,
        host,
        address,
        ..
    } = launch(Seed::Administrator);
    wait_serving(&host, address);
    let sign_in = post_until_available(address, "/api/v1/session/sign-in", &credentials());
    assert_eq!(sign_in.status, 200, "sign-in: {}", sign_in.body);
    let headers = pair(&sign_in, "owner-1").headers();
    let instance_id = sign_in.body["result"]["instance_id"]
        .as_str()
        .unwrap()
        .to_owned();

    let op_id = org_host::ids::uuid_v7().unwrap();
    let created = http(
        address,
        "POST",
        "/api/v1/organizations",
        &headers,
        &create_body(
            &instance_id,
            &op_id,
            "Solaris",
            "solaris",
            remote("solaris"),
        ),
    );
    assert!(matches!(created.status, 200 | 202), "{}", created.body);
    let org = wait_org_completed(address, &headers, &instance_id, &op_id);
    let org_id = org["id"].as_str().unwrap().to_owned();
    let old_hq_id = org["offices"][0]["id"].as_str().unwrap().to_owned();
    wait_offices_active(&host, 1);

    let snap = http(
        address,
        "GET",
        &format!("/api/v1/organizations/{org_id}/snapshot"),
        &headers,
        "",
    );
    assert_eq!(snap.status, 200, "snap: {}", snap.body);
    let root_team_id = snap.body["result"]["snapshot"]["teams"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let platform_team_id = org_host::ids::uuid_v7().unwrap();
    let mk_team = org_mutation(
        address,
        &headers,
        &instance_id,
        &org_id,
        "teams/create",
        json!({ "team_id": platform_team_id, "name": "Platform", "owner_team_id": root_team_id }),
    );
    assert_eq!(mk_team.status, 200, "create team: {}", mk_team.body);

    let sam_member_id = org_host::ids::uuid_v7().unwrap();
    let add_sam = org_mutation(
        address,
        &headers,
        &instance_id,
        &org_id,
        "members/add-person",
        json!({
            "member_id": sam_member_id,
            "name": "Sam",
            "contact": "sam@solaris.test",
            "handle": "sam",
            "home_team_id": platform_team_id
        }),
    );
    assert_eq!(add_sam.status, 200, "add Sam: {}", add_sam.body);

    let project_id = org_host::ids::uuid_v7().unwrap();
    let proj_op = org_host::ids::uuid_v7().unwrap();
    let mk_proj = http(
        address,
        "POST",
        &format!("/api/v1/organizations/{org_id}/projects"),
        &headers,
        &json!({
            "operation_id": proj_op,
            "scope": { "kind": "organization", "instance_id": instance_id, "org_id": org_id },
            "params": {
                "project_id": project_id,
                "name": "Apollo",
                "owner_team_id": platform_team_id
            }
        })
        .to_string(),
    );
    assert_eq!(mk_proj.status, 200, "create project: {}", mk_proj.body);

    stop(host, &base);

    let state_src = base.join("instance").join(&org_id).join("durable/state");
    let corrupt_rel = format!("teams/{root_team_id}/teams/{platform_team_id}/team.yaml");
    let repo_url = publish_state_to_bare_repo(&state_src, bare_repo, &corrupt_rel);
    (
        repo_url,
        org_id,
        old_hq_id,
        root_team_id,
        sam_member_id,
        corrupt_rel,
    )
}

#[test]
#[ignore = "root runs with ORG_TEST_OFFICE_BIN and ORG_TEST_WEB_BIN; actual web/host/office processes"]
fn actual_open_existing_organization_from_git_restores_records_and_refuses_invalid_files() {
    let git_dir = tempfile::tempdir().unwrap();
    let bare_repo = git_dir.path().join("solaris-org.git");
    let seeded = seed_source_org_into_git(&bare_repo);

    verify_reopen_with_existing_person(&seeded);
    verify_reopen_with_new_owner_person(&seeded);
}

fn verify_reopen_with_existing_person(
    (repo_url, org_id, old_hq_id, root_team_id, sam_member_id, corrupt_rel): &(
        String,
        String,
        String,
        String,
        String,
        String,
    ),
) {
    // Instance 2: completely fresh host database, zero knowledge of Solaris.
    let Launched {
        temp: _temp,
        base,
        host,
        address,
        ..
    } = launch(Seed::Administrator);
    wait_serving(&host, address);
    let sign_in = post_until_available(address, "/api/v1/session/sign-in", &credentials());
    assert_eq!(sign_in.status, 200, "inst2 sign-in: {}", sign_in.body);
    let headers = pair(&sign_in, "owner-2").headers();
    let instance_id = sign_in.body["result"]["instance_id"]
        .as_str()
        .unwrap()
        .to_owned();

    assert_preview_checks(
        address,
        &headers,
        repo_url,
        org_id,
        sam_member_id,
        corrupt_rel,
    );

    // AC #1 + #3: Administrator selects Sam's person record as owner and opens Solaris.
    let open_op = org_host::ids::uuid_v7().unwrap();
    let open_payload = json!({
        "operation_id": open_op,
        "repository_remote_url": repo_url,
        "state_branch": "org/state",
        "owner_member_id": sam_member_id
    })
    .to_string();
    let opened = http(
        address,
        "POST",
        "/api/v1/organizations/open",
        &headers,
        &open_payload,
    );
    assert_eq!(opened.status, 200, "open org: {}", opened.body);
    assert_eq!(opened.body["result"]["organization"]["id"], *org_id);
    assert_eq!(
        opened.body["result"]["organization"]["employee_id"],
        *sam_member_id
    );
    let new_hq_id = opened.body["result"]["organization"]["offices"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(new_hq_id, *old_hq_id);
    assert!(!opened.body["result"]["local_only_not_restored"]
        .as_array()
        .unwrap()
        .is_empty());

    // Idempotent replay with the same operation_id returns the same organization and loss list.
    let replayed = http(
        address,
        "POST",
        "/api/v1/organizations/open",
        &headers,
        &open_payload,
    );
    assert_eq!(replayed.status, 200, "replayed open: {}", replayed.body);
    assert_eq!(replayed.body["result"]["organization"]["id"], *org_id);
    assert!(!replayed.body["result"]["local_only_not_restored"]
        .as_array()
        .unwrap()
        .is_empty());

    // Duplicate open under a new operation_id is refused with 409 AlreadyExists.
    let dup = http(
        address,
        "POST",
        "/api/v1/organizations/open",
        &headers,
        &json!({
            "operation_id": org_host::ids::uuid_v7().unwrap(),
            "repository_remote_url": repo_url,
            "state_branch": "org/state",
            "owner_member_id": sam_member_id
        })
        .to_string(),
    );
    assert_eq!(dup.status, 409, "duplicate open: {}", dup.body);

    wait_offices_active(&host, 1);
    assert_restored_state_and_mutate(
        address,
        &headers,
        &instance_id,
        org_id,
        root_team_id,
        sam_member_id,
    );

    // Delete runtime/ on Instance 2 and verify clean rebuild on restart.
    stop(host, &base);
    let runtime_dir = base.join("instance").join(org_id).join("runtime");
    fs::remove_dir_all(&runtime_dir).unwrap();
    fs::create_dir_all(&runtime_dir).unwrap();
    org_platform::fs::set_owner_only_dir(&runtime_dir).unwrap();

    let host2 = super::organizations::restart(&base, address);
    wait_serving(&host2, address);
    let sign_in_2 = post_until_available(address, "/api/v1/session/sign-in", &credentials());
    assert_eq!(sign_in_2.status, 200);
    let headers_2 = pair(&sign_in_2, "owner-3").headers();
    wait_offices_active(&host2, 1);
    let snap2 = http(
        address,
        "GET",
        &format!("/api/v1/organizations/{org_id}/snapshot"),
        &headers_2,
        "",
    );
    assert_eq!(snap2.status, 200);
    assert!(snap2.body["result"]["snapshot"]["teams"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["name"] == "Reliability"));
    stop(host2, &base);
}

fn assert_preview_checks(
    address: SocketAddr,
    headers: &str,
    repo_url: &str,
    org_id: &str,
    sam_member_id: &str,
    corrupt_rel: &str,
) {
    // AC #2: Invalid organization file refuses the preview and names the offending file.
    let bad_preview = http(
        address,
        "POST",
        "/api/v1/organizations/open/preview",
        headers,
        &json!({ "repository_remote_url": repo_url, "state_branch": "org/corrupted" }).to_string(),
    );
    assert_eq!(
        bad_preview.status, 400,
        "corrupt preview: {}",
        bad_preview.body
    );
    let expected_path = format!("state/{corrupt_rel}");
    assert_eq!(bad_preview.body["error"]["field"], expected_path);
    assert!(bad_preview.body["error"]["message"]
        .as_str()
        .unwrap()
        .contains(&expected_path));

    // AC #3: Preview of valid branch returns org ID, counts, active people, and loss boundaries.
    let preview = http(
        address,
        "POST",
        "/api/v1/organizations/open/preview",
        headers,
        &json!({ "repository_remote_url": repo_url, "state_branch": "org/state" }).to_string(),
    );
    assert_eq!(preview.status, 200, "valid preview: {}", preview.body);
    assert_eq!(preview.body["result"]["organization_id"], org_id);
    assert_eq!(preview.body["result"]["teams_count"], 2);
    assert_eq!(preview.body["result"]["projects_count"], 1);
    let people = preview.body["result"]["people"].as_array().unwrap();
    assert!(people.iter().any(|p| p["id"] == sam_member_id));
    assert!(!preview.body["result"]["local_only_not_restored"]
        .as_array()
        .unwrap()
        .is_empty());
}

fn assert_restored_state_and_mutate(
    address: SocketAddr,
    headers: &str,
    instance_id: &str,
    org_id: &str,
    root_team_id: &str,
    sam_member_id: &str,
) {
    let snap = http(
        address,
        "GET",
        &format!("/api/v1/organizations/{org_id}/snapshot"),
        headers,
        "",
    );
    assert_eq!(snap.status, 200, "reopened snap: {}", snap.body);
    let teams = snap.body["result"]["snapshot"]["teams"].as_array().unwrap();
    assert!(teams.iter().any(|t| t["name"] == "Solaris"));
    assert!(teams.iter().any(|t| t["name"] == "Platform"));
    let members = snap.body["result"]["snapshot"]["members"]
        .as_array()
        .unwrap();
    assert!(members
        .iter()
        .any(|m| m["id"] == sam_member_id && m["owner"] == true));
    assert!(members.iter().any(|m| m["handle"] == "hr"));

    let projects = http(
        address,
        "GET",
        &format!("/api/v1/organizations/{org_id}/projects"),
        headers,
        "",
    );
    assert_eq!(projects.status, 200, "reopened projects: {}", projects.body);
    let proj_list = projects.body["result"]["projects"].as_array().unwrap();
    assert!(proj_list
        .iter()
        .any(|p| p["name"] == "Apollo" && p["slug"] == "apollo"));

    let new_team_id = org_host::ids::uuid_v7().unwrap();
    let mutated = org_mutation(
        address,
        headers,
        instance_id,
        org_id,
        "teams/create",
        json!({ "team_id": new_team_id, "name": "Reliability", "owner_team_id": root_team_id }),
    );
    assert_eq!(mutated.status, 200, "mutate reopened org: {}", mutated.body);
}

fn verify_reopen_with_new_owner_person(
    (repo_url, org_id, _, _, _, _): &(String, String, String, String, String, String),
) {
    // Instance 3: proves new_owner_person validation (handle collision -> 400 without poisoning
    // HostDb) followed by clean open with a fresh owner person record.
    let Launched {
        temp: _temp,
        base,
        host,
        address,
        ..
    } = launch(Seed::Administrator);
    wait_serving(&host, address);
    let sign_in = post_until_available(address, "/api/v1/session/sign-in", &credentials());
    assert_eq!(sign_in.status, 200);
    let headers = pair(&sign_in, "owner-new").headers();

    let new_owner_id = org_host::ids::uuid_v7().unwrap();
    let collision = http(
        address,
        "POST",
        "/api/v1/organizations/open",
        &headers,
        &json!({
            "operation_id": org_host::ids::uuid_v7().unwrap(),
            "repository_remote_url": repo_url,
            "state_branch": "org/state",
            "owner_member_id": new_owner_id,
            "new_owner_person": {
                "employee_id": new_owner_id,
                "name": "Duplicate Sam",
                "handle": "sam"
            }
        })
        .to_string(),
    );
    assert_eq!(
        collision.status, 400,
        "handle collision: {}",
        collision.body
    );
    assert_eq!(collision.body["error"]["field"], "new_owner_person.handle");

    // HostDb was not poisoned by the refused open: retrying with a unique handle succeeds.
    let opened = http(
        address,
        "POST",
        "/api/v1/organizations/open",
        &headers,
        &json!({
            "operation_id": org_host::ids::uuid_v7().unwrap(),
            "repository_remote_url": repo_url,
            "state_branch": "org/state",
            "owner_member_id": new_owner_id,
            "new_owner_person": {
                "employee_id": new_owner_id,
                "name": "Nadia Founder",
                "handle": "nadia"
            }
        })
        .to_string(),
    );
    assert_eq!(opened.status, 200, "new owner open: {}", opened.body);
    assert_eq!(opened.body["result"]["organization"]["id"], *org_id);
    assert_eq!(
        opened.body["result"]["organization"]["employee_id"],
        new_owner_id
    );
    wait_offices_active(&host, 1);

    let snap = http(
        address,
        "GET",
        &format!("/api/v1/organizations/{org_id}/snapshot"),
        &headers,
        "",
    );
    assert_eq!(snap.status, 200);
    assert!(snap.body["result"]["snapshot"]["members"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["id"] == new_owner_id && m["handle"] == "nadia" && m["owner"] == true));
    stop(host, &base);
}
