//! The host's own log, to stderr (launchd, the systemd journal or the container runtime
//! keeps it).
//!
//! One thread of its own writes the lines, in the order they were logged. Whoever logs a line
//! waits until it is written, so that a line is out before what follows it (a refusal, then
//! the end of the process), but for [`WRITE_WAIT`] at most: stderr can be a pipe that nobody
//! reads any more, and a stopping service must not wait on it past its deadline (X5 review,
//! fourth round, P1). Once a line has waited that long, nobody waits again until the writer
//! has written one; more than [`QUEUED`] lines waiting are dropped, and how many is said when
//! the writer can write again. Where the writer's thread cannot start, a line is written by
//! whoever logs it, as before.

use std::collections::VecDeque;
use std::io::Write;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::sync::lock;

/// The longest text from a server that goes into one log line.
const MAX_UNTRUSTED_BYTES: usize = 2048;

/// The longest a caller of [`log`] waits for its line to be written.
const WRITE_WAIT: Duration = Duration::from_millis(100);

/// The most lines that wait for the writer; more are dropped and counted.
const QUEUED: usize = 256;

/// Logs one line after passing it through the shared `org-redact` choke-point. Never pass it
/// secrets directly; pass text a server sent through [`untrusted`].
pub fn log(message: &str) {
    let redacted = org_redact::redact_text(message, &[]);
    #[cfg(any(test, all(feature = "test-support", debug_assertions)))]
    CAPTURED.with(|captured| {
        if let Some(lines) = captured.borrow_mut().as_mut() {
            lines.push(redacted.clone());
        }
    });
    write_line(format!("agentic-org: {redacted}\n"));
}

/// The lines waiting for the writer, and how far it has got.
struct Writer {
    state: Mutex<Lines>,
    changed: Condvar,
}

#[derive(Default)]
struct Lines {
    waiting: VecDeque<String>,
    /// How many lines were ever queued, and how many of them are written: a line's number
    /// is the count of queued lines once it is queued.
    queued: u64,
    written: u64,
    /// A line waited [`WRITE_WAIT`] and was not written: no caller waits until one is.
    stuck: bool,
    /// Lines dropped because [`QUEUED`] lines were waiting.
    dropped: u64,
}

/// The writer, with its thread started at the first line; `None` if the thread could not
/// start.
fn writer() -> Option<&'static Arc<Writer>> {
    static WRITER: OnceLock<Option<Arc<Writer>>> = OnceLock::new();
    WRITER
        .get_or_init(|| {
            let writer = Arc::new(Writer {
                state: Mutex::new(Lines::default()),
                changed: Condvar::new(),
            });
            let writing = Arc::clone(&writer);
            std::thread::Builder::new()
                .name("host-log".into())
                .spawn(move || write_all(&writing))
                .ok()
                .map(|_| writer)
        })
        .as_ref()
}

fn write_line(line: String) {
    let Some(writer) = writer() else {
        let _ = std::io::stderr().write_all(line.as_bytes());
        return;
    };
    let mut state = lock(&writer.state);
    if state.waiting.len() >= QUEUED {
        state.dropped += 1;
        return;
    }
    state.waiting.push_back(line);
    state.queued += 1;
    let mine = state.queued;
    writer.changed.notify_all();
    let until = Instant::now() + WRITE_WAIT;
    while state.written < mine && !state.stuck {
        let now = Instant::now();
        if now >= until {
            state.stuck = true;
            return;
        }
        state = writer
            .changed
            .wait_timeout(state, until - now)
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .0;
    }
}

/// The writer's thread: each line in turn, for as long as the process lives. A write that
/// blocks holds up this thread alone.
fn write_all(writer: &Writer) {
    loop {
        let mut state = lock(&writer.state);
        while state.waiting.is_empty() {
            state = writer
                .changed
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        let line = state.waiting.pop_front().unwrap_or_default();
        let dropped = std::mem::take(&mut state.dropped);
        drop(state);
        let mut stderr = std::io::stderr().lock();
        if dropped > 0 {
            let _ = writeln!(
                stderr,
                "agentic-org: {dropped} log lines were dropped while the log could not be written"
            );
        }
        let _ = stderr.write_all(line.as_bytes());
        drop(stderr);
        let mut state = lock(&writer.state);
        state.written += 1;
        state.stuck = false;
        writer.changed.notify_all();
    }
}

#[cfg(any(test, all(feature = "test-support", debug_assertions)))]
thread_local! {
    static CAPTURED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// Runs `f` and returns what it logged on this thread, so a test can assert
/// that no secret reaches the host log.
#[cfg(any(test, all(feature = "test-support", debug_assertions)))]
pub fn capture<R>(f: impl FnOnce() -> R) -> (R, Vec<String>) {
    CAPTURED.with(|captured| *captured.borrow_mut() = Some(Vec::new()));
    let result = f();
    let lines = CAPTURED.with(|captured| captured.borrow_mut().take().unwrap_or_default());
    (result, lines)
}

/// Text a server sent (e.g. a `failed` message), made safe for one log line: control characters,
/// line and paragraph separators and direction controls are escaped, so it can't forge lines or
/// reorder what is shown, and it is cut to about 2 KiB.
pub fn untrusted(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if out.len() >= MAX_UNTRUSTED_BYTES {
            out.push('…');
            break;
        }
        if c.is_control() || breaks_or_reorders(c) {
            out.extend(c.escape_default());
        } else {
            out.push(c);
        }
    }
    out
}

/// Characters that some viewers show as a line break, or that reorder the text around them.
fn breaks_or_reorders(c: char) -> bool {
    matches!(
        c,
        '\u{2028}' | '\u{2029}' // line and paragraph separators
            | '\u{200E}' | '\u{200F}' | '\u{061C}' // direction marks
            | '\u{202A}'..='\u{202E}' // embeddings and overrides
            | '\u{2066}'..='\u{2069}' // isolates
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Runs this test binary's test `test` alone in a child process whose stderr is a pipe
    /// that is full and never read, with `variable` set, and returns the child's exit status
    /// and its stdout, or `None` if it had not ended `within` (it is then killed). A write
    /// to its stderr blocks for as long as it lives.
    pub(crate) fn with_full_stderr(
        test: &str,
        variable: &str,
        within: Duration,
    ) -> Option<(std::process::ExitStatus, String)> {
        let (reader, mut full) = std::io::pipe().unwrap();
        org_platform::posix::set_nonblocking(&full, true).unwrap();
        loop {
            match full.write(&[b'x'; 4096]) {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => panic!("filling the pipe: {e}"),
            }
        }
        // Blocking again (the child shares this open file), and full.
        org_platform::posix::set_nonblocking(&full, false).unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture", "--test-threads=1"])
            .env(variable, "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(full)
            .spawn()
            .unwrap();
        let until = Instant::now() + within;
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break Some(status);
            }
            if Instant::now() >= until {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let mut stdout = String::new();
        let _ = std::io::Read::read_to_string(&mut child.stdout.take().unwrap(), &mut stdout);
        drop(reader);
        status.map(|status| (status, stdout))
    }

    const FULL_STDERR_CHILD: &str = "ORG_HOST_TEST_LOG_FULL_STDERR";

    /// X5 review, fourth round (P1): a line logged while stderr is a full pipe that nobody
    /// reads returns within the writer's wait, and every later one at once: a stopping host
    /// never blocks on its log past its deadline.
    #[test]
    fn a_log_line_does_not_block_on_a_stderr_nobody_reads() {
        if std::env::var_os(FULL_STDERR_CHILD).is_some() {
            let started = Instant::now();
            log("first line, which cannot be written");
            let first = started.elapsed();
            for n in 0..2 * QUEUED {
                log(&format!("line {n}"));
            }
            let all = started.elapsed();
            assert!(first < Duration::from_secs(1), "{first:?}");
            assert!(all < Duration::from_secs(1), "{all:?}");
            println!("logging returned after {all:?}");
            return;
        }
        let (status, stdout) = with_full_stderr(
            "host_log::tests::a_log_line_does_not_block_on_a_stderr_nobody_reads",
            FULL_STDERR_CHILD,
            Duration::from_secs(20),
        )
        .expect("logging blocked on a full stderr");
        assert!(
            status.success() && stdout.contains("logging returned after"),
            "{status:?} {stdout}"
        );
    }

    #[test]
    fn capture_collects_this_threads_lines_only_while_active() {
        log("before");
        let ((), lines) = capture(|| {
            log("one");
            std::thread::spawn(|| log("other thread")).join().unwrap();
        });
        assert_eq!(lines, ["one"]);
        log("after");
        assert_eq!(capture(|| ()).1, Vec::<String>::new());
    }

    #[test]
    fn untrusted_text_is_escaped_and_cut() {
        assert_eq!(untrusted("a\nb\u{1b}[2Jc"), "a\\nb\\u{1b}[2Jc");
        assert_eq!(untrusted("plain é"), "plain é");
        assert_eq!(untrusted("a\u{2028}b\u{202E}c"), "a\\u{2028}b\\u{202e}c");
        let long = untrusted(&"x".repeat(10_000));
        assert!(long.len() <= MAX_UNTRUSTED_BYTES + '…'.len_utf8());
        assert!(long.ends_with('…'));
    }

    #[test]
    fn host_log_redacts_secrets_through_org_redact() {
        let api_key = ["sk-ant-", "api03-hostlogsecret99"].join("");
        let ((), lines) = capture(|| {
            log(&format!(
                "auth failure Authorization: Bearer host-bearer-987654 key={api_key} \
                 X-Org-Session-Proof: host-proof-112233"
            ));
            log("Cookie: sid=host-cookie-445566");
        });
        let joined = lines.join("\n");
        for secret in [
            "host-bearer-987654",
            api_key.as_str(),
            "host-proof-112233",
            "host-cookie-445566",
        ] {
            assert!(!joined.contains(secret), "leaked `{secret}` in {joined}");
        }
        assert!(joined.contains(org_redact::REDACTED), "{joined}");
    }
}
