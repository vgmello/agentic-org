//! App tokens (ADR-0003 §2.1) on the control pipe, against the fake office server: one per
//! office per host run, in the first line of every start, never in a log.
//!
//! The tests never see a token: the fake records a fingerprint of each one it gets.

mod support;

use std::thread;
use std::time::Duration;

use org_office_host::Host;
use org_office_launch::{write_host_message, CreateOffice, HostMessage, StartOffice};
use org_proto::{
    AppToken, Empty, ErrorCode, OfficeList, OfficeRole, OfficeStatus, OrganizationCreate,
    OrganizationCreateParams,
};
use serde_json::json;
use support::{alive, eventually, has_hex_run, hq, restart_host, Client, TestHome};

const WAIT: Duration = Duration::from_secs(10);

const A: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";
const B: &str = "01BBBBBBBBBBBBBBBBBBBBBBBB";

fn status(host: &Host, office_id: &str) -> OfficeStatus {
    Client::connect(host.socket_path())
        .call::<OfficeList>(&Empty {})
        .unwrap()
        .offices
        .into_iter()
        .find(|o| o.id == office_id)
        .map_or(OfficeStatus::Unknown, |o| o.status)
}

fn wait_for(host: &Host, office_id: &str, want: OfficeStatus) {
    eventually(WAIT, &format!("{office_id} to be {want:?}"), || {
        status(host, office_id) == want
    });
}

/// A host message as one pipe line, for a wrapper script to send in the host's place.
fn line(message: &HostMessage) -> String {
    let mut out = Vec::new();
    write_host_message(&mut out, message).unwrap();
    String::from_utf8(out).unwrap()
}

fn kill(pid: u32) {
    // SAFETY: sends a signal to a process the test started (through the host).
    assert_eq!(unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) }, 0);
}

#[test]
fn every_start_gets_its_office_token_and_restarts_reuse_it() {
    let home = TestHome::new();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    home.write_office(&hq(B, "01ORGB", "Beta", "2026-11-08T00:00:00.000Z"));
    home.script(json!({ A: [{"do": "crash"}, {"do": "crash"}, {"do": "ready"}] }));
    let host = Host::start(home.config()).unwrap();
    wait_for(&host, A, OfficeStatus::Active);
    wait_for(&host, B, OfficeStatus::Active);

    let a = home.token_fingerprints(A);
    assert_eq!(home.runs(A), 3);
    assert_eq!(a.len(), 3, "a start without a token: {a:?}");
    assert!(
        a.iter().all(|fp| *fp == a[0]),
        "a restart got a new token: {a:?}"
    );
    let b = home.token_fingerprints(B);
    assert_eq!(b.len(), 1);
    assert_ne!(a[0], b[0], "two offices share a token");
    assert!(home.first_line_refusals(A).is_empty());

    // The fake wrote its fingerprints to stderr, so the office logs exist and were checked.
    let handle = &A[A.len() - 12..];
    let logs = home.logs();
    assert!(logs
        .iter()
        .any(|(path, text)| path.ends_with(format!("{handle}.log")) && text.contains(&a[0])));
    home.assert_no_token_in_logs();
    host.shutdown();
}

#[test]
fn a_host_restart_makes_new_tokens() {
    let home = TestHome::new();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    for _ in 0..2 {
        let host = restart_host(&home.config());
        wait_for(&host, A, OfficeStatus::Active);
        host.shutdown();
    }
    let a = home.token_fingerprints(A);
    assert_eq!(a.len(), 2);
    assert_ne!(
        a[0], a[1],
        "the second host run reused the first run's token"
    );
    home.assert_no_token_in_logs();
}

#[test]
fn a_created_office_keeps_its_token_when_it_restarts() {
    let home = TestHome::new();
    let host = Host::start(home.config()).unwrap();
    let mut client = Client::connect(host.socket_path());
    let org = client
        .call::<OrganizationCreate>(&OrganizationCreateParams {
            name: "Acme".into(),
        })
        .unwrap();
    let office_id = org.offices[0].id.clone();
    wait_for(&host, &office_id, OfficeStatus::Active);
    let created = home.token_fingerprints("create");
    assert_eq!(created.len(), 1);

    // Kill the server twice; each restart (`--office`) gets the token the create got.
    let mut pid = home.pids("create")[0];
    for restarts in 1..=2 {
        kill(pid);
        eventually(WAIT, "the office to restart", || {
            home.runs(&office_id) == restarts && !alive(pid)
        });
        wait_for(&host, &office_id, OfficeStatus::Active);
        pid = *home.pids(&office_id).last().unwrap();
    }
    assert_eq!(
        home.token_fingerprints(&office_id),
        [created[0].clone(), created[0].clone()]
    );

    // Another organization's office gets a token of its own.
    let other = client
        .call::<OrganizationCreate>(&OrganizationCreateParams {
            name: "Beta".into(),
        })
        .unwrap();
    let fingerprints = home.token_fingerprints("create");
    assert_eq!(fingerprints.len(), 2);
    assert_ne!(fingerprints[0], fingerprints[1]);
    wait_for(&host, &other.offices[0].id, OfficeStatus::Active);
    home.assert_no_token_in_logs();
    host.shutdown();
}

#[test]
fn a_malformed_or_wrong_first_line_makes_the_office_unavailable() {
    let create = line(&HostMessage::Create(CreateOffice {
        org_id: "01ORGA".into(),
        org_name: "Acme".into(),
        office_id: A.into(),
        office_name: "HQ".into(),
        role: OfficeRole::Hq,
        app_token: AppToken::from_bytes([7; 32]),
    }));
    let cases = [
        (
            "malformed",
            r#"printf 'not json\n' | "$FAKE" "$@""#.to_owned(),
        ),
        (
            "unknown-op",
            r#"printf '{"v":1,"op":"stop"}\n' | "$FAKE" "$@""#.to_owned(),
        ),
        (
            "create-op",
            format!(r#"printf '%s' '{create}' | "$FAKE" "$@""#),
        ),
    ];
    for (name, body) in cases {
        let home = TestHome::new();
        home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
        let mut config = home.config();
        config.server_program = home.wrapper(name, &body);
        let host = Host::start(config).unwrap();
        wait_for(&host, A, OfficeStatus::Unavailable);
        // Several backoff periods (100 + 200 + 400 ms) pass without a restart:
        // invalid_request is fatal.
        thread::sleep(Duration::from_millis(800));
        assert_eq!(status(&host, A), OfficeStatus::Unavailable, "{name}");
        assert_eq!(home.runs(A), 1, "{name}");
        assert_eq!(home.first_line_refusals(A).len(), 1, "{name}");
        assert!(home.token_fingerprints(A).is_empty(), "{name}");
        host.shutdown();
    }
}

#[test]
fn a_server_whose_stdin_closes_before_its_first_line_exits_and_is_restarted() {
    let home = TestHome::new();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    let mut config = home.config();
    config.server_program = home.wrapper("no-first-line", r#"exec "$FAKE" "$@" </dev/null"#);
    let host = Host::start(config).unwrap();
    // The contract: the host is gone, so the server exits 0 without a `failed` line. That
    // isn't fatal: the host restarts it with backoff.
    eventually(WAIT, "two runs", || {
        home.eof_before_first_line(A).len() >= 2
    });
    assert_ne!(status(&host, A), OfficeStatus::Unavailable);
    assert!(home.first_line_refusals(A).is_empty());
    assert!(home.token_fingerprints(A).is_empty());
    host.shutdown();
}

#[test]
fn a_create_whose_first_line_is_a_start_line_fails() {
    let start = line(&HostMessage::Start(StartOffice {
        app_token: AppToken::from_bytes([7; 32]),
    }));
    let home = TestHome::new();
    let mut config = home.config();
    config.server_program = home.wrapper(
        "start-op",
        &format!(r#"printf '%s' '{start}' | "$FAKE" "$@""#),
    );
    let host = Host::start(config).unwrap();
    let err = Client::connect(host.socket_path())
        .call::<OrganizationCreate>(&OrganizationCreateParams {
            name: "Acme".into(),
        })
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::StorageError);
    assert_eq!(home.first_line_refusals("create").len(), 1);
    assert!(home.token_fingerprints("create").is_empty());
    host.shutdown();
}

#[test]
fn the_log_check_finds_a_token() {
    let hex = "0123456789abcdef".repeat(4);
    assert!(has_hex_run(&format!("x {hex} y"), 64));
    assert!(!has_hex_run(&format!("x {} y", &hex[..63]), 64));
    assert!(!has_hex_run(&hex.to_uppercase(), 64));
}
