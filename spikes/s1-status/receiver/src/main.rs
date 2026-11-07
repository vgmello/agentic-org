//! Spike S1 receiver (THROWAWAY). Listens on a Unix socket for presence events
//! (HTTP/1.1 POST, JSON body), checks the peer uid, applies the design §5
//! status rules (rules.rs, keyed by session id), prints one status line per
//! event with timestamps, raises a native macOS notification on each new
//! "Needs you" reason (on a notifier thread), and watches the socket peer's
//! pid with kqueue (EVFILT_PROC/NOTE_EXIT) to detect crashes, after a 500 ms
//! grace so a SessionEnd still in flight wins.
//!
//! Usage:
//!   s1-receiver listen [--socket PATH] [--state-dir DIR] [--log FILE] [--notify native|osascript|none] [--bundle ID] [--poll-secs N]
//!   s1-receiver poll-bench [N]
//!   s1-receiver notify-test [--notify M] [--bundle ID]

mod rules;

use rules::{parse_body, trunc, Engine, Outcome};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CRASH_GRACE: Duration = Duration::from_millis(500);
const MAX_BODY: usize = 64 * 1024;

fn now_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_micros() as f64 / 1000.0
}

struct NotifyJob {
    label: String,
    text: String,
    t_origin: f64,
    origin: &'static str,
}

struct Shared {
    engine: Engine,
    state_dir: String,
    watched: std::collections::HashSet<i32>,
    log: Option<std::fs::File>,
    kq: i32,
    tx: Sender<NotifyJob>,
}

type St = Arc<Mutex<Shared>>;

/// Never let a panic elsewhere poison the state for good.
fn lk(st: &St) -> MutexGuard<'_, Shared> {
    st.lock().unwrap_or_else(|e| e.into_inner())
}

fn log_line(sh: &mut Shared, line: &str) {
    println!("{line}");
    if let Some(f) = sh.log.as_mut() {
        let _ = writeln!(f, "{line}");
    }
}

/// Raise a notification; returns (ms spent in the call, result).
fn notify(mode: &str, title: &str, body: &str) -> (f64, String) {
    let t0 = Instant::now();
    let res = match mode {
        "native" => notify_rust::Notification::new()
            .summary(title)
            .body(body)
            .show()
            .map(|_| "ok".to_string())
            .unwrap_or_else(|e| format!("err {e}")),
        "osascript" => {
            let q = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
            let script = format!("display notification {} with title {}", q(body), q(title));
            match Command::new("osascript").arg("-e").arg(script).status() {
                Ok(s) => format!("exit {s}"),
                Err(e) => format!("err {e}"),
            }
        }
        _ => "skipped".into(),
    };
    (t0.elapsed().as_secs_f64() * 1000.0, res)
}

fn notifier_loop(st: St, rx: std::sync::mpsc::Receiver<NotifyJob>, mode: String) {
    for job in rx {
        let t0 = now_ms();
        let (dur, res) = notify(&mode, &format!("{} needs you", job.label), &job.text);
        let mut sh = lk(&st);
        log_line(
            &mut sh,
            &format!(
                "{:.3} NOTIFY[{mode}] {}: {} | {res} | {}->notify-call-start {:.1} ms, call {dur:.1} ms",
                now_ms(),
                job.label,
                job.text,
                job.origin,
                t0 - job.t_origin
            ),
        );
    }
}

fn watch_pid(sh: &mut Shared, pid: i32, label: &str) {
    if !sh.watched.insert(pid) {
        return;
    }
    let ev = libc::kevent {
        ident: pid as usize,
        filter: libc::EVFILT_PROC,
        flags: libc::EV_ADD | libc::EV_ONESHOT,
        fflags: libc::NOTE_EXIT,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    let r = unsafe { libc::kevent(sh.kq, &ev, 1, std::ptr::null_mut(), 0, std::ptr::null()) };
    let msg = if r < 0 {
        sh.watched.remove(&pid);
        format!("{:.3} KQUEUE watch peer pid={pid} ({label}) FAILED: {}", now_ms(), std::io::Error::last_os_error())
    } else {
        format!("{:.3} KQUEUE watching peer pid={pid} ({label})", now_ms())
    };
    log_line(sh, &msg);
}

fn dispatch(sh: &mut Shared, o: &Outcome, t_origin: f64, origin: &'static str) {
    for text in &o.notify {
        let _ = sh.tx.send(NotifyJob { label: o.agent.clone(), text: text.clone(), t_origin, origin });
    }
}

/// Apply one event. Called BEFORE the 204 is written, so a SessionEnd is
/// applied before the session process can exit.
fn handle_body(st: &St, body: &[u8], peer_uid: u32, peer_pid: i32, t_recv: f64) {
    let ev = match parse_body(body) {
        Ok(v) => v,
        Err(e) => {
            log_line(&mut lk(st), &format!("{t_recv:.3} {e}"));
            return;
        }
    };
    let mut sh = lk(st);
    if let Some(f) = sh.log.as_mut() {
        let mut raw = ev.clone();
        raw["_t_recv"] = json!(t_recv);
        raw["_peer_pid"] = json!(peer_pid);
        raw["_peer_uid"] = json!(peer_uid);
        raw["_session_pid"] = json!(session_pid(&ev, peer_pid));
        let _ = writeln!(f, "RAW {raw}");
    }
    let spid = session_pid(&ev, peer_pid);
    let o = sh.engine.event(&ev, spid);
    let hook = ev["hook"].as_str().unwrap_or("?");
    let t_hook = ev["t_after_next"].as_f64().or(ev["t_hook"].as_f64()).unwrap_or(f64::NAN);
    let detail = ["tool", "notification_type", "reason", "source", "blocked", "decided", "background_tasks", "subagent_type"]
        .iter()
        .filter(|k| !ev[**k].is_null())
        .map(|k| format!("{k}={}", ev[*k]))
        .collect::<Vec<_>>()
        .join(" ");
    let line = format!(
        "{t_recv:.3} {} sess={} seq={} {hook} {detail} -> {} | hook->recv {:.1} ms | peer pid={peer_pid} session pid={spid} | {}{}",
        o.agent,
        trunc(&o.session, 8),
        ev["seq"].as_i64().map(|x| x.to_string()).unwrap_or("-".into()),
        o.status,
        t_recv - t_hook,
        if o.dropped { "DROPPED " } else { "" },
        o.note
    );
    log_line(&mut sh, &line);
    if let Some(p) = o.watch {
        let label = o.agent.clone();
        watch_pid(&mut sh, p, &label);
    }
    dispatch(&mut sh, &o, t_hook, "hook");
}

/// The parent of `pid`, from the kernel (proc_pidinfo PROC_PIDTBSDINFO).
fn parent_pid(pid: i32) -> Option<i32> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    let n = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, &mut info as *mut _ as *mut libc::c_void, size) };
    (n == size).then_some(info.pbi_ppid as i32)
}

/// The session process behind a connection. With the curl transport the peer is
/// a short-lived curl child of the session (still alive: we answer only after
/// applying the event), so the session is its parent. Both pids come from the
/// kernel; the event only says which of the two to take.
fn session_pid(ev: &Value, peer_pid: i32) -> i32 {
    if ev["transport"].as_str() == Some("curl") {
        // A curl whose session already died is reparented to launchd (ppid 1): unknown.
        parent_pid(peer_pid).filter(|p| *p > 1).unwrap_or(-1)
    } else {
        peer_pid
    }
}

/// The session's latest normalized event, as the mod wrote it (`<state_dir>/<session>.json`).
fn read_state(dir: &str, session: &str) -> Option<Value> {
    if session.is_empty() || !session.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    let text = std::fs::read_to_string(Path::new(dir).join(format!("{session}.json"))).ok()?;
    serde_json::from_str(&text).ok() // a torn write reads as None: treated as no SessionEnd
}

fn peer_creds(stream: &UnixStream) -> (u32, i32) {
    let fd = stream.as_raw_fd();
    let mut uid: libc::uid_t = u32::MAX;
    let mut gid: libc::gid_t = u32::MAX;
    unsafe { libc::getpeereid(fd, &mut uid, &mut gid) };
    // SOL_LOCAL = 0, LOCAL_PEERPID = 0x002 (sys/un.h)
    let mut pid: libc::pid_t = -1;
    let mut len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    unsafe { libc::getsockopt(fd, 0, 0x002, &mut pid as *mut _ as *mut libc::c_void, &mut len) };
    (uid, pid)
}

fn serve(st: St, stream: UnixStream) {
    let (uid, pid) = peer_creds(&stream);
    let me = unsafe { libc::getuid() };
    let Ok(mut w) = stream.try_clone() else { return };
    if uid != me {
        log_line(&mut lk(&st), &format!("{:.3} REJECT peer uid={uid} (mine {me}) pid={pid}", now_ms()));
        let _ = w.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return;
    }
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut r = BufReader::new(stream);
    let mut clen: usize = 0;
    let mut first = String::new();
    if r.read_line(&mut first).is_err() {
        return;
    }
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                clen = v.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0u8; clen.min(MAX_BODY)];
    if r.read_exact(&mut body).is_err() {
        return;
    }
    let t_recv = now_ms();
    handle_body(&st, &body, uid, pid, t_recv); // apply first ...
    let _ = w.write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"); // ... then answer
}

fn kqueue_loop(st: St, kq: i32) {
    loop {
        let mut out: [libc::kevent; 8] = unsafe { std::mem::zeroed() };
        let n = unsafe { libc::kevent(kq, std::ptr::null(), 0, out.as_mut_ptr(), 8, std::ptr::null()) };
        if n <= 0 {
            continue;
        }
        let t_exit = now_ms();
        for ev in &out[..n as usize] {
            let pid = ev.ident as i32;
            let data = ev.data;
            let st = st.clone();
            // Grace period: a SessionEnd still in flight must win over the exit.
            std::thread::spawn(move || {
                std::thread::sleep(CRASH_GRACE);
                let mut sh = lk(&st);
                sh.watched.remove(&pid);
                let dir = sh.state_dir.clone();
                let outs = sh.engine.exited(pid, &|sid| read_state(&dir, sid));
                for o in &outs {
                    let line = format!(
                        "{:.3} KQUEUE NOTE_EXIT pid={pid} (exit at {t_exit:.3}, data={data}) {} sess={} -> {} | {}",
                        now_ms(),
                        o.agent,
                        trunc(&o.session, 8),
                        o.status,
                        o.note
                    );
                    log_line(&mut sh, &line);
                    dispatch(&mut sh, o, t_exit, "exit");
                }
            });
        }
    }
}

fn agents_json() -> Option<Value> {
    let out = Command::new("claude").args(["agents", "--json"]).output().ok()?;
    serde_json::from_slice(&out.stdout).ok()
}

fn reconcile_loop(st: St, secs: u64) {
    loop {
        std::thread::sleep(Duration::from_secs(secs));
        let t0 = Instant::now();
        let Some(list) = agents_json() else { continue };
        let el = t0.elapsed().as_secs_f64() * 1000.0;
        let mut sh = lk(&st);
        let mut lines = vec![format!("{:.3} RECONCILE claude agents --json took {el:.0} ms", now_ms())];
        for (sid, x) in sh.engine.sessions.iter() {
            let found = list.as_array().and_then(|arr| arr.iter().find(|j| j["sessionId"].as_str() == Some(sid.as_str())));
            lines.push(match found {
                Some(j) => format!("  {}: agents.json pid={} status={} waitingFor={} | ours pid={:?} {}", x.agent, j["pid"], j["status"], j["waitingFor"], x.pid, x.status_text()),
                None => format!("  {} {}: not in claude agents --json | ours {}", x.agent, trunc(sid, 8), x.status_text()),
            });
        }
        for l in lines {
            log_line(&mut sh, &l);
        }
    }
}

fn rusage_children() -> (f64, f64, i64) {
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_CHILDREN, &mut ru) };
    let tv = |t: libc::timeval| t.tv_sec as f64 * 1000.0 + t.tv_usec as f64 / 1000.0;
    (tv(ru.ru_utime), tv(ru.ru_stime), ru.ru_maxrss as i64)
}

fn poll_bench(n: usize) {
    let mut walls = vec![];
    let mut cpus = vec![];
    for i in 0..n {
        let (u0, s0, _) = rusage_children();
        let t0 = Instant::now();
        let out = Command::new("claude").args(["agents", "--json"]).output().expect("run claude");
        let wall = t0.elapsed().as_secs_f64() * 1000.0;
        let (u1, s1, rss) = rusage_children();
        let cpu = (u1 - u0) + (s1 - s0);
        let count = serde_json::from_slice::<Value>(&out.stdout).ok().and_then(|v| v.as_array().map(|a| a.len())).unwrap_or(0);
        println!("call {i:2}: wall {wall:7.1} ms  cpu(user+sys) {cpu:7.1} ms  (user {:.1}, sys {:.1})  sessions={count}  maxrss(children, bytes)={rss}", u1 - u0, s1 - s0);
        walls.push(wall);
        cpus.push(cpu);
    }
    let stats = |v: &mut Vec<f64>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p = |q: f64| v[((v.len() as f64 - 1.0) * q).round() as usize];
        (p(0.5), p(0.9), v[v.len() - 1], v.iter().sum::<f64>() / v.len() as f64)
    };
    let (wp50, wp90, wmax, wmean) = stats(&mut walls);
    let (cp50, cp90, cmax, cmean) = stats(&mut cpus);
    println!("SUMMARY n={n}: wall p50 {wp50:.1} ms, p90 {wp90:.1}, max {wmax:.1}, mean {wmean:.1} | cpu p50 {cp50:.1} ms, p90 {cp90:.1}, max {cmax:.1}, mean {cmean:.1}");
    println!("At a 10 s poll interval: ~{:.2}% of one core on average (cpu mean / 10 000 ms)", cmean / 100.0);
}

/// The socket's directory must be ours and private (0700); a live socket is never removed.
fn prepare_socket(sock: &str) -> Result<(), String> {
    let path = Path::new(sock);
    let dir = path.parent().ok_or("socket path has no parent")?;
    if !dir.exists() {
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir).map_err(|e| format!("mkdir {dir:?}: {e}"))?;
    }
    let md = std::fs::symlink_metadata(dir).map_err(|e| format!("stat {dir:?}: {e}"))?;
    let me = unsafe { libc::getuid() };
    if !md.is_dir() || md.uid() != me || md.permissions().mode() & 0o077 != 0 {
        return Err(format!("{dir:?} must be a directory owned by uid {me} with mode 0700 (is uid {} mode {:o})", md.uid(), md.permissions().mode() & 0o777));
    }
    if let Ok(m) = std::fs::symlink_metadata(path) {
        if !m.file_type().is_socket() {
            return Err(format!("{sock} exists and is not a socket"));
        }
        if UnixStream::connect(path).is_ok() {
            return Err(format!("{sock} is live (another receiver is listening); not removing it"));
        }
        std::fs::remove_file(path).map_err(|e| format!("remove stale {sock}: {e}"))?;
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    if let Some(b) = arg("--bundle") {
        // notify-rust/mac-notification-sys default to com.apple.Finder when unbundled, which usernoted denies.
        println!("set_application({b}) -> {:?}", notify_rust::set_application(&b));
    }
    match args.get(1).map(|s| s.as_str()) {
        Some("poll-bench") => poll_bench(args.get(2).and_then(|s| s.parse().ok()).unwrap_or(20)),
        Some("notify-test") => {
            let mode = arg("--notify").unwrap_or("native".into());
            let (d, r) = notify(&mode, "S1 receiver", "notification test");
            println!("notify[{mode}] -> {r} in {d:.1} ms");
        }
        Some("listen") => {
            let sock = arg("--socket").unwrap_or("/tmp/ac-spike-s1/run/events.sock".into());
            // The mod's state files: separate from the socket dir; sessions may create it, so
            // the receiver only reads it and never refuses to start over its mode.
            let state_dir = arg("--state-dir").unwrap_or("/tmp/ac-spike-s1/state".into());
            let notify_mode = arg("--notify").unwrap_or("native".into());
            let log = arg("--log").map(|p| std::fs::OpenOptions::new().create(true).append(true).open(p).expect("open log"));
            if let Err(e) = prepare_socket(&sock) {
                eprintln!("refusing to start: {e}");
                std::process::exit(1);
            }
            // Bind with umask 0177 so the socket is created 0600 (no chmod race).
            let old = unsafe { libc::umask(0o177) };
            let listener = UnixListener::bind(&sock).expect("bind");
            unsafe { libc::umask(old) };
            let kq = unsafe { libc::kqueue() };
            let (tx, rx) = channel();
            let st: St = Arc::new(Mutex::new(Shared { engine: Engine::default(), state_dir: state_dir.clone(), watched: Default::default(), log, kq, tx }));
            {
                let mode = std::fs::metadata(&sock).unwrap().permissions().mode() & 0o777;
                let dmode = std::fs::metadata(Path::new(&sock).parent().unwrap()).unwrap().permissions().mode() & 0o777;
                log_line(&mut lk(&st), &format!("{:.3} LISTEN {sock} mode {mode:o} (dir mode {dmode:o}) state-dir={state_dir} notify={notify_mode} uid={}", now_ms(), unsafe { libc::getuid() }));
            }
            {
                let st = st.clone();
                std::thread::spawn(move || notifier_loop(st, rx, notify_mode));
            }
            {
                let st = st.clone();
                std::thread::spawn(move || kqueue_loop(st, kq));
            }
            if let Some(secs) = arg("--poll-secs").and_then(|s| s.parse().ok()) {
                let st = st.clone();
                std::thread::spawn(move || reconcile_loop(st, secs));
            }
            for conn in listener.incoming() {
                match conn {
                    Ok(c) => {
                        let st = st.clone();
                        std::thread::spawn(move || serve(st, c));
                    }
                    Err(e) => eprintln!("accept: {e}"),
                }
            }
        }
        _ => {
            eprintln!("usage: s1-receiver listen [--socket P] [--log F] [--notify native|osascript|none] [--bundle ID] [--poll-secs N] | poll-bench [N] | notify-test [--notify M]");
            std::process::exit(2);
        }
    }
}
