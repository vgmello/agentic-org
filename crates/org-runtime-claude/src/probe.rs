//! Probes: `claude --version` and `claude auth status`, planned and read purely. Quota has
//! no probe: Claude reports it in-stream (`rate_limit_event`).

use org_runtime::{
    Probe, ProbeContext, ProbeOutput, ProbePlan, ProbeResult, ProbeRun, Unavailable,
};
use serde_json::Value;

use crate::plan::env_name;

const VERSION_TIMEOUT_MS: u64 = 15_000;
const AUTH_TIMEOUT_MS: u64 = 30_000;

/// Both probes are one-shot: no exchange on stdin.
pub(crate) fn plan(probe: Probe, ctx: &ProbeContext) -> Option<ProbeRun> {
    let (args, timeout_ms): (&[&str], u64) = match probe {
        Probe::Version => (&["--version"], VERSION_TIMEOUT_MS),
        Probe::Auth => (&["auth", "status"], AUTH_TIMEOUT_MS),
        // Optional for Claude: quota arrives in-stream. And any probe this build doesn't know.
        _ => return None,
    };
    Some(ProbeRun::OneShot(ProbePlan {
        program: ctx.runtime_path.clone(),
        args: args.iter().map(|a| a.to_string()).collect(),
        env_add: vec![(env_name("DISABLE_AUTOUPDATER"), "1".to_owned())],
        stdin_lines: vec![],
        timeout_ms,
    }))
}

pub(crate) fn parse(
    probe: Probe,
    out: &ProbeOutput,
    tested: &org_runtime::VersionRange,
) -> ProbeResult {
    if out.timed_out {
        return other("the probe timed out");
    }
    match probe {
        Probe::Version => parse_version(out, tested),
        Probe::Auth => parse_auth(out),
        _ => other("Claude has no such probe"),
    }
}

/// `claude --version` prints `2.1.311 (Claude Code)`: the version is the first word.
fn parse_version(out: &ProbeOutput, tested: &org_runtime::VersionRange) -> ProbeResult {
    if out.exit_code != Some(0) {
        return ProbeResult::Unavailable {
            reason: Unavailable::NotInstalled,
        };
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let Some(raw) = stdout.split_whitespace().next() else {
        return unavailable_version();
    };
    let Some(version) = numeric(raw) else {
        return unavailable_version();
    };
    let in_range = match (numeric(&tested.min), numeric(&tested.max)) {
        (Some(min), Some(max)) => Some(min <= version && version <= max),
        _ => None,
    };
    ProbeResult::Version {
        raw: raw.to_owned(),
        in_tested_range: in_range,
    }
}

/// `claude auth status`: JSON on stdout (`loggedIn`, `authMethod`, ...), exit 0 when signed
/// in and 1 when not (spike `results/item00.json`).
fn parse_auth(out: &ProbeOutput) -> ProbeResult {
    let Ok(status) = serde_json::from_slice::<Value>(&out.stdout) else {
        return match out.exit_code {
            Some(0) | Some(1) => other("Claude's auth status isn't JSON"),
            _ => ProbeResult::Unavailable {
                reason: Unavailable::NotInstalled,
            },
        };
    };
    let logged_in = status.get("loggedIn").and_then(Value::as_bool);
    match (out.exit_code, logged_in) {
        (Some(0), Some(true)) => ProbeResult::Auth {
            signed_in: true,
            method: status
                .get("authMethod")
                .and_then(Value::as_str)
                .filter(|m| *m != "none")
                .map(str::to_owned),
        },
        (Some(1), Some(false)) => ProbeResult::Auth {
            signed_in: false,
            method: None,
        },
        _ => other("Claude's auth status contradicts its exit code"),
    }
}

/// `major.minor.patch` (more or fewer parts allowed), numerically.
fn numeric(v: &str) -> Option<Vec<u64>> {
    v.split('.').map(|p| p.parse().ok()).collect()
}

fn unavailable_version() -> ProbeResult {
    ProbeResult::Unavailable {
        reason: Unavailable::Version,
    }
}

fn other(detail: &str) -> ProbeResult {
    ProbeResult::Unavailable {
        reason: Unavailable::Other {
            detail: detail.to_owned(),
        },
    }
}
