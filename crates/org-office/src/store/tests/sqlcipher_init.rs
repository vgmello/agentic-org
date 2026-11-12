use super::*;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Barrier;
use std::time::{Duration, Instant};
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
    let source = rust_code(include_str!("../../store.rs"));
    let initializer = &source[source.find("pubfnsqlcipher_ready()").unwrap()
        ..source.find("fninitialize_sqlcipher(").unwrap()];
    assert!(source.contains("staticREADY:OnceLock<Result<(),String>>"));
    assert!(
        initializer.find("initialized(&READY").unwrap()
            < initializer.find("Connection::open_in_memory").unwrap()
    );
    for (function, open) in [
        ("fnconnect(", "Connection::open_with_flags"),
        ("fnconnection_for_test(", "Connection::open("),
    ] {
        let body = &source[source.find(function).unwrap()..];
        assert!(body.find("sqlcipher_ready()?").unwrap() < body.find(open).unwrap());
    }
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
                let compact = rust_code(&source);
                let relative = entry.path().strip_prefix(root).unwrap().to_owned();
                override_policy(&relative, &compact);
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
    let mut command = probe_compiler(
        std::env::var_os("CC"),
        std::env::var_os("OPENSSL_DIR"),
        std::env::var_os("OPENSSL_INCLUDE_DIR"),
        std::env::var_os("OPENSSL_LIB_DIR"),
    );
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
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", NAME, "--nocapture"])
            .env(CHILD, "1");
        bounded_child(&mut command, &format!("cold child {attempt}"));
    }
}

// Strip Rust comments and quoted literals before checking actual code identifiers. This
// handles nested block comments, raw strings and escaped strings without reading paths/SQL.
fn rust_code(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut code = String::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if bytes[i..].starts_with(b"/*") {
            i += 2;
            let mut depth = 1;
            while i < bytes.len() && depth > 0 {
                if bytes[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if bytes[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
        } else if bytes[i] == b'r' && {
            let mut quote = i + 1;
            while quote < bytes.len() && bytes[quote] == b'#' {
                quote += 1;
            }
            quote < bytes.len() && bytes[quote] == b'"'
        } {
            let start = i;
            i += 1;
            while bytes[i] == b'#' {
                i += 1;
            }
            let hashes = i - start - 1;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'"'
                    && bytes
                        .get(i + 1..i + 1 + hashes)
                        .is_some_and(|tail| tail.iter().all(|b| *b == b'#'))
                {
                    i += 1 + hashes;
                    break;
                }
                i += 1;
            }
        } else if bytes[i] == b'"' {
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i = (i + 2).min(bytes.len());
                } else if bytes[i] == b'"' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
        } else if bytes[i] == b'\''
            && source[i + 1..]
                .chars()
                .next()
                .is_some_and(|c| bytes.get(i + 1 + c.len_utf8()) == Some(&b'\''))
        {
            i += 2 + source[i + 1..].chars().next().unwrap().len_utf8();
        } else if bytes[i..].starts_with(b"'\\") {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\'' {
                i += 1;
            }
            i = (i + 1).min(bytes.len());
        } else {
            let c = source[i..].chars().next().unwrap();
            if !c.is_whitespace() {
                code.push(c);
            }
            i += c.len_utf8();
        }
    }
    code
}

fn override_policy(path: &Path, code: &str) {
    let lint = "disallowed_methods";
    if path == Path::new("crates/org-office/src/store.rs") {
        assert_eq!(
            code.matches(lint).count(),
            3,
            "store override identifier count"
        );
        assert_eq!(
            code.matches(&format!("#[allow(clippy::{lint})]")).count(),
            3,
            "store overrides must use the plain allow form"
        );
    } else {
        assert!(
            !(code.contains(lint)
                && (code.contains("rusqlite")
                    || code.contains("Connection")
                    || code.contains("#![allow(")
                    || code.contains("#![expect("))),
            "{} has an unapproved SQLCipher opener override",
            path.display()
        );
    }
}

#[test]
fn sqlcipher_override_policy_covers_other_files_and_comment_spoofs() {
    let lint = "disallowed_methods";
    for attribute in [
        format!("#[allow(clippy::{lint})]"),
        format!("#[allow(clippy::{lint}, dead_code)]"),
        format!("#[allow(clippy::{lint}, reason = \"fixture\")]"),
        format!("#[expect(clippy::{lint})]"),
        format!("#![allow(clippy::{lint})]"),
    ] {
        let code = rust_code(&format!(
            "{attribute} fn bypass() {{ let _ = rusqlite::Connection::open(\":memory:\"); }}"
        ));
        for file in [
            "crates/org-office/src/store/status.rs",
            "apps/org-office-host/src/lib.rs",
        ] {
            assert!(std::panic::catch_unwind(|| override_policy(Path::new(file), &code)).is_err());
        }
    }
    let plain = format!("#[allow(clippy::{lint})]");
    let source = format!("{plain}{plain}{plain}");
    override_policy(
        Path::new("crates/org-office/src/store.rs"),
        &rust_code(&source),
    );
    for bad in [
        source.replacen(&plain, &format!("#[expect(clippy::{lint})]"), 1),
        format!("//{plain}\n{plain}{plain}"),
    ] {
        assert!(std::panic::catch_unwind(|| override_policy(
            Path::new("crates/org-office/src/store.rs"),
            &rust_code(&bad)
        ))
        .is_err());
    }
    // The existing unrelated child-wait exception stays legal, and quoted/comment
    // lookalikes cannot satisfy a fence-order check or invent an override.
    override_policy(
        Path::new("crates/org-supervise/src/spawn.rs"),
        &rust_code(&plain),
    );
    assert_eq!(
        rust_code(
            "// sqlcipher_ready()?;\n/* a /* nested */ sqlcipher_ready()? */ Connection::open(p)"
        ),
        "Connection::open(p)"
    );
    assert_eq!(
        rust_code(r###"r#"// not code"# "escaped\"//not code" '\"' Connection::open(p)"###),
        "Connection::open(p)"
    );
    assert_eq!(
        rust_code("let lifetime: &'a str; // tail\n"),
        "letlifetime:&'astr;"
    );
}

// All child output goes to ordinary temporary files, so a full pipe cannot block exit.
// The absolute timeout is a deadlock watchdog, never race evidence. Drop reaps on panic.
struct BoundedChild(Child);
impl Drop for BoundedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn bounded_child(command: &mut Command, label: &str) {
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("stdout");
    let err = dir.path().join("stderr");
    command
        .stdout(std::fs::File::create(&out).unwrap())
        .stderr(std::fs::File::create(&err).unwrap());
    let mut child = BoundedChild(command.spawn().unwrap());
    let status = child_status(&mut child.0, Duration::from_secs(60)).expect(label);
    assert!(
        status.success(),
        "{label}: {} {}",
        std::fs::read_to_string(out).unwrap(),
        std::fs::read_to_string(err).unwrap()
    );
}

static DENY_PRAGMA: AtomicUsize = AtomicUsize::new(0);
static DENIED_KEYS: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" fn deny_probe_pragma(
    _: *mut std::ffi::c_void,
    action: i32,
    first: *const std::ffi::c_char,
    _: *const std::ffi::c_char,
    _: *const std::ffi::c_char,
    _: *const std::ffi::c_char,
) -> i32 {
    if action == rusqlite::ffi::SQLITE_PRAGMA && !first.is_null() {
        // SAFETY: SQLite supplies a valid NUL-terminated argument for this callback.
        let name = unsafe { std::ffi::CStr::from_ptr(first) }.to_bytes();
        let denied = if DENY_PRAGMA.load(Ordering::SeqCst) == 1 {
            b"key".as_slice()
        } else {
            b"cipher_version".as_slice()
        };
        if name.eq_ignore_ascii_case(denied) {
            DENIED_KEYS.fetch_add(1, Ordering::SeqCst);
            return rusqlite::ffi::SQLITE_DENY;
        }
    }
    rusqlite::ffi::SQLITE_OK
}
unsafe extern "C" fn deny_probe_on_open(
    db: *mut rusqlite::ffi::sqlite3,
    _: *mut *mut std::ffi::c_char,
    _: *const rusqlite::ffi::sqlite3_api_routines,
) -> i32 {
    // SAFETY: SQLite passes the live new connection; callback/context outlive the child.
    unsafe {
        rusqlite::ffi::sqlite3_set_authorizer(db, Some(deny_probe_pragma), std::ptr::null_mut())
    }
}

#[test]
fn sqlcipher_real_keyed_probe_observes_success_and_refusal() {
    let db = connection_for_test(":memory:").unwrap();
    let version = keyed_cipher_version(&db).unwrap();
    assert!(!version.is_empty());
    assert_eq!(
        version,
        db.query_row("PRAGMA cipher_version", (), |r| r.get::<_, String>(0))
            .unwrap()
    );
    // No global extension here: only this privately owned connection is affected.
    DENY_PRAGMA.store(1, Ordering::SeqCst);
    // SAFETY: db is live and exclusively borrowed here; the static callback captures no data.
    assert_eq!(
        unsafe {
            rusqlite::ffi::sqlite3_set_authorizer(
                db.handle(),
                Some(deny_probe_pragma),
                std::ptr::null_mut(),
            )
        },
        0
    );
    let error = keyed_cipher_version(&db).unwrap_err();
    assert!(error.contains("SQLite code 23"));
    DENY_PRAGMA.store(2, Ordering::SeqCst);
    let error = keyed_cipher_version(&db).unwrap_err();
    assert!(error.contains("SQLite code 23"));
}

#[test]
fn sqlcipher_real_ready_and_connect_composition_in_cold_children() {
    const ENV: &str = "ORG_TEST_SQLCIPHER_COMPOSITION";
    if let Ok(mode) = std::env::var(ENV) {
        if mode == "hang" {
            loop {
                std::thread::park();
            }
        }
        assert!(READY.get().is_none(), "composition test must start cold");
        if mode == "ready" {
            sqlcipher_ready().unwrap();
            assert_eq!(READY.get(), Some(&Ok(())));
            return;
        }
        if mode == "connect-key-refused" {
            sqlcipher_ready().unwrap();
        }
        DENY_PRAGMA.store(
            if mode == "ready-version-refused" {
                2
            } else {
                1
            },
            Ordering::SeqCst,
        );
        // SAFETY: this exact-test child has one thread touching SQLite. The callback
        // installs a per-connection authorizer; the process exits after this test.
        assert_eq!(
            unsafe { rusqlite::ffi::sqlite3_auto_extension(Some(deny_probe_on_open)) },
            0
        );
        if mode == "connect-key-refused" {
            let key = Key::from_db_key(org_proto::DbKey::from_bytes([0xab; 32]));
            let error = connect(Path::new(":memory:"), &key, true, true).unwrap_err();
            let text = format!("{error:?}");
            assert!(text.contains("SQLite code 23"));
            assert!(!text.contains(&"ab".repeat(32)));
            assert!(!text.contains("PRAGMA"));
        } else {
            let error = sqlcipher_ready().unwrap_err();
            assert!(error
                .source()
                .unwrap()
                .to_string()
                .contains("SQLite code 23"));
            assert!(READY.get().unwrap().is_err());
            assert_eq!(DENIED_KEYS.load(Ordering::SeqCst), 1);
            assert!(sqlcipher_ready().is_err());
            assert_eq!(DENIED_KEYS.load(Ordering::SeqCst), 1, "failure was retried");
        }
        return;
    }
    for mode in [
        "ready",
        "ready-key-refused",
        "ready-version-refused",
        "connect-key-refused",
    ] {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "store::sqlcipher_init_tests::sqlcipher_real_ready_and_connect_composition_in_cold_children", "--nocapture"]).env(ENV, mode);
        bounded_child(&mut command, mode);
    }
}

fn child_status(child: &mut Child, timeout: Duration) -> std::io::Result<std::process::ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "child deadline elapsed",
            ));
        }
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        std::thread::sleep(remaining.min(Duration::from_millis(10)));
    }
}

#[test]
fn sqlcipher_child_watchdog_times_out_and_can_reap_a_blocked_child() {
    let mut child = BoundedChild(Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "store::sqlcipher_init_tests::sqlcipher_real_ready_and_connect_composition_in_cold_children"])
        .env("ORG_TEST_SQLCIPHER_COMPOSITION", "hang")
        .stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap());
    assert_eq!(
        child_status(&mut child.0, Duration::ZERO)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::TimedOut
    );
    child.0.kill().unwrap();
    assert!(!child.0.wait().unwrap().success());
}

#[test]
fn sqlcipher_probe_query_errors_distinguish_missing_cipher_from_sqlite_failure() {
    assert_eq!(
        cipher_failure_reason(rusqlite::Error::QueryReturnedNoRows),
        "this SQLite build has no encryption (SQLCipher)"
    );
    let reason = cipher_failure_reason(rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_NOMEM),
        Some("private query text".into()),
    ));
    assert_eq!(reason, "could not apply the database key (SQLite code 7)");
}

// CC is an executable (including a path with spaces); no shell interpolation. Native
// OpenSSL builds honor the documented directory overrides with explicit ones taking precedence.
fn probe_compiler(
    cc: Option<std::ffi::OsString>,
    openssl: Option<std::ffi::OsString>,
    include: Option<std::ffi::OsString>,
    lib: Option<std::ffi::OsString>,
) -> Command {
    let mut command = Command::new(cc.unwrap_or_else(|| {
        if cfg!(target_os = "macos") {
            "clang".into()
        } else {
            "cc".into()
        }
    }));
    if !cfg!(target_os = "macos") {
        let base = openssl.map(std::path::PathBuf::from);
        if let Some(path) = include
            .map(std::path::PathBuf::from)
            .or_else(|| base.as_ref().map(|p| p.join("include")))
        {
            command.arg("-I").arg(path);
        }
        if let Some(path) = lib
            .map(std::path::PathBuf::from)
            .or_else(|| base.as_ref().map(|p| p.join("lib")))
        {
            command.arg("-L").arg(path);
        }
    }
    command
}

#[test]
fn sqlcipher_probe_compiler_honors_explicit_paths_without_shell_parsing() {
    let command = probe_compiler(
        Some("/compiler with spaces/cc".into()),
        Some("/ssl".into()),
        Some("/explicit include".into()),
        Some("/explicit lib".into()),
    );
    assert_eq!(command.get_program(), "/compiler with spaces/cc");
    let args: Vec<_> = command.get_args().collect();
    if cfg!(target_os = "macos") {
        assert!(args.is_empty());
    } else {
        assert_eq!(args, ["-I", "/explicit include", "-L", "/explicit lib"]);
        let fallback = probe_compiler(None, Some("/ssl".into()), None, None);
        assert_eq!(
            fallback.get_args().collect::<Vec<_>>(),
            ["-I", "/ssl/include", "-L", "/ssl/lib"]
        );
    }
}
