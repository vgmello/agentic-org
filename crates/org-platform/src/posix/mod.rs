//! Safe wrappers over the POSIX calls the workspace makes outside this crate's
//! capabilities: descriptors and readiness, directory-relative files, this process's
//! identity and limits, signals, the account database and terminals.
//!
//! These calls are the same on every system this crate builds for, so this module has no
//! adapters and no platform predicate (as [`crate::process::WrapperSignals`] has none). It
//! exists so that no other crate of the workspace needs `unsafe` (each declares
//! `#![forbid(unsafe_code)]`, which `tests/rust_hygiene.rs` checks): every `unsafe` block
//! is here, once, with the argument for it beside it. A system difference does not belong
//! here; it gets a capability with adapters.
//!
//! Each wrapper is safe to call with any argument:
//!
//! - A descriptor that a call changes, reads or writes through is borrowed from its owner
//!   ([`std::os::fd::AsFd`]); a new one comes back owned ([`std::os::fd::OwnedFd`]). A call
//!   that only asks about a number (its flags, whether it is open) takes a raw number, and
//!   a number that is not open is the kernel's `EBADF`.
//! - A name is a NUL-terminated [`std::ffi::CStr`]; a buffer is a slice whose length is
//!   passed with it; a record the system fills in is returned by value.
//! - A failure is the system's [`std::io::Error`]; nothing is retried unless the function
//!   says so.
//!
//! The consumer keeps its policy (which flags, modes, owners and signals it accepts);
//! these only make each call.

use std::io;
use std::os::fd::RawFd;
use std::time::Duration;

mod account;
mod child;
mod descriptor;
mod directory;
mod process;
mod signal;
mod terminal;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
#[cfg(test)]
mod tests;

pub use account::{
    account_by_name, account_by_uid, initialize_groups, set_gid, set_groups, set_uid, AccountEntry,
};
pub use child::{answer_from_child, fork_single_threaded, ChildAnswerError, ForkChild, Forked};
pub use descriptor::{
    bytes_waiting, descriptor_flags, duplicate_above, is_close_on_exec, is_open, poll, poll_slot,
    receive, reopen_standard_on_null, replace_standard, set_close_on_exec, set_nonblocking,
    set_socket_option, set_status_flags, socket_family, socket_option, status_flags, Standard,
};
pub use directory::{
    make_directory_at, make_fifo, open, open_at, remove_at, rename_at, status, status_at,
    status_of_number, Entries,
};
pub use process::{
    effective_gid, effective_uid, exit_immediately, kill, kill_group, open_max, page_size,
    process_exists, process_group, real_gid, real_uid, reap, resource_limit, session_id,
    set_resource_limit, set_umask, wait_pid, Limit, Resource,
};
pub use signal::{
    block_signals, default_disposition, ignore_signal, is_ignored, pending_signals,
    request_stop_on, set_signal_mask, stop_on_signal, stop_requested, wait_for_signal, SignalSet,
};
pub use terminal::{local_time, set_terminal_attributes, terminal_attributes};

/// Wait up to `tick` (at least a millisecond) for bytes, end of stream or a socket error
/// on a descriptor owned by the caller. A number that is not open is `EBADF`; an
/// interrupted wait is returned for the caller to handle.
pub fn poll_readable(fd: RawFd, tick: Duration) -> io::Result<bool> {
    if fd < 0 {
        return Err(io::Error::from_raw_os_error(libc::EBADF));
    }
    let mut slots = [poll_slot(fd, libc::POLLIN)];
    poll(&mut slots, Some(tick.max(Duration::from_millis(1))))?;
    readable(&slots[0])
}

/// Wait up to `timeout` for bytes, end of stream or an error on either of two descriptors
/// owned by the caller (for example a socket and a wake pipe), and say which is ready. A
/// zero timeout only checks.
pub fn poll_readable_either(
    fd_a: RawFd,
    fd_b: RawFd,
    timeout: Duration,
) -> io::Result<(bool, bool)> {
    if fd_a < 0 || fd_b < 0 {
        return Err(io::Error::from_raw_os_error(libc::EBADF));
    }
    let mut slots = [poll_slot(fd_a, libc::POLLIN), poll_slot(fd_b, libc::POLLIN)];
    poll(&mut slots, Some(timeout))?;
    if (slots[0].revents | slots[1].revents) & libc::POLLNVAL != 0 {
        return Err(io::Error::from_raw_os_error(libc::EBADF));
    }
    Ok((readable(&slots[0])?, readable(&slots[1])?))
}

fn readable(slot: &libc::pollfd) -> io::Result<bool> {
    if slot.revents & libc::POLLNVAL != 0 {
        return Err(io::Error::from_raw_os_error(libc::EBADF));
    }
    Ok(slot.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0)
}

/// A C call's status: a negative result is the thread's `errno`.
fn check(result: libc::c_int) -> io::Result<libc::c_int> {
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(result)
    }
}

/// A C call's byte count: a negative result is the thread's `errno`.
fn check_size(result: libc::ssize_t) -> io::Result<usize> {
    usize::try_from(result).map_err(|_| io::Error::last_os_error())
}
