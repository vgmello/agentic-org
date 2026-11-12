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
    let cold_cache = tempfile::tempdir().unwrap();
    let result = std::process::Command::new(env!("CARGO"))
        .env("CARGO_HOME", cold_cache.path())
        .current_dir(root)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--offline",
            "--no-deps",
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
    // This guards accidental co-linking through workspace path dependencies, including
    // dev/build/optional/target-specific edges and libraries. Registry crates cannot
    // depend on these unpublished workspace stores. Unknown local paths fail closed.
    let packages = metadata["packages"].as_array().unwrap();
    let package_index = |name| packages.iter().position(|p| p["name"] == name).unwrap();
    let host = package_index("org-office-host");
    let office = package_index("org-office");
    let edges: Vec<Vec<usize>> = packages
        .iter()
        .map(|package| {
            package["dependencies"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|dependency| dependency["path"].as_str())
                .map(|path| {
                    packages
                        .iter()
                        .position(|p| {
                            std::path::Path::new(p["manifest_path"].as_str().unwrap()).parent()
                                == Some(std::path::Path::new(path))
                        })
                        .expect("local dependency absent from workspace co-link inventory")
                })
                .collect()
        })
        .collect();
    for member in metadata["workspace_members"].as_array().unwrap() {
        let start = packages.iter().position(|p| p["id"] == *member).unwrap();
        let mut seen = std::collections::HashSet::new();
        let mut pending = vec![start];
        while let Some(index) = pending.pop() {
            if seen.insert(index) {
                pending.extend(&edges[index]);
            }
        }
        assert!(
            !(seen.contains(&host) && seen.contains(&office)),
            "{} links both SQLCipher fences: merge them before linking",
            packages[start]["name"]
        );
    }
}

#[test]
fn dependency_policy_covers_all_local_edges_and_library_targets() {
    let mut valid = serde_json::json!({
        "packages": [
            {"name":"org-office-host","id":"host","manifest_path":"/workspace/host/Cargo.toml","dependencies":[],"targets":[{"kind":["lib"]}]},
            {"name":"org-office","id":"office","manifest_path":"/workspace/office/Cargo.toml","dependencies":[],"targets":[{"kind":["lib"]}]},
            {"name":"bridge","id":"bridge","manifest_path":"/workspace/bridge/Cargo.toml","dependencies":[],"targets":[{"kind":["lib"]}]},
            {"name":"consumer","id":"consumer","manifest_path":"/workspace/consumer/Cargo.toml","dependencies":[],"targets":[{"kind":["bin"]}]}
        ],
        "workspace_members":["host","office","bridge","consumer"],
        "resolve":null
    });
    let edge = |path: &str, kind: Option<&str>| {
        serde_json::json!({
            "name":"renamed-dependency","path":format!("/workspace/{path}"),
            "kind":kind,"optional":true,"target":"cfg(target_os = \"linux\")"
        })
    };
    for store in ["host", "office"] {
        valid["packages"][3]["dependencies"] = serde_json::json!([edge(store, None)]);
        require_separate_fences(&valid);
    }
    for kind in [None, Some("dev"), Some("build")] {
        for target in ["bin", "lib", "cdylib", "staticlib", "proc-macro"] {
            let mut invalid = valid.clone();
            invalid["packages"][3]["targets"][0]["kind"] = serde_json::json!([target]);
            invalid["packages"][3]["dependencies"] =
                serde_json::json!([edge("host", kind), edge("bridge", kind)]);
            invalid["packages"][2]["dependencies"] = serde_json::json!([edge("office", kind)]);
            assert!(std::panic::catch_unwind(|| require_separate_fences(&invalid)).is_err());
        }
    }
    for (host, bridge) in [
        (vec![edge("office", None)], vec![]),
        (
            vec![edge("bridge", Some("dev"))],
            vec![edge("office", Some("build"))],
        ),
    ] {
        let mut invalid = valid.clone();
        invalid["packages"][0]["dependencies"] = serde_json::json!(host);
        invalid["packages"][2]["dependencies"] = serde_json::json!(bridge);
        assert!(std::panic::catch_unwind(|| require_separate_fences(&invalid)).is_err());
    }
    // A registry dependency has no local path and cannot link an unpublished store.
    valid["packages"][3]["dependencies"] = serde_json::json!([{"name":"serde","path":null}]);
    require_separate_fences(&valid);
    valid["packages"][3]["dependencies"] = serde_json::json!([edge("unlisted", None)]);
    assert!(std::panic::catch_unwind(|| require_separate_fences(&valid)).is_err());
}

#[test]
fn cipher_version_errors_distinguish_missing_cipher_from_sql_failures() {
    assert_eq!(
        cipher_version_failure(rusqlite::Error::QueryReturnedNoRows),
        Failure::NoCipher
    );
    let failure = rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_IOERR),
        Some("private key canary".into()),
    );
    assert_eq!(
        cipher_version_failure(failure),
        Failure::Sql(rusqlite::ErrorCode::SystemIoFailure)
    );
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
