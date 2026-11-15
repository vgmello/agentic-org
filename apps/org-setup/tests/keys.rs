//! Key custody against the fake office server: the host creates every office's database key,
//! stores it before the office's server starts, sends it on the first line of every create,
//! start and discard, and deletes it only after a discard reports `discarded`.
//!
//! Every host here keeps its keys in a development key folder next to its test home (never the
//! Keychain); the tests never see a key in a log: the fake records only fingerprints.

// Development key folders exist in debug builds only (a release host uses the Keychain, which
// tests never touch), and `office.restart` needs the debug-build development fallback.
#![cfg(debug_assertions)]
// Only macOS can spawn the office servers the other tests here start (BUG-187); off macOS their
// helpers are unused.
#![cfg_attr(not(target_os = "macos"), allow(unused_imports, dead_code))]

mod support;

use std::fs;
use std::thread;
use std::time::{Duration, Instant};

use org_office_host::{Config, Host, HostError};
use org_proto::{
    ApiError, ErrorCode, OfficeStatus, OfficeStatusReason, OrganizationCreate,
    OrganizationCreateParams,
};
use serde_json::json;
use support::{alive, eventually, hq, office_info, restart_host, Client, TestHome};

const WAIT: Duration = Duration::from_secs(10);
const A: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";
const B: &str = "01BBBBBBBBBBBBBBBBBBBBBBBB";
const T: &str = "2026-11-08T00:00:00.000Z";

/// A config whose app-only calls (`office.restart`) accept this test process.
fn app_config(home: &TestHome) -> Config {
    let mut config = home.config();
    config.dev_app_path = Some(std::env::current_exe().unwrap());
    config
}

fn create(host: &Host, name: &str) -> Result<org_proto::Organization, ApiError> {
    Client::for_host(host)
        .call::<OrganizationCreate>(&OrganizationCreateParams { name: name.into() })
}

fn restart(host: &Host, office_id: &str) -> Result<(), ApiError> {
    org_office_host::test_support::Fixture::new(host).restart(office_id)
}

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

fn kill(pid: u32) {
    // SAFETY: sends a signal to a process the test started (through the host).
    assert_eq!(unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) }, 0);
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
fn a_create_stores_its_key_before_its_server_starts_and_every_start_gets_it() {
    let home = TestHome::registered();
    let mut config = home.config();
    // Lists the key folder as each `--create` server starts.
    config.set_server_program(home.wrapper(
        "list-keys",
        &format!(
            "if [ \"$1\" = --create ]; then ls '{}' > \"$ORG_OFFICE_HOME/keys-at-create\"; fi\n\
             exec \"$FAKE\" \"$@\"",
            home.keys_dir().display()
        ),
    ));
    let host = home.start(config.clone()).unwrap();
    let org = create(&host, "Acme").unwrap();
    let office_id = org.offices[0].id.clone();
    let listed = fs::read_to_string(home.path().join("keys-at-create")).unwrap();
    assert_eq!(
        listed,
        format!("{office_id}.key\n"),
        "the key wasn't stored before the spawn"
    );
    // The server got the stored key, and nothing else in the folder.
    let stored = home.stored_key_fingerprint(&office_id).unwrap();
    assert_eq!(
        home.db_key_fingerprints("create"),
        std::slice::from_ref(&stored)
    );
    assert_eq!(home.key_ids(), std::slice::from_ref(&office_id));

    // A restart after a crash, and the next host run, load the same key.
    wait_for_state(&host, &office_id, OfficeStatus::Active, None);
    let pid = home.pids("create")[0];
    kill(pid);
    eventually(WAIT, "the office to restart", || {
        home.runs(&office_id) == 1 && !alive(pid)
    });
    wait_for_state(&host, &office_id, OfficeStatus::Active, None);
    host.shutdown();
    let host = restart_host(&home, &config);
    wait_for_state(&host, &office_id, OfficeStatus::Active, None);
    assert_eq!(
        home.db_key_fingerprints(&office_id),
        [stored.clone(), stored]
    );
    host.shutdown();
    home.assert_no_secret_in_home();
}

#[cfg(target_os = "macos")]
#[test]
fn a_missing_or_unreadable_key_is_key_missing_with_no_spawn_until_a_restart_finds_it() {
    let home = TestHome::registered();
    home.write_office(&hq(A, "01ORGA", "Acme", T));
    home.remove_key(A);
    home.write_office(&hq(B, "01ORGB", "Beta", T));
    home.write_key_text(B, "not a key");
    let host = home.start(app_config(&home)).unwrap();
    for office in [A, B] {
        wait_for_state(
            &host,
            office,
            OfficeStatus::Unavailable,
            Some(OfficeStatusReason::KeyMissing),
        );
    }
    // Fatal: no restart on its own, and no server ever started.
    thread::sleep(Duration::from_millis(500));
    assert_eq!((home.runs(A), home.runs(B)), (0, 0));

    // Once the key is back, a restart starts it with that key.
    home.write_key(A);
    restart(&host, A).unwrap();
    wait_for_state(&host, A, OfficeStatus::Active, None);
    assert_eq!(home.runs(A), 1);
    assert_eq!(
        home.db_key_fingerprints(A),
        [home.stored_key_fingerprint(A).unwrap()]
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

#[cfg(target_os = "macos")]
#[test]
fn a_failed_create_keeps_its_key_until_a_discard_reports_discarded() {
    let home = TestHome::registered();
    // The first discard (right after the create) is refused; the next host run's succeeds.
    home.script(json!({
        "create": {"do": "fail", "code": "storage_error"},
        "discard": [{"do": "fail", "code": "storage_error"}, {"do": "ready"}]
    }));
    let host = home.start(home.config()).unwrap();
    let err = create(&host, "Acme").unwrap_err();
    assert_eq!(err.code, ErrorCode::StorageError);
    let created = home.db_key_fingerprints("create");
    assert_eq!(created.len(), 1);
    // The discard got the create's key, refused, and the folder and the key stay.
    let folders = home.office_folders();
    assert_eq!(folders.len(), 1);
    let office_id = folders[0].clone();
    assert_eq!(
        home.discard_keys(),
        [(office_id.clone(), created[0].clone())]
    );
    assert_eq!(home.key_ids(), std::slice::from_ref(&office_id));
    assert_eq!(
        home.stored_key_fingerprint(&office_id).as_ref(),
        Some(&created[0])
    );
    host.shutdown();

    // The next start discards the folder with the kept key; then, and only then, the key goes.
    let host = restart_host(&home, &home.config());
    eventually(WAIT, "the startup discard", || {
        home.office_folders().is_empty()
    });
    eventually(WAIT, "the key to be deleted", || home.key_ids().is_empty());
    assert_eq!(
        home.discard_keys(),
        [
            (office_id.clone(), created[0].clone()),
            (office_id, created[0].clone())
        ]
    );
    host.shutdown();
    home.assert_no_secret_in_home();
}

#[cfg(target_os = "macos")]
#[test]
fn a_crashed_or_timed_out_create_is_discarded_with_its_key_which_goes_only_after_discarded() {
    let home = TestHome::registered();
    // The first create crashes and its discard crashes too; the second hangs until the create
    // times out, and its discard succeeds.
    home.script(json!({
        "create": [{"do": "crash"}, {"do": "hang", "ignore_eof": true}],
        "discard": [{"do": "crash"}, {"do": "ready"}]
    }));
    let mut config = home.config();
    config.timings.create_timeout = Duration::from_secs(5);
    let host = home.start(config).unwrap();
    for name in ["Acme", "Beta"] {
        let err = create(&host, name).unwrap_err();
        assert_eq!(err.code, ErrorCode::StorageError, "{name}");
    }
    let created = home.db_key_fingerprints("create");
    assert_eq!(created.len(), 2);
    let discards = home.discard_keys();
    assert_eq!(discards.len(), 2);
    for ((_, sent), made) in discards.iter().zip(&created) {
        assert_eq!(sent, made, "a discard didn't get its create's key");
    }
    // The crashed discard kept its office's key (and folder); the successful one deleted both.
    let (kept, gone) = (&discards[0].0, &discards[1].0);
    assert_eq!(home.key_ids(), std::slice::from_ref(kept));
    assert_eq!(
        home.stored_key_fingerprint(kept).as_ref(),
        Some(&created[0])
    );
    assert_eq!(home.office_folders(), std::slice::from_ref(kept));
    assert!(!home.has_key(gone));
    host.shutdown();
    home.assert_no_secret_in_home();
}

#[cfg(target_os = "macos")]
#[test]
fn already_exists_and_a_refused_create_keep_the_key_and_discard_nothing() {
    let home = TestHome::registered();
    home.script(json!({"create": [
        {"do": "fail", "code": "already_exists"},
        {"do": "ready", "report": {"org_name": "Other"}}
    ]}));
    let host = home.start(home.config()).unwrap();
    let err = create(&host, "Acme").unwrap_err();
    assert_eq!(err.code, ErrorCode::AlreadyExists);
    let err = create(&host, "Beta").unwrap_err();
    assert_eq!(err.code, ErrorCode::StorageError);
    assert_eq!(home.runs("discard"), 0);
    // Both keys stay: the already-existing folder's (a later start may discard it with this
    // key) and the refused office's, moved aside for a person with its data.
    let refused = home.refused_folders();
    assert_eq!(refused.len(), 1);
    let ids = home.key_ids();
    assert_eq!(ids.len(), 2, "{ids:?}");
    assert!(ids.contains(&refused[0]), "{ids:?}");
    let created: Vec<String> = ids
        .iter()
        .map(|id| home.stored_key_fingerprint(id).unwrap())
        .collect();
    for fingerprint in home.db_key_fingerprints("create") {
        assert!(created.contains(&fingerprint));
    }
    host.shutdown();
    home.assert_no_secret_in_home();
}

#[test]
fn a_create_whose_server_cant_start_keeps_its_key() {
    let home = TestHome::registered();
    let mut config = home.config();
    config.set_server_program(home.path().join("no-such-server"));
    let host = home.start(config).unwrap();
    let err = create(&host, "Acme").unwrap_err();
    assert_eq!(err.code, ErrorCode::StorageError);
    // Stored before the spawn; its discard couldn't start either, so nothing reported
    // `discarded` and the key stays.
    assert_eq!(home.key_ids().len(), 1);
    assert!(home.office_folders().is_empty());
    host.shutdown();
    home.assert_no_secret_in_home();
}

#[cfg(target_os = "macos")]
#[test]
fn a_discard_line_carries_the_offices_key_or_null() {
    let home = TestHome::registered();
    // Unfinished creates: one with a key and databases, one with neither, and one with
    // databases but no key (which can't be checked, so it's kept for a person).
    const KEYED: &str = "01KEYEDKEYEDKEYEDKEYEDKEYE";
    const BARE: &str = "01BAREBAREBAREBAREBAREBARE";
    const LOCKED: &str = "01LOCKEDLOCKEDLOCKEDLOCKED";
    for id in [KEYED, BARE, LOCKED] {
        fs::create_dir_all(home.office_dir(id)).unwrap();
    }
    for id in [KEYED, LOCKED] {
        fs::write(home.office_dir(id).join("db"), b"databases").unwrap();
    }
    home.write_key(KEYED);
    let keyed = home.stored_key_fingerprint(KEYED).unwrap();
    let host = home.start(home.config()).unwrap();
    eventually(WAIT, "the startup discards", || {
        home.discard_keys().len() == 3 && home.office_folders() == [LOCKED]
    });
    let mut discards = home.discard_keys();
    discards.sort();
    assert_eq!(
        discards,
        [
            (BARE.to_owned(), "null".to_owned()),
            (KEYED.to_owned(), keyed),
            (LOCKED.to_owned(), "null".to_owned()),
        ]
    );
    eventually(WAIT, "the discarded office's key to go", || {
        home.key_ids().is_empty()
    });
    host.shutdown();
    home.assert_no_secret_in_home();
}
