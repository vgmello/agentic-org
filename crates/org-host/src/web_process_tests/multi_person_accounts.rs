//! Real-process E2E proof for `TASK-38` (`A6`: multi-person accounts — browser invites,
//! existing-account attach, password change & reset links, access removal, org owner handover,
//! and instance administrator handover).

use std::net::SocketAddr;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::json;

use super::{
    browser_sessions::{credentials, pair, wait_serving},
    create_body, http, launch, lookup, post_until_available, remote, Launched, Seed, PASSWORD,
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
    let op_id = crate::ids::uuid_v7().unwrap();
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

#[test]
#[ignore = "root runs with ORG_TEST_OFFICE_BIN and ORG_TEST_WEB_BIN; actual web/worker listeners require permitted local bind"]
fn actual_multi_person_invites_password_reset_handovers_and_access_removal() {
    let Launched {
        temp: _temp,
        address,
        program: _,
        host,
        socket: _,
        link: _,
        base,
    } = launch(Seed::Administrator);
    let instance_id = host.instance.as_ref().unwrap().id().to_owned();
    wait_serving(&host, address);

    // 1. Owner signs in and creates Lumen Labs.
    let owner_in = post_until_available(address, "/api/v1/session/sign-in", &credentials());
    assert_eq!(owner_in.status, 200, "owner sign-in: {}", owner_in.body);
    let owner_headers = pair(&owner_in, "owner").headers();
    let owner_account_id = owner_in.body["result"]["account"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let op_org = crate::ids::uuid_v7().unwrap();
    let create_reply = http(
        address,
        "POST",
        "/api/v1/organizations",
        &owner_headers,
        &create_body(
            &instance_id,
            &op_org,
            "Lumen Labs",
            "lumen-labs",
            remote("lumen-labs"),
        ),
    );
    assert!(
        matches!(create_reply.status, 200 | 202),
        "org create: {} {}",
        create_reply.status,
        create_reply.body
    );
    let org = wait_org_completed(address, &owner_headers, &instance_id, &op_org);
    let org_id = org["id"].as_str().unwrap().to_owned();
    let owner_member_id = org["employee_id"].as_str().unwrap().to_owned();
    wait_offices_active(&host, 1);

    // 2. Owner creates a team and adds Sam Ortiz as a second person in the org.
    let initial_snap = http(
        address,
        "GET",
        &format!("/api/v1/organizations/{org_id}/snapshot"),
        &owner_headers,
        "",
    );
    assert_eq!(
        initial_snap.status, 200,
        "initial snap: {}",
        initial_snap.body
    );
    let root_team_id = initial_snap.body["result"]["snapshot"]["teams"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let team_id = crate::ids::uuid_v7().unwrap();
    let team_reply = org_mutation(
        address,
        &owner_headers,
        &instance_id,
        &org_id,
        "teams/create",
        json!({ "team_id": team_id, "name": "Engineering", "owner_team_id": root_team_id }),
    );
    assert_eq!(team_reply.status, 200, "team create: {}", team_reply.body);

    let sam_member_id = crate::ids::uuid_v7().unwrap();
    let add_sam = org_mutation(
        address,
        &owner_headers,
        &instance_id,
        &org_id,
        "members/add-person",
        json!({
            "member_id": sam_member_id,
            "name": "Sam Ortiz",
            "contact": "sam@lumen.test",
            "handle": "sam",
            "home_team_id": team_id,
        }),
    );
    assert_eq!(add_sam.status, 200, "add Sam: {}", add_sam.body);

    // 3. Owner invites Sam, inspects the link, and Sam registers a UUIDv7 account.
    let invite_reply = http(
        address,
        "POST",
        &format!("/api/v1/organizations/{org_id}/access/invite"),
        &owner_headers,
        &json!({ "member_id": sam_member_id }).to_string(),
    );
    assert_eq!(invite_reply.status, 200, "invite: {}", invite_reply.body);
    let invite_url = invite_reply.body["result"]["url"].as_str().unwrap();
    let invite_token = invite_url
        .split_once("/register#")
        .expect("invite link format")
        .1
        .to_owned();

    let inspect_reply = http(
        address,
        "POST",
        "/api/v1/links/inspect",
        "",
        &json!({ "link": invite_token, "kind": "registration" }).to_string(),
    );
    assert_eq!(inspect_reply.status, 200, "inspect: {}", inspect_reply.body);
    assert_eq!(inspect_reply.body["result"]["kind"], "organization_invite");
    assert_eq!(inspect_reply.body["result"]["org_id"], org_id);
    assert_eq!(inspect_reply.body["result"]["org_name"], "Lumen Labs");
    assert_eq!(inspect_reply.body["result"]["member_id"], sam_member_id);

    let sam_pw1 = "sam-initial-password-1234";
    let sam_reg = post_until_available(
        address,
        "/api/v1/links/register",
        &json!({
            "link": invite_token,
            "login": "sam",
            "password": sam_pw1,
        })
        .to_string(),
    );
    assert_eq!(sam_reg.status, 200, "sam register: {}", sam_reg.body);
    let sam_account_id = sam_reg.body["result"]["account"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        org_proto::is_uuid_v7(&sam_account_id),
        "new account ID must be UUIDv7: {sam_account_id}"
    );
    assert_eq!(sam_reg.body["result"]["admin"], false);
    assert_ne!(sam_account_id, owner_account_id);
    let sam_headers_1 = pair(&sam_reg, "sam-1").headers();

    // 4. Both Owner and Sam are concurrently signed in and read the org snapshot;
    //    Sam cannot invite others (403 Forbidden).
    let sam_snap = http(
        address,
        "GET",
        &format!("/api/v1/organizations/{org_id}/snapshot"),
        &sam_headers_1,
        "",
    );
    assert_eq!(sam_snap.status, 200, "sam snapshot: {}", sam_snap.body);
    let sam_invite_denied = http(
        address,
        "POST",
        &format!("/api/v1/organizations/{org_id}/access/invite"),
        &sam_headers_1,
        &json!({ "member_id": sam_member_id }).to_string(),
    );
    assert_eq!(
        sam_invite_denied.status, 403,
        "non-owner invite must be 403: {}",
        sam_invite_denied.body
    );

    // 5. Sam opens a second browser session, then changes password on session 1:
    //    session 1 stays live (200), session 2 immediately ends (401).
    let sam_in_2 = post_until_available(
        address,
        "/api/v1/session/sign-in",
        &json!({ "login": "sam", "password": sam_pw1 }).to_string(),
    );
    assert_eq!(sam_in_2.status, 200, "sam sign-in 2: {}", sam_in_2.body);
    let sam_headers_2 = pair(&sam_in_2, "sam-2").headers();
    let sam_pw2 = "sam-changed-password-5678";
    let change_pw = http(
        address,
        "POST",
        "/api/v1/account/password",
        &sam_headers_1,
        &json!({ "current_password": sam_pw1, "new_password": sam_pw2 }).to_string(),
    );
    assert_eq!(change_pw.status, 200, "change pw: {}", change_pw.body);
    assert_eq!(
        http(address, "GET", "/api/v1/session", &sam_headers_1, "").status,
        200
    );
    assert_eq!(
        http(address, "GET", "/api/v1/session", &sam_headers_2, "").status,
        401
    );

    // 6. Owner issues a password reset link for Sam; Sam redeems it -> all Sam sessions end.
    let reset_link_reply = http(
        address,
        "POST",
        &format!("/api/v1/organizations/{org_id}/access/reset-link"),
        &owner_headers,
        &json!({ "member_id": sam_member_id }).to_string(),
    );
    assert_eq!(
        reset_link_reply.status, 200,
        "reset link: {}",
        reset_link_reply.body
    );
    let reset_token = reset_link_reply.body["result"]["url"]
        .as_str()
        .unwrap()
        .split_once("/set-password#")
        .expect("set-password link format")
        .1
        .to_owned();
    let inspect_reset = http(
        address,
        "POST",
        "/api/v1/links/inspect",
        "",
        &json!({ "link": reset_token, "kind": "set_password" }).to_string(),
    );
    assert_eq!(
        inspect_reset.status, 200,
        "inspect reset: {}",
        inspect_reset.body
    );
    assert_eq!(inspect_reset.body["result"]["kind"], "set_password");
    assert_eq!(inspect_reset.body["result"]["login"], "sam");

    let sam_pw3 = "sam-reset-password-9012";
    let set_pw = http(
        address,
        "POST",
        "/api/v1/links/set-password",
        "",
        &json!({ "link": reset_token, "password": sam_pw3 }).to_string(),
    );
    assert_eq!(set_pw.status, 200, "set pw: {}", set_pw.body);
    assert_eq!(set_pw.body["result"]["login"], "sam");
    assert_eq!(
        http(address, "GET", "/api/v1/session", &sam_headers_1, "").status,
        401
    );

    let sam_in_3 = post_until_available(
        address,
        "/api/v1/session/sign-in",
        &json!({ "login": "sam", "password": sam_pw3 }).to_string(),
    );
    assert_eq!(sam_in_3.status, 200, "sam sign-in 3: {}", sam_in_3.body);
    let sam_headers_3 = pair(&sam_in_3, "sam-3").headers();

    // 7. Owner cannot remove their own access (409 Conflict) until handing over ownership.
    let remove_owner = http(
        address,
        "POST",
        &format!("/api/v1/organizations/{org_id}/access/remove"),
        &owner_headers,
        &json!({ "member_id": owner_member_id }).to_string(),
    );
    assert_eq!(
        remove_owner.status, 409,
        "owner access removal must be 409: {}",
        remove_owner.body
    );

    // Wrong password on owner handover -> 403.
    let bad_handover = http(
        address,
        "POST",
        &format!("/api/v1/organizations/{org_id}/owner/handover"),
        &owner_headers,
        &json!({ "to_member_id": sam_member_id, "password": "wrong-password-1234" }).to_string(),
    );
    assert_eq!(
        bad_handover.status, 403,
        "bad pw handover: {}",
        bad_handover.body
    );

    // Valid owner handover -> reloads office worker with Sam as owner.
    let owner_handover = http(
        address,
        "POST",
        &format!("/api/v1/organizations/{org_id}/owner/handover"),
        &owner_headers,
        &json!({ "to_member_id": sam_member_id, "password": PASSWORD }).to_string(),
    );
    assert_eq!(
        owner_handover.status, 200,
        "owner handover: {}",
        owner_handover.body
    );

    // Wait for the reloaded office worker to report Sam as owner and accept Sam's team creation.
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        let snap = http(
            address,
            "GET",
            &format!("/api/v1/organizations/{org_id}/snapshot"),
            &sam_headers_3,
            "",
        );
        if snap.status == 200 {
            let members = snap.body["result"]["snapshot"]["members"].as_array();
            let sam_is_owner = members.is_some_and(|list| {
                list.iter()
                    .any(|m| m["id"] == sam_member_id && m["owner"] == true)
            });
            if sam_is_owner {
                break;
            }
        }
        assert!(
            Instant::now() < until,
            "timed out waiting for reloaded office to bind Sam as owner"
        );
        thread::sleep(Duration::from_millis(100));
    }
    let sam_team_id = crate::ids::uuid_v7().unwrap();
    let sam_team = org_mutation(
        address,
        &sam_headers_3,
        &instance_id,
        &org_id,
        "teams/create",
        json!({ "team_id": sam_team_id, "name": "Design", "owner_team_id": root_team_id }),
    );
    assert_eq!(
        sam_team.status, 200,
        "new owner Sam must be able to mutate org structure: {}",
        sam_team.body
    );

    // 8. Administrator handover transfers admin flag atomically and invalidates cached sessions.
    let admin_handover = http(
        address,
        "POST",
        "/api/v1/instance/admin/handover",
        &owner_headers,
        &json!({ "to_account_id": sam_account_id, "password": PASSWORD }).to_string(),
    );
    assert_eq!(
        admin_handover.status, 200,
        "admin handover: {}",
        admin_handover.body
    );
    let owner_sess_after = http(address, "GET", "/api/v1/session", &owner_headers, "");
    assert_eq!(owner_sess_after.status, 200);
    assert_eq!(owner_sess_after.body["result"]["admin"], false);
    let sam_sess_after = http(address, "GET", "/api/v1/session", &sam_headers_3, "");
    assert_eq!(sam_sess_after.status, 200);
    assert_eq!(sam_sess_after.body["result"]["admin"], true);

    // 9. Sam removes old Owner's access to Lumen Labs -> Owner's session ends immediately,
    //    and signing back in as Owner yields 0 organizations (SIGN-9).
    let remove_old_owner = http(
        address,
        "POST",
        &format!("/api/v1/organizations/{org_id}/access/remove"),
        &sam_headers_3,
        &json!({ "member_id": owner_member_id }).to_string(),
    );
    assert_eq!(
        remove_old_owner.status, 200,
        "remove old owner: {}",
        remove_old_owner.body
    );
    assert_eq!(
        http(address, "GET", "/api/v1/session", &owner_headers, "").status,
        401
    );
    let owner_re_in = post_until_available(address, "/api/v1/session/sign-in", &credentials());
    assert_eq!(
        owner_re_in.status, 200,
        "SIGN-9 sign-in: {}",
        owner_re_in.body
    );
    assert_eq!(owner_re_in.body["result"]["organizations"], json!([]));

    // 10. SIGN-35 over HTTP: Sam re-invites Owner to Lumen Labs, and Owner attaches their
    //     existing account via `POST /api/v1/links/register/sign-in`.
    let reinvite = http(
        address,
        "POST",
        &format!("/api/v1/organizations/{org_id}/access/invite"),
        &sam_headers_3,
        &json!({ "member_id": owner_member_id }).to_string(),
    );
    assert_eq!(reinvite.status, 200, "reinvite owner: {}", reinvite.body);
    let reinvite_token = reinvite.body["result"]["url"]
        .as_str()
        .unwrap()
        .split_once("/register#")
        .expect("reinvite link format")
        .1
        .to_owned();
    let attach = post_until_available(
        address,
        "/api/v1/links/register/sign-in",
        &json!({
            "link": reinvite_token,
            "login": "Owner",
            "password": PASSWORD
        })
        .to_string(),
    );
    assert_eq!(attach.status, 200, "SIGN-35 attach: {}", attach.body);
    let attached_orgs = attach.body["result"]["organizations"]
        .as_array()
        .expect("attached organizations list");
    assert!(
        attached_orgs
            .iter()
            .any(|o| o["org_id"] == org_id && o["employee_id"] == owner_member_id),
        "SIGN-35 attach must re-link Owner to Lumen Labs: {}",
        attach.body
    );

    stop(host, &base);
}
