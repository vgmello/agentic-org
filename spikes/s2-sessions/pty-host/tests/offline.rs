//! Offline tests: `claude_bin` points at small shell scripts that play
//! `claude agents --json --all` and `claude attach <id8>`.
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use pty_host::{open, AttachHandle, DeliveryError, ExitReason, Frame, OpenOptions, MAX_CHUNK, SEQ_PREFIX};

const ID: &str = "abcdef12";
const AGENTS_OK: &str = r#"[{"id":"abcdef12","name":"zz-fake","sessionId":"abcdef12-0000-0000-0000-000000000000","kind":"background"}]"#;

fn fake(test: &str, agents_json: &str, attach_body: &str) -> OsString {
    let dir: PathBuf = std::env::temp_dir().join(format!("pty-host-fake-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("claude");
    let script = format!(
        "#!/bin/sh\nif [ \"$1\" = agents ]; then printf '%s\\n' '{agents_json}'; exit 0; fi\n\
         if [ \"$1\" != attach ] || [ \"$2\" != {ID} ]; then echo \"bad args: $*\" >&2; exit 64; fi\n{attach_body}\n"
    );
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path.into_os_string()
}

fn opts(bin: OsString) -> OpenOptions {
    OpenOptions { session_id: ID.into(), cols: 80, rows: 24, claude_bin: Some(bin) }
}

fn start(bin: OsString) -> (AttachHandle, Receiver<Frame>) {
    let (tx, rx) = mpsc::channel();
    let h = open(opts(bin), move |f| tx.send(f).map_err(|e| DeliveryError(e.to_string()))).unwrap();
    (h, rx)
}

struct Collected {
    data: Vec<Vec<u8>>,
    titles: Vec<String>,
    exit: (u32, Option<String>, ExitReason),
    after_exit: usize,
}

/// Collect frames until Exit, then check nothing follows it.
fn collect(rx: &Receiver<Frame>, within: Duration) -> Collected {
    let end = Instant::now() + within;
    let mut data = Vec::new();
    let mut titles = Vec::new();
    loop {
        let left = end.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left).expect("timed out waiting for the Exit frame") {
            Frame::Data(b) => data.push(b),
            Frame::Title(t) => titles.push(t),
            Frame::Exit { code, signal, reason } => {
                std::thread::sleep(Duration::from_millis(200));
                let after_exit = rx.try_iter().count();
                return Collected { data, titles, exit: (code, signal, reason), after_exit };
            }
        }
    }
}

fn payload(c: &Collected) -> Vec<u8> {
    c.data.iter().flat_map(|f| f[SEQ_PREFIX..].to_vec()).collect()
}

#[test]
fn frames_have_contiguous_seq_bounded_payload_and_one_exit() {
    let bin = fake(
        "big",
        AGENTS_OK,
        "head -c 100000 /dev/zero | tr '\\000' 'a'\nprintf '\\033[?1049l\\r\\nSession abcdef12 has exited.\\r\\n'\nexit 0",
    );
    let (mut h, rx) = start(bin);
    let c = collect(&rx, Duration::from_secs(10));
    for (i, f) in c.data.iter().enumerate() {
        assert!(f.len() > SEQ_PREFIX, "empty frame {i}");
        assert!(f.len() - SEQ_PREFIX <= MAX_CHUNK, "frame {i} payload {} > 16 KiB", f.len() - SEQ_PREFIX);
        assert_eq!(u64::from_be_bytes(f[..SEQ_PREFIX].try_into().unwrap()), i as u64, "seq gap at frame {i}");
    }
    let p = payload(&c);
    assert!(p.starts_with(&[b'a'; 100_000]) && p[100_000] != b'a', "payload is not 100000 'a's then the trailer");
    assert_eq!(c.exit.0, 0);
    assert_eq!(c.exit.2, ExitReason::SessionEnded);
    assert_eq!(c.after_exit, 0, "frames after Exit");
    h.close();
    assert_eq!(h.signals_sent(), 0, "close() after self-exit must not signal");
}

#[test]
fn session_text_on_screen_is_not_session_ended() {
    // The phrase appears on the session's own screen (inside the alternate
    // screen), then attach leaves and says something else.
    let bin = fake(
        "falsepos",
        AGENTS_OK,
        "printf '\\033[?1049hSession abcdef12 has exited\\r\\n'\nprintf '\\033[?1049l bye\\r\\n'\nexit 0",
    );
    let (_h, rx) = start(bin);
    assert_eq!(collect(&rx, Duration::from_secs(5)).exit.2, ExitReason::AttachExited);
}

#[test]
fn other_id_or_bare_phrase_is_not_session_ended() {
    let bin = fake("otherid", AGENTS_OK, "printf '\\033[?1049l\\r\\nSession 11111111 has exited. the job has exited\\r\\n'\nexit 0");
    let (_h, rx) = start(bin);
    assert_eq!(collect(&rx, Duration::from_secs(5)).exit.2, ExitReason::AttachExited);
}

#[test]
fn self_exit_reports_code_and_close_sends_no_signal() {
    let bin = fake("selfexit", AGENTS_OK, "printf hi\nexit 3");
    let (mut h, rx) = start(bin);
    let c = collect(&rx, Duration::from_secs(5));
    assert_eq!((c.exit.0, c.exit.2), (3, ExitReason::AttachExited));
    h.close();
    h.close();
    assert_eq!(h.signals_sent(), 0);
}

#[test]
fn close_is_idempotent_and_hangs_up_once() {
    let bin = fake("sleeper", AGENTS_OK, "printf ready\nexec sleep 30");
    let (mut h, rx) = start(bin);
    std::thread::sleep(Duration::from_millis(300));
    let t = Instant::now();
    h.close();
    h.close();
    assert!(t.elapsed() < Duration::from_secs(1), "close took {:?}", t.elapsed());
    let c = collect(&rx, Duration::from_secs(1));
    assert_eq!(c.exit.2, ExitReason::Closed);
    assert_eq!(c.exit.1.as_deref().map(|s| s.contains("Hangup")), Some(true), "{:?}", c.exit);
    assert_eq!(h.signals_sent(), 1, "one SIGHUP, no SIGKILL");
    assert!(h.write(b"x").is_err());
}

#[test]
fn close_finishes_when_client_ignores_sighup() {
    let bin = fake("nohup", AGENTS_OK, "trap '' HUP\nprintf ready\nwhile :; do sleep 0.1; done");
    let (mut h, rx) = start(bin);
    std::thread::sleep(Duration::from_millis(300));
    let t = Instant::now();
    h.close();
    let took = t.elapsed();
    assert!(took < Duration::from_millis(3500), "close took {took:?}");
    assert!(took >= Duration::from_secs(2), "SIGKILL should come only after 2 s, took {took:?}");
    let c = collect(&rx, Duration::from_secs(1));
    assert_eq!(c.exit.2, ExitReason::Closed);
    assert_eq!(h.signals_sent(), 2, "SIGHUP then SIGKILL");
}

#[test]
fn exit_is_prompt_when_a_grandchild_keeps_the_pty_open() {
    // A grandchild in its own session holds the slave fd. On macOS the master
    // still sees EOF/EIO once the client exits (this test also passes with a
    // blocking read), so it only checks that Exit comes promptly. The hang
    // fix is proven by close_finishes_when_client_ignores_sighup_and_keeps_printing.
    let bin = fake(
        "grandchild",
        AGENTS_OK,
        "perl -e 'use POSIX; POSIX::setsid(); sleep 5' &\nprintf bye\nexit 0",
    );
    let (_h, rx) = start(bin);
    let t = Instant::now();
    let c = collect(&rx, Duration::from_secs(4));
    assert!(t.elapsed() < Duration::from_millis(2500), "took {:?}", t.elapsed());
    assert_eq!(c.exit.2, ExitReason::AttachExited);
}

#[test]
fn failed_delivery_closes_the_client_itself() {
    let bin = fake("delivery", AGENTS_OK, "printf ready\nexec sleep 30");
    let (tx, rx) = mpsc::channel();
    let mut first = true;
    let t = Instant::now();
    let mut h = open(opts(bin), move |f| match f {
        Frame::Data(_) if first => {
            first = false;
            Err(DeliveryError("webview gone".into()))
        }
        Frame::Data(_) | Frame::Title(_) => panic!("no frames after a failed delivery"),
        Frame::Exit { reason, signal, .. } => {
            tx.send((reason, signal)).unwrap();
            Ok(())
        }
    })
    .unwrap();
    let (reason, signal) = rx.recv_timeout(Duration::from_secs(4)).unwrap();
    assert_eq!(reason, ExitReason::Closed);
    // Ended by our SIGHUP alone (no SIGKILL needed). The time bound is loose
    // because open() itself spawns the fake twice under a parallel test run.
    assert!(signal.as_deref().is_some_and(|s| s.contains("Hangup")), "{signal:?}");
    assert!(t.elapsed() < Duration::from_secs(3), "took {:?}", t.elapsed());
    h.close();
    assert_eq!(h.signals_sent(), 1);
}

#[test]
fn refuses_ids_not_listed_exactly_or_contained_in_another_name() {
    let none = fake("noid", r#"[{"id":"11111111","name":"zz-fake"}]"#, "exit 0");
    let named = fake(
        "inname",
        r#"[{"id":"abcdef12","name":"zz-fake"},{"id":"22222222","name":"zz-ABCDEF12-x"}]"#,
        "exit 0",
    );
    for bin in [none, named] {
        let e = open(opts(bin), |_| Ok(())).err().expect("must refuse");
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput, "{e}");
    }
}

#[test]
fn id_match_is_case_insensitive_and_own_name_may_contain_it() {
    // The listed id is upper case, and the target's own name contains its id.
    let bin = fake("ownname", r#"[{"id":"ABCDEF12","name":"zz-abcdef12"}]"#, "printf hi\nexit 0");
    let (_h, rx) = start(bin);
    assert_eq!(collect(&rx, Duration::from_secs(5)).exit.2, ExitReason::AttachExited);
}

#[test]
fn agents_listing_times_out_after_about_3_s() {
    let dir = std::env::temp_dir().join(format!("pty-host-fake-{}-slowagents", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("claude");
    std::fs::write(&path, "#!/bin/sh\nexec sleep 20\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let t = Instant::now();
    let e = open(opts(path.into_os_string()), |_| Ok(())).err().expect("must time out");
    assert_eq!(e.kind(), std::io::ErrorKind::TimedOut, "{e}");
    assert!(t.elapsed() < Duration::from_secs(4), "took {:?}", t.elapsed());
}

#[test]
fn panicking_callback_closes_the_client() {
    let bin = fake("panic", AGENTS_OK, "printf ready\nexec sleep 30");
    let (tx, rx) = mpsc::channel();
    let t = Instant::now();
    let h = open(opts(bin), move |f| match f {
        Frame::Exit { reason, signal, .. } => {
            tx.send((reason, signal)).unwrap();
            Ok(())
        }
        _ => panic!("callback bug"),
    })
    .unwrap();
    let (reason, signal) = rx.recv_timeout(Duration::from_secs(4)).unwrap();
    assert_eq!(reason, ExitReason::Closed);
    // Ended by our SIGHUP alone (no SIGKILL needed). The time bound is loose
    // because open() itself spawns the fake twice under a parallel test run.
    assert!(signal.as_deref().is_some_and(|s| s.contains("Hangup")), "{signal:?}");
    assert!(t.elapsed() < Duration::from_secs(3), "took {:?}", t.elapsed());
    assert_eq!(h.signals_sent(), 1);
}

#[test]
fn rejects_small_resize() {
    let bin = fake("resize", AGENTS_OK, "printf ready\nexec sleep 30");
    let (mut h, _rx) = start(bin);
    assert!(h.resize(39, 24).is_err());
    assert!(h.resize(80, 9).is_err());
    assert!(h.resize(40, 10).is_ok());
    h.close();
}

#[test]
fn osc_titles_split_across_reads_become_title_frames() {
    // A title split over two writes (so two reads), a repeat (dropped), an ST-terminated
    // agent-view title, and an OSC 8 link that is not a title.
    let bin = fake(
        "title",
        AGENTS_OK,
        "printf 'x\\033]0;\\342\\234\\263 zz-sp'\nsleep 0.3\nprintf 'ike\\007y'\nprintf '\\033]0;\\342\\234\\263 zz-spike\\007'\n\
         printf '\\033]8;;http://x\\007link\\033]8;;\\007'\nprintf '\\033]2;1 awaiting input \\302\\267 claude agents\\033\\\\'\nsleep 0.2\nexit 0",
    );
    let (_h, rx) = start(bin);
    let c = collect(&rx, Duration::from_secs(5));
    assert!(c.data.len() >= 2, "expected the title to arrive over at least two reads, got {} frames", c.data.len());
    assert_eq!(c.titles, vec!["\u{2733} zz-spike".to_string(), "1 awaiting input \u{b7} claude agents".to_string()]);
    assert!(c.titles.last().unwrap().ends_with("claude agents"));
    assert_eq!(c.exit.2, ExitReason::AttachExited);
}

#[test]
fn close_finishes_when_client_ignores_sighup_and_keeps_printing() {
    // Prints every ~10 ms (more often than the 50 ms poll timeout), so the
    // reader never hits a poll timeout: the SIGKILL deadline must still run.
    // The loop is bounded (~15-30 s) so a failing run leaves no orphan behind.
    let bin = fake(
        "nohup-chatty",
        AGENTS_OK,
        "trap '' HUP\ni=0\nwhile [ $i -lt 1500 ]; do printf x; sleep 0.01; i=$((i+1)); done",
    );
    let (h, rx) = start(bin);
    std::thread::sleep(Duration::from_millis(300));
    let (done_tx, done_rx) = mpsc::channel();
    let t = Instant::now();
    std::thread::spawn(move || {
        let mut h = h;
        h.close();
        done_tx.send(h.signals_sent()).unwrap();
    });
    let signals = done_rx.recv_timeout(Duration::from_secs(4)).expect("close() did not return within 4 s");
    assert!(t.elapsed() < Duration::from_secs(4));
    assert_eq!(signals, 2, "SIGHUP then SIGKILL");
    let c = collect(&rx, Duration::from_secs(1));
    assert_eq!(c.exit.2, ExitReason::Closed);
}
