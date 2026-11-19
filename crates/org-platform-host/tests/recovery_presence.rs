//! Exercise the unmodified native bridge in isolated, no-prompt processes.
//!
//! The fixture substitutes SDK replies, not authentication. This does not test
//! physical OS presence or Rust's deadline/session-cancellation machinery.
#![cfg(target_os = "macos")]

use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

const BRIDGE: &[u8] = include_bytes!("../src/presence/bridge.m");
const FIXTURE: &[u8] = include_bytes!("fixtures/recovery_presence.m");
const MAX_OUTPUT_BYTES: u64 = 1024 * 1024;

const SCENARIOS: [(&str, i32, &str); 16] = [
    (
        "denied_then_success",
        0,
        "PASS scenario=denied_then_success ",
    ),
    ("yes_with_error", 0, "PASS scenario=yes_with_error "),
    ("success_duplicates", 0, "PASS scenario=success_duplicates "),
    (
        "cancel_late_success",
        0,
        "PASS scenario=cancel_late_success ",
    ),
    ("cancel_late_denial", 0, "PASS scenario=cancel_late_denial "),
    ("reentrant_success", 0, "PASS scenario=reentrant_success "),
    ("reentrant_denial", 0, "PASS scenario=reentrant_denial "),
    (
        "invalidate_exception_success",
        0,
        "PASS scenario=invalidate_exception_success ",
    ),
    (
        "invalidate_exception_cancel",
        0,
        "PASS scenario=invalidate_exception_cancel ",
    ),
    ("can_denied", 0, "PASS scenario=can_denied "),
    ("can_yes_with_error", 0, "PASS scenario=can_yes_with_error "),
    ("evaluate_exception", 0, "PASS scenario=evaluate_exception "),
    (
        "synchronous_success",
        0,
        "PASS scenario=synchronous_success ",
    ),
    ("synchronous_denial", 0, "PASS scenario=synchronous_denial "),
    (
        "interception_negative_control",
        87,
        "INTERCEPTION_GUARD_REFUSED begin_count=0",
    ),
    (
        "sentinel_negative_control",
        86,
        "REAL_SDK_ENTRY_BLOCKED selector=evaluatePolicy:localizedReason:reply: begin_count=0",
    ),
];

/// Reap on every unwind, including a timeout or output-bound assertion.
struct RunningChild(Child);

impl Drop for RunningChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn read_output(mut file: File) -> String {
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_OUTPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .expect("read native child diagnostics");
    assert!(
        bytes.len() as u64 <= MAX_OUTPUT_BYTES,
        "native child output exceeded the diagnostic bound"
    );
    String::from_utf8_lossy(&bytes).into_owned()
}

fn run_bounded(
    mut command: Command,
    temporary: &Path,
    label: &str,
    timeout: Duration,
) -> (ExitStatus, String) {
    let log_path = temporary.join(format!("{label}.log"));
    let log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&log_path)
        .expect("create private child log");
    command
        .stdin(Stdio::null())
        .stdout(log.try_clone().expect("clone private stdout log"))
        .stderr(log);
    println!("{label}: {command:?}");
    let mut child = RunningChild(
        command
            .spawn()
            .expect("launch required native tool/fixture"),
    );
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.0.try_wait().expect("poll native child") {
            break status;
        }
        assert!(
            fs::metadata(&log_path).expect("stat child log").len() <= MAX_OUTPUT_BYTES,
            "{label}: native child exceeded output bound"
        );
        if start.elapsed() >= timeout {
            child.0.kill().expect("kill timed-out native child");
            child.0.wait().expect("reap timed-out native child");
            let output = read_output(File::open(&log_path).expect("open timeout diagnostics"));
            panic!("{label}: exceeded {timeout:?}\n{output}");
        }
        thread::sleep(Duration::from_millis(10));
    };
    let output = read_output(File::open(&log_path).expect("open child diagnostics"));
    println!("{label}: {status}, elapsed={:?}\n{output}", start.elapsed());
    (status, output)
}

fn unchanged(path: &Path, compiled_bytes: &[u8]) {
    assert!(
        fs::read(path).expect("read native test input") == compiled_bytes,
        "native input changed since this integration test was compiled: {}",
        path.display()
    );
}

#[test]
fn exact_native_bridge_lifecycle_with_arc_and_asan_without_os_prompt() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let bridge = manifest.join("src/presence/bridge.m");
    let fixture = manifest.join("tests/fixtures/recovery_presence.m");
    let temporary = tempfile::Builder::new()
        .prefix("agentic-recovery-presence-")
        .tempdir()
        .expect("private disposable native test directory");

    for (variant, sanitizer_flags) in [
        ("arc", &[][..]),
        (
            "asan",
            &["-fsanitize=address", "-fno-omit-frame-pointer"][..],
        ),
    ] {
        unchanged(&bridge, BRIDGE);
        unchanged(&fixture, FIXTURE);
        let executable = temporary.path().join(format!("probe-{variant}"));
        let mut compiler = Command::new("xcrun");
        compiler
            .arg("clang")
            .args([
                "-fobjc-arc",
                "-fblocks",
                "-Wall",
                "-Wextra",
                "-Werror",
                "-O1",
                "-g",
            ])
            .args(sanitizer_flags)
            // Separate translation units: no inclusion, macro replacement or
            // copy/edit of the production bridge and no product library link.
            .arg(&bridge)
            .arg(&fixture)
            .args([
                "-framework",
                "Foundation",
                "-framework",
                "LocalAuthentication",
                "-framework",
                "Security",
                "-framework",
                "SystemConfiguration",
                "-lbsm",
                "-o",
            ])
            .arg(&executable);
        let (status, output) = run_bounded(
            compiler,
            temporary.path(),
            &format!("compile-{variant}"),
            Duration::from_secs(120),
        );
        assert!(
            status.success(),
            "required {variant} compiler failed; no toolchain/sanitizer skip:\n{output}"
        );
        unchanged(&bridge, BRIDGE);
        unchanged(&fixture, FIXTURE);

        for (scenario, expected_exit, marker) in SCENARIOS {
            let mut command = Command::new(&executable);
            command.arg(scenario);
            if variant == "asan" {
                // LeakSanitizer is not claimed. Both translation units are
                // address-instrumented; the fixture also checks weak collection.
                command.env(
                    "ASAN_OPTIONS",
                    "halt_on_error=1:abort_on_error=1:detect_stack_use_after_return=1:detect_leaks=0",
                );
            }
            let (status, output) = run_bounded(
                command,
                temporary.path(),
                &format!("{variant}-{scenario}"),
                Duration::from_secs(15),
            );
            assert_eq!(
                status.code(),
                Some(expected_exit),
                "{variant}/{scenario}: unexpected native exit:\n{output}"
            );
            assert!(
                output.contains(marker) && !output.contains("AddressSanitizer"),
                "{variant}/{scenario}: missing scenario proof or sanitizer diagnostic:\n{output}"
            );
        }
    }
    unchanged(&bridge, BRIDGE);
    unchanged(&fixture, FIXTURE);
    temporary
        .close()
        .expect("remove all disposable executables, dSYMs and logs");
}
