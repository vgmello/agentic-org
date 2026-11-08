//! The take-over terminal's master side.
//!
//! Why it exists: on take-over a person drives the runtime's own interactive UI, so the
//! child gets a pseudo-terminal instead of pipes. The office relays the master's bytes to the
//! lease holder and applies its resizes.
//!
//! How the child gets it: the child is spawned with `POSIX_SPAWN_SETSID`, so it leads a new
//! session, and its first file action opens the slave **by path**, without `O_NOCTTY`, as fd
//! 0 (dup'd to 1 and 2). That's what POSIX requires for a session leader to acquire a
//! controlling terminal. Verified on macOS 26.7.2 (`tests/pty.rs`): `ps` shows the slave as
//! the child's terminal and the child in its foreground group, `/dev/tty` opens in it, and a
//! `^C` written to the master kills it with `SIGINT`. (macOS in fact gave the child the
//! controlling tty in every variant tried: with `O_NOCTTY` on that open, and with the slave
//! passed by `dup2` instead; the open without `O_NOCTTY` is kept as the portable form.)
//!
//! This process opens the slave too, with `O_NOCTTY`, only to set the initial size (macOS
//! refuses `TIOCSWINSZ` on a master whose slave was never opened) and only until the spawn
//! returns, so the terminal never becomes this process's controlling terminal and reading the
//! master sees the child's end.

use std::fs::File;
use std::os::fd::AsFd;

use crate::error::{Error, Result};
use crate::sys;

/// The master side of a child's terminal. Read it for the child's output, write it for its
/// input (`Read` and `Write` work on `&File`, so one thread can read while another writes).
///
/// When the child (and everything holding the slave) has exited, reading the master fails
/// with `EIO` on macOS rather than returning end-of-file.
#[derive(Debug)]
pub struct Pty {
    master: File,
}

impl Pty {
    pub(crate) fn new(master: File) -> Self {
        Pty { master }
    }

    /// The master, for the caller's I/O.
    pub fn master(&self) -> &File {
        &self.master
    }

    /// Takes the master out, for a caller that wants to own it.
    pub fn into_master(self) -> File {
        self.master
    }

    /// Sets the terminal's size (`TIOCSWINSZ`); the child's foreground group gets `SIGWINCH`.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        sys::set_winsize(self.master.as_fd(), cols, rows)
            .map_err(|e| Error::io("resize the terminal", e))
    }

    /// The terminal's size, `(cols, rows)`.
    pub fn size(&self) -> Result<(u16, u16)> {
        sys::get_winsize(self.master.as_fd()).map_err(|e| Error::io("read the terminal size", e))
    }
}
