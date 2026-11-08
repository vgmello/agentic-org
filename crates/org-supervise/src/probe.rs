//! Running a [`ProbePlan`]: a short-lived process on pipes, with `/` as its cwd, no files
//! written, its stdin fed, its output captured (capped), a timeout, and its process group
//! killed at the end.
//!
//! Why it exists: preflight and quota checks run the runtime's own binary, so they go through
//! the same spawn (allowlisted environment, only fds 0-2, own group) as sessions.
//!
//! A probe can be a **conversation**: [`run_probe_with`] hands each stdout line (framed by the
//! port's [`LineFramer`]) to a [`ProbeConversation`], which may answer with lines to write and
//! say when it's done. [`run_probe`] is the fixed form: just the plan's `stdin_lines`.
//!
//! One thread, `poll(2)`, non-blocking pipes: no reader thread can be left blocked on a pipe
//! that an escaped grandchild still holds open.

use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsFd, AsRawFd};
use std::path::Path;
use std::time::{Duration, Instant};

use org_runtime::{Frame, Line, LineFramer, ProbeOutput, ProbePlan, Stdio};

use crate::child::{spawn_inner, Spawn};
use crate::env::Env;
use crate::error::{Error, Result};
use crate::sys;

/// The most a probe's stdout or stderr keeps; the rest is read and dropped.
pub const PROBE_OUTPUT_CAP: usize = 1 << 20;

/// How long output is still read after the probe exits (its pipes can outlive it briefly).
const DRAIN_AFTER_EXIT: Duration = Duration::from_millis(250);

/// How long a probe gets to exit after its conversation is done (stdin is closed then).
const DONE_GRACE: Duration = Duration::from_millis(500);

/// The longest single `poll` wait, so the exit is noticed promptly.
const POLL_SLICE: Duration = Duration::from_millis(20);

/// Drives a probe's stdin from its stdout, one line at a time.
pub trait ProbeConversation {
    /// Called with each stdout frame, in order (the last, unterminated line included). Returns
    /// the lines to write next, and whether the exchange is complete.
    fn on_frame(&mut self, frame: &Frame) -> ProbeReply;
}

/// A [`ProbeConversation`]'s answer to one frame.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProbeReply {
    /// Lines to write to the probe's stdin, in order. Dropped if stdin is already closed
    /// (a plan with a conversation sets `keep_stdin_open_until_exit`).
    pub write: Vec<Line>,
    /// The exchange is complete: stdin is closed, and the probe gets a short grace to exit
    /// before its group is killed (it isn't reported as timed out).
    pub done: bool,
}

/// No conversation: only the plan's fixed `stdin_lines`.
struct Fixed;

impl ProbeConversation for Fixed {
    fn on_frame(&mut self, _frame: &Frame) -> ProbeReply {
        ProbeReply::default()
    }
}

/// Runs `plan` with `env` plus its `env_add`, writing its `stdin_lines`.
pub fn run_probe(plan: &ProbePlan, env: &Env) -> Result<ProbeOutput> {
    run_probe_with(plan, env, &mut Fixed)
}

/// Runs `plan` with `env` plus its `env_add`: writes its `stdin_lines`, then whatever
/// `conversation` answers to each stdout line. Stdin is closed after the fixed lines unless
/// the plan keeps it open; then it stays open until the probe exits, the conversation is
/// done, or the timeout. At the end what's left of the probe's group is killed and the probe
/// is reaped (with orphan cleanup if `env` has a mark).
pub fn run_probe_with(
    plan: &ProbePlan,
    env: &Env,
    conversation: &mut dyn ProbeConversation,
) -> Result<ProbeOutput> {
    let env = env.with_additions(&plan.env_add)?;
    let mut child = spawn_inner(Spawn {
        program: &plan.program,
        args: &plan.args,
        cwd: Path::new("/"),
        env: &env,
        stdio: Stdio::Piped,
        cleanup: Vec::new(),
        cleanup_root: None,
    })?;
    let started = Instant::now();
    // A timeout too large to add is as good as none (a year from now).
    let deadline = started
        .checked_add(Duration::from_millis(plan.timeout_ms))
        .unwrap_or_else(|| started + Duration::from_secs(365 * 24 * 3600));

    let mut stdin = child.take_stdin();
    let mut stdout = child.take_stdout();
    let mut stderr = child.take_stderr();
    for f in [&stdin, &stdout, &stderr].into_iter().flatten() {
        sys::set_nonblocking(f.as_fd()).map_err(|e| Error::io("configure a pipe", e))?;
    }

    let mut pending: Vec<u8> = plan
        .stdin_lines
        .iter()
        .flat_map(|l| l.as_bytes().iter().copied())
        .collect();
    let mut close_when_flushed = !plan.keep_stdin_open_until_exit;
    let mut out = Capped::default();
    let mut err = Capped::default();
    let mut framer = LineFramer::new();
    let mut done_at: Option<Instant> = None;
    let mut exited_at: Option<Instant> = None;
    let mut timed_out = false;
    let mut buf = vec![0u8; 64 * 1024];

    loop {
        if pending.is_empty() && close_when_flushed {
            stdin = None;
        }
        let now = Instant::now();
        if exited_at.is_none() && child.has_exited()? {
            exited_at = Some(now);
        }
        if let Some(at) = exited_at {
            if (stdout.is_none() && stderr.is_none()) || now >= at + DRAIN_AFTER_EXIT {
                break;
            }
        } else if now >= deadline {
            timed_out = true;
            break;
        } else if done_at.is_some_and(|at| now >= at + DONE_GRACE) {
            break;
        }

        let mut fds = Vec::with_capacity(3);
        let mut slots = Vec::with_capacity(3);
        if let Some(f) = &stdout {
            fds.push(pollfd(f, libc::POLLIN));
            slots.push(Slot::Out);
        }
        if let Some(f) = &stderr {
            fds.push(pollfd(f, libc::POLLIN));
            slots.push(Slot::Err);
        }
        if let Some(f) = stdin.as_ref().filter(|_| !pending.is_empty()) {
            fds.push(pollfd(f, libc::POLLOUT));
            slots.push(Slot::In);
        }
        let limit = exited_at
            .map(|at| at + DRAIN_AFTER_EXIT)
            .unwrap_or(deadline)
            .saturating_duration_since(now)
            .min(POLL_SLICE);
        let ms = libc::c_int::try_from(limit.as_millis()).unwrap_or(0).max(1);
        sys::poll(&mut fds, ms).map_err(|e| Error::io("poll the probe", e))?;

        for (fd, slot) in fds.iter().zip(&slots) {
            if fd.revents == 0 {
                continue;
            }
            match slot {
                Slot::Out => {
                    let mut eof = false;
                    if let Some(f) = &mut stdout {
                        eof = drain(f, &mut buf, |chunk| {
                            out.push(chunk);
                            for frame in framer.push(chunk) {
                                answer(conversation, &frame, &mut pending, &mut done_at);
                            }
                        });
                    }
                    if eof {
                        stdout = None;
                        if let Some(frame) = framer.finish() {
                            answer(conversation, &frame, &mut pending, &mut done_at);
                        }
                    }
                }
                Slot::Err => {
                    let mut eof = false;
                    if let Some(f) = &mut stderr {
                        eof = drain(f, &mut buf, |chunk| err.push(chunk));
                    }
                    if eof {
                        stderr = None;
                    }
                }
                Slot::In => {
                    if let Some(f) = &mut stdin {
                        match f.write(&pending) {
                            Ok(n) => {
                                pending.drain(..n);
                            }
                            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                            Err(_) => {
                                // EPIPE: the probe stopped reading.
                                pending.clear();
                                stdin = None;
                            }
                        }
                    }
                }
            }
        }
        if done_at.is_some() {
            close_when_flushed = true;
        }
        if stdin.is_none() {
            pending.clear();
        }
    }

    // Kill before closing stdin, so a timed-out probe can't exit cleanly on EOF meanwhile.
    if !child.has_exited()? {
        child.kill();
    }
    drop(stdin);
    drop(stdout);
    drop(stderr);
    let exit = child.reap()?;
    Ok(ProbeOutput {
        stdout: out.bytes,
        stderr: err.bytes,
        exit_code: exit.info.code,
        timed_out,
    })
}

#[derive(Clone, Copy)]
enum Slot {
    Out,
    Err,
    In,
}

fn pollfd(f: &File, events: libc::c_short) -> libc::pollfd {
    libc::pollfd {
        fd: f.as_raw_fd(),
        events,
        revents: 0,
    }
}

/// Reads what's available; returns whether the stream ended (EOF or an error).
/// At most this many reads per stream per turn of the loop, so a probe that writes as fast as
/// it's read can't keep the loop from checking its deadline.
const READS_PER_TURN: usize = 4;

/// Reads what's available (a bounded amount); returns whether the stream ended (EOF or an
/// error).
fn drain(f: &mut File, buf: &mut [u8], mut sink: impl FnMut(&[u8])) -> bool {
    for _ in 0..READS_PER_TURN {
        match f.read(buf) {
            Ok(0) => return true,
            Ok(n) => sink(&buf[..n]),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return false,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return true,
        }
    }
    false
}

fn answer(
    conversation: &mut dyn ProbeConversation,
    frame: &Frame,
    pending: &mut Vec<u8>,
    done_at: &mut Option<Instant>,
) {
    if done_at.is_some() {
        return;
    }
    let reply = conversation.on_frame(frame);
    for line in &reply.write {
        pending.extend_from_slice(line.as_bytes());
    }
    if reply.done {
        *done_at = Some(Instant::now());
    }
}

/// Output kept up to [`PROBE_OUTPUT_CAP`].
#[derive(Default)]
struct Capped {
    bytes: Vec<u8>,
}

impl Capped {
    fn push(&mut self, chunk: &[u8]) {
        let room = PROBE_OUTPUT_CAP.saturating_sub(self.bytes.len());
        self.bytes
            .extend_from_slice(&chunk[..chunk.len().min(room)]);
    }
}
