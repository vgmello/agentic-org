//! Keeping the host's own fds out of the servers it spawns.
//!
//! On macOS std creates a pipe, a socket or an accepted connection and marks it close-on-exec in
//! a second step (there is no `SOCK_CLOEXEC`, `pipe2` or `accept4`). A spawn between the two
//! steps would leak that fd into the new server: another office's stdin (it would never see EOF
//! when the host goes away), `host.sock`, or a client's connection. So every spawn, and every
//! such creation outside a spawn, holds [`SPAWN_LOCK`] across both steps. Files are opened with
//! `O_CLOEXEC` in one step and need no lock.
//!
//! This goes away once servers are spawned through `org-supervise`'s `posix_spawn` with
//! `POSIX_SPAWN_CLOEXEC_DEFAULT`, which closes every fd the child isn't given.

use std::sync::Mutex;

/// Held across every spawn, and across every two-step fd creation in the host.
pub(crate) static SPAWN_LOCK: Mutex<()> = Mutex::new(());
