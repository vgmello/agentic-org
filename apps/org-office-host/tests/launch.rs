//! Launch v3 (X-02 §5.1, §5.4) against the fake office server: the first line names the
//! instance and a rendezvous; the keys (the database key, a fresh grant key per start, a
//! create's recovery code) and one transfer descriptor go only to the admitted image's
//! connection; events count only from there, never from stdout.
//!
//! The tests never see a key: the fake records a fingerprint of each one it gets.

// Every host here keeps its offices' keys in a development key folder, which only debug builds
// have (a release host uses the Keychain, which tests never touch): debug builds only.
#![cfg(debug_assertions)]
// Only macOS can spawn the office servers the other tests here start (BUG-187); off macOS their
// helpers are unused.
#![cfg_attr(not(target_os = "macos"), allow(unused_imports, dead_code))]

mod support;

use std::collections::HashSet;
use std::thread;
use std::time::Duration;

use org_office_host::Host;
use org_proto::{
    Empty, ErrorCode, OfficeList, OfficeStatus, OrganizationCreate, OrganizationCreateParams,
};
use serde_json::json;
use support::{eventually, has_hex_run, hq, restart_host, Client, TestHome};

const WAIT: Duration = Duration::from_secs(10);

const A: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";
const B: &str = "01BBBBBBBBBBBBBBBBBBBBBBBB";
const T: &str = "2026-11-08T00:00:00.000Z";

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

fn create(host: &Host, name: &str) -> Result<org_proto::Organization, org_proto::ApiError> {
    Client::for_host(host)
        .call::<OrganizationCreate>(&OrganizationCreateParams { name: name.into() })
}

#[cfg(target_os = "macos")]
#[test]
fn every_start_gets_a_fresh_grant_key_and_one_transfer_socket() {
    let home = TestHome::registered();
    home.write_office(&hq(A, "01ORGA", "Acme", T));
    home.write_office(&hq(B, "01ORGB", "Beta", T));
    home.script(json!({ A: [{"do": "crash"}, {"do": "crash"}, {"do": "ready"}] }));
    let host = home.start(home.config()).unwrap();
    wait_for(&host, A, OfficeStatus::Active);
    wait_for(&host, B, OfficeStatus::Active);
    host.shutdown();
    let host = restart_host(&home, &home.config());
    wait_for(&host, A, OfficeStatus::Active);
    host.shutdown();

    let a = home.grant_fingerprints(A);
    assert_eq!(home.runs(A), 4);
    assert_eq!(a.len(), 4, "a start without a grant key: {a:?}");
    let b = home.grant_fingerprints(B);
    let all: HashSet<_> = a.iter().chain(&b).collect();
    assert_eq!(all.len(), 6, "a grant key was reused: {a:?} {b:?}");
    assert_eq!(home.transfers(A), [1, 1, 1, 1]);
    // Every line names this host's instance; a start names no owner.
    for line in home.first_lines(A) {
        assert_eq!(line, (home.instance_id().to_owned(), "-".to_owned()));
    }
    home.assert_no_secret_in_home();
}

#[cfg(target_os = "macos")]
#[test]
fn a_create_names_an_owner_the_host_chose_and_gets_a_recovery_code() {
    let home = TestHome::registered();
    let host = home.start(home.config()).unwrap();
    let mut owners = Vec::new();
    for name in ["Acme", "Beta"] {
        let org = create(&host, name).unwrap();
        wait_for(&host, &org.offices[0].id, OfficeStatus::Active);
    }
    for (instance, owner) in home.first_lines("create") {
        assert_eq!(instance, home.instance_id());
        assert!(owner.len() == 26 && owner.bytes().all(|b| b.is_ascii_alphanumeric()));
        owners.push(owner);
    }
    assert_eq!(owners.len(), 2);
    assert_ne!(owners[0], owners[1]);
    assert_eq!(home.recovery_codes("create"), 2);
    assert_eq!(home.transfers("create"), [1, 1]);
    host.shutdown();
    home.assert_no_secret_in_home();
}

/// X-02 §5.1 step 4: a server whose rendezvous peer isn't the admitted image (here `sh`, which
/// was admitted, starts the fake as its child, or `exec`s it) is killed and gets no key.
#[cfg(target_os = "macos")]
#[test]
fn a_peer_that_isnt_the_admitted_image_gets_no_keys() {
    for (name, body) in [
        ("child", r#""$FAKE" "$@""#),
        ("exec", r#"exec "$FAKE" "$@""#),
    ] {
        let home = TestHome::registered();
        home.write_office(&hq(A, "01ORGA", "Acme", T));
        let mut config = home.config();
        config.set_server_program(home.wrapper(name, body));
        let host = home.start(config).unwrap();
        eventually(WAIT, "two refused starts", || home.runs(A) >= 2);
        thread::sleep(Duration::from_millis(200));
        assert_ne!(status(&host, A), OfficeStatus::Active, "{name}");
        assert!(home.grant_fingerprints(A).is_empty(), "{name}");
        assert!(home.db_key_fingerprints(A).is_empty(), "{name}");
        assert!(home.transfers(A).is_empty(), "{name}");
        host.shutdown();
    }
}

/// X-02 §5.1 step 7: events count only from the rendezvous. A `discarded` forged on stdout
/// doesn't delete a key, and a `created` forged there doesn't make an organization.
#[cfg(target_os = "macos")]
#[test]
fn lines_forged_on_stdout_neither_delete_a_key_nor_create() {
    let home = TestHome::registered();
    let unfinished = "01UNFINISHEDUNFINISHEDUNFI";
    std::fs::create_dir_all(home.office_dir(unfinished)).unwrap();
    std::fs::write(home.office_dir(unfinished).join("db"), b"databases").unwrap();
    home.write_key(unfinished);
    home.script(json!({
        "discard": {"do": "fail", "code": "storage_error", "forge_stdout": true},
        "create": {"do": "fail", "code": "invalid_name", "forge_stdout": true}
    }));
    let host = home.start(home.config()).unwrap();
    eventually(WAIT, "the startup discard", || home.runs("discard") == 1);
    let err = create(&host, "Acme").unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidName);
    host.shutdown();
    assert!(home.key_ids().contains(&unfinished.to_owned()));
    assert!(home.office_dir(unfinished).exists());
}

#[test]
fn the_log_check_finds_a_token() {
    let hex = "0123456789abcdef".repeat(4);
    assert!(has_hex_run(&format!("x {hex} y"), 64));
    assert!(!has_hex_run(&format!("x {} y", &hex[..63]), 64));
    assert!(!has_hex_run(&hex.to_uppercase(), 64));
}
