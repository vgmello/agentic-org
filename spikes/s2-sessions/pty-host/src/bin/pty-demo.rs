//! Demo for spike S2: attach to a background session through pty-host and run a
//! small script of steps, logging frame seqs and byte counts.
//!
//! pty-demo <short-id> [--cols N] [--rows N] [--dump FILE] [STEP...]
//! Steps: wait:SECS  send:TEXT  enter  key:esc|ctrl-c|ctrl-z|left  resize:COLSxROWS
//!        expect:TEXT:SECS  mark:LABEL  close  waitexit:SECS
//! Default steps: wait:3 send:"Reply with one word: ready" enter expect:MAXMARK:40
//!                resize:60x20 wait:2 resize:140x40 wait:2 close

use std::io::Write;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use pty_host::{open, AttachHandle, DeliveryError, Frame, OpenOptions, SEQ_PREFIX};

enum Ev {
    Data(u64, Vec<u8>),
    Exit(String),
    Title(String),
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: pty-demo <short-id> [--cols N] [--rows N] [--dump FILE] [STEP...]");
        std::process::exit(2);
    }
    let id = args.remove(0);
    let (mut cols, mut rows, mut dump) = (100u16, 30u16, None::<String>);
    let mut steps = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--cols" => { cols = args[i + 1].parse().unwrap(); i += 1 }
            "--rows" => { rows = args[i + 1].parse().unwrap(); i += 1 }
            "--dump" => { dump = Some(args[i + 1].clone()); i += 1 }
            s => steps.push(s.to_string()),
        }
        i += 1;
    }
    if steps.is_empty() {
        steps = ["wait:3", "send:Reply with one word: ready", "enter", "expect:MAXMARK:40", "resize:60x20", "wait:2", "resize:140x40", "wait:2", "close"]
            .iter().map(|s| s.to_string()).collect();
    }

    let (tx, rx) = mpsc::channel::<Ev>();
    let t0 = Instant::now();
    let mut handle: AttachHandle = open(OpenOptions { session_id: id.clone(), cols, rows, claude_bin: None }, move |f| {
        let ev = match f {
            Frame::Data(b) => {
                let seq = u64::from_be_bytes(b[..SEQ_PREFIX].try_into().unwrap());
                Ev::Data(seq, b[SEQ_PREFIX..].to_vec())
            }
            Frame::Title(t) => Ev::Title(t),
            Frame::Exit { code, signal, reason } => {
                Ev::Exit(format!("{{\"kind\":\"exit\",\"code\":{code},\"signal\":{signal:?},\"reason\":\"{}\"}}", reason.as_str()))
            }
        };
        tx.send(ev).map_err(|e| DeliveryError(e.to_string()))
    })
    .expect("open");
    log(t0, &format!("opened attach {id} at {cols}x{rows}, pid {:?}", handle.pid()));

    let mut st = State { all: Vec::new(), frames: 0, next_seq: 0, max_chunk: 0, first_byte: None, settled: None, last_data: t0, exited: None, titles: 0 };
    for step in &steps {
        let (verb, rest) = step.split_once(':').unwrap_or((step.as_str(), ""));
        match verb {
            "wait" => pump(&rx, &mut st, t0, secs(rest), None),
            "send" => { handle.write(rest.as_bytes()).ok(); log(t0, &format!("write {} bytes", rest.len())); pump(&rx, &mut st, t0, 0.3, None) }
            "enter" => { handle.write(b"\r").ok(); log(t0, "write CR") }
            "key" => {
                let b: &[u8] = match rest { "esc" => b"\x1b", "ctrl-c" => b"\x03", "ctrl-z" => b"\x1a", "left" => b"\x1b[D", other => other.as_bytes() };
                handle.write(b).ok();
                log(t0, &format!("key {rest}"));
            }
            "resize" => {
                let (c, r) = rest.split_once('x').unwrap();
                let before = st.all.len();
                handle.resize(c.parse().unwrap(), r.parse().unwrap()).ok();
                log(t0, &format!("resize {rest}"));
                pump(&rx, &mut st, t0, 1.0, None);
                log(t0, &format!("  {} bytes within 1s of resize", st.all.len() - before));
            }
            "expect" => {
                let (text, s) = rest.rsplit_once(':').unwrap();
                let from = st.all.len();
                pump(&rx, &mut st, t0, secs(s), Some((text, from)));
            }
            "mark" => log(t0, &format!("mark {rest}: total {} bytes", st.all.len())),
            "close" => {
                let c0 = Instant::now();
                handle.close();
                handle.close(); // idempotent
                pump(&rx, &mut st, t0, 0.5, None);
                log(t0, &format!("close() returned in {} ms", c0.elapsed().as_millis()));
            }
            "waitexit" => pump(&rx, &mut st, t0, secs(rest), None),
            _ => panic!("unknown step {step}"),
        }
        if st.exited.is_some() && verb != "close" {
            log(t0, "attach client already exited; skipping remaining steps");
            break;
        }
    }
    handle.close();
    pump(&rx, &mut st, t0, 0.3, None);

    log(t0, &format!(
        "summary: titles={} frames={} bytes={} max_chunk={} seq_contiguous={} first_byte_ms={:?} first_settled_frame_ms={:?} exit={}",
        st.titles, st.frames, st.all.len(), st.max_chunk, st.next_seq == st.frames,
        st.first_byte.map(|d| d.as_millis()), st.settled.map(|d| d.as_millis()),
        st.exited.as_deref().unwrap_or("none")
    ));
    if let Some(p) = dump {
        std::fs::File::create(&p).unwrap().write_all(&st.all).unwrap();
        log(t0, &format!("raw output written to {p}"));
    }
}

struct State {
    all: Vec<u8>,
    frames: u64,
    next_seq: u64,
    max_chunk: usize,
    first_byte: Option<Duration>,
    /// First 300 ms quiet gap after output started: a rough "first full frame".
    settled: Option<Duration>,
    last_data: Instant,
    exited: Option<String>,
    titles: u64,
}

fn pump(rx: &mpsc::Receiver<Ev>, st: &mut State, t0: Instant, secs: f64, expect: Option<(&str, usize)>) {
    let end = Instant::now() + Duration::from_secs_f64(secs);
    loop {
        if let Some((text, from)) = expect {
            if contains(&st.all[from..], text.as_bytes()) {
                log(t0, &format!("expect {text:?}: found"));
                return;
            }
        }
        let now = Instant::now();
        if now >= end {
            if let Some((text, _)) = expect { log(t0, &format!("expect {text:?}: TIMEOUT")); }
            return;
        }
        match rx.recv_timeout((end - now).min(Duration::from_millis(50))) {
            Ok(Ev::Data(seq, b)) => {
                if seq != st.next_seq { log(t0, &format!("SEQ GAP: got {seq}, want {}", st.next_seq)); }
                st.next_seq = seq + 1;
                st.frames += 1;
                st.max_chunk = st.max_chunk.max(b.len());
                if st.first_byte.is_none() { st.first_byte = Some(t0.elapsed()); }
                st.all.extend_from_slice(&b);
                st.last_data = Instant::now();
            }
            Ok(Ev::Title(t)) => { st.titles += 1; if st.titles <= 3 || t.ends_with("claude agents") { log(t0, &format!("title frame: {t:?}")); } }
            Ok(Ev::Exit(e)) => { log(t0, &format!("exit frame: {e}")); st.exited = Some(e); }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if st.settled.is_none() && st.first_byte.is_some() && st.last_data.elapsed() > Duration::from_millis(300) {
                    st.settled = Some(st.last_data - t0);
                }
            }
            Err(_) => return,
        }
    }
}

fn secs(s: &str) -> f64 { s.parse().unwrap() }
fn contains(h: &[u8], n: &[u8]) -> bool { !n.is_empty() && h.windows(n.len()).any(|w| w == n) }
fn log(t0: Instant, msg: &str) { println!("[{:>7.3}s] {msg}", t0.elapsed().as_secs_f64()); }
