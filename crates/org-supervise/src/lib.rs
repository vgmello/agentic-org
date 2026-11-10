//! Synchronous process primitives. No protocol, storage or domain knowledge.
//!
//! Slice 1: explicit environment, piped spawning, generated files, identity and ownership.
//! The caller persists the mark before spawning and the identity before obtaining stdin.
//! It owns and drains stdout/stderr throughout the child's life; this crate never reads them.
//! Exit observation never reaps. Dropping the owner kills its group and reaps only that child.
//! Full graceful stopping, orphan discovery and PTYs are separate deliveries. In particular,
//! this slice must not run real harnesses whose tools detach from their process group.
//!
//! All processes in the embedding program must preserve child ownership: never install
//! SIGCHLD=SIG_IGN/SA_NOCLDWAIT or reap arbitrary children. Preparation checks the former;
//! detecting a stolen child disarms this owner before any further signal.

#![deny(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]
#![deny(clippy::disallowed_methods)]

#[cfg(not(any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))))]
compile_error!("org-supervise requires macOS or GNU/Linux");

mod child;
mod environment;
mod files;
mod platform;
pub mod runtime;
mod spawn;

pub use child::{ChildIdentity, ChildStdin, OwnedChild, Spawned};
pub use environment::{Environment, SearchPath};
pub use spawn::{PendingChild, PreparedSpawn, RecordedSpawn, SessionMark, SpawnSpec};

use std::io;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn unsupported(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, message)
}
