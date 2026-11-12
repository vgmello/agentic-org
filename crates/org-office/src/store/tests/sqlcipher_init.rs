#[path = "opener_inventory.rs"]
mod opener_inventory;
#[path = "opener_policy.rs"]
mod opener_policy;
#[path = "opener_scope.rs"]
mod opener_scope;
#[path = "opener_sources.rs"]
mod opener_sources;

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
    let raw = include_str!("../../store.rs");
    let source = rust_code(raw);
    let initializer = rust_code(opener_policy::function(raw, "sqlcipher_ready").body);
    assert!(source.contains("staticREADY:OnceLock<Result<(),String>>"));
    assert!(
        initializer.find("initialized(&READY").unwrap()
            < initializer.find("Connection::open_in_memory").unwrap()
    );
    for (function, open) in [
        ("fn connect(", "Connection::open_with_flags"),
        ("fn connection_for_test(", "Connection::open("),
    ] {
        let name = function
            .strip_prefix("fn ")
            .unwrap()
            .strip_suffix('(')
            .unwrap();
        let body = rust_code(opener_policy::function(raw, name).body);
        assert!(body.find("sqlcipher_ready()?").unwrap() < body.find(open).unwrap());
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    check_source_inventory(root);
}

fn check_source_inventory(root: &Path) {
    let scope = opener_inventory::tracked(root);
    check_tracked_sources(&scope);
    opener_sources::cargo_roots(&scope);
}

fn check_tracked_sources(scope: &opener_scope::SourceScope) {
    for path in scope.files.keys() {
        if path.extension().is_some_and(|extension| extension == "rs") {
            check_rust_file(&scope.root, path, &std::fs::read_to_string(path).unwrap());
        }
    }
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

// Strip comments, quoted literals and whitespace, retaining the fn keyword boundary.
// Nested comments, raw strings and escaped strings cannot masquerade as actual code.
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
        } else if bytes[i..].starts_with(b"fn")
            && (i == 0
                || source[..i]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_whitespace() || "{}[]();,:=!<>+-*/&|".contains(c)))
            && (source[i + 2..]
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
                || bytes[i + 2..].starts_with(b"/*")
                || bytes[i + 2..].starts_with(b"//"))
        {
            // Preserve this keyword boundary: `fn wrongfnconnect` must not become
            // indistinguishable from the declaration `fn connect` or a call named fnconnect.
            code.push_str("fn ");
            i += 2;
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

fn check_rust_file(root: &Path, path: &Path, source: &str) {
    assert!(
        std::panic::catch_unwind(|| {
            opener_policy::modules(root, path, source);
            let compact = rust_code(source);
            let relative = path.strip_prefix(root).unwrap().to_owned();
            override_policy(&relative, source);
            // No unsafe SQLite opener is needed by the current Rust fence. Check
            // identifiers (including aliased imports), not only qualified calls.
            for (stem, suffix) in [("sqlite3", "_open"), ("from", "_handle")] {
                let opener = format!("{stem}{suffix}");
                assert!(
                    !compact.contains(&opener),
                    "{} bypasses the SQLite init fence with {opener}",
                    path.display()
                );
            }
        })
        .is_ok(),
        "{} has unsupported or weakening Rust source",
        path.display()
    );
}

fn skip_policy_directory(relative: &Path) -> bool {
    // Generated/dependency trees are irrelevant, but a source module may legally
    // be named build, dist or target. Never omit a directory beneath src.
    !relative.components().any(|part| part.as_os_str() == "src")
        && matches!(
            relative.file_name().and_then(|name| name.to_str()),
            Some("target" | "node_modules" | ".git" | ".svelte-kit" | "dist" | "build")
        )
}

fn opener_policy_config(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "Cargo.toml")
        || (path
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == ".cargo")
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("config")))
}

fn check_clippy_config(path: &Path, config: &str) {
    assert!(
        std::panic::catch_unwind(|| opener_policy::clippy(config)).is_ok(),
        "{} has unsupported or weakening Clippy configuration",
        path.display()
    );
}

fn check_opener_config(path: &Path, config: &str) {
    assert!(
        std::panic::catch_unwind(|| {
            if path.file_name().is_some_and(|name| name == "Cargo.toml") {
                opener_policy::config(config);
            } else {
                opener_policy::cargo_config(config);
            }
        })
        .is_ok(),
        "{} has unsupported or weakening SQLCipher opener configuration",
        path.display()
    );
}

#[test]
fn sqlcipher_policy_scans_source_directories_and_cargo_configuration() {
    for name in [
        "target",
        "node_modules",
        ".git",
        ".svelte-kit",
        "dist",
        "build",
    ] {
        assert!(skip_policy_directory(Path::new(name)));
        assert!(skip_policy_directory(&Path::new("apps/org-ui").join(name)));
        assert!(!skip_policy_directory(
            &Path::new("crates/org-office/src").join(name)
        ));
    }
    assert!(!skip_policy_directory(Path::new("crates/org-office/tests")));
    for path in [
        "Cargo.toml",
        "crates/org-office/Cargo.toml",
        ".cargo/config",
        ".cargo/config.toml",
        "crates/org-office/.cargo/config.toml",
    ] {
        let path = Path::new(path);
        assert!(opener_policy_config(path));
        check_opener_config(path, "[build]\nrustflags = ['-Dclippy::all']");
        for lint in ["disallowed_method", "disallowed_methods"] {
            for config in [
                format!("[lints.clippy]\n{lint} = \"allow\""),
                format!("[build]\nrustflags = [\"-A\", \"clippy::{lint}\"]"),
            ] {
                assert!(std::panic::catch_unwind(|| check_opener_config(path, &config)).is_err());
            }
        }
    }
    for path in [
        "clippy.toml",
        ".cargo/README.md",
        "config.toml",
        "apps/org-ui/package.json",
    ] {
        assert!(!opener_policy_config(Path::new(path)));
    }
}

#[test]
fn sqlcipher_override_policy_rejects_renamed_lint_and_preserves_exact_exceptions() {
    let plain = "#[allow(clippy::disallowed_methods)]";
    let approved = office_policy_fixture(plain);
    let store = Path::new("crates/org-office/src/store.rs");
    override_policy(store, &approved);
    for lint in ["disallowed_method", "disallowed_methods"] {
        for attribute in [
            format!("#[allow(clippy::{lint})]"),
            format!("#[allow(renamed_and_removed_lints,clippy::{lint})]"),
        ] {
            let rogue = rust_code(&format!(
                "{attribute} pub fn rogue(){{ let _=rusqlite::Connection::open(\"x\"); }}"
            ));
            for path in ["crates/org-office/src/lib.rs", "other.rs"] {
                assert!(
                    std::panic::catch_unwind(|| override_policy(Path::new(path), &rogue)).is_err()
                );
            }
            assert!(std::panic::catch_unwind(|| override_policy(
                store,
                &format!("{approved}{rogue}")
            ))
            .is_err());
        }
    }
}

fn override_policy(path: &Path, source: &str) {
    let compact = rust_code(source);
    let code = compact.as_str();
    reject_lint_groups(path, code);
    let lint = "disallowed_method";
    if path == Path::new("crates/org-office/src/store.rs") {
        assert_eq!(
            code.matches(lint).count(),
            3,
            "store override identifier count"
        );
        assert_eq!(
            code.matches(&format!("#[allow(clippy::{lint}s)]")).count(),
            3,
            "store overrides must use the plain allow form"
        );
        let plain = format!("#[allow(clippy::{lint}s)]");
        for (function, statement) in office_opener_statements() {
            require_exception_statement(source, function, &plain, statement);
        }
        assert!(code.contains("#[cfg(test)]fn connection_for_test("));
    } else if path == Path::new("crates/org-supervise/src/lib.rs") {
        assert_eq!(code.matches(lint).count(), 1);
        assert!(code.contains(&format!("#![deny(clippy::{lint}s)]")));
    } else if path == Path::new("crates/org-supervise/src/spawn.rs") {
        // Preserve the existing test-only Command::status exception, not a SQL opener.
        assert_eq!(code.matches(lint).count(), 1);
        require_exception_statement(source,
            "fn every_parent_pipe_is_cloexec_and_stays_out_of_an_unrelated_exec(",
            &format!("#[expect(clippy::{lint}s,reason=)]"),
            "letstatus=std::process::Command::new().env_clear().args([,,,]).args(descriptors.map(|fd|fd.to_string())).status().unwrap();");
        assert!(code.contains(
            "#[test]fn every_parent_pipe_is_cloexec_and_stays_out_of_an_unrelated_exec("
        ));
        assert!(!code.contains("rusqlite") && !code.contains("Connection"));
    } else if let Some((_, function, statement)) = host_opener_statements()
        .into_iter()
        .find(|(file, _, _)| path == Path::new(file))
    {
        assert_eq!(
            code.matches(lint).count(),
            1,
            "host override identifier count"
        );
        require_exception_statement(
            source,
            function,
            &format!("#[allow(clippy::{lint}s)]"),
            statement,
        );
        if function == "pub(super)fn connection_for_test(" {
            assert!(code.contains("#[cfg(test)]pub(super)fn connection_for_test("));
        }
    } else {
        assert!(
            !code.contains(lint),
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
    let source = office_policy_fixture(&plain);
    // This fixture already uses the canonical token form.
    override_policy(Path::new("crates/org-office/src/store.rs"), &source);
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
    // The existing unrelated child-exec exception stays legal, and quoted/comment
    // lookalikes cannot satisfy a fence-order check or invent an override.
    let child = format!("#[test] fn every_parent_pipe_is_cloexec_and_stays_out_of_an_unrelated_exec() {{ #[expect(clippy::{lint}, reason = \"test-only exec\")] let status = std::process::Command::new(\"/bin/sh\").env_clear().args([\"-c\",\"payload\",\"check\",]).args(descriptors.map(|fd|fd.to_string())).status().unwrap(); }}");
    override_policy(Path::new("crates/org-supervise/src/spawn.rs"), &child);
    override_policy(
        Path::new("crates/org-supervise/src/lib.rs"),
        &rust_code(&format!("#![deny(clippy::{lint})]")),
    );
    for (path, approved) in [
        ("crates/org-office/src/store.rs", source.clone()),
        (
            "crates/org-supervise/src/lib.rs",
            format!("#![deny(clippy::{lint})]"),
        ),
        ("crates/org-supervise/src/spawn.rs", child.clone()),
    ] {
        let extra = format!("{approved}#[allow(clippy::disallowed_method)]fn extra(){{}}");
        assert!(std::panic::catch_unwind(|| override_policy(Path::new(path), &extra)).is_err());
    }
    for (path, source) in [
        (
            "crates/org-office/src/lib.rs",
            format!("{plain} mod store;"),
        ),
        ("other.rs", child.clone()),
        ("crates/org-supervise/src/lib.rs", plain.clone()),
        ("crates/org-supervise/src/spawn.rs", plain.clone()),
        (
            "crates/org-supervise/src/spawn.rs",
            format!("{child} fn rusqlite() {{}}"),
        ),
        (
            "crates/org-supervise/src/spawn.rs",
            format!("{child} fn Connection() {{}}"),
        ),
        (
            "crates/org-supervise/src/spawn.rs",
            format!("{child} {plain}"),
        ),
        (
            "crates/org-supervise/src/spawn.rs",
            format!("{child} fn bypass() {{ rusqlite::Connection::open(); }}"),
        ),
    ] {
        assert!(std::panic::catch_unwind(|| override_policy(Path::new(path), &source)).is_err());
    }
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
    bounded_output(command, label);
}

// Capture is bounded in time, not output size.
fn bounded_output(command: &mut Command, label: &str) -> String {
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
    std::fs::read_to_string(out).unwrap()
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

// Fixed statements make the exception scope reviewable: an attribute on a function,
// module or a larger initializer cannot silently authorize additional raw opens.
fn office_opener_statements() -> [(&'static str, &'static str); 3] {
    [
        (
            "sqlcipher_ready",
            "letdb=Connection::open_in_memory().map_err(key_failure_reason)?;",
        ),
        (
            "connect",
            "letdb=Connection::open_with_flags(db_path,flags).map_err(OfficeError::storage)?;",
        ),
        (
            "connection_for_test",
            "letdb=Connection::open(path).map_err(OfficeError::storage)?;",
        ),
    ]
}
fn require_exception_statement(source: &str, function: &str, attribute: &str, statement: &str) {
    let name = function
        .strip_prefix("pub(super)fn ")
        .or_else(|| function.strip_prefix("pubfn "))
        .or_else(|| function.strip_prefix("fn "))
        .unwrap_or(function)
        .trim_end_matches('(');
    let function = opener_policy::function(source, name);
    assert_eq!(
        rust_code(function.body).matches(attribute).count(),
        1,
        "exception escaped its approved function"
    );
    assert_eq!(
        function
            .statements
            .iter()
            .filter(|candidate| rust_code(candidate) == format!("{attribute}{statement}"))
            .count(),
        1,
        "exception escaped its approved statement in {name}"
    );
}

fn office_policy_fixture(attribute: &str) -> String {
    office_opener_statements()
        .map(|(function, statement)| {
            let prefix = if function == "connection_for_test" {
                "#[cfg(test)]"
            } else {
                ""
            };
            format!("{prefix}fn {function}(){{{attribute}{statement}}}")
        })
        .join("")
}

fn reject_lint_groups(path: &Path, code: &str) {
    // Only inspect attributes, not function calls named allow/expect. cfg_attr's
    // nested allow/expect/warn lists are covered too. Quoted reasons were removed above.
    for (start, _) in code.match_indices('#') {
        let tail = &code[start + 1..];
        let Some(attribute) = tail.strip_prefix('[').or_else(|| tail.strip_prefix("![")) else {
            continue;
        };
        let attribute = attribute.split(']').next().unwrap();
        for keyword in ["allow(", "expect(", "warn("] {
            for parameters in attribute.split(keyword).skip(1) {
                for lint in parameters.split(')').next().unwrap().split(',') {
                    assert!(
                        !opener_policy::lint_group(lint),
                        "{} has a broad lint override that can hide an opener",
                        path.display()
                    );
                }
            }
        }
    }
}

#[test]
fn sqlcipher_policy_rejects_effective_lint_groups_and_widened_scopes() {
    let file = Path::new("crates/org-office/src/lib.rs");
    for group in [
        "warnings",
        "r#warnings",
        "renamed_and_removed_lints",
        "r#renamed_and_removed_lints",
        "clippy",
        "clippy::all",
        "clippy::restriction",
        "clippy::r#restriction",
        "clippy::pedantic",
        "clippy::nursery",
        "clippy::cargo",
        "clippy::style",
        "clippy::correctness",
        "clippy::suspicious",
        "clippy::complexity",
        "clippy::perf",
    ] {
        for source in [
            format!("#![allow({group})] mod store;"),
            format!("#![warn({group})] mod store;"),
            format!("#[expect(dead_code,{group},reason=\"test\")] mod store;"),
            format!("#[cfg_attr(test,allow(dead_code,{group}))] mod store;"),
        ] {
            assert!(std::panic::catch_unwind(|| override_policy(file, &source)).is_err());
        }
    }
    override_policy(
        file,
        &rust_code("#[allow(dead_code,clippy::too_many_arguments,reason=\"warnings\")] fn x(){}"),
    );
    override_policy(
        file,
        &rust_code("#![deny(warnings,renamed_and_removed_lints)]"),
    );
    let lint = "disallowed_methods";
    let plain = format!("#[allow(clippy::{lint})]");
    let code = office_policy_fixture(&plain);
    override_policy(Path::new("crates/org-office/src/store.rs"), &code);
    for (function, statement) in office_opener_statements() {
        let moved = code
            .replacen(&format!("{plain}{statement}"), statement, 1)
            .replacen(
                &format!("fn {function}("),
                &format!("{plain}fn {function}("),
                1,
            );
        assert!(std::panic::catch_unwind(|| override_policy(
            Path::new("crates/org-office/src/store.rs"),
            &moved
        ))
        .is_err());
    }
    let missing_cfg = code.replace("#[cfg(test)]", "");
    assert!(std::panic::catch_unwind(|| override_policy(
        Path::new("crates/org-office/src/store.rs"),
        &missing_cfg
    ))
    .is_err());
    // A matching attribute on a surrounding module does not bind it to the command.
    let broad =
        format!("#[expect(clippy::{lint},reason=)] mod wide {{ fn call(){{ Conn::open(); }} }}");
    assert!(std::panic::catch_unwind(|| override_policy(
        Path::new("crates/org-supervise/src/spawn.rs"),
        &broad
    ))
    .is_err());
}

#[test]
fn sqlcipher_exception_statement_requires_one_attribute_on_the_named_statement() {
    let attr = "#[allow(clippy::disallowed_methods)]";
    let code = format!("fn allowed(){{{attr}letx=op();}}");
    require_exception_statement(&code, "fn allowed(", attr, "letx=op();");
    for code in [
        format!("fn allowed(){{{attr}letx=op();{attr}lety=op();}}"),
        format!("fn allowed(){{{attr}letx={{extra();op()}};}}"),
        format!("fn allowed(){{{attr}letx=other();}}"),
        format!("{attr}fn allowed(){{letx=op();}}"),
        format!("fn other(){{{attr}letx=op();}}"),
    ] {
        assert!(std::panic::catch_unwind(|| require_exception_statement(
            &code,
            "fn allowed(",
            attr,
            "letx=op();"
        ))
        .is_err());
    }
}

#[test]
fn sqlcipher_function_matcher_requires_the_declaration_name_token() {
    let attr = "#[allow(clippy::disallowed_methods)]";
    for name in [
        "wrongfnconnect",
        "_fnconnect",
        "fnconnect",
        "éfnconnect",
        "e\u{301}fnconnect",
        "r#fnconnect",
    ] {
        let source = format!("fn {name}(){{{attr}let x=op();}}");
        assert!(
            std::panic::catch_unwind(|| require_exception_statement(
                &source,
                "fn connect(",
                attr,
                "letx=op();"
            ))
            .is_err(),
            "accepted function {name}"
        );
        let source = format!("fn {name}(){{}} fn connect(){{{attr}let x=op();}}");
        require_exception_statement(&source, "fn connect(", attr, "letx=op();");
    }
    for prefix in ["wrongfn", "_fn", "r#fn", "éfn", "e\u{301}fn"] {
        let source = format!("{prefix} connect(){{{attr}let x=op();}}");
        assert!(
            !rust_code(&source).contains("fn connect("),
            "accepted non-keyword {prefix}"
        );
        let source = format!("{prefix} connect(){{}} fn connect(){{{attr}let x=op();}}");
        assert_eq!(rust_code(&source).matches("fn connect(").count(), 1);
    }
    for declaration in [
        "fn connect",
        "fn/**/connect",
        "pub fn connect",
        "pub/**/fn/**/connect",
    ] {
        let source = format!("{declaration}(){{{attr}let x=op();}}");
        require_exception_statement(&source, "fn connect(", attr, "letx=op();");
    }
}
// Each process-private host opener has one statement-scoped exception.
fn host_opener_statements() -> [(&'static str, &'static str, &'static str); 3] {
    [
        ("apps/org-office-host/src/hostdb.rs", "fn connect(",
         "letdb=Connection::open_with_flags(path,OpenFlags::SQLITE_OPEN_READ_WRITE|OpenFlags::SQLITE_OPEN_NOFOLLOW,)?;"),
        ("apps/org-office-host/src/hostdb/sqlcipher.rs", "pub(super)fn ready(",
         "letdb=Connection::open_in_memory()?;"),
        ("apps/org-office-host/src/hostdb/tests.rs", "pub(super)fn connection_for_test(",
         "letdb=Connection::open_with_flags(path,flags).map_err(Error::from)?;"),
    ]
}

#[test]
fn host_override_inventory_accepts_only_three_exact_statements() {
    let lint = "disallowed_methods";
    let plain = format!("#[allow(clippy::{lint})]");
    for (file, function, statement) in host_opener_statements() {
        let prefix = if function == "pub(super)fn connection_for_test(" {
            "#[cfg(test)]"
        } else {
            ""
        };
        let accepted = format!("{prefix}{function}){{{plain}{statement}}}");
        override_policy(Path::new(file), &accepted);
        let wrong_function = function.replace("fn ", "fn wrongfn");
        let wrong_name = accepted.replace(function, &wrong_function);
        assert!(
            std::panic::catch_unwind(|| override_policy(Path::new(file), &wrong_name)).is_err()
        );
        override_policy(Path::new(file), &format!("{wrong_function}){{}}{accepted}"));
        let wrong_path = format!(
            "unapproved/{}",
            Path::new(file).file_name().unwrap().to_str().unwrap()
        );
        assert!(
            std::panic::catch_unwind(|| override_policy(Path::new(&wrong_path), &accepted))
                .is_err()
        );
        for bad in [
            format!("{accepted}{plain}fn extra(){{}}"),
            format!("{accepted}#[expect(clippy::{lint})]fn extra(){{}}"),
            format!("{accepted}#[allow(clippy::disallowed_method)]fn extra(){{}}"),
            accepted.replace(&plain, "#[allow(clippy::disallowed_method)]"),
            accepted.replace(
                &plain,
                "#[allow(renamed_and_removed_lints,clippy::disallowed_method)]",
            ),
            accepted.replace(&plain, ""),
            accepted.replace(&plain, &format!("{plain}{plain}")),
            accepted.replace(&plain, &format!("#[expect(clippy::{lint})]")),
            accepted.replace(&plain, &format!("#[allow(clippy::{lint},dead_code)]")),
            format!("{plain}{prefix}{function}){{{statement}}}"),
            format!("{plain}mod bypass{{{prefix}{function}){{{statement}}}}}"),
            format!("{prefix}fn wrong(){{{plain}{statement}}}"),
            accepted.replace(statement, "letdb={extra();Connection::open_in_memory()?;};"),
        ] {
            assert!(
                std::panic::catch_unwind(|| override_policy(Path::new(file), &bad)).is_err(),
                "{file}: {bad}"
            );
        }
        if !prefix.is_empty() {
            assert!(std::panic::catch_unwind(|| override_policy(
                Path::new(file),
                &accepted.replace(prefix, "")
            ))
            .is_err());
        }
    }
}

#[test]
fn sqlcipher_cargo_configuration_refuses_unknown_keys_and_tables() {
    let path = Path::new(".cargo/config.toml");
    let mut accepted = Vec::new();
    for source in [
        "include = ['lints.toml']",
        "[[include]]\npath = 'lints.toml'",
        "[unknown_table]",
        "[build]\nunknown_key = 'value'",
        "[build]\nrustc-wrapper = 'redirect'",
        "[build]\nrustc-workspace-wrapper = 'redirect'",
        "[target.x86_64-unknown-linux-gnu]\nlinker = 'redirect'",
        "[alias]\ncheck = 'clippy -- --cap-lints=allow'",
        "[env.OTHER.unknown_table]",
        "[env]\nOTHER.unknown_key = 'value'",
    ] {
        if std::panic::catch_unwind(|| check_opener_config(path, source)).is_ok() {
            accepted.push(source);
        }
    }
    assert!(
        accepted.is_empty(),
        "accepted unknown Cargo configuration: {accepted:?}"
    );
}

#[test]
fn sqlcipher_cargo_configuration_preserves_known_keys_and_manifest_metadata() {
    for source in [
        "[build]\nrustflags = ['-Dclippy::all']",
        "target.'cfg(unix)'.rustflags = '-D clippy::all'",
        "target.'cfg(target_os = \"linux\")'.rustflags = '-D clippy::all'",
        "[target.x86_64-unknown-linux-gnu]\nrustflags = '-D clippy::all'",
        "[env]",
    ] {
        check_opener_config(Path::new(".cargo/config.toml"), source);
    }
    check_opener_config(
        Path::new("Cargo.toml"),
        "[package.metadata]\nnote = 'application-specific manifest metadata'",
    );
    for name in [
        "CLIPPY_ARGS",
        "CLIPPY_CONF_DIR",
        "CLIPPY_FUTURE",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "OTHER",
    ] {
        for source in [
            format!("[env]\n{name} = 'redirect'"),
            format!("[env.{name}]\nvalue = 'redirect'"),
            format!("[env]\n{name} = {{ value = 'redirect', force = true }}"),
        ] {
            assert!(
                std::panic::catch_unwind(|| check_opener_config(
                    Path::new(".cargo/config.toml"),
                    &source
                ))
                .is_err(),
                "accepted {source}"
            );
        }
    }
}

#[test]
fn sqlcipher_parent_adapters_pair_bare_and_prefixed_names_and_config_classes() {
    let attr = "#[allow(clippy::disallowed_methods)]";
    for name in [
        "allowed",
        "fn allowed(",
        "pubfn allowed(",
        "pub(super)fn allowed(",
    ] {
        require_exception_statement(
            &format!("fn allowed(){{{attr}let x=op();}}"),
            name,
            attr,
            "letx=op();",
        );
    }
    check_opener_config(Path::new("Cargo.toml"), "[package.metadata]\nANY='data'");
    check_opener_config(
        Path::new(".cargo/config.toml"),
        "[build]\nrustflags='-Dclippy::all'",
    );
    assert!(std::panic::catch_unwind(|| check_opener_config(
        Path::new(".cargo/config.toml"),
        "[package.metadata]\nANY='data'"
    ))
    .is_err());
    assert!(std::panic::catch_unwind(|| check_opener_config(
        Path::new("Cargo.toml"),
        "[lints.clippy]\ndisallowed_methods='allow'"
    ))
    .is_err());
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("input.rs");
    std::fs::write(&file, "").unwrap();
    check_rust_file(dir.path(), &file, "fn allowed(){}");
    for input in [
        "include!(\"other.rs\");",
        "#[allow(clippy::all)] fn allowed(){}",
        "fn wrong(){Connection::from_handle();}",
    ] {
        assert!(
            std::panic::catch_unwind(|| check_rust_file(dir.path(), &file, input)).is_err(),
            "accepted {input}"
        );
    }
}

#[test]
fn sqlcipher_policy_diagnostics_name_the_file_and_selected_function() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("context.rs");
    std::fs::write(&file, "").unwrap();
    let panic = std::panic::catch_unwind(|| {
        check_rust_file(temp.path(), &file, "use std::fmt::Result as Alias;");
    })
    .unwrap_err();
    assert!(panic
        .downcast_ref::<String>()
        .unwrap()
        .contains(&file.display().to_string()));
    let config = Path::new("fixtures/clippy.toml");
    let repository_config = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../clippy.toml");
    check_clippy_config(config, &std::fs::read_to_string(repository_config).unwrap());
    let panic =
        std::panic::catch_unwind(|| check_clippy_config(config, "unknown=true")).unwrap_err();
    assert!(panic
        .downcast_ref::<String>()
        .unwrap()
        .contains("fixtures/clippy.toml"));
    let panic = std::panic::catch_unwind(|| opener_policy::function("fn other(){}", "missing"))
        .err()
        .unwrap();
    assert!(panic.downcast_ref::<String>().unwrap().contains("missing"));
}
