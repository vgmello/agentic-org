//! Synchronous process primitives. No protocol, storage or domain knowledge.
//!
//! Explicit environment, piped spawning, generated files, owned stop/cleanup and probes.
//! The caller persists the mark before spawning and the identity before obtaining stdin.
//! It owns and drains stdout/stderr throughout the child's life; this crate never reads them.
//! Exit observation never reaps. Dropping the owner kills its group and reaps only that child.
//! Cleanup and reaping are explicit after exit. [`recovery`] provides native orphan
//! discovery on macOS and Linux. Real harnesses whose tools detach require recovery
//! integration before use. PTYs are a later delivery.
//!
//! This crate is the ownership policy. Everything it asks of the kernel goes through
//! `org_platform::process`, which has a macOS adapter, a GNU/Linux adapter and a
//! refusing one for every other Unix target: there the preparation check, the first
//! step of every spawn, probe and recovery, returns that refusal before anything is
//! started or signalled. Its plain POSIX calls (signals, reaping, directory-relative
//! files) are `org_platform::posix`'s, so this crate has no `unsafe` code.
//!
//! All processes in the embedding program must preserve child ownership: never install
//! SIGCHLD=SIG_IGN/SA_NOCLDWAIT or reap arbitrary children. Preparation checks the former;
//! detecting a stolen child disarms this owner before any further signal.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![deny(clippy::disallowed_methods)]

mod child;
mod cleanup;
mod environment;
mod files;
pub mod recovery;
pub mod runtime;
mod spawn;
mod stop;

pub use child::{ChildIdentity, ChildStdin, OwnedChild, SignalTarget, Spawned};
pub use cleanup::{CleanupFailure, CleanupReport, SocketCleanup};
pub use environment::{Environment, SearchPath};
pub use spawn::{PendingChild, PreparedSpawn, RecordedSpawn, SessionMark, SpawnSpec};
pub use stop::{StopAction, StopFailure, StopMode, StopPolicy, StopReport, StopSignal, StopStep};

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
