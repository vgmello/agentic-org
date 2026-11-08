//! Running a probe ([`ProbeRun`]): a short-lived process on pipes, with `/` as its cwd, no
//! files written, its stdin fed, its output captured (capped), a timeout, and its process
//! group killed at the end.
//!
//! Why it exists: preflight and quota checks run the runtime's own binary, so they go through
//! the same spawn (allowlisted environment, only fds 0-2, own group) as sessions.
//!
//! The two shapes the port defines:
//! - [`ProbeRun::OneShot`]: the plan's `stdin_lines`, then stdin is closed; the whole output
//!   comes back as a [`ProbeOutput`] for `parse_probe`.
//! - [`ProbeRun::Conversation`]: the plan's `stdin_lines`, then the [`ProbeSession`] drives
//!   stdin from stdout lines (and its own ticks) until its first [`ProbeStep::Done`], which
//!   is the result. Then stdin is closed and the process stopped. Without a `Done`, stdout
//!   ending gives the session's own conclusion, and the timeout gives "timed out".
//!
//! One thread, `poll(2)`, non-blocking pipes, a bounded amount read per turn: no reader can
//! be left blocked on a pipe an escaped grandchild holds, and a probe flooding its output
//! still meets its deadline.

use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsFd, AsRawFd};
use std::path::Path;
use std::time::{Duration, Instant};

use org_runtime::{
    Frame, LineFramer, ProbeOutput, ProbePlan, ProbeResult, ProbeRun, ProbeSession, ProbeStep,
    Stdio, Unavailable,
};

use crate::child::{spawn_inner, Spawn};
use crate::env::Env;
use crate::error::{Error, Result};
use crate::sys;

/// The most a probe's stdout or stderr keeps; the rest is read and dropped.
pub const PROBE_OUTPUT_CAP: usize = 1 << 20;

/// How long output is still read after the probe exits (its pipes can outlive it briefly).
const DRAIN_AFTER_EXIT: Duration = Duration::from_millis(250);

/// How long a conversation's process gets to exit after its `Done` (stdin is closed then),
/// before its group is killed.
const DONE_GRACE: Duration = Duration::from_millis(500);

/// The longest single `poll` wait, so the exit is noticed promptly.
const POLL_SLICE: Duration = Duration::from_millis(20);

/// At most this many reads per stream per turn of the loop.
const READS_PER_TURN: usize = 4;

/// What a probe came to.
#[derive(Debug)]
pub enum ProbeOutcome {
    /// A one-shot probe's output, for `parse_probe`.
    OneShot(ProbeOutput),
    /// A conversation's result.
    Conversation {
        /// The session's `Done`; or its conclusion when stdout ended first; or
        /// [`Unavailable::Other`] "timed out".
        result: ProbeResult,
        /// What the probe wrote to stderr (capped), for the logs.
        stderr: Vec<u8>,
        /// Its exit code, if it exited on its own with one.
        exit_code: Option<i32>,
    },
}

/// Runs a probe with `env` plus its plan's `env_add`. At the end what's left of the probe's
/// group is killed, and the probe is reaped (with orphan cleanup if `env` has a mark).
pub fn run_probe(run: ProbeRun, env: &Env) -> Result<ProbeOutcome> {
    match run {
        ProbeRun::OneShot(plan) => {
            let r = drive(&plan, env, None)?;
            let mut out = ProbeOutput::new(r.stdout, r.stderr, r.exit_code);
            out.timed_out = r.timed_out;
            Ok(ProbeOutcome::OneShot(out))
        }
        ProbeRun::Conversation { plan, mut session } => {
            let r = drive(&plan, env, Some(session.as_mut()))?;
            let result = match r.conclusion {
                Some(result) => result,
                None => ProbeResult::Unavailable {
                    reason: Unavailable::Other {
                        detail: "timed out".to_owned(),
                    },
                },
            };
            Ok(ProbeOutcome::Conversation {
                result,
                stderr: r.stderr,
                exit_code: r.exit_code,
            })
        }
    }
}

struct Driven {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    exit_code: Option<i32>,
    timed_out: bool,
    /// A conversation's result (its `Done`, or its conclusion when stdout ended first).
    conclusion: Option<ProbeResult>,
}

/// The conversation's state inside the loop.
struct Talk<'s> {
    session: &'s mut dyn ProbeSession,
    framer: LineFramer,
    done: Option<(ProbeResult, Instant)>,
}

impl Talk<'_> {
    fn apply(&mut self, steps: Vec<ProbeStep>, pending: &mut Vec<u8>, now: Instant) {
        for step in steps {
            if self.done.is_some() {
                return;
            }
            match step {
                ProbeStep::Write(line) => pending.extend_from_slice(line.as_bytes()),
                ProbeStep::Done(result) => self.done = Some((result, now)),
            }
        }
    }

    fn frames(&mut self, chunk: &[u8], pending: &mut Vec<u8>) {
        for frame in self.framer.push(chunk) {
            self.frame(frame, pending);
        }
    }

    fn frame(&mut self, frame: Frame, pending: &mut Vec<u8>) {
        match frame {
            Frame::Line(line) => {
                if self.done.is_none() {
                    let now = Instant::now();
                    let steps = self.session.on_line(&line, now);
                    self.apply(steps, pending, now);
                }
            }
            // An over-long line is dropped, not passed.
            Frame::Truncated(_) => {}
        }
    }
}

fn drive(plan: &ProbePlan, env: &Env, session: Option<&mut dyn ProbeSession>) -> Result<Driven> {
    let env = env.with_additions(&plan.env_add)?;
    let mut child = spawn_inner(Spawn {
        program: &plan.program,
        args: &plan.args,
        cwd: Path::new("/"),
        env: &env,
        stdio: Stdio::Piped,
        cleanup: Vec::new(),
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
    let mut talk = session.map(|session| Talk {
        session,
        framer: LineFramer::new(),
        done: None,
    });
    if let Some(t) = &mut talk {
        let steps = t.session.start(started);
        t.apply(steps, &mut pending, started);
    }
    // A one-shot closes stdin after its lines; a conversation keeps it open until `Done`.
    let mut close_when_flushed = talk.is_none();
    let mut out = Capped::default();
    let mut err = Capped::default();
    let mut stdout_closed_conclusion: Option<ProbeResult> = None;
    let mut exited_at: Option<Instant> = None;
    let mut timed_out = false;
    let mut buf = vec![0u8; 64 * 1024];

    loop {
        let now = Instant::now();
        // The session's own deadlines.
        if let Some(t) = talk.as_mut().filter(|t| t.done.is_none()) {
            if t.session.next_deadline().is_some_and(|d| now >= d) {
                let steps = t.session.on_tick(now);
                t.apply(steps, &mut pending, now);
            }
        }
        let done_at = talk
            .as_ref()
            .and_then(|t| t.done.as_ref().map(|(_, at)| *at));
        if done_at.is_some() {
            close_when_flushed = true;
        }
        if pending.is_empty() && close_when_flushed {
            stdin = None;
        }
        if exited_at.is_none() && child.has_exited()? {
            exited_at = Some(now);
        }
        if let Some(at) = exited_at {
            if (stdout.is_none() && stderr.is_none()) || now >= at + DRAIN_AFTER_EXIT {
                break;
            }
        } else if done_at.is_some_and(|at| now >= at + DONE_GRACE) {
            break;
        } else if now >= deadline {
            // A conversation that's done isn't timed out.
            timed_out = done_at.is_none();
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
        let mut until = exited_at
            .map(|at| at + DRAIN_AFTER_EXIT)
            .unwrap_or(deadline);
        if let Some(d) = talk
            .as_ref()
            .filter(|t| t.done.is_none())
            .and_then(|t| t.session.next_deadline())
        {
            until = until.min(d);
        }
        let limit = until.saturating_duration_since(now).min(POLL_SLICE);
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
                            if let Some(t) = &mut talk {
                                t.frames(chunk, &mut pending);
                            }
                        });
                    }
                    if eof {
                        stdout = None;
                        if let Some(t) = &mut talk {
                            if let Some(frame) = t.framer.finish() {
                                t.frame(frame, &mut pending);
                            }
                            if t.done.is_none() {
                                stdout_closed_conclusion =
                                    Some(t.session.on_stdout_closed(Instant::now()));
                            }
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
        if stdin.is_none() {
            pending.clear();
        }
        // A conversation whose stdout ended without a `Done` has concluded: no more ticks,
        // and its process is stopped now rather than waited for.
        if stdout_closed_conclusion.is_some() {
            break;
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

    let conclusion = match talk {
        Some(t) => match t.done {
            Some((result, _)) => Some(result),
            // Only the timeout leaves no conclusion.
            None if timed_out => None,
            // Stdout ended (or the probe exited and its output was drained) without a `Done`:
            // the session concludes with what it has.
            None => Some(
                stdout_closed_conclusion
                    .unwrap_or_else(|| t.session.on_stdout_closed(Instant::now())),
            ),
        },
        None => None,
    };
    Ok(Driven {
        stdout: out.bytes,
        stderr: err.bytes,
        exit_code: exit.info.code,
        timed_out,
        conclusion,
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

/// Reads what's available, a bounded amount; returns whether the stream ended (EOF or an
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
