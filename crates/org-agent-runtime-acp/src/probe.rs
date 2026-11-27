//! Sans-I/O version and authentication probes (one-shot and JSON-RPC 2.0 ACP [`ProbeSession`]).

use std::time::{Duration, Instant};

use serde_json::Value;

use org_agent_runtime::{
    static_env_names, EnvName, Probe, ProbeContext, ProbeOutput, ProbePlan, ProbeResult, ProbeRun,
    ProbeSession, ProbeStep, SpecPath, Unavailable,
};

use crate::profile::{AcpProfile, AuthProbeStrategy};
use crate::protocol::{encode_initialize, is_auth_rpc_error, rpc_error, rpc_result};

const AUTH_PROBE_TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) fn plan_probe(
    profile: &'static AcpProfile,
    probe: Probe,
    ctx: &ProbeContext,
) -> Option<ProbeRun> {
    if ctx.config_dir.as_ref().is_some_and(SpecPath::is_root)
        || ctx.runtime_dir.as_ref().is_some_and(SpecPath::is_root)
    {
        return None;
    }
    let env_add = build_probe_env(profile, ctx);
    match probe {
        Probe::Version => Some(ProbeRun::OneShot(ProbePlan {
            program: ctx.runtime_path.clone(),
            args: vec!["--version".to_owned()],
            env_add,
            auth_env_names: Vec::new(),
            stdin_lines: Vec::new(),
            timeout_ms: 10_000,
        })),
        Probe::Auth => plan_auth_probe(profile, ctx, env_add),
        Probe::Quota => None,
        _ => None,
    }
}

fn build_probe_env(profile: &'static AcpProfile, ctx: &ProbeContext) -> Vec<(EnvName, String)> {
    let mut env_add: Vec<(EnvName, String)> = profile
        .probe_env_add
        .iter()
        .map(|&(k, v)| (EnvName::try_from(k).expect("static env"), v.to_owned()))
        .collect();
    if let Some(cfg_dir) = &ctx.config_dir {
        env_add.push((
            EnvName::try_from(profile.config_dir_env).expect("static config dir env"),
            cfg_dir.as_str().to_owned(),
        ));
    }
    env_add
}

fn plan_auth_probe(
    profile: &'static AcpProfile,
    ctx: &ProbeContext,
    env_add: Vec<(EnvName, String)>,
) -> Option<ProbeRun> {
    let auth_env_names = static_env_names(profile.auth_env_names);
    match profile.auth_probe {
        AuthProbeStrategy::OneShot { args, .. } => Some(ProbeRun::OneShot(ProbePlan {
            program: ctx.runtime_path.clone(),
            args: args.iter().map(|s| (*s).to_owned()).collect(),
            env_add,
            auth_env_names,
            stdin_lines: Vec::new(),
            timeout_ms: 15_000,
        })),
        AuthProbeStrategy::AcpInitialize { method } => Some(ProbeRun::Conversation {
            plan: ProbePlan {
                program: ctx.runtime_path.clone(),
                args: profile
                    .headless_base_args
                    .iter()
                    .map(|s| (*s).to_owned())
                    .collect(),
                env_add,
                auth_env_names,
                stdin_lines: Vec::new(),
                timeout_ms: 15_000,
            },
            session: Box::new(AcpAuthProbeSession::new(profile.display_name, method)),
        }),
    }
}

pub(crate) fn parse_probe(
    profile: &'static AcpProfile,
    probe: Probe,
    out: &ProbeOutput,
) -> ProbeResult {
    if out.timed_out {
        return ProbeResult::Unavailable {
            reason: Unavailable::Other {
                detail: "probe timed out".to_owned(),
            },
        };
    }
    match probe {
        Probe::Version => parse_version(profile, out),
        Probe::Auth => match profile.auth_probe {
            AuthProbeStrategy::OneShot { method, .. } => parse_auth_oneshot(method, out),
            AuthProbeStrategy::AcpInitialize { .. } => ProbeResult::Unavailable {
                reason: Unavailable::Other {
                    detail: format!(
                        "{} auth probe runs as an ACP conversation",
                        profile.display_name
                    ),
                },
            },
        },
        Probe::Quota => ProbeResult::Unavailable {
            reason: Unavailable::Other {
                detail: format!(
                    "{} reports quota in-stream; no standalone quota probe",
                    profile.display_name
                ),
            },
        },
        _ => ProbeResult::Unavailable {
            reason: Unavailable::Other {
                detail: "unsupported probe".to_owned(),
            },
        },
    }
}

fn parse_version(profile: &'static AcpProfile, out: &ProbeOutput) -> ProbeResult {
    if out.exit_code != Some(0) {
        return ProbeResult::Unavailable {
            reason: Unavailable::NotInstalled,
        };
    }
    let raw = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if raw.is_empty() {
        return ProbeResult::Unavailable {
            reason: Unavailable::Version,
        };
    }
    let in_tested_range = profile
        .tested_semver_range
        .and_then(|(min, max)| extract_semver(&raw).map(|v| v >= min && v <= max));
    ProbeResult::Version {
        raw,
        in_tested_range,
    }
}

fn parse_auth_oneshot(method: &'static str, out: &ProbeOutput) -> ProbeResult {
    if out.exit_code == Some(0) {
        let stdout = String::from_utf8_lossy(&out.stdout);
        let lower = stdout.trim().to_ascii_lowercase();
        let looks_signed_out = lower.is_empty()
            || [
                "not logged in",
                "not authenticated",
                "authentication required",
                "unauthorized",
                "login required",
                "invalid api key",
                "missing api key",
                "please sign in",
                "error:",
            ]
            .iter()
            .any(|needle| lower.contains(needle));
        let signed_in = !looks_signed_out;
        return ProbeResult::Auth {
            signed_in,
            method: signed_in.then(|| method.to_owned()),
        };
    }
    ProbeResult::Auth {
        signed_in: false,
        method: None,
    }
}

fn extract_semver(text: &str) -> Option<(u64, u64, u64)> {
    for token in text.split_whitespace() {
        let clean = token.trim_start_matches('v');
        let mut parts = clean.split('.');
        let (Some(a), Some(b), Some(c)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let c_num = c
            .split(|ch: char| !ch.is_ascii_digit())
            .next()
            .unwrap_or("");
        if let (Ok(maj), Ok(min), Ok(pat)) = (a.parse(), b.parse(), c_num.parse()) {
            return Some((maj, min, pat));
        }
    }
    None
}

/// Sans-I/O JSON-RPC 2.0 ACP `initialize` probe session checking binary readiness and sign-in.
pub(crate) struct AcpAuthProbeSession {
    display_name: &'static str,
    method: &'static str,
    sent: bool,
    done: bool,
    deadline: Option<Instant>,
}

impl AcpAuthProbeSession {
    pub(crate) fn new(display_name: &'static str, method: &'static str) -> Self {
        Self {
            display_name,
            method,
            sent: false,
            done: false,
            deadline: None,
        }
    }

    fn finish(&mut self, result: ProbeResult) -> Vec<ProbeStep> {
        self.done = true;
        self.deadline = None;
        vec![ProbeStep::Done(result)]
    }

    fn evaluate_init_response(&mut self, val: &Value) -> Vec<ProbeStep> {
        if let Some(err) = rpc_error(val) {
            if is_auth_rpc_error(err) {
                return self.finish(ProbeResult::Auth {
                    signed_in: false,
                    method: None,
                });
            }
            return self.finish(ProbeResult::Unavailable {
                reason: Unavailable::Other {
                    detail: format!(
                        "{} ACP initialize probe returned an error",
                        self.display_name
                    ),
                },
            });
        }
        let Some(result) = rpc_result(val) else {
            return self.finish(ProbeResult::Unavailable {
                reason: Unavailable::Other {
                    detail: format!(
                        "{} ACP initialize probe response missing result",
                        self.display_name
                    ),
                },
            });
        };
        let proto_ver = result
            .get("protocolVersion")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if proto_ver < 1 {
            return self.finish(ProbeResult::Unavailable {
                reason: Unavailable::Other {
                    detail: format!(
                        "{} ACP initialize probe reported unsupported protocol version",
                        self.display_name
                    ),
                },
            });
        }
        if response_reports_signed_out(result) {
            return self.finish(ProbeResult::Auth {
                signed_in: false,
                method: None,
            });
        }
        self.finish(ProbeResult::Auth {
            signed_in: true,
            method: Some(self.method.to_owned()),
        })
    }
}

fn response_reports_signed_out(result: &Value) -> bool {
    let explicitly_false = [
        result.get("authenticated"),
        result.get("signedIn"),
        result.pointer("/auth/authenticated"),
        result.pointer("/auth/signedIn"),
    ]
    .into_iter()
    .flatten()
    .any(|v| v.as_bool() == Some(false));
    explicitly_false
}

impl ProbeSession for AcpAuthProbeSession {
    fn start(&mut self, now: Instant) -> Vec<ProbeStep> {
        if self.sent || self.done {
            return Vec::new();
        }
        self.sent = true;
        self.deadline = Some(now + AUTH_PROBE_TIMEOUT);
        let org_agent_runtime::Output::Write(line) = encode_initialize(1) else {
            unreachable!("encode_initialize always returns Output::Write");
        };
        vec![ProbeStep::Write(line)]
    }

    fn on_line(&mut self, line: &[u8], _now: Instant) -> Vec<ProbeStep> {
        if self.done {
            return Vec::new();
        }
        let Ok(text) = std::str::from_utf8(line) else {
            return Vec::new();
        };
        if text.trim().is_empty() {
            return Vec::new();
        }
        let Ok(val) = serde_json::from_str::<Value>(text) else {
            return Vec::new();
        };
        if val.get("id").and_then(Value::as_u64) != Some(1) {
            return Vec::new();
        }
        self.evaluate_init_response(&val)
    }

    fn on_tick(&mut self, now: Instant) -> Vec<ProbeStep> {
        if self.done {
            return Vec::new();
        }
        if self.deadline.is_some_and(|d| now >= d) {
            return self.finish(ProbeResult::Unavailable {
                reason: Unavailable::Other {
                    detail: "probe timed out".to_owned(),
                },
            });
        }
        Vec::new()
    }

    fn on_stdout_closed(&mut self, _now: Instant) -> ProbeResult {
        self.done = true;
        self.deadline = None;
        ProbeResult::Unavailable {
            reason: Unavailable::Other {
                detail: format!(
                    "{} ACP probe closed stdout before initialize response",
                    self.display_name
                ),
            },
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.deadline
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::profiles::GEMINI_PROFILE;

    #[test]
    fn acp_auth_probe_session_handles_ok_auth_error_signed_out_and_timeout() {
        let now = Instant::now();
        let Some(ProbeRun::Conversation { plan, mut session }) = plan_probe(
            &GEMINI_PROFILE,
            Probe::Auth,
            &ProbeContext::new(PathBuf::from("/usr/local/bin/gemini")),
        ) else {
            panic!("expected conversation auth probe for gemini");
        };
        assert_eq!(plan.args, vec!["--acp".to_owned()]);
        let start_steps = session.start(now);
        assert_eq!(start_steps.len(), 1);
        assert!(matches!(start_steps[0], ProbeStep::Write(_)));

        let ok = session.on_line(
            br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"name":"gemini","version":"0.24.0"}}}"#,
            now,
        );
        assert_eq!(
            ok,
            vec![ProbeStep::Done(ProbeResult::Auth {
                signed_in: true,
                method: Some("gemini-acp".to_owned()),
            })]
        );

        let mut auth_err = AcpAuthProbeSession::new("Gemini", "gemini-acp");
        auth_err.start(now);
        let res = auth_err.on_line(
            br#"{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"Authentication required"}}"#,
            now,
        );
        assert_eq!(
            res,
            vec![ProbeStep::Done(ProbeResult::Auth {
                signed_in: false,
                method: None,
            })]
        );

        let mut timed_out = AcpAuthProbeSession::new("Gemini", "gemini-acp");
        timed_out.start(now);
        let deadline = timed_out.next_deadline().expect("deadline set");
        let expired = timed_out.on_tick(deadline);
        assert!(matches!(
            expired.as_slice(),
            [ProbeStep::Done(ProbeResult::Unavailable {
                reason: Unavailable::Other { detail }
            })] if detail == "probe timed out"
        ));
    }
}
