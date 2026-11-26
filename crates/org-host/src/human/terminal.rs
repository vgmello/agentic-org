//! Bounded secret input from the controlling terminal, with restoration before
//! pending termination/job-control signals are unblocked. Never reads stdin/env.
//! A pending termination signal or end of input cancels the entry with
//! [`PASSWORD_CANCELLED`], which callers turn into a cancellation.
use crate::cli_error::PASSWORD_CANCELLED;
use org_host_control::Password;
use org_platform::posix::{self, SignalSet};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::fd::AsRawFd,
    time::Duration,
};
/// The termination and job-control signals held off while the terminal echoes nothing.
const HELD: [libc::c_int; 5] = [
    libc::SIGINT,
    libc::SIGTERM,
    libc::SIGHUP,
    libc::SIGQUIT,
    libc::SIGTSTP,
];
/// The mask this thread had before [`Signals::block`], restored when dropped.
struct Signals(SignalSet);
impl Signals {
    fn block() -> Result<Self, String> {
        let held = SignalSet::of(&HELD).map_err(|_| "signal mask unavailable")?;
        let old = posix::block_signals(&held).map_err(|_| "signal mask unavailable")?;
        Ok(Self(old))
    }
    fn pending(&self) -> bool {
        posix::pending_signals().map_or(true, |set| HELD.iter().any(|&s| set.contains(s)))
    }
}
impl Drop for Signals {
    fn drop(&mut self) {
        // Restoring the previous mask on the same thread.
        let _ = posix::set_signal_mask(&self.0);
    }
}
/// The terminal's settings from before the entry, put back when dropped.
struct Restore<'a>(&'a File, libc::termios);
impl Drop for Restore<'_> {
    fn drop(&mut self) {
        // The terminal outlives the guard and the settings came from it.
        let _ = posix::set_terminal_attributes(self.0, &self.1);
    }
}
pub(super) fn password() -> Result<Password, String> {
    secret(b"Password: ")
}
pub(super) fn secret(label: &[u8]) -> Result<Password, String> {
    let tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|_| "password requires a controlling terminal")?;
    let signals = Signals::block()?;
    let old = posix::terminal_attributes(&tty).map_err(|_| "terminal unavailable")?;
    let mut hidden = old;
    hidden.c_lflag &= !libc::ECHO;
    posix::set_terminal_attributes(&tty, &hidden).map_err(|_| "cannot protect password entry")?;
    let restore = Restore(&tty, old);
    let mut io = &tty;
    let mut bytes = Vec::new();
    let result = (|| {
        io.write_all(label).map_err(|_| "terminal unavailable")?;
        io.flush().map_err(|_| "terminal unavailable")?;
        loop {
            if signals.pending() {
                return Err(PASSWORD_CANCELLED);
            }
            let mut slot = [posix::poll_slot(tty.as_raw_fd(), libc::POLLIN)];
            let ready = posix::poll(&mut slot, Some(Duration::from_millis(100)))
                .map_err(|_| "terminal read failed")?;
            if ready == 0 {
                continue;
            }
            let mut b = [0];
            // End of input (Ctrl+D at the prompt) cancels the entry, as Ctrl+C does.
            io.read_exact(&mut b).map_err(|error| {
                if error.kind() == std::io::ErrorKind::UnexpectedEof {
                    PASSWORD_CANCELLED
                } else {
                    "password input failed"
                }
            })?;
            if b[0] == b'\n' {
                break;
            }
            if bytes.len() >= 1024 {
                return Err("password exceeds input budget");
            }
            bytes.push(b[0]);
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| "password must be UTF-8")?;
        Password::new(text.into())
    })();
    crate::secrets::zero(&mut bytes);
    drop(restore);
    let _ = io.write_all(b"\n");
    drop(signals);
    result.map_err(str::to_owned)
}
