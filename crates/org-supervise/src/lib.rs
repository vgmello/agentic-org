//! # org-supervise: process control for Agentic Org (macOS)
//!
//! The one place that spawns agent runtimes, their probes, office servers and take-over
//! terminals (`docs/design/agent-sessions.md`, "Processes and safety" and "Where the code
//! lives"). It owns process groups, the environment allowlist, the stop sequence, reaping,
//! the cleanup of a runtime's stale files in the zombie window, and orphan cleanup. It speaks
//! no runtime protocol: the pump in `org-office-sessions` owns the pipes once they're spawned.
//!
//! It carries out what `org_runtime` adapters plan ([`org_runtime::LaunchPlan`],
//! [`org_runtime::ProbePlan`]) and is used by `org-office-sessions`, `org-office-server` and
//! `org-office-host` (which reuses [`orphans`] from its watchdog).
//!
//! ## The pieces, and why each exists
//! - [`EnvBuilder`] / [`Env`]: decision 6's environment allowlist, built from named office
//!   values, so nothing of this process's environment reaches a child. `Env`'s `Debug` shows
//!   names only, so the session token can't reach a log.
//! - [`spawn_session`]: carries out a launch plan: writes its dirs and files without ever
//!   following a symlink ([`PlanTargets`]), then spawns. [`spawn`] spawns any other
//!   [`Command`] (an office server) the same way.
//! - [`Child`]: the pid, process group, pipes or [`Pty`], the stop sequence
//!   ([`Child::stop`]) and reaping ([`Child::reap`]) with the zombie-window cleanup
//!   ([`CleanupReport`]) and orphan cleanup.
//! - [`orphans`]: finds and kills a session's leftovers by group, tool session and
//!   `ORG_SESSION_MARK`, checking each process's identity right before each signal.
//! - [`run_probe`]: a probe ([`org_runtime::ProbeRun`]), one-shot or a conversation driven by
//!   its [`org_runtime::ProbeSession`], with a timeout, capped output and a group kill.
//! - [`remove_stale`]: the startup form of the stale-file cleanup, for a pid that's gone.
//!
//! ## Spawning guarantees (every child)
//! - `posix_spawn` with `POSIX_SPAWN_CLOEXEC_DEFAULT`: the child holds **only** fds 0, 1
//!   and 2 (pipes or the terminal slave), whatever other threads of this process are opening
//!   at that moment. That's why `std::process::Command` isn't used.
//! - The program runs by its absolute real path, never through a `PATH` lookup or a symlink.
//! - Its own process group (pipes) or session (terminal); an empty signal mask and default
//!   signal dispositions (Rust's ignored `SIGPIPE` doesn't leak in).
//! - Its environment is exactly the [`Env`] it's given (plus a plan's `env_add`).
//!
//! `unsafe` code lives in one private module (`sys`), with a `SAFETY` comment on every block;
//! the rest of the crate is safe Rust. On other platforms the crate is empty.

#![cfg(target_os = "macos")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

mod child;
mod cleanup;
mod env;
mod error;
mod files;
pub mod orphans;
mod probe;
mod pty;
mod sys;

pub use child::{spawn, spawn_session, Child, Command, Exit, DEFAULT_TERM_GRACE, ORPHAN_GRACE};
pub use cleanup::{remove_stale, CleanupOutcome, CleanupReport};
pub use env::{AgentSessionEnv, BaseEnv, Env, EnvBuilder, SessionToken, DEFAULT_TERM};
pub use error::{Error, Result};
pub use files::PlanTargets;
pub use probe::{run_probe, ProbeOutcome, PROBE_OUTPUT_CAP};
pub use pty::Pty;

#[cfg(test)]
mod fd_tests;
