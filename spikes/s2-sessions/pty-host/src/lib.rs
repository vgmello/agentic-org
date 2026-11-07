//! Spike S2: run `claude attach <id8>` in a PTY and stream its output.
//!
//! Throwaway code. One [`AttachHandle`] is one attach client (one panel in one
//! window); several handles on the same session are fine. The session itself
//! lives in Claude Code's background daemon and is never signalled here:
//! closing a handle ends only the attach client.
//!
//! Frames go to a single callback, from a single reader thread, in order:
//! zero or more [`Frame::Data`] then exactly one [`Frame::Exit`].

use std::ffi::OsString;
use std::io::{Read, Write};
use std::os::unix::io::RawFd;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, Child, CommandBuilder, ExitStatus, MasterPty, PtySize};

/// Largest payload of one data frame.
pub const MAX_CHUNK: usize = 16 * 1024;
/// Bytes of the big-endian sequence number in front of every data frame.
pub const SEQ_PREFIX: usize = 8;
/// Smallest PTY size the Claude Code TUI renders usably (prompt box + status line).
pub const MIN_COLS: u16 = 40;
pub const MIN_ROWS: u16 = 10;
/// After SIGHUP, how long the client gets before SIGKILL.
const KILL_AFTER: Duration = Duration::from_secs(2);
/// After SIGKILL, or after the client exited while something else still holds
/// the PTY open, how long the reader keeps draining before it gives up.
const GIVE_UP_AFTER: Duration = Duration::from_millis(1000);
/// Once the client has exited, stop reading after this much silence.
const QUIET_AFTER_EXIT: Duration = Duration::from_millis(300);
/// Longest wait for `claude agents --json --all` in `open`.
const AGENTS_TIMEOUT: Duration = Duration::from_secs(3);

/// Why the attach client went away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    /// We closed it (`close()`, drop, or the frame callback failed).
    Closed,
    /// Attach left the alternate screen and printed `Session <id8> has exited`:
    /// the session stopped while attached.
    SessionEnded,
    /// It exited on its own for any other reason: the user detached (Ctrl+Z,
    /// Esc in the agent view), a bad id, or a crash.
    AttachExited,
}

impl ExitReason {
    pub fn as_str(self) -> &'static str {
        match self {
            ExitReason::Closed => "closed",
            ExitReason::SessionEnded => "session_ended",
            ExitReason::AttachExited => "attach_exited",
        }
    }
}

#[derive(Debug)]
pub enum Frame {
    /// 8-byte big-endian seq, then 1..=[`MAX_CHUNK`] raw PTY bytes. Seq starts
    /// at 0 and has no gaps.
    Data(Vec<u8>),
    /// The window title changed (OSC 0 or OSC 2, BEL- or ST-terminated, also
    /// when split across reads). Sent after the Data frame that completed it;
    /// repeats of the same title are dropped. In a session it is
    /// `<spinner> <name>`; in the agent view it ends with `claude agents`.
    Title(String),
    /// Last frame. `code` is the exit status (`u32::MAX` if unknown); `signal`
    /// is set when the client died from a signal (portable-pty gives a name).
    Exit { code: u32, signal: Option<String>, reason: ExitReason },
}

/// The callback could not deliver a frame (e.g. the webview is gone). The
/// handle then closes its attach client itself.
#[derive(Debug)]
pub struct DeliveryError(pub String);

#[derive(Debug, Clone)]
pub struct OpenOptions {
    /// The background session's short id: the 8 hex chars `claude --bg`
    /// prints and `claude agents --json` lists as `id`. Attach takes no full
    /// uuid, and matches any other string as part of a session name.
    pub session_id: String,
    pub cols: u16,
    pub rows: u16,
    /// Path to `claude`; defaults to `claude` on PATH. Tests point it at fakes.
    pub claude_bin: Option<OsString>,
}

type BoxedChild = Box<dyn Child + Send + Sync>;

struct Shared {
    /// `None` once reaped. Signals are sent only while holding this lock and
    /// only if `try_wait` says the child is still running, so a reaped (and
    /// possibly reused) pid is never signalled.
    child: Mutex<Option<BoxedChild>>,
    status: Mutex<Option<ExitStatus>>,
    pid: u32,
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    /// When close was requested (by `close()` or a failed delivery).
    closing_since: Mutex<Option<Instant>>,
    closing: AtomicBool,
    signals_sent: AtomicU32,
}

impl Shared {
    /// Send `sig` to the client's process group, only if the client is still
    /// running (not yet reaped). Reaps it if it has exited.
    fn signal_if_running(&self, sig: libc::c_int) -> bool {
        let mut guard = self.child.lock().unwrap();
        let Some(child) = guard.as_mut() else { return false };
        match child.try_wait() {
            Ok(None) if self.pid > 1 => {
                // Negative pid = the whole process group (portable-pty runs the
                // child under setsid, so its pid is its pgid).
                unsafe {
                    libc::kill(-(self.pid as libc::pid_t), sig);
                }
                self.signals_sent.fetch_add(1, Ordering::SeqCst);
                true
            }
            Ok(Some(st)) => {
                *self.status.lock().unwrap() = Some(st);
                *guard = None;
                false
            }
            _ => false,
        }
    }

    /// Reap without blocking. True once the client has exited.
    fn poll_exit(&self) -> bool {
        let mut guard = self.child.lock().unwrap();
        let Some(child) = guard.as_mut() else { return true };
        match child.try_wait() {
            Ok(Some(st)) => {
                *self.status.lock().unwrap() = Some(st);
                *guard = None;
                true
            }
            Ok(None) => false,
            Err(_) => {
                *guard = None;
                true
            }
        }
    }

    fn request_close(&self) {
        if !self.closing.swap(true, Ordering::SeqCst) {
            *self.closing_since.lock().unwrap() = Some(Instant::now());
            self.signal_if_running(libc::SIGHUP);
        }
    }
}

impl Drop for Shared {
    /// Backstop: whatever path got us here, never leave the client running.
    fn drop(&mut self) {
        kill_and_reap(self);
    }
}

pub struct AttachHandle {
    shared: Arc<Shared>,
    reader: Option<thread::JoinHandle<()>>,
    pub opened_at: Instant,
}

/// Inherited variables that make `claude` think it runs inside another
/// Claude Code session. Everything starting with `CLAUDE` goes, except these.
const ENV_KEEP: &[&str] = &["CLAUDE_CONFIG_DIR"];
const ENV_DROP_EXTRA: &[&str] = &["AI_AGENT"];

fn scrubbed(key: &str) -> bool {
    (key.starts_with("CLAUDE") && !ENV_KEEP.contains(&key)) || ENV_DROP_EXTRA.contains(&key)
}

fn scrubbed_keys() -> Vec<OsString> {
    std::env::vars_os().map(|(k, _)| k).filter(|k| k.to_str().is_some_and(scrubbed)).collect()
}

fn invalid(msg: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, msg.into())
}

fn is_short_id(id: &str) -> bool {
    id.len() == 8 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn check_size(cols: u16, rows: u16) -> std::io::Result<()> {
    if cols < MIN_COLS || rows < MIN_ROWS {
        return Err(invalid(format!("PTY must be at least {MIN_COLS}x{MIN_ROWS}, got {cols}x{rows}")));
    }
    Ok(())
}

/// `attach` resolves its argument as an id *or* as part of a session name, so
/// make sure `id8` is the exact `id` of one session and no session name
/// contains it. Uses `claude agents --json --all` (stopped sessions count:
/// attaching to one wakes it).
fn verify_id(bin: &OsString, id8: &str) -> std::io::Result<()> {
    let mut cmd = Command::new(bin);
    cmd.args(["agents", "--json", "--all"]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    for k in scrubbed_keys() {
        cmd.env_remove(k);
    }
    let mut child = cmd.spawn()?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let reader = thread::spawn(move || {
        let mut v = Vec::new();
        let _ = stdout.read_to_end(&mut v);
        v
    });
    let deadline = Instant::now() + AGENTS_TIMEOUT;
    let status = loop {
        if let Some(st) = child.try_wait()? {
            break st;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "`claude agents --json --all` took longer than 3 s",
            ));
        }
        thread::sleep(Duration::from_millis(20));
    };
    let out = reader.join().unwrap_or_default();
    if !status.success() {
        return Err(std::io::Error::other(format!("`claude agents --json --all` failed: {status}")));
    }
    let list: serde_json::Value = serde_json::from_slice(&out).map_err(std::io::Error::other)?;
    let sessions = list.as_array().ok_or_else(|| std::io::Error::other("agents --json: not an array"))?;
    let want = id8.to_ascii_lowercase();
    let is_target = |s: &serde_json::Value| s.get("id").and_then(|v| v.as_str()).is_some_and(|i| i.eq_ignore_ascii_case(&want));
    let exact = sessions.iter().filter(|s| is_target(s)).count();
    if exact != 1 {
        return Err(invalid(format!("no single background session has id {id8} (matches: {exact})")));
    }
    // Any *other* session whose name contains the id could be what attach picks.
    let named = sessions
        .iter()
        .filter(|s| !is_target(s))
        .filter(|s| s.get("name").and_then(|v| v.as_str()).is_some_and(|n| n.to_ascii_lowercase().contains(&want)))
        .count();
    if named > 0 {
        return Err(invalid(format!("{named} other session name(s) contain {id8}; attach could match a name")));
    }
    Ok(())
}

/// Open `claude attach <id8>` in a new PTY. `on_frame` is called from the
/// handle's reader thread. Rejects (does not clamp) sizes under 40x10.
pub fn open<F>(opts: OpenOptions, on_frame: F) -> std::io::Result<AttachHandle>
where
    F: FnMut(Frame) -> Result<(), DeliveryError> + Send + 'static,
{
    if !is_short_id(&opts.session_id) {
        return Err(invalid(
            "session_id must be the 8-hex short id (attach takes no full uuid, and names are ambiguous)",
        ));
    }
    check_size(opts.cols, opts.rows)?;
    let bin = opts.claude_bin.clone().unwrap_or_else(|| "claude".into());
    verify_id(&bin, &opts.session_id)?;

    let pty = native_pty_system()
        .openpty(PtySize { rows: opts.rows, cols: opts.cols, pixel_width: 0, pixel_height: 0 })
        .map_err(io_err)?;

    let mut cmd = CommandBuilder::new(&bin);
    cmd.arg("attach");
    cmd.arg(&opts.session_id);
    for k in scrubbed_keys() {
        cmd.env_remove(k);
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("LANG", "en_US.UTF-8");
    if let Ok(cwd) = std::env::current_dir() {
        cmd.cwd(cwd);
    }

    let child: BoxedChild = pty.slave.spawn_command(cmd).map_err(io_err)?;
    drop(pty.slave); // so the master sees EOF when the child exits
    let pid = child.process_id().unwrap_or(0);

    let shared = Arc::new(Shared {
        child: Mutex::new(Some(child)),
        status: Mutex::new(None),
        pid,
        master: Mutex::new(None),
        writer: Mutex::new(None),
        closing_since: Mutex::new(None),
        closing: AtomicBool::new(false),
        signals_sent: AtomicU32::new(0),
    });
    // From here on, a failure must not leak the child.
    let fail = |e: std::io::Error| {
        kill_and_reap(&shared);
        e
    };
    if pid <= 1 {
        return Err(fail(std::io::Error::other("spawned child has no usable pid")));
    }
    let fd = pty.master.as_raw_fd().ok_or_else(|| fail(std::io::Error::other("PTY master has no fd")))?;
    let reader = pty.master.try_clone_reader().map_err(|e| fail(io_err(e)))?;
    let writer = pty.master.take_writer().map_err(|e| fail(io_err(e)))?;
    *shared.writer.lock().unwrap() = Some(writer);
    *shared.master.lock().unwrap() = Some(pty.master);

    let sh = shared.clone();
    let id8 = opts.session_id.clone();
    let reader_thread = thread::Builder::new()
        .name(format!("pty-attach-{id8}"))
        .spawn(move || read_loop(sh, fd, reader, id8, on_frame))
        .map_err(fail)?;

    Ok(AttachHandle { shared, reader: Some(reader_thread), opened_at: Instant::now() })
}

fn kill_and_reap(sh: &Shared) {
    if sh.pid <= 1 {
        return; // nothing we may signal or wait for
    }
    sh.signal_if_running(libc::SIGKILL);
    if let Some(mut c) = sh.child.lock().unwrap().take() {
        if let Ok(st) = c.wait() {
            *sh.status.lock().unwrap() = Some(st);
        }
    }
}

/// Incremental OSC parser: pulls OSC 0/2 titles out of the byte stream.
#[derive(Default)]
pub struct TitleParser {
    state: OscState,
    buf: Vec<u8>,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum OscState {
    #[default]
    Ground,
    Esc,
    Osc,
    OscEsc,
}

impl TitleParser {
    const MAX_OSC: usize = 4096;

    /// Feed raw output; returns the titles completed by these bytes.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<String> {
        let mut out = Vec::new();
        for &b in bytes {
            self.state = match (self.state, b) {
                (OscState::Ground, 0x1b) => OscState::Esc,
                (OscState::Ground, _) => OscState::Ground,
                (OscState::Esc, b']') => {
                    self.buf.clear();
                    OscState::Osc
                }
                (OscState::Esc, 0x1b) => OscState::Esc,
                (OscState::Esc, _) => OscState::Ground,
                (OscState::Osc, 0x07) => {
                    self.finish(&mut out);
                    OscState::Ground
                }
                (OscState::Osc, 0x1b) => OscState::OscEsc,
                (OscState::Osc, _) => {
                    if self.buf.len() < Self::MAX_OSC {
                        self.buf.push(b);
                    }
                    OscState::Osc
                }
                (OscState::OscEsc, b'\\') => {
                    self.finish(&mut out);
                    OscState::Ground
                }
                // ESC inside an OSC that is not ST: abandon it, treat as a new escape.
                (OscState::OscEsc, b']') => {
                    self.buf.clear();
                    OscState::Osc
                }
                (OscState::OscEsc, 0x1b) => OscState::Esc,
                (OscState::OscEsc, _) => OscState::Ground,
            };
        }
        out
    }

    fn finish(&mut self, out: &mut Vec<String>) {
        let body = std::mem::take(&mut self.buf);
        if let Some(rest) = body.strip_prefix(b"0;").or_else(|| body.strip_prefix(b"2;")) {
            out.push(String::from_utf8_lossy(rest).into_owned());
        }
    }
}

/// The reader thread. It polls the master with a short timeout, so it never
/// blocks for good, even if the client ignores SIGHUP or a grandchild keeps
/// the PTY slave open.
fn read_loop<F>(sh: Arc<Shared>, fd: RawFd, mut reader: Box<dyn Read + Send>, id8: String, mut on_frame: F)
where
    F: FnMut(Frame) -> Result<(), DeliveryError>,
{
    let mut seq: u64 = 0;
    let mut buf = vec![0u8; MAX_CHUNK];
    // Last bytes, to spot attach's own "Session <id8> has exited" after it
    // leaves the alternate screen.
    let mut tail: Vec<u8> = Vec::new();
    let mut deliver = true;
    let mut killed_at: Option<Instant> = None;
    let mut exited_at: Option<Instant> = None;
    let mut last_data = Instant::now();
    let mut titles = TitleParser::default();
    let mut last_title: Option<String> = None;

    loop {
        let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        let r = unsafe { libc::poll(&mut pfd, 1, 50) };
        if r > 0 {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    last_data = Instant::now();
                    tail.extend_from_slice(&buf[..n]);
                    if tail.len() > 8192 {
                        tail.drain(..tail.len() - 8192);
                    }
                    if deliver {
                        let mut frame = Vec::with_capacity(SEQ_PREFIX + n);
                        frame.extend_from_slice(&seq.to_be_bytes());
                        frame.extend_from_slice(&buf[..n]);
                        seq += 1;
                        let mut ok = deliver_frame(&mut on_frame, Frame::Data(frame));
                        for t in titles.feed(&buf[..n]) {
                            if ok && last_title.as_deref() != Some(t.as_str()) {
                                last_title = Some(t.clone());
                                ok = deliver_frame(&mut on_frame, Frame::Title(t));
                            }
                        }
                        if !ok {
                            // Nobody is listening: end this attach client (not the session).
                            deliver = false;
                            sh.request_close();
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break, // EIO on macOS once every slave fd is closed
            }
        } else if r < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            break;
        }

        // Deadlines, checked on every pass (not only on poll timeouts), so a
        // client that keeps printing can't keep us here past them.
        if exited_at.is_none() && sh.poll_exit() {
            exited_at = Some(Instant::now());
        }
        if let Some(t) = exited_at {
            // The client is gone; something else may still hold the slave open.
            if last_data.elapsed() > QUIET_AFTER_EXIT || t.elapsed() > GIVE_UP_AFTER {
                break;
            }
        }
        let since = *sh.closing_since.lock().unwrap();
        if let Some(since) = since {
            if killed_at.is_none() && since.elapsed() > KILL_AFTER {
                sh.signal_if_running(libc::SIGKILL);
                killed_at = Some(Instant::now());
            }
            if killed_at.is_some_and(|k| k.elapsed() > GIVE_UP_AFTER) {
                break;
            }
        }
    }

    // Make sure the client is gone and reaped. EOF can arrive a moment before
    // the exit is reapable, so give it a short grace before killing.
    let grace = Instant::now() + Duration::from_millis(500);
    while !sh.poll_exit() {
        if Instant::now() > grace {
            kill_and_reap(&sh);
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let reason = if sh.closing.load(Ordering::SeqCst) {
        ExitReason::Closed
    } else if session_ended(&tail, &id8) {
        ExitReason::SessionEnded
    } else {
        ExitReason::AttachExited
    };
    let (code, signal) = match sh.status.lock().unwrap().as_ref() {
        Some(s) => (s.exit_code(), s.signal().map(|s| s.to_string())),
        None => (u32::MAX, None),
    };
    sh.writer.lock().unwrap().take();
    sh.master.lock().unwrap().take();
    deliver_frame(&mut on_frame, Frame::Exit { code, signal, reason });
}

/// Call the frame callback; an `Err` or a panic both count as "nobody is
/// listening", so a panicking callback can't leave the client running.
fn deliver_frame<F>(on_frame: &mut F, f: Frame) -> bool
where
    F: FnMut(Frame) -> Result<(), DeliveryError>,
{
    matches!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| on_frame(f))), Ok(Ok(())))
}

/// True if, after the last "leave alternate screen", attach printed
/// `Session <id8> has exited`.
pub fn session_ended(tail: &[u8], id8: &str) -> bool {
    let leave = b"\x1b[?1049l";
    let Some(pos) = rfind(tail, leave) else { return false };
    let needle = format!("Session {id8} has exited");
    find(&tail[pos + leave.len()..], needle.as_bytes()).is_some()
}

impl AttachHandle {
    /// Raw bytes from xterm.js `onData`/`onBinary`, as is.
    pub fn write(&self, bytes: &[u8]) -> std::io::Result<()> {
        let mut w = self.shared.writer.lock().unwrap();
        match w.as_mut() {
            Some(w) => {
                w.write_all(bytes)?;
                w.flush()
            }
            None => Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "attach client has exited")),
        }
    }

    /// TIOCSWINSZ on the master; the kernel sends SIGWINCH to the foreground
    /// group. Rejects sizes under 40x10, like `open`.
    pub fn resize(&self, cols: u16, rows: u16) -> std::io::Result<()> {
        check_size(cols, rows)?;
        let m = self.shared.master.lock().unwrap();
        match m.as_ref() {
            Some(m) => m.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }).map_err(io_err),
            None => Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "attach client has exited")),
        }
    }

    /// End the attach client only: SIGHUP to its process group if it is still
    /// running, SIGKILL after 2 s, stop reading ~1 s later, then up to 0.5 s to
    /// reap. Idempotent; the session keeps running. Returns once the Exit frame
    /// has been delivered (within ~3.5 s in the worst case).
    pub fn close(&mut self) {
        self.shared.request_close();
        if let Some(t) = self.reader.take() {
            let _ = t.join();
        }
    }

    pub fn pid(&self) -> u32 {
        self.shared.pid
    }

    /// How many signals this handle has sent to its client (for tests).
    pub fn signals_sent(&self) -> u32 {
        self.shared.signals_sent.load(Ordering::SeqCst)
    }
}

impl Drop for AttachHandle {
    fn drop(&mut self) {
        self.close();
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).rposition(|w| w == needle)
}

fn io_err(e: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::other(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_split_across_reads_with_bel_or_st() {
        let mut p = TitleParser::default();
        assert!(p.feed(b"abc\x1b]0;\xe2\x9c\xb3 zz-sp").is_empty());
        assert_eq!(p.feed(b"ike\x07more\x1b]2;claude ag"), vec!["\u{2733} zz-spike".to_string()]);
        assert!(p.feed(b"ents\x1b").is_empty());
        assert_eq!(p.feed(b"\\"), vec!["claude agents".to_string()]);
        // Other OSCs (8 links, 7501) are not titles.
        assert!(p.feed(b"\x1b]8;;http://x\x07\x1b]7501;?\x07").is_empty());
        assert_eq!(p.feed(b"\x1b]0;\x07"), vec![String::new()]);
    }

    #[test]
    fn scrub_rules() {
        for k in [
            "CLAUDECODE",
            "CLAUDE_PID",
            "CLAUDE_JOB_DIR",
            "CLAUDE_EFFORT",
            "CLAUDE_CODE_SESSION_KIND",
            "CLAUDE_CODE_NEW_THING",
            "AI_AGENT",
        ] {
            assert!(scrubbed(k), "{k}");
        }
        for k in ["CLAUDE_CONFIG_DIR", "PATH", "ANTHROPIC_API_KEY", "HOME"] {
            assert!(!scrubbed(k), "{k}");
        }
    }

    #[test]
    fn session_ended_needs_real_id_after_leaving_alt_screen() {
        let id = "abcdef12";
        assert!(session_ended(b"\x1b[?1049h screen \x1b[?1049l\r\nSession abcdef12 has exited.\r\n", id));
        // On the session's own screen, before leaving the alternate screen: not ours.
        assert!(!session_ended(b"\x1b[?1049h Session abcdef12 has exited \x1b[?1049l bye", id));
        // Another id, or the bare phrase.
        assert!(!session_ended(b"\x1b[?1049l Session 11111111 has exited.", id));
        assert!(!session_ended(b"\x1b[?1049l the job has exited", id));
        // No alternate-screen exit at all.
        assert!(!session_ended(b"Session abcdef12 has exited.", id));
    }

    #[test]
    fn rejects_bad_ids_and_small_sizes_before_spawning() {
        let bin = Some(OsString::from("/nonexistent/claude"));
        for bad in ["zz-spike-a", "370c5656-58dc-4018-b00a-fdd1095e4d23", "370C5656", "370c565"] {
            let r = open(OpenOptions { session_id: bad.into(), cols: 80, rows: 24, claude_bin: bin.clone() }, |_| Ok(()));
            assert_eq!(r.err().map(|e| e.kind()), Some(std::io::ErrorKind::InvalidInput), "{bad}");
        }
        for (c, r) in [(39, 24), (80, 9), (20, 5)] {
            let res =
                open(OpenOptions { session_id: "abcdef12".into(), cols: c, rows: r, claude_bin: bin.clone() }, |_| Ok(()));
            assert_eq!(res.err().map(|e| e.kind()), Some(std::io::ErrorKind::InvalidInput), "{c}x{r}");
        }
    }
}
