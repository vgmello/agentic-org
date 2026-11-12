use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Barrier};
use std::time::Duration;

thread_local! {
    static FAIL: std::cell::Cell<Option<Failure>> = const { std::cell::Cell::new(None) };
}

pub(super) fn take_failure() -> Option<Failure> {
    FAIL.with(|failure| failure.take())
}

pub(in crate::hostdb) fn fail_with(error: Error) {
    let failure = match error {
        Error::NoCipher => Failure::NoCipher,
        Error::Sql(code) => Failure::Sql(code),
        other => panic!("unsupported initialization fault: {other:?}"),
    };
    FAIL.with(|slot| slot.set(Some(failure)));
}

#[test]
fn cargo_dependencies_keep_the_two_fences_in_separate_processes() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let result = std::process::Command::new(env!("CARGO"))
        .current_dir(root)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--offline",
            "--all-features",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    require_separate_fences(&metadata);
}

fn require_separate_fences(metadata: &serde_json::Value) {
    let packages = metadata["packages"].as_array().unwrap();
    let package_id = |name| {
        packages.iter().find(|p| p["name"] == name).unwrap()["id"]
            .as_str()
            .unwrap()
    };
    let host = package_id("org-office-host");
    let office = package_id("org-office");
    let nodes = metadata["resolve"]["nodes"].as_array().unwrap();
    let reachable = |start: &str| {
        let mut seen = std::collections::HashSet::new();
        let mut pending = vec![start.to_owned()];
        while let Some(id) = pending.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            let node = nodes.iter().find(|node| node["id"] == id).unwrap();
            pending.extend(
                node["dependencies"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap().to_owned()),
            );
        }
        seen
    };
    // If either store starts sharing a process, merge into one fence before linking.
    assert!(
        !reachable(host).contains(office),
        "host now depends on office: share one SQLCipher fence"
    );
    let members = metadata["workspace_members"].as_array().unwrap();
    for package in packages {
        if !members.contains(&package["id"])
            || !package["targets"].as_array().unwrap().iter().any(|target| {
                target["kind"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|kind| kind == "bin")
            })
        {
            continue;
        }
        let dependencies = reachable(package["id"].as_str().unwrap());
        assert!(
            !(dependencies.contains(host) && dependencies.contains(office)),
            "{} links both SQLCipher fences: merge them before linking",
            package["name"]
        );
    }
}

#[test]
fn dependency_policy_rejects_direct_transitive_and_binary_colinking() {
    let valid = serde_json::json!({
        "packages": [
            {"name":"org-office-host","id":"host","targets":[{"kind":["lib"]}]},
            {"name":"org-office","id":"office","targets":[{"kind":["lib"]}]},
            {"name":"bridge","id":"bridge","targets":[{"kind":["lib"]}]},
            {"name":"consumer","id":"consumer","targets":[{"kind":["bin"]}]}
        ],
        "workspace_members":["host","office","bridge","consumer"],
        "resolve":{"nodes":[
            {"id":"host","dependencies":[]}, {"id":"office","dependencies":[]},
            {"id":"bridge","dependencies":[]}, {"id":"consumer","dependencies":["host"]}
        ]}
    });
    require_separate_fences(&valid);
    let mut office_only = valid.clone();
    office_only["resolve"]["nodes"][3]["dependencies"] = serde_json::json!(["office"]);
    require_separate_fences(&office_only);
    for (host, bridge, consumer) in [
        (vec!["office"], vec![], vec![]),
        (vec!["bridge"], vec!["office"], vec![]),
        (vec![], vec![], vec!["host", "office"]),
    ] {
        let mut invalid = valid.clone();
        invalid["resolve"]["nodes"][0]["dependencies"] = serde_json::json!(host);
        invalid["resolve"]["nodes"][2]["dependencies"] = serde_json::json!(bridge);
        invalid["resolve"]["nodes"][3]["dependencies"] = serde_json::json!(consumer);
        assert!(std::panic::catch_unwind(|| require_separate_fences(&invalid)).is_err());
    }
}

#[test]
fn shipped_opener_propagates_nocipher_and_sql_failure_before_any_file_open() {
    for failure in [
        Failure::NoCipher,
        Failure::Sql(rusqlite::ErrorCode::OutOfMemory),
    ] {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("missing.db");
        let key = crate::host_secrets::Value::new(vec![0; 32]).unwrap();
        FAIL.with(|slot| slot.set(Some(failure)));
        let actual = super::super::connect(&path, &key);
        assert_eq!(actual.err(), Some(Error::from(failure)));
        assert!(!path.exists());
    }
}

#[test]
fn successful_and_failed_initialization_are_cached_for_every_waiter() {
    for outcome in [
        Ok(()),
        Err(Failure::NoCipher),
        Err(Failure::Sql(rusqlite::ErrorCode::OutOfMemory)),
    ] {
        let state = OnceLock::new();
        let calls = AtomicUsize::new(0);
        let start = Barrier::new(16);
        std::thread::scope(|scope| {
            for _ in 0..16 {
                let expected = outcome.map_err(Error::from);
                let state = &state;
                let calls = &calls;
                let start = &start;
                scope.spawn(move || {
                    start.wait();
                    assert_eq!(
                        initialized(state, || {
                            calls.fetch_add(1, Ordering::SeqCst);
                            outcome
                        }),
                        expected
                    );
                });
            }
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            initialized(&state, || panic!("cached result retried")),
            outcome.map_err(Error::from)
        );
    }
}

#[test]
fn no_waiter_opens_until_the_initialization_body_finishes() {
    let state = OnceLock::new();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (started_tx, started_rx) = mpsc::channel();
    let (opened_tx, opened_rx) = mpsc::channel();
    std::thread::scope(|scope| {
        let state = &state;
        scope.spawn(move || {
            initialized(state, || {
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(())
            })
            .unwrap()
        });
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        for _ in 0..8 {
            let started = started_tx.clone();
            let opened = opened_tx.clone();
            scope.spawn(move || {
                started.send(()).unwrap();
                let result = initialized(state, || panic!("another initializer entered"));
                opened.send(result).unwrap();
            });
        }
        for _ in 0..8 {
            started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        let early = opened_rx.recv_timeout(Duration::from_millis(100));
        // Always release the initializer before asserting, so even a failing test reaps workers.
        release_tx.send(()).unwrap();
        assert_eq!(early, Err(mpsc::RecvTimeoutError::Timeout));
        for _ in 0..8 {
            assert_eq!(
                opened_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
                Ok(())
            );
        }
    });
}

#[test]
fn initialization_errors_refuse_before_probe_and_probe_errors_are_preserved() {
    for code in [rusqlite::ffi::SQLITE_ERROR, rusqlite::ffi::SQLITE_NOMEM] {
        assert_eq!(
            initialize(|| code, || panic!("probe after failed initialization")),
            Err(Failure::Sql(rusqlite::ffi::Error::new(code).code))
        );
    }
    let done = std::cell::Cell::new(false);
    assert_eq!(
        initialize(
            || {
                done.set(true);
                0
            },
            || {
                assert!(done.get(), "probe preceded initialization");
                Ok("4.5.0".into())
            }
        ),
        Ok(())
    );
    assert_eq!(
        initialize(|| 0, || Ok(String::new())),
        Err(Failure::NoCipher)
    );
    let error = Failure::Sql(rusqlite::ErrorCode::PermissionDenied);
    assert_eq!(initialize(|| 0, || Err(error)), Err(error));
}

#[test]
fn probe_failure_discards_sql_and_key_payloads() {
    let secret = "ab".repeat(32);
    let sql = format!("PRAGMA key=\"x'{secret}'\";");
    for error in [
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_IOERR),
            Some(sql.clone()),
        ),
        rusqlite::Error::SqlInputError {
            error: rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_IOERR),
            msg: sql.clone(),
            sql: sql.clone(),
            offset: 0,
        },
        rusqlite::Error::InvalidParameterName(sql),
    ] {
        let has_code = !matches!(error, rusqlite::Error::InvalidParameterName(_));
        let failure = Failure::from(error);
        assert_eq!(
            failure,
            Failure::Sql(if has_code {
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_IOERR).code
            } else {
                rusqlite::ErrorCode::Unknown
            })
        );
        let rendered = format!("{failure:?} {:?}", Error::from(failure));
        assert!(!rendered.contains(&secret));
        assert!(!rendered.contains("PRAGMA"));
    }
}

#[test]
fn durable_opener_waits_for_the_process_fence_before_opening() {
    let source = include_str!("../../hostdb.rs");
    let body =
        &source[source.find("fn connect(").unwrap()..source.find("fn require_cipher(").unwrap()];
    assert!(
        body.find("sqlcipher::ready()?").unwrap()
            < body.find("Connection::open_with_flags").unwrap()
    );
    assert_eq!(
        source
            .matches("#[allow(clippy::disallowed_methods)]")
            .count(),
        1
    );
    let source = include_str!("../sqlcipher.rs");
    assert!(source.contains("static READY: OnceLock<Result<(), Failure>>"));
    let body = &source[source.find("pub(super) fn ready()").unwrap()..];
    assert!(body.find("initialized(&READY").unwrap() < body.find("sqlite3_initialize()").unwrap());
    assert!(
        body.find("sqlite3_initialize()").unwrap()
            < body.find("Connection::open_in_memory").unwrap()
    );
    assert_eq!(
        source
            .matches("#[allow(clippy::disallowed_methods)]")
            .count(),
        1
    );
}

static DENY_PRAGMA: AtomicUsize = AtomicUsize::new(0);
static DENIED: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" fn deny_pragma(
    _: *mut std::ffi::c_void,
    action: i32,
    name: *const std::ffi::c_char,
    _: *const std::ffi::c_char,
    _: *const std::ffi::c_char,
    _: *const std::ffi::c_char,
) -> i32 {
    if action == rusqlite::ffi::SQLITE_PRAGMA && !name.is_null() {
        // SAFETY: SQLite supplies this live NUL-terminated callback argument.
        let name = unsafe { std::ffi::CStr::from_ptr(name) }.to_bytes();
        let denied = if DENY_PRAGMA.load(Ordering::SeqCst) == 1 {
            b"key".as_slice()
        } else {
            b"cipher_version".as_slice()
        };
        if name.eq_ignore_ascii_case(denied) {
            DENIED.fetch_add(1, Ordering::SeqCst);
            return rusqlite::ffi::SQLITE_DENY;
        }
    }
    rusqlite::ffi::SQLITE_OK
}
unsafe extern "C" fn deny_on_open(
    db: *mut rusqlite::ffi::sqlite3,
    _: *mut *mut std::ffi::c_char,
    _: *const rusqlite::ffi::sqlite3_api_routines,
) -> i32 {
    // SAFETY: the exact-test child owns this new connection; callback state is static.
    unsafe { rusqlite::ffi::sqlite3_set_authorizer(db, Some(deny_pragma), std::ptr::null_mut()) }
}

#[test]
fn real_keyed_probe_propagates_both_pragma_failures() {
    let db = crate::hostdb::tests::connection_for_test(":memory:", rusqlite::OpenFlags::default())
        .unwrap();
    assert!(!keyed_cipher_version(&db).unwrap().is_empty());
    // SAFETY: this connection is exclusively owned; callback/state outlive it and capture no data.
    assert_eq!(
        unsafe {
            rusqlite::ffi::sqlite3_set_authorizer(
                db.handle(),
                Some(deny_pragma),
                std::ptr::null_mut(),
            )
        },
        0
    );
    for mode in [1, 2] {
        DENY_PRAGMA.store(mode, Ordering::SeqCst);
        assert_eq!(
            keyed_cipher_version(&db),
            Err(Failure::Sql(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_AUTH).code
            ))
        );
    }
}

struct ReapedChild(std::process::Child);
impl Drop for ReapedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
fn real_ready_and_production_connect_are_composed_in_cold_children() {
    const ENV: &str = "ORG_TEST_HOST_SQLCIPHER_COMPOSITION";
    if let Ok(mode) = std::env::var(ENV) {
        assert!(
            READY.get().is_none(),
            "child must begin with a cold host fence"
        );
        if mode == "ready" {
            assert_eq!(ready(), Ok(()));
            assert_eq!(READY.get(), Some(&Ok(())));
            return;
        }
        if mode == "connect-key-refused" {
            ready().unwrap();
        }
        DENY_PRAGMA.store(
            if mode == "ready-version-refused" {
                2
            } else {
                1
            },
            Ordering::SeqCst,
        );
        // SAFETY: this exact-test child is the only SQLite user, and exits after the case.
        assert_eq!(
            unsafe { rusqlite::ffi::sqlite3_auto_extension(Some(deny_on_open)) },
            0
        );
        let expected = Error::Sql(rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_AUTH).code);
        if mode == "connect-key-refused" {
            let key = crate::host_secrets::Value::new(vec![0xab; 32]).unwrap();
            let actual = crate::hostdb::connect(std::path::Path::new(":memory:"), &key).err();
            assert_eq!(actual, Some(expected));
            let rendered = format!("{actual:?}");
            assert!(!rendered.contains(&"ab".repeat(32)));
            assert!(!rendered.contains("PRAGMA"));
        } else {
            assert_eq!(ready(), Err(expected));
            assert!(READY.get().unwrap().is_err());
            assert_eq!(DENIED.load(Ordering::SeqCst), 1);
            assert!(ready().is_err());
            assert_eq!(
                DENIED.load(Ordering::SeqCst),
                1,
                "failed probe must never retry"
            );
        }
        return;
    }
    for mode in [
        "ready",
        "ready-key-refused",
        "ready-version-refused",
        "connect-key-refused",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("stdout");
        let err = dir.path().join("stderr");
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "hostdb::sqlcipher::tests::real_ready_and_production_connect_are_composed_in_cold_children", "--nocapture"])
            .env(ENV, mode)
            .stdout(std::fs::File::create(&out).unwrap())
            .stderr(std::fs::File::create(&err).unwrap());
        let mut child = ReapedChild(command.spawn().unwrap());
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        let status = loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                break status;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "cold child timed out: {mode}"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(
            status.success(),
            "{mode}: {} {}",
            std::fs::read_to_string(out).unwrap(),
            std::fs::read_to_string(err).unwrap()
        );
    }
}
