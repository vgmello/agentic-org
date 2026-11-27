//! Bounded capture of one Git child process (`TASK-92.1`): a stdout byte cap, a stderr cap
//! that keeps draining so Git never blocks on a full pipe, and a wall-clock timeout. The one
//! place `org-git` waits on a child under limits; [`crate::GitRunner::run_local_bounded`] is
//! its only production caller.

use std::io::{self, Read};
use std::process::{Child, ChildStderr, ExitStatus};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

/// Largest stderr text a bounded invocation keeps (the rest is drained and dropped).
pub const MAX_BOUNDED_STDERR_BYTES: usize = 64 * 1024;

/// Longest timeout honoured; longer requests are clamped so the deadline never overflows.
const MAX_TIMEOUT: Duration = Duration::from_secs(60 * 60);

/// How often a child that already closed stdout is polled for its exit.
const EXIT_POLL: Duration = Duration::from_millis(5);

/// How long stderr is awaited once Git has exited (its pipe normally closes at once).
const STDERR_GRACE: Duration = Duration::from_secs(1);

/// Bounds for one [`crate::GitRunner::run_local_bounded`] invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputLimits {
    /// Most raw stdout bytes kept; Git is killed as soon as it writes more.
    pub max_stdout: usize,
    /// Wall-clock limit (clamped to one hour); Git is killed when it runs longer.
    pub timeout: Duration,
}

/// Captured output of a bounded, local-only Git invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedOutput {
    /// Exit code; `-1` when Git was killed (for example after `stdout_truncated`).
    pub status: i32,
    /// Raw stdout, at most [`OutputLimits::max_stdout`] bytes.
    pub stdout: Vec<u8>,
    /// Git wrote more than [`OutputLimits::max_stdout`] bytes and was killed.
    pub stdout_truncated: bool,
    /// Lossy stderr text, at most [`MAX_BOUNDED_STDERR_BYTES`] bytes.
    pub stderr: String,
}

/// Why a bounded capture produced no output.
#[derive(Debug)]
pub(crate) enum CaptureError {
    Io(io::Error),
    TimedOut,
}

/// Captures `child` (spawned with piped stdout and stderr) under `limits`, always reaping it.
///
/// Never waits on a reader thread past the deadline: a process Git started could keep a pipe
/// open after Git is killed, and its reader then finishes on its own, detached.
pub(crate) fn capture(
    mut child: Child,
    limits: OutputLimits,
) -> Result<BoundedOutput, CaptureError> {
    let deadline = Instant::now() + limits.timeout.min(MAX_TIMEOUT);
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        reap(&mut child);
        return Err(CaptureError::Io(io::Error::other(
            "git stdout and stderr must be piped",
        )));
    };
    let max_stdout = limits.max_stdout;
    let readers = spawn_reader("git-stderr", move || read_stderr(stderr)).and_then(|stderr| {
        let stdout = spawn_reader("git-stdout", move || read_capped(stdout, max_stdout))?;
        Ok((stdout, stderr))
    });
    let (stdout_rx, stderr_rx) = match readers {
        Ok(readers) => readers,
        Err(e) => {
            reap(&mut child);
            return Err(CaptureError::Io(e));
        }
    };

    let stdout = match stdout_rx.recv_timeout(remaining(deadline)) {
        Ok(read) => read,
        Err(RecvTimeoutError::Timeout) => {
            reap(&mut child);
            return Err(CaptureError::TimedOut);
        }
        Err(RecvTimeoutError::Disconnected) => Err(io::Error::other("git stdout reader failed")),
    };
    let over_cap = matches!(&stdout, Ok(bytes) if bytes.len() > max_stdout);
    let status = if over_cap || stdout.is_err() {
        // Over the cap the reader stopped draining, so Git would block: kill it.
        reap(&mut child);
        -1
    } else {
        match wait_until(&mut child, deadline).map_err(CaptureError::Io)? {
            Some(exit) => exit.code().unwrap_or(-1),
            None => return Err(CaptureError::TimedOut),
        }
    };
    let mut stdout = stdout.map_err(CaptureError::Io)?;
    stdout.truncate(max_stdout);
    let stderr = stderr_rx
        .recv_timeout(remaining(deadline).min(STDERR_GRACE))
        .unwrap_or_default();
    Ok(BoundedOutput {
        status,
        stdout,
        stdout_truncated: over_cap,
        stderr,
    })
}

/// Runs `f` on a named thread and answers a receiver for its result.
fn spawn_reader<T: Send + 'static>(
    name: &str,
    f: impl FnOnce() -> T + Send + 'static,
) -> io::Result<Receiver<T>> {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || {
            let _ = tx.send(f());
        })?;
    Ok(rx)
}

fn remaining(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

/// Kills and reaps `child`, ignoring failures (it may already have exited).
fn reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Waits for `child` to exit until `deadline`; past it, kills and reaps it and answers `None`.
fn wait_until(child: &mut Child, deadline: Instant) -> io::Result<Option<ExitStatus>> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(Some(status)),
            Ok(None) => {}
            Err(e) => {
                reap(child);
                return Err(e);
            }
        }
        let now = Instant::now();
        if now >= deadline {
            reap(child);
            return Ok(None);
        }
        thread::sleep(EXIT_POLL.min(deadline - now));
    }
}

/// Reads at most `max + 1` bytes, so a result longer than `max` means "more was written".
pub(crate) fn read_capped(src: impl Read, max: usize) -> io::Result<Vec<u8>> {
    let limit = u64::try_from(max).unwrap_or(u64::MAX).saturating_add(1);
    let mut buf = Vec::new();
    src.take(limit).read_to_end(&mut buf)?;
    Ok(buf)
}

/// Keeps the first [`MAX_BOUNDED_STDERR_BYTES`] of stderr and drains the rest, so Git never
/// blocks writing stderr while stdout is still being read.
fn read_stderr(mut src: ChildStderr) -> String {
    let kept = read_capped(&mut src, MAX_BOUNDED_STDERR_BYTES).unwrap_or_default();
    let _ = io::copy(&mut src, &mut io::sink());
    let cut_short = kept.len() > MAX_BOUNDED_STDERR_BYTES;
    decode_bounded(&kept, MAX_BOUNDED_STDERR_BYTES, cut_short).0
}

/// Decodes at most `max` bytes of captured output as text without ever splitting a
/// character. `cut_short` says the capture already stopped the output early (as does a
/// `bytes` longer than `max`): a multi-byte sequence the cut split is then dropped whole.
/// Other invalid bytes become U+FFFD, and text that grew past `max` through replacement is
/// cut at a character boundary. Answers the text and whether anything was cut.
pub(crate) fn decode_bounded(bytes: &[u8], max: usize, cut_short: bool) -> (String, bool) {
    let mut cut = cut_short || bytes.len() > max;
    let mut kept = &bytes[..bytes.len().min(max)];
    if cut {
        kept = &kept[..complete_prefix_len(kept)];
    }
    let mut text = String::from_utf8_lossy(kept).into_owned();
    if text.len() > max {
        text.truncate(text.floor_char_boundary(max));
        cut = true;
    }
    (text, cut)
}

/// Length of `bytes` without a trailing UTF-8 sequence that a byte cap cut short.
fn complete_prefix_len(bytes: &[u8]) -> usize {
    let len = bytes.len();
    for back in 1..=len.min(4) {
        let byte = bytes[len - back];
        if byte & 0xC0 == 0x80 {
            continue; // continuation byte: keep looking for its lead byte
        }
        let needed = match byte {
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF7 => 4,
            _ => 1,
        };
        return if needed > back { len - back } else { len };
    }
    len
}

#[cfg(test)]
#[path = "bounded_tests.rs"]
mod tests;
