//! Launch secrets on the launch rendezvous, against the fake office server: every start's handoff
//! carries a grant key of its own (a fresh one per run), and a worker that refuses its first line
//! never gets one. The handoff carries no app token.
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
use org_proto::{Empty, OfficeList, OfficeStatus, OfficeStatusReason};
use serde_json::json;
use support::{eventually, has_hex_run, hq, office_info, Client, TestHome};

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
    home.assert_no_secret_in_home();
    host.shutdown();
}

/// The worker can report on the rendezvous only once it has read a valid first line
/// naming it; a refusal before that goes to stdout, which the host does not read. So a worker
/// that refuses its first line and exits is a crash: restarted with backoff, never given a key,
/// and noticed when it exits rather than at the end of the rendezvous wait.
#[cfg(target_os = "macos")]
#[test]
fn a_worker_that_refuses_its_first_line_is_restarted_as_a_crash_and_gets_no_secret() {
    // A retired v2 create line, keys included (the launch library no longer writes v2): the
    // worker refuses it by its version without decoding it.
    let v2_create = format!(
        r#"{{"v":2,"op":"create","org_id":"01ORGA","org_name":"Acme","office_id":"{A}","office_name":"HQ","role":"hq","app_token":"{}","db_key":"{}"}}"#,
        "07".repeat(32),
        "08".repeat(32),
    );
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
            "v2-create",
            format!(r#"printf '%s\n' '{v2_create}' | "$FAKE" "$@""#),
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

/// A worker that refuses every first line is given up on: with a give-up limit of 3 its third
/// short exit makes the office `unavailable`/`restarts_exhausted`, and no further run starts.
#[cfg(target_os = "macos")]
#[test]
fn a_worker_that_always_refuses_its_first_line_is_given_up_on() {
    let home = TestHome::registered();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    let mut config = home.config();
    config.set_server_program(home.wrapper("malformed", r#"printf 'not json\n' | "$FAKE" "$@""#));
    config.timings.give_up_after = 3;
    config.timings.backoff_initial = Duration::from_millis(20);
    config.timings.backoff_max = Duration::from_millis(40);
    let host = home.start(config).unwrap();
    eventually(WAIT, "the office to be given up on", || {
        office_info(&host, A).is_some_and(|info| {
            info.status == OfficeStatus::Unavailable
                && info.status_reason == Some(OfficeStatusReason::RestartsExhausted)
        })
    });
    assert_eq!(home.first_line_refusals(A).len(), 3);
    // Many backoff periods later, still three runs: nothing restarts it.
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(home.first_line_refusals(A).len(), 3);
    assert_eq!(home.runs(A), 3);
    assert_eq!(status(&host, A), OfficeStatus::Unavailable);
    assert!(home.grant_fingerprints(A).is_empty());
    assert!(home.db_key_fingerprints(A).is_empty());
    host.shutdown();
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
    assert!(home.grant_fingerprints(A).is_empty());
    host.shutdown();
}

#[test]
fn the_log_check_finds_a_token() {
    let hex = "0123456789abcdef".repeat(4);
    assert!(has_hex_run(&format!("x {hex} y"), 64));
    assert!(!has_hex_run(&format!("x {} y", &hex[..63]), 64));
    assert!(!has_hex_run(&hex.to_uppercase(), 64));
}
