//! Helpers for tests of commands: an [`ExecutionContext`] whose streams are captured in
//! memory, with a chosen terminal and environment. Public behind `test-support` so the
//! consumer's tests use it too.

use std::ffi::OsString;
use std::io::{self, Write};
use std::sync::{Arc, Mutex, PoisonError};

use super::{ExecutionContext, Modes};
use crate::terminal::Terminal;

/// A cloneable in-memory stream: every clone writes into, and reads, the same buffer.
#[derive(Clone, Debug, Default)]
pub struct SharedBuf(Arc<Mutex<Vec<u8>>>);

impl SharedBuf {
    pub fn new() -> Self {
        Self::default()
    }

    /// Everything written so far (invalid UTF-8 replaced).
    pub fn text(&self) -> String {
        let bytes = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

impl Write for SharedBuf {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A stream whose reader went away: every write and flush fails with `BrokenPipe`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ClosedPipe;

impl Write for ClosedPipe {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::ErrorKind::BrokenPipe.into())
    }
}

/// The two captured streams of a [`captured`] context.
#[derive(Clone, Debug, Default)]
pub struct Captured {
    pub stdout: SharedBuf,
    pub stderr: SharedBuf,
}

/// A context with `modes` and `terminal` whose environment is exactly `env` and whose
/// streams are captured.
pub fn captured(
    modes: Modes,
    terminal: Terminal,
    env: &[(&str, &str)],
) -> (ExecutionContext, Captured) {
    let streams = Captured::default();
    let lookup = |name: &str| {
        env.iter()
            .rev()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| OsString::from(value))
    };
    let context = ExecutionContext::with_streams(
        modes,
        terminal,
        &lookup,
        Box::new(streams.stdout.clone()),
        Box::new(streams.stderr.clone()),
    );
    (context, streams)
}

/// Every stream a terminal, `width` columns wide.
pub fn tty(width: Option<usize>) -> Terminal {
    Terminal {
        stdin_tty: true,
        stdout_tty: true,
        stderr_tty: true,
        width,
    }
}
