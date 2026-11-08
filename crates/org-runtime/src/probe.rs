//! Probes: short runs that check a runtime outside a session (its version, its sign-in, its
//! quota). The adapter plans them ([`crate::AgentRuntime::plan_probe`]); the supervisor runs
//! them. A probe is one of two shapes ([`ProbeRun`]):
//! - **one-shot**: write a few fixed lines, close stdin, and read the whole output with
//!   [`crate::AgentRuntime::parse_probe`] (`--version`, `auth status`);
//! - **conversation**: an exchange that needs answers read before the next line is written,
//!   driven by a sans-I/O [`ProbeSession`] (Codex's `initialize`, then `account/read`).

use std::path::PathBuf;
use std::time::Instant;

use crate::event::redact_opt;
use crate::{Line, ProbePlan, RateLimit, SpecPath};

/// What a probe needs to know: which binary to run, and where its own files are. A probe has
/// no session, so it gets none of [`crate::LaunchContext`]'s per-run values.
///
/// It's `#[non_exhaustive]` so it can grow: build one with [`ProbeContext::new`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProbeContext {
    /// The runtime's binary, by the real path recorded at preflight (or found on the user's
    /// `PATH` at preflight, for the first version probe).
    pub runtime_path: PathBuf,
    /// The runtime's version, if known yet, for gating flags.
    pub runtime_version: Option<String>,
    /// The agent's runtime dir, when the probe is for one agent (e.g. Antigravity's per-agent
    /// `--gemini-dir`, so a sign-in check sees the same config the sessions use). `None` for
    /// a runtime-wide probe. A real path.
    pub runtime_dir: Option<SpecPath>,
}

impl ProbeContext {
    /// A context for a runtime-wide probe of this binary, with no version known yet.
    pub fn new(runtime_path: PathBuf) -> Self {
        ProbeContext {
            runtime_path,
            runtime_version: None,
            runtime_dir: None,
        }
    }
}

/// How to run one probe.
///
/// Exhaustive on purpose, like the plan's enums: it's an instruction to the supervisor.
pub enum ProbeRun {
    /// Write [`ProbePlan::stdin_lines`], close stdin, wait for the exit (or the timeout), and
    /// give the output to [`crate::AgentRuntime::parse_probe`].
    OneShot(ProbePlan),
    /// Write [`ProbePlan::stdin_lines`], then drive `session`: feed it each stdout line and
    /// its ticks, write what it returns, and stop as soon as it returns
    /// [`ProbeStep::Done`]. Stdin stays open until then.
    Conversation {
        /// The process.
        plan: ProbePlan,
        /// The exchange.
        session: Box<dyn ProbeSession>,
    },
}

impl std::fmt::Debug for ProbeRun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProbeRun::OneShot(plan) => f.debug_tuple("OneShot").field(plan).finish(),
            ProbeRun::Conversation { plan, .. } => f
                .debug_struct("Conversation")
                .field("plan", plan)
                .finish_non_exhaustive(),
        }
    }
}

/// A probe's exchange as a pure state machine, like [`crate::Session`] but for one short
/// run: no I/O, no clock, time as an input.
///
/// The supervisor writes the plan's [`ProbePlan::stdin_lines`] first, then calls
/// [`start`](Self::start), then [`on_line`](Self::on_line) for each stdout line (split by a
/// [`crate::LineFramer`]; an over-long line is dropped, not passed), [`on_tick`](Self::on_tick)
/// at [`next_deadline`](Self::next_deadline), and [`on_stdout_closed`](Self::on_stdout_closed)
/// if stdout ends first. It applies the [`ProbeStep`]s in order. Stderr isn't passed: it's
/// kept with the probe's output for the logs.
///
/// **A probe that finished never ends as a timeout.** The first [`ProbeStep::Done`] is the
/// result: the supervisor then closes stdin and stops the process, and nothing that happens
/// after (an exit code, the plan's overall timeout) changes it. The plan's timeout produces
/// a timed-out result only if no `Done` came first: the supervisor then reports
/// [`ProbeResult::Unavailable`] with [`Unavailable::Other`] ("timed out").
pub trait ProbeSession: Send {
    /// The process started.
    fn start(&mut self, now: Instant) -> Vec<ProbeStep>;
    /// One stdout line, without its `\n`.
    fn on_line(&mut self, line: &[u8], now: Instant) -> Vec<ProbeStep>;
    /// Time passed: the session's own protocol deadlines.
    fn on_tick(&mut self, now: Instant) -> Vec<ProbeStep>;
    /// Stdout ended before `Done`: the session concludes with what it has.
    fn on_stdout_closed(&mut self, now: Instant) -> ProbeResult;
    /// When the session next needs [`on_tick`](Self::on_tick), if ever.
    fn next_deadline(&self) -> Option<Instant>;
}

/// Something the supervisor does for a [`ProbeSession`], in order.
///
/// Exhaustive on purpose: it's an instruction to the supervisor.
#[derive(Debug, Clone, PartialEq)]
pub enum ProbeStep {
    /// Write this line to stdin.
    Write(Line),
    /// The probe's result. The exchange is over.
    Done(ProbeResult),
}

/// What to check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Probe {
    /// The runtime's version, at preflight.
    Version,
    /// Whether the runtime is signed in, at preflight.
    Auth,
    /// The account's quota, for a runtime that doesn't report it in-stream (polled).
    Quota,
}

/// What a one-shot probe process produced. Output past the supervisor's cap is cut off.
///
/// It's `#[non_exhaustive]` so it can grow: build one with [`ProbeOutput::new`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProbeOutput {
    /// Everything it wrote to stdout.
    pub stdout: Vec<u8>,
    /// Everything it wrote to stderr.
    pub stderr: Vec<u8>,
    /// Its exit code, or `None` if a signal ended it.
    pub exit_code: Option<i32>,
    /// Whether the timeout ([`crate::ProbePlan::timeout_ms`]) stopped it **before it
    /// exited**. A probe that exited in time is never timed out, however long reading its
    /// output took.
    pub timed_out: bool,
}

impl ProbeOutput {
    /// A probe that exited (in time) with this output and exit code.
    pub fn new(stdout: Vec<u8>, stderr: Vec<u8>, exit_code: Option<i32>) -> Self {
        ProbeOutput {
            stdout,
            stderr,
            exit_code,
            timed_out: false,
        }
    }
}

/// What a probe found.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ProbeResult {
    /// The runtime's version.
    Version {
        /// As the runtime printed it, trimmed.
        raw: String,
        /// Whether it's within [`crate::Capabilities::tested_versions`], or `None` if the
        /// adapter can't tell.
        in_tested_range: Option<bool>,
    },
    /// The runtime's sign-in.
    Auth {
        /// Whether it's signed in.
        signed_in: bool,
        /// How, in the runtime's words (e.g. a subscription or an API key), where it says.
        method: Option<String>,
    },
    /// The account's rate-limit windows.
    Quota {
        /// One per window the runtime reported.
        limits: Vec<RateLimit>,
    },
    /// The probe shows the runtime can't be used.
    Unavailable {
        /// Why.
        reason: Unavailable,
    },
}

impl ProbeResult {
    /// Runs `f` over every free-text string in the result (versions, methods, details, rate
    /// limit names and times), like [`crate::SessionEvent::redact`], before the office stores
    /// or shows it: probe output is runtime text.
    pub fn redact(&mut self, f: &mut dyn FnMut(&mut String)) {
        match self {
            ProbeResult::Version {
                raw,
                in_tested_range: _,
            } => f(raw),
            ProbeResult::Auth {
                signed_in: _,
                method,
            } => redact_opt(method, f),
            ProbeResult::Quota { limits } => {
                for limit in limits {
                    limit.redact(f);
                }
            }
            ProbeResult::Unavailable { reason } => match reason {
                Unavailable::Other { detail } => f(detail),
                Unavailable::NotInstalled
                | Unavailable::SignedOut
                | Unavailable::Version
                | Unavailable::Quota => {}
            },
        }
    }
}

/// Why a runtime can't be used.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Unavailable {
    /// The binary isn't there or won't run.
    NotInstalled,
    /// It isn't signed in.
    SignedOut,
    /// Its version can't be used (unreadable, or too old for this adapter).
    Version,
    /// Its quota is used up.
    Quota,
    /// Anything else (including a probe that timed out).
    Other {
        /// What happened. Never a secret: adapters don't quote raw probe output here.
        detail: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LimitStatus, Window};

    #[test]
    fn probe_result_redact_visits_every_string() {
        let mut limit = RateLimit::new(
            Window::Other {
                id: "s:window".into(),
                mins: None,
            },
            LimitStatus::Ok,
        );
        limit.resets_at = Some("s:resets".into());
        let mut results = vec![
            ProbeResult::Version {
                raw: "s:raw".into(),
                in_tested_range: Some(true),
            },
            ProbeResult::Auth {
                signed_in: true,
                method: Some("s:method".into()),
            },
            ProbeResult::Quota {
                limits: vec![limit],
            },
            ProbeResult::Unavailable {
                reason: Unavailable::Other {
                    detail: "s:detail".into(),
                },
            },
        ];
        let mut seen = Vec::new();
        for result in &mut results {
            result.redact(&mut |s| {
                seen.push(s.clone());
                *s = "R".into();
            });
        }
        assert_eq!(
            seen,
            ["s:raw", "s:method", "s:window", "s:resets", "s:detail"]
        );
        assert!(!format!("{results:?}").contains("\"s:"));
    }
}
