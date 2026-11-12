use super::*;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Barrier;
use tempfile::TempDir;

#[test]
fn sqlcipher_once_body_runs_once_before_all_waiters_continue() {
    let state = OnceLock::new();
    let calls = AtomicUsize::new(0);
    let barrier = Barrier::new(16);
    std::thread::scope(|scope| {
        for _ in 0..16 {
            scope.spawn(|| {
                barrier.wait();
                initialized(&state, || {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
                .unwrap();
                assert_eq!(state.get(), Some(&Ok(())));
                assert_eq!(calls.load(Ordering::SeqCst), 1);
            });
        }
    });
}

#[test]
fn sqlcipher_once_failure_is_shared_and_never_retried() {
    let state = OnceLock::new();
    let calls = AtomicUsize::new(0);
    let barrier = Barrier::new(16);
    std::thread::scope(|scope| {
        for _ in 0..16 {
            scope.spawn(|| {
                barrier.wait();
                let error = initialized(&state, || {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Err("could not initialize SQLCipher (SQLite code 7)".into())
                })
                .unwrap_err();
                assert!(error
                    .source()
                    .unwrap()
                    .to_string()
                    .contains("SQLite code 7"));
            });
        }
    });
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(initialized(&state, || panic!("initialization failure was retried")).is_err());
}

#[test]
fn sqlcipher_initialization_refuses_errors_and_missing_cipher_before_admitting_openers() {
    for code in [rusqlite::ffi::SQLITE_ERROR, rusqlite::ffi::SQLITE_NOMEM] {
        let error = initialize_sqlcipher(
            || code,
            || panic!("probe must not run after initialization fails"),
        )
        .unwrap_err();
        assert_eq!(
            error,
            format!("could not initialize SQLCipher (SQLite code {code})")
        );
    }
    let initialized = std::cell::Cell::new(false);
    initialize_sqlcipher(
        || {
            initialized.set(true);
            rusqlite::ffi::SQLITE_OK
        },
        || {
            assert!(initialized.get());
            Ok("4.19.0".into())
        },
    )
    .unwrap();
    assert_eq!(
        initialize_sqlcipher(|| 0, || Ok(String::new())).unwrap_err(),
        "this SQLite build has no encryption (SQLCipher)"
    );
    assert_eq!(
        initialize_sqlcipher(|| 0, || Err("sanitized probe failure".into())).unwrap_err(),
        "sanitized probe failure"
    );
}

#[test]
fn sqlcipher_key_errors_keep_only_the_extended_code_and_never_key_text() {
    let secret = "ab".repeat(32);
    let sql = format!("PRAGMA key = \"x'{secret}'\";");
    for error in [
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_IOERR_READ),
            Some(sql.clone()),
        ),
        rusqlite::Error::SqlInputError {
            error: rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_IOERR_READ),
            msg: sql.clone(),
            sql: sql.clone(),
            offset: 0,
        },
        rusqlite::Error::InvalidParameterName(sql),
    ] {
        let has_code = !matches!(error, rusqlite::Error::InvalidParameterName(_));
        let error = key_failure(error);
        let rendered = format!("{error} {error:?}");
        assert!(!rendered.contains(&secret));
        assert!(!rendered.contains("PRAGMA"));
        assert_eq!(rendered.contains("SQLite code 266"), has_code);
        let mut cause = error.source();
        while let Some(error) = cause {
            assert!(!error.to_string().contains(&secret));
            assert!(!error.to_string().contains("PRAGMA"));
            cause = error.source();
        }
    }
}

#[test]
fn sqlcipher_openers_and_clippy_overrides_keep_the_fence() {
    let source = include_str!("../../store.rs");
    let initializer = &source[source.find("pub fn sqlcipher_ready()").unwrap()
        ..source.find("fn initialize_sqlcipher(").unwrap()];
    assert!(initializer.contains("static READY: OnceLock<Result<(), String>>"));
    assert!(
        initializer.find("initialized(&READY").unwrap()
            < initializer.find("Connection::open_in_memory").unwrap()
    );
    for (function, open) in [
        ("fn connect(", "Connection::open_with_flags"),
        ("fn connection_for_test(", "Connection::open("),
    ] {
        let body = &source[source.find(function).unwrap()..];
        assert!(body.find("sqlcipher_ready()?").unwrap() < body.find(open).unwrap());
    }
    assert_eq!(
        source
            .matches("#[allow(clippy::disallowed_methods)]")
            .count(),
        3
    );
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    assert!(
        root.join("clippy.toml").is_file(),
        "workspace Clippy fence is missing"
    );
    let mut directories = vec![root.to_owned()];
    let mut configs = 0;
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name();
            if entry.file_type().unwrap().is_dir() {
                if !matches!(
                    name.to_str(),
                    Some("target" | "node_modules" | ".git" | ".svelte-kit" | "dist" | "build")
                ) {
                    directories.push(entry.path());
                }
            } else if entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "rs")
            {
                let source = std::fs::read_to_string(entry.path()).unwrap();
                let compact: String = source.chars().filter(|c| !c.is_whitespace()).collect();
                // No unsafe SQLite opener is needed by the current Rust fence. Check
                // identifiers (including aliased imports), not only qualified calls.
                for (stem, suffix) in [("sqlite3", "_open"), ("from", "_handle")] {
                    let opener = format!("{stem}{suffix}");
                    assert!(
                        !compact.contains(&opener),
                        "{} bypasses the SQLite init fence with {opener}",
                        entry.path().display()
                    );
                }
            } else if name == "clippy.toml" || name == ".clippy.toml" {
                configs += 1;
                let config = std::fs::read_to_string(entry.path()).unwrap();
                for method in [
                    "open",
                    "open_with_flags",
                    "open_in_memory",
                    "open_in_memory_with_flags",
                    "open_with_flags_and_vfs",
                    "open_in_memory_with_flags_and_vfs",
                ] {
                    assert!(
                        config.contains(&format!("path = \"rusqlite::Connection::{method}\"")),
                        "{} drops the init fence",
                        entry.path().display()
                    );
                }
            }
        }
    }
    assert!(configs > 0);
}

#[test]
fn sqlcipher_upstream_init_gap_has_a_deterministic_barrier_probe() {
    // The real C initializer is paused after SQLite publishes isInit. Its completion is
    // the initializer closure of the same OnceLock helper used by sqlcipher_ready().
    let dir = TempDir::new().unwrap();
    let binary = dir.path().join("cold-init");
    let source = sqlcipher_src::source_dir();
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/store/tests/sqlcipher_cold_init.c");
    let mut command = Command::new(if cfg!(target_os = "macos") {
        "clang"
    } else {
        "cc"
    });
    command.args([
        "-O0",
        "-DSQLITE_THREADSAFE=1",
        "-DSQLITE_HAS_CODEC",
        "-DSQLITE_TEMP_STORE=2",
        "-DSQLITE_EXTRA_INIT=blocked_cipher_init",
        "-DSQLITE_EXTRA_SHUTDOWN=sqlcipher_extra_shutdown",
        "-DHAVE_STDINT_H=1",
    ]);
    command
        .arg("-I")
        .arg(source)
        .arg(source.join(sqlcipher_src::SOURCE_FILE))
        .arg(fixture);
    if cfg!(target_os = "macos") {
        command.args([
            "-DSQLCIPHER_CRYPTO_CC",
            "-framework",
            "Security",
            "-framework",
            "CoreFoundation",
        ]);
    } else {
        command.args(["-DSQLCIPHER_CRYPTO_OPENSSL", "-lcrypto", "-ldl", "-lm"]);
    }
    let output = command
        .arg("-lpthread")
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // Same controlled interleaving, first without the boundary, then through it.
    let mut bypass = InitProbe::start(&binary);
    assert_eq!(bypass.request(b'k'), "initialize=0 open=0 key=1 cipher=1");
    assert_eq!(bypass.request(b'i'), "extra_init=complete");
    assert_eq!(bypass.request(b'k'), "initialize=0 open=0 key=0 cipher=1");
    bypass.finish();

    let mut fenced = InitProbe::start(&binary);
    let state = OnceLock::new();
    initialized(&state, || {
        assert_eq!(fenced.request(b'i'), "extra_init=complete");
        Ok(())
    })
    .unwrap();
    assert_eq!(fenced.request(b'k'), "initialize=0 open=0 key=0 cipher=1");
    fenced.finish();
}

// The subprocess owns the instrumented SQLite; all protocol output is numeric/fixed.
struct InitProbe {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl InitProbe {
    fn start(binary: &Path) -> Self {
        let mut child = Command::new(binary)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut probe = Self {
            input: child.stdin.take().unwrap(),
            output: BufReader::new(child.stdout.take().unwrap()),
            child,
        };
        assert_eq!(probe.line(), "extra_init=blocked");
        probe
    }

    fn line(&mut self) -> String {
        let mut line = String::new();
        assert!(self.output.read_line(&mut line).unwrap() > 0);
        line.trim_end().to_owned()
    }

    fn request(&mut self, command: u8) -> String {
        self.input.write_all(&[command]).unwrap();
        self.input.flush().unwrap();
        self.line()
    }

    fn finish(&mut self) {
        self.input.write_all(b"q").unwrap();
        self.input.flush().unwrap();
        assert!(self.child.wait().unwrap().success());
    }
}

impl Drop for InitProbe {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn sqlcipher_cold_parallel_open_stress_guard() {
    const CHILD: &str = "ORG_TEST_SQLCIPHER_COLD_CHILD";
    const NAME: &str = "store::sqlcipher_init_tests::sqlcipher_cold_parallel_open_stress_guard";
    if std::env::var_os(CHILD).is_some() {
        // This is the child's first SQLite use: every opener is released together.
        let barrier = Barrier::new(16);
        let key = Key::from_db_key(org_proto::DbKey::from_bytes([5; 32]));
        std::thread::scope(|scope| {
            for _ in 0..16 {
                scope.spawn(|| {
                    barrier.wait();
                    let db = connect(Path::new(":memory:"), &key, true, true).unwrap();
                    db.execute_batch(
                        "CREATE TABLE probe(value TEXT); INSERT INTO probe VALUES ('ok');",
                    )
                    .unwrap();
                    assert_eq!(
                        db.query_row("SELECT value FROM probe", (), |r| r.get::<_, String>(0))
                            .unwrap(),
                        "ok"
                    );
                });
            }
        });
        return;
    }
    // A fixed-size stress experiment, not retry-until-green. Every child must succeed.
    for attempt in 0..50 {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", NAME, "--nocapture"])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "cold child {attempt}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
