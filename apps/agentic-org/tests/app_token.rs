//! Launch secrets on the v3 rendezvous, against the fake office server: every start's handoff
//! carries a grant key of its own (a fresh one per run), and a worker that refuses its first line
//! never gets one. v3 hands off no app token.
//!
//! The tests never see a key: the fake records a fingerprint of each one it gets.

// Every host here keeps its offices' keys in a development key folder, which only debug builds
// have (a release host uses the Keychain, which tests never touch): debug builds only.
#![cfg(debug_assertions)]
// Only macOS can spawn the office servers the other tests here start (BUG-187); off macOS their
// helpers are unused.
#![cfg_attr(not(target_os = "macos"), allow(unused_imports, dead_code))]

mod support;

use std::time::Duration;

use org_host::Host;
use org_office_launch::{write_host_message, CreateOffice, HostMessage};
use org_proto::{AppToken, DbKey, Empty, OfficeList, OfficeRole, OfficeStatus};
use serde_json::json;
use support::{eventually, has_hex_run, hq, Client, TestHome};

const WAIT: Duration = Duration::from_secs(10);

const A: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";
const B: &str = "01BBBBBBBBBBBBBBBBBBBBBBBB";

fn status(host: &Host, office_id: &str) -> OfficeStatus {
    Client::for_host(host)
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

#[cfg(target_os = "macos")]
#[test]
fn every_start_gets_a_grant_key_of_its_own() {
    let home = TestHome::registered();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    home.write_office(&hq(B, "01ORGB", "Beta", "2026-11-08T00:00:00.000Z"));
    home.script(json!({ A: [{"do": "crash"}, {"do": "crash"}, {"do": "ready"}] }));
    let host = home.start(home.config()).unwrap();
    wait_for(&host, A, OfficeStatus::Active);
    wait_for(&host, B, OfficeStatus::Active);

    let a = home.grant_fingerprints(A);
    assert_eq!(home.runs(A), 3);
    assert_eq!(a.len(), 3, "a start without a grant key: {a:?}");
    let mut distinct = a.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(distinct.len(), 3, "a restart reused a grant key: {a:?}");
    let b = home.grant_fingerprints(B);
    assert_eq!(b.len(), 1);
    assert!(!a.contains(&b[0]), "two offices share a grant key");
    assert!(home.first_line_refusals(A).is_empty());
    assert!(
        home.token_fingerprints(A).is_empty(),
        "v3 hands off no app token"
    );
    home.assert_no_secret_in_home();
    host.shutdown();
}

/// Under v3 the worker can report on the rendezvous only once it has read a valid first line
/// naming it; a refusal before that goes to stdout, which the host does not read. So a worker
/// that refuses its first line and exits is a crash: restarted with backoff, never given a key,
/// and noticed when it exits rather than at the end of the rendezvous wait.
#[cfg(target_os = "macos")]
#[test]
fn a_worker_that_refuses_its_first_line_is_restarted_as_a_crash_and_gets_no_secret() {
    let create = line(&HostMessage::Create(CreateOffice {
        org_id: "01ORGA".into(),
        org_name: "Acme".into(),
        office_id: A.into(),
        office_name: "HQ".into(),
        role: OfficeRole::Hq,
        app_token: AppToken::from_bytes([7; 32]),
        db_key: DbKey::from_bytes([8; 32]),
    }));
    let cases = [
        (
            "malformed",
            r#"printf 'not json\n' | "$FAKE" "$@""#.to_owned(),
        ),
        (
            "unknown-op",
            r#"printf '{"v":2,"op":"stop"}\n' | "$FAKE" "$@""#.to_owned(),
        ),
        (
            "create-op",
            format!(r#"printf '%s' '{create}' | "$FAKE" "$@""#),
        ),
    ];
    for (name, body) in cases {
        let home = TestHome::registered();
        home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
        let mut config = home.config();
        config.set_server_program(home.wrapper(name, &body));
        let host = home.start(config).unwrap();
        // Two runs well inside the host's 5 s rendezvous wait: the exit ended the first wait.
        eventually(
            Duration::from_secs(3),
            &format!("{name}: a restart"),
            || home.first_line_refusals(A).len() >= 2,
        );
        assert_ne!(status(&host, A), OfficeStatus::Unavailable, "{name}");
        assert_ne!(status(&host, A), OfficeStatus::Active, "{name}");
        assert!(home.grant_fingerprints(A).is_empty(), "{name}");
        assert!(home.db_key_fingerprints(A).is_empty(), "{name}");
        host.shutdown();
    }
}

#[cfg(target_os = "macos")]
#[test]
fn a_server_whose_stdin_closes_before_its_first_line_exits_and_is_restarted() {
    let home = TestHome::registered();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    let mut config = home.config();
    config.set_server_program(home.wrapper("no-first-line", r#"exec "$FAKE" "$@" </dev/null"#));
    let host = home.start(config).unwrap();
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
fn the_log_check_finds_a_token() {
    let hex = "0123456789abcdef".repeat(4);
    assert!(has_hex_run(&format!("x {hex} y"), 64));
    assert!(!has_hex_run(&format!("x {} y", &hex[..63]), 64));
    assert!(!has_hex_run(&hex.to_uppercase(), 64));
}
