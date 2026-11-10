//! Synchronous process primitives. No protocol, storage or domain knowledge.
//!
//! Explicit environment, piped spawning, generated files, owned stop/cleanup and probes.
//! The caller persists the mark before spawning and the identity before obtaining stdin.
//! It owns and drains stdout/stderr throughout the child's life; this crate never reads them.
//! Exit observation never reaps. Dropping the owner kills its group and reaps only that child.
//! Cleanup/reaping is explicit after exit. Orphan discovery and PTYs are later deliveries.
//! Real harnesses whose tools detach require that discovery before use.
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
mod cleanup;
mod environment;
mod files;
mod platform;
pub mod runtime;
mod spawn;
mod stop;

pub use child::{ChildIdentity, ChildStdin, OwnedChild, Spawned};
pub use cleanup::{CleanupFailure, CleanupReport, SocketCleanup};
pub use environment::{Environment, SearchPath};
pub use spawn::{PendingChild, PreparedSpawn, RecordedSpawn, SessionMark, SpawnSpec};
pub use stop::{
    SignalTarget, StopAction, StopFailure, StopMode, StopPolicy, StopReport, StopSignal, StopStep,
};

use std::io;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn unsupported(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, message)
}

#[cfg(test)]
#[path = "../tests/support/core_limit.rs"]
mod test_core_limit;
