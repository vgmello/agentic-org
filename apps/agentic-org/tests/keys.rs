//! Key custody against the fake office server: a start hands the stored office secrets key over
//! the rendezvous, a missing or unreadable key starts the office anyway with `null`, and the key
//! is never touched by a failed start. (Create-time key custody is the HQ create's, tested in org-host;
//! the launch discard handoff carries no key and is tested in org-host's supervisor.)
//!
//! Every host here keeps its keys in a development key folder next to its test home (never the
//! Keychain); the tests never see a key in a log: the fake records only fingerprints.

// Development key folders exist in debug builds only (a release host uses the Keychain, which
// tests never touch).
#![cfg(debug_assertions)]
// Only macOS can spawn the office servers the other tests here start (BUG-187); off macOS their
// helpers are unused.
#![cfg_attr(not(target_os = "macos"), allow(unused_imports, dead_code))]

mod support;

use std::thread;
use std::time::{Duration, Instant};

use org_host::{Host, HostError};
use org_proto::{OfficeStatus, OfficeStatusReason};
use serde_json::json;
use support::{hq, office_info, TestHome};

const WAIT: Duration = Duration::from_secs(10);
const A: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";
const B: &str = "01BBBBBBBBBBBBBBBBBBBBBBBB";
const T: &str = "2026-11-08T00:00:00.000Z";

fn wait_for_state(
    host: &Host,
    office_id: &str,
    status: OfficeStatus,
    reason: Option<OfficeStatusReason>,
) {
    let deadline = Instant::now() + WAIT;
    loop {
        let info = office_info(host, office_id).expect("the office is listed");
        if info.status == status && info.status_reason == reason {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{office_id} never became {status:?} ({reason:?}); last {info:?}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_debug_host_without_a_dev_key_folder_refuses_to_start() {
    let home = TestHome::new();
    for dir in [None, Some(std::path::PathBuf::from("relative/keys"))] {
        let mut config = home.config();
        config.dev_key_dir = dir.clone();
        match home.start(config) {
            Err(e @ HostError::NoDevKeyDir) => {
                assert!(e.to_string().contains("ORG_OFFICE_DEV_KEY_DIR"), "{e}");
            }
            Err(other) => panic!("{dir:?}: {other}"),
            Ok(_) => panic!("{dir:?}: a debug host started without a dev key folder"),
        }
        assert!(!home.path().exists(), "{dir:?}: it touched the home");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn a_missing_or_unreadable_key_still_starts_the_office_with_a_null_key() {
    let home = TestHome::registered();
    home.write_office(&hq(A, "01ORGA", "Acme", T));
    home.remove_key(A);
    home.write_office(&hq(B, "01ORGB", "Beta", T));
    home.write_key_text(B, "not a key");
    let host = home.start(home.config()).unwrap();
    // The data is plain SQLite: each office serves it, its handoff's key `null`.
    for office in [A, B] {
        wait_for_state(&host, office, OfficeStatus::Active, None);
        assert_eq!(home.runs(office), 1, "{office}");
        assert_eq!(home.db_key_fingerprints(office), ["null"], "{office}");
    }

    host.shutdown();

    // Once the key is back, the next start hands it over.
    home.write_key(A);
    let host = home.start(home.config()).unwrap();
    wait_for_state(&host, A, OfficeStatus::Active, None);
    assert_eq!(home.runs(A), 2);
    assert_eq!(
        home.db_key_fingerprints(A),
        ["null".to_owned(), home.stored_key_fingerprint(A).unwrap()]
    );
    // The host never replaces or deletes an item, even one that isn't a key.
    assert_eq!(home.key_text(B).as_deref(), Some("not a key"));
    host.shutdown();
    home.assert_no_secret_in_home();
}

#[cfg(target_os = "macos")]
#[test]
fn a_servers_key_missing_is_key_missing_with_no_restarts_and_the_key_untouched() {
    let home = TestHome::registered();
    home.write_office(&hq(A, "01ORGA", "Acme", T));
    let before = home.key_text(A).unwrap();
    home.script(json!({ A: {"do": "fail", "code": "key_missing"} }));
    let host = home.start(home.config()).unwrap();
    wait_for_state(
        &host,
        A,
        OfficeStatus::Unavailable,
        Some(OfficeStatusReason::KeyMissing),
    );
    // Several backoff periods pass without a restart.
    thread::sleep(Duration::from_millis(800));
    assert_eq!(home.runs(A), 1);
    assert_eq!(home.key_text(A).unwrap(), before);
    host.shutdown();
    home.assert_no_secret_in_home();
}
