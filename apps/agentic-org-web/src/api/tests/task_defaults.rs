//! `TASK-88`: the final-summary and default-definition-of-done routes through the full
//! gateway (routing, session pair, body checks), against the scripted host. A valid call
//! goes on to resolve the organization's HQ (503 here: the fixture has no office socket);
//! a malformed one is refused before the host is asked anything.

use super::*;

#[test]
fn final_summary_and_default_dod_routes_reach_hq_only_when_well_formed() {
    let (gateway, calls) = gateway(organization_script(|method, _| {
        (method == "organization.list").then(|| {
            vec![json!({"result": {"organizations": [host_organization()], "creating": []}})]
        })
    }));
    let pair = sign_in(&gateway).pair();
    let base = format!("/api/v1/organizations/{ORG}");

    for (method, target, body) in [
        (Method::GET, format!("{base}/task-defaults/dod?x=1"), ""),
        (Method::GET, format!("{base}/task-defaults/other"), ""),
        (Method::POST, format!("{base}/task-defaults/dod"), "[]"),
        (
            Method::POST,
            format!("{base}/task-defaults/dod/project"),
            "{}",
        ),
        (
            Method::POST,
            format!("{base}/tasks/final-summary"),
            "{\"task\":\"T-A1\"}",
        ),
    ] {
        let reply = send(&gateway, method, &target, body, Some(&pair), &[]);
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{target} {body}");
    }
    // `final-summary` is an action, never a task reference.
    let reserved = send(
        &gateway,
        Method::GET,
        &format!("{base}/tasks/final-summary"),
        "",
        Some(&pair),
        &[],
    );
    assert_eq!(reserved.status, StatusCode::BAD_REQUEST);
    assert!(
        !calls.methods().contains(&"organization.list".to_owned()),
        "refused requests must fail before resolving HQ"
    );

    let unauth = send(
        &gateway,
        Method::GET,
        &format!("{base}/task-defaults/dod"),
        "",
        None,
        &[],
    );
    assert_eq!(unauth.status, StatusCode::UNAUTHORIZED);

    let command = "0193a5c0-0000-7000-8000-0000000000aa";
    for (method, target, body) in [
        (
            Method::GET,
            format!("{base}/task-defaults/dod"),
            String::new(),
        ),
        (
            Method::POST,
            format!("{base}/task-defaults/dod"),
            format!(
                r#"{{"command_id":"{command}","expected_revision":1,"default_dod":["Tests pass"]}}"#
            ),
        ),
        (
            Method::POST,
            format!("{base}/task-defaults/dod/project"),
            format!(
                r#"{{"command_id":"{command}","project":"P-1","expected_revision":1,"default_dod":[]}}"#
            ),
        ),
        (
            Method::POST,
            format!("{base}/tasks/final-summary"),
            format!(
                r#"{{"command_id":"{command}","task":"T-A1","expected_revision":2,"final_summary":"Shipped"}}"#
            ),
        ),
        (
            Method::POST,
            format!("{base}/tasks/transition"),
            format!(
                r#"{{"command_id":"{command}","task":"T-A1","expected_revision":2,"status":"done","done_anyway_reason":"Waived by owner","final_summary":"Shipped"}}"#
            ),
        ),
    ] {
        let reply = send(&gateway, method, &target, &body, Some(&pair), &[]);
        assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE, "{target}");
    }
    assert!(calls.methods().contains(&"organization.list".to_owned()));
}
