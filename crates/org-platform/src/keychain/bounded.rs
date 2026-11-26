//! Runs a program that prints a secret, and captures its standard output bounded in size
//! and in time. Nothing here depends on the system (only `std::process`), so it is compiled
//! and tested on every target; the adapter decides which program runs.

use std::ffi::OsStr;
use std::io::{ErrorKind, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use super::SecretBytes;

/// How often the program is checked for having exited.
const POLL_INTERVAL: Duration = Duration::from_millis(10);
/// How long the reader may take to reach the end of the output once the program is gone,
/// beyond what is left of the timeout.
const READER_GRACE: Duration = Duration::from_millis(250);
/// The size of each read.
const SCRATCH_BYTES: usize = 4096;

/// What the reader made of the program's output.
#[derive(Debug)]
enum Drained {
    /// All of it: at most `max_bytes`.
    Complete(SecretBytes),
    /// More than `max_bytes`. What was read is wiped.
    Oversize,
    /// The buffer could not be reserved, or a read failed. What was read is wiped.
    Failed,
}

/// Runs `program` with `args` and returns its standard output exactly as printed, when the
/// program exits successfully within `timeout` having printed at most `max_bytes`. Anything
/// else gives `None`: the program cannot start, exits unsuccessfully, runs past `timeout`,
/// prints more, or its output cannot be read. Every byte read is wiped on the way.
///
/// Standard input and standard error are null; the environment is inherited. A separate
/// thread reads standard output from the moment the program starts, into a buffer of
/// `max_bytes + 1` reserved up front, and past that keeps reading and discarding to the end,
/// so the program never blocks on a full pipe. A program still running at `timeout`, or one
/// that cannot be checked, is killed; it is always reaped, never left a zombie. The reader is
/// then joined, unless something the program started still holds the output open; it is
/// left to finish on its own then, and its buffers are wiped when it does.
pub(crate) fn run_bounded_capture(
    program: &Path,
    args: &[&OsStr],
    max_bytes: usize,
    timeout: Duration,
) -> Option<SecretBytes> {
    let deadline = Instant::now().checked_add(timeout);
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let Some(stdout) = child.stdout.take() else {
        kill_and_reap(&mut child);
        return None;
    };
    let (sender, receiver) = mpsc::sync_channel(1);
    let reader = thread::Builder::new()
        .name("bounded-capture".to_owned())
        .spawn(move || {
            // When the receiver is gone the result is dropped, and so wiped.
            let _ = sender.send(drain(stdout, max_bytes));
        });
    let Ok(reader) = reader else {
        kill_and_reap(&mut child);
        return None;
    };

    let succeeded = wait_for_exit(&mut child, deadline);
    // The program is reaped. Unless something it started holds the pipe open, the reader
    // reaches the end of the output at once.
    let patience = if succeeded {
        remaining(deadline).max(READER_GRACE)
    } else {
        READER_GRACE
    };
    let drained = receiver.recv_timeout(patience);
    if !matches!(drained, Err(RecvTimeoutError::Timeout)) {
        // The reader has sent its result, or has died: joining it cannot block.
        let _ = reader.join();
    }
    match drained {
        Ok(Drained::Complete(bytes)) if succeeded => Some(bytes),
        _ => None,
    }
}

/// Waits for the program until `deadline` and says whether it exited successfully. On
/// timeout, or when it cannot be checked, it is killed. Either way it has been reaped when
/// this returns.
fn wait_for_exit(child: &mut Child, deadline: Option<Instant>) -> bool {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => {
                let left = remaining(deadline);
                if left.is_zero() {
                    kill_and_reap(child);
                    return false;
                }
                thread::sleep(POLL_INTERVAL.min(left));
            }
            Err(_) => {
                kill_and_reap(child);
                return false;
            }
        }
    }
}

/// What is left until `deadline`; without one (a timeout too long to add to the clock),
/// forever.
fn remaining(deadline: Option<Instant>) -> Duration {
    deadline.map_or(Duration::MAX, |deadline| {
        deadline.saturating_duration_since(Instant::now())
    })
}

/// Kills the program, which may already have exited, and reaps it.
fn kill_and_reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Reads `output` to its end: the first `max_bytes + 1` bytes into a buffer reserved up
/// front, and the rest through the scratch buffer only, discarded.
fn drain(mut output: impl Read, max_bytes: usize) -> Drained {
    // One byte more than allowed tells "exactly `max_bytes`" from "more".
    let limit = max_bytes.saturating_add(1);
    let mut captured = SecretBytes::with_capacity(limit);
    let mut scratch = SecretBytes::zeroed(SCRATCH_BYTES);
    let mut oversize = false;
    loop {
        let read = match output.read(scratch.as_mut_slice()) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return Drained::Failed,
        };
        if let Some(captured) = captured.as_mut().filter(|_| !oversize) {
            let wanted = read.min(limit - captured.as_slice().len());
            captured.extend_within_capacity(&scratch.as_slice()[..wanted]);
            oversize = captured.as_slice().len() > max_bytes;
        }
    }
    match captured {
        Some(_) if oversize => Drained::Oversize,
        Some(captured) => Drained::Complete(captured),
        None => Drained::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    /// 256 KiB, four times the default pipe buffer on Linux and sixteen times macOS's, in
    /// 64-byte lines printed by the shell itself.
    const LARGE_OUTPUT: &str = "i=0; while [ $i -lt 4096 ]; do printf '%064d' 0; i=$((i+1)); done";
    const LARGE_OUTPUT_BYTES: usize = 256 * 1024;

    fn sh(script: &str, max_bytes: usize, timeout: Duration) -> (Option<SecretBytes>, Duration) {
        let started = Instant::now();
        let output = run_bounded_capture(
            Path::new("/bin/sh"),
            &[OsStr::new("-c"), OsStr::new(script)],
            max_bytes,
            timeout,
        );
        (output, started.elapsed())
    }

    fn captured(script: &str, max_bytes: usize) -> Option<Vec<u8>> {
        sh(script, max_bytes, Duration::from_secs(10))
            .0
            .map(|bytes| bytes.as_slice().to_vec())
    }

    #[test]
    fn output_is_returned_exactly_as_printed() {
        assert_eq!(
            captured("printf ' tok\\nen \\n'", 64).as_deref(),
            Some(&b" tok\nen \n"[..])
        );
        // Standard input is null, so a program reading it sees its end at once.
        assert_eq!(captured("cat", 64).as_deref(), Some(&b""[..]));
    }

    #[test]
    fn the_environment_is_inherited() {
        let expected = std::env::var_os("HOME")
            .map_or_else(|| b"unset".to_vec(), |home| home.into_encoded_bytes());
        assert_eq!(
            captured("printf %s \"${HOME-unset}\"", 4096),
            Some(expected)
        );
    }

    #[test]
    fn exactly_max_bytes_is_accepted_and_one_more_is_refused() {
        assert_eq!(captured("printf abcd", 4).as_deref(), Some(&b"abcd"[..]));
        assert_eq!(captured("printf abcde", 4), None);
        assert_eq!(captured(":", 0).as_deref(), Some(&b""[..]));
        assert_eq!(captured("printf a", 0), None);
    }

    #[test]
    fn output_larger_than_the_pipe_buffer_is_refused_without_stalling() {
        // The program prints far more than the pipe holds. Were the output read only after
        // the program exits, it would block on the full pipe until the timeout.
        let (output, took) = sh(LARGE_OUTPUT, 1024, Duration::from_secs(10));
        assert!(output.is_none());
        assert!(took < Duration::from_secs(2), "took {took:?}");
    }

    #[test]
    fn output_larger_than_the_pipe_buffer_within_the_limit_is_returned_whole() {
        let (output, took) = sh(LARGE_OUTPUT, LARGE_OUTPUT_BYTES, Duration::from_secs(10));
        let output = output.expect("the whole output");
        assert_eq!(output.as_slice().len(), LARGE_OUTPUT_BYTES);
        assert!(output.as_slice().iter().all(|&byte| byte == b'0'));
        assert!(took < Duration::from_secs(2), "took {took:?}");
    }

    #[test]
    fn an_unsuccessful_program_gives_nothing_even_with_output() {
        assert_eq!(captured("printf secret; exit 3", 64), None);
        assert_eq!(captured("kill -TERM $$", 64), None);
    }

    #[test]
    fn a_program_past_its_timeout_is_killed_promptly() {
        let (output, took) = sh("exec sleep 30", 64, Duration::from_millis(200));
        assert!(output.is_none());
        assert!(took < Duration::from_secs(2), "took {took:?}");
    }

    #[test]
    fn a_descendant_holding_the_output_open_cannot_hold_the_caller() {
        // The shell exits at once, successfully, but its background `sleep` keeps standard
        // output open, so the reader never reaches the end.
        let (output, took) = sh("sleep 5 & printf partial", 64, Duration::from_millis(300));
        assert!(output.is_none());
        assert!(took < Duration::from_secs(2), "took {took:?}");
    }

    #[test]
    fn a_missing_program_gives_nothing() {
        let output = run_bounded_capture(
            Path::new("/nonexistent/org-platform-keychain-test"),
            &[],
            64,
            Duration::from_secs(1),
        );
        assert!(output.is_none());
    }

    /// Gives `chunks` in order, one per read (an error as an error), then the end.
    struct Script(Vec<Result<&'static str, ErrorKind>>);

    impl Read for Script {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.0.is_empty() {
                return Ok(0);
            }
            let chunk = self.0.remove(0).map_err(io::Error::from)?;
            buf[..chunk.len()].copy_from_slice(chunk.as_bytes());
            Ok(chunk.len())
        }
    }

    #[test]
    fn drain_keeps_reading_past_the_limit_and_reports_oversize() {
        let Drained::Complete(bytes) = drain(Script(vec![Ok("ab"), Ok("cd")]), 4) else {
            panic!("four bytes fit in four");
        };
        assert_eq!(bytes.as_slice(), b"abcd");
        assert!(matches!(
            drain(Script(vec![Ok("abc"), Ok("de"), Ok("fgh")]), 4),
            Drained::Oversize
        ));
    }

    #[test]
    fn drain_retries_an_interrupted_read_and_fails_on_any_other_error() {
        let read = Script(vec![Ok("ab"), Err(ErrorKind::Interrupted), Ok("c")]);
        let Drained::Complete(bytes) = drain(read, 8) else {
            panic!("an interrupted read is retried");
        };
        assert_eq!(bytes.as_slice(), b"abc");
        assert!(matches!(
            drain(Script(vec![Ok("ab"), Err(ErrorKind::BrokenPipe)]), 8),
            Drained::Failed
        ));
    }

    #[test]
    fn drain_fails_when_the_buffer_cannot_be_reserved() {
        assert!(matches!(
            drain(Script(vec![Ok("ab")]), usize::MAX),
            Drained::Failed
        ));
    }
}
