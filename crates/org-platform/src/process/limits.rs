//! Process-wide limits a consumer sets once, first thing in `main`: no core dumps, and a
//! file mode creation mask that keeps what the process makes its user's alone. The same
//! calls on every system.

use std::io;

/// Sets the soft and the hard core-dump limit of this process to 0, and nothing else: for
/// an executable that is not closed to its user's other processes ([`super::harden_self`]) but
/// must never write its memory to disk (the office worker, first thing in `main`). The
/// same call on every system.
///
/// A system without an adapter refuses, as [`super::harden_self`] does there: the caller
/// does not go on. Each adapter (`macos.rs`, `linux.rs`, `refused.rs`) answers for its
/// system; no platform predicate is written here.
pub fn disable_core_dumps() -> io::Result<()> {
    super::native::disable_core_dumps()
}

/// Sets this process's file mode creation mask to `077`, so that what it makes is its
/// user's alone whatever mask started it (a container runtime starts a process with
/// `022`). Call it before any thread exists, so that every thread has it. The same call on
/// every system; it cannot fail.
pub fn owner_only_umask() {
    // SAFETY: umask has no preconditions, takes a plain mode and cannot fail.
    unsafe {
        libc::umask(0o077);
    }
}
