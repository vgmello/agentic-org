//! A terminal's settings (`tcgetattr`, `tcsetattr`) and the local calendar time.

use std::io;
use std::os::fd::{AsFd, AsRawFd};

use super::check;

/// The settings of the terminal `fd` (`tcgetattr`).
pub fn terminal_attributes(fd: impl AsFd) -> io::Result<libc::termios> {
    // SAFETY: termios is plain data for which all-zero bytes are a valid value.
    let mut settings: libc::termios = unsafe { std::mem::zeroed() };
    // SAFETY: `fd` is borrowed from its owner; `settings` is a live, exclusively borrowed
    // termios that tcgetattr writes.
    check(unsafe { libc::tcgetattr(fd.as_fd().as_raw_fd(), &mut settings) })?;
    Ok(settings)
}

/// Apply `settings` to the terminal `fd` now (`tcsetattr` with `TCSANOW`).
pub fn set_terminal_attributes(fd: impl AsFd, settings: &libc::termios) -> io::Result<()> {
    // SAFETY: `fd` is borrowed from its owner; `settings` is an initialized termios that
    // tcsetattr only reads.
    check(unsafe { libc::tcsetattr(fd.as_fd().as_raw_fd(), libc::TCSANOW, settings) }).map(drop)
}

/// The local calendar time of `seconds` since the Unix epoch (`localtime_r`), or `None`
/// when the system cannot represent it.
pub fn local_time(seconds: libc::time_t) -> Option<libc::tm> {
    // SAFETY: tm is plain data for which all-zero bytes are a valid value.
    let mut broken_down: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: `seconds` is a live time_t it reads and `broken_down` a live, exclusively
    // borrowed tm it writes; the reentrant variant keeps no static state.
    let result = unsafe { libc::localtime_r(&seconds, &mut broken_down) };
    (!result.is_null()).then_some(broken_down)
}
