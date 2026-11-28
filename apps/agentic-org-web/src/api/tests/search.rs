//! `TASK-91`: `GET /api/v1/organizations/{org}/search` through the full gateway (ingress,
//! routing, the session pair) against the scripted host: the cookie and its proof header are
//! both required, in every mode, and a malformed request is refused before the host or an
//! office is asked anything.

use super::*;

fn search_path(query: &str) -> String {
    format!("/api/v1/organizations/{ORG}/search{query}")
}

fn host_script() -> Arc<Script> {
    organization_script(|method, _| {
        (method == "organization.list").then(|| {
            vec![json!({"result": {"organizations": [host_organization()], "creating": []}})]
        })
    })
}

#[test]
fn search_needs_the_cookie_and_its_proof_and_refuses_bad_queries_before_any_office() {
    let (gateway, calls) = gateway(host_script());
    let target = search_path("?q=zebra");

    // No session, or the cookie without its proof header: 401.
    let unauth = send(&gateway, Method::GET, &target, "", None, &[]);
    assert_eq!(unauth.status, StatusCode::UNAUTHORIZED);
    let pair = sign_in(&gateway).pair();
    let cookie_only = send(
        &gateway,
        Method::GET,
        &target,
        "",
        None,
        &[("cookie", &pair.cookie)],
    );
    assert_eq!(cookie_only.status, StatusCode::UNAUTHORIZED);

    for refused in [
        search_path(""),
        search_path("?q="),
        search_path(&format!("?q={}", "a".repeat(513))),
        search_path("?q=a&q=b"),
        search_path("?q=a&unknown=1"),
        search_path("?q=a&limit=0"),
        search_path("?q=a&limit=101"),
        search_path("?q=a&types=bogus_entity"),
        search_path("?q=a&author_id=not-a-uuid"),
        search_path("?q=a&fields=body"),
        "/api/v1/organizations/not-an-org/search?q=a".to_owned(),
    ] {
        let reply = send(&gateway, Method::GET, &refused, "", Some(&pair), &[]);
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{refused}");
        let text = reply.body.to_string();
        assert!(!text.to_ascii_lowercase().contains("sql"), "{text}");
    }
    let posted = send(&gateway, Method::POST, &target, "{}", Some(&pair), &[]);
    assert_eq!(posted.status, StatusCode::BAD_REQUEST);
    assert!(
        !calls.methods().contains(&"organization.list".to_owned()),
        "refused requests must fail before resolving the office"
    );

    // A valid search goes to the organization's office (503 here: the fixture runs none).
    for valid in [
        search_path("?q=zebra"),
        search_path(
            "?q=foo%22+OR+1%3D1&types=task,message,document,feature&limit=5&fields=ref,title",
        ),
        search_path("?q=plan&types=private_decision&count_only=true"),
    ] {
        let reply = send(&gateway, Method::GET, &valid, "", Some(&pair), &[]);
        assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE, "{valid}");
    }
    assert!(calls.methods().contains(&"organization.list".to_owned()));
}

#[test]
fn search_has_no_sign_in_less_mode() {
    let (gateway, calls) = gateway_with_auth_enabled(
        host_script(),
        org_host_control::listen::ListenMode::HttpLoopback,
        false,
    );
    let reply = send(
        &gateway,
        Method::GET,
        &search_path("?q=zebra"),
        "",
        None,
        &[],
    );
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    assert!(!calls.methods().contains(&"organization.list".to_owned()));
}
