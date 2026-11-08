//! Probes: short runs that check a runtime outside a session (its version, its sign-in, its
//! quota). The adapter plans them ([`crate::AgentRuntime::plan_probe`]) and reads their
//! output ([`crate::AgentRuntime::parse_probe`]); the supervisor runs them.

use std::path::PathBuf;

use crate::event::redact_opt;
use crate::RateLimit;

/// What a probe needs to know: only which binary to run. A probe has no session, so it gets
/// none of [`crate::LaunchContext`]'s per-run values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeContext {
    /// The runtime's binary, by the real path recorded at preflight (or found on the user's
    /// `PATH` at preflight, for the first version probe).
    pub runtime_path: PathBuf,
    /// The runtime's version, if known yet, for gating flags.
    pub runtime_version: Option<String>,
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

/// What a probe process produced. Output past the supervisor's cap is cut off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeOutput {
    /// Everything it wrote to stdout.
    pub stdout: Vec<u8>,
    /// Everything it wrote to stderr.
    pub stderr: Vec<u8>,
    /// Its exit code, or `None` if a signal ended it.
    pub exit_code: Option<i32>,
    /// Whether it was stopped for running past [`crate::ProbePlan::timeout_ms`].
    pub timed_out: bool,
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
        let limit = RateLimit {
            window: Window::Other {
                id: "s:window".into(),
                mins: None,
            },
            used_fraction: None,
            resets_at: Some("s:resets".into()),
            status: LimitStatus::Ok,
        };
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
