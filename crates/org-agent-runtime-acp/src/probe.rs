//! Sans-I/O version and authentication probes (one-shot and JSON-RPC 2.0 ACP [`ProbeSession`]).

use std::time::{Duration, Instant};

use serde_json::Value;

use org_agent_runtime::{
    static_env_names, EnvName, Probe, ProbeContext, ProbeOutput, ProbePlan, ProbeResult, ProbeRun,
    ProbeSession, ProbeStep, SpecPath, Unavailable,
};

use crate::profile::{AcpProfile, AuthProbeStrategy};
use crate::protocol::{
    encode_initialize, encode_session_open, is_auth_rpc_error, rpc_error, rpc_result,
};

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
            profile
                .config_dir_mapping
                .resolve_env_value(cfg_dir.as_str()),
        ));
    }
    env_add
}

fn probe_session_cwd(ctx: &ProbeContext) -> String {
    ctx.runtime_dir
        .as_ref()
        .or(ctx.config_dir.as_ref())
        .map(|p| p.as_str().to_owned())
        .unwrap_or_else(|| ".".to_owned())
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
            session: Box::new(AcpAuthProbeSession::new(
                profile.display_name,
                method,
                probe_session_cwd(ctx),
            )),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthProbePhase {
    NotStarted,
    AwaitingInitialize,
    AwaitingSessionNew,
    Done,
}

/// Sans-I/O JSON-RPC 2.0 ACP `initialize` + `session/new` probe session checking binary readiness
/// and actual credential acceptance (failing closed on unauthenticated `initialize` responses).
pub(crate) struct AcpAuthProbeSession {
    display_name: &'static str,
    method: &'static str,
    probe_cwd: String,
    phase: AuthProbePhase,
    saw_auth_methods: bool,
    deadline: Option<Instant>,
}

impl AcpAuthProbeSession {
    pub(crate) fn new(display_name: &'static str, method: &'static str, probe_cwd: String) -> Self {
        Self {
            display_name,
            method,
            probe_cwd,
            phase: AuthProbePhase::NotStarted,
            saw_auth_methods: false,
            deadline: None,
        }
    }

    fn finish(&mut self, result: ProbeResult) -> Vec<ProbeStep> {
        self.phase = AuthProbePhase::Done;
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
        self.saw_auth_methods = response_has_auth_methods(result);
        self.phase = AuthProbePhase::AwaitingSessionNew;
        let org_agent_runtime::Output::Write(line) =
            encode_session_open(2, &self.probe_cwd, &[], None, None, "session/new")
        else {
            unreachable!("encode_session_open always returns Output::Write");
        };
        vec![ProbeStep::Write(line)]
    }

    fn evaluate_session_new_response(&mut self, val: &Value) -> Vec<ProbeStep> {
        if let Some(err) = rpc_error(val) {
            if is_auth_rpc_error(err) || self.saw_auth_methods {
                return self.finish(ProbeResult::Auth {
                    signed_in: false,
                    method: None,
                });
            }
            return self.finish(ProbeResult::Unavailable {
                reason: Unavailable::Other {
                    detail: format!(
                        "{} ACP session/new probe returned an error",
                        self.display_name
                    ),
                },
            });
        }
        let Some(result) = rpc_result(val) else {
            if self.saw_auth_methods {
                return self.finish(ProbeResult::Auth {
                    signed_in: false,
                    method: None,
                });
            }
            return self.finish(ProbeResult::Unavailable {
                reason: Unavailable::Other {
                    detail: format!(
                        "{} ACP session/new probe response missing result",
                        self.display_name
                    ),
                },
            });
        };
        if response_reports_signed_out(result) {
            return self.finish(ProbeResult::Auth {
                signed_in: false,
                method: None,
            });
        }
        let has_session_id = result
            .get("sessionId")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty());
        if !has_session_id {
            if self.saw_auth_methods {
                return self.finish(ProbeResult::Auth {
                    signed_in: false,
                    method: None,
                });
            }
            return self.finish(ProbeResult::Unavailable {
                reason: Unavailable::Other {
                    detail: format!(
                        "{} ACP session/new probe response missing sessionId",
                        self.display_name
                    ),
                },
            });
        }
        self.finish(ProbeResult::Auth {
            signed_in: true,
            method: Some(self.method.to_owned()),
        })
    }
}

fn response_reports_signed_out(result: &Value) -> bool {
    [
        result.get("authenticated"),
        result.get("signedIn"),
        result.pointer("/auth/authenticated"),
        result.pointer("/auth/signedIn"),
    ]
    .into_iter()
    .flatten()
    .any(|v| v.as_bool() == Some(false))
}

fn response_has_auth_methods(result: &Value) -> bool {
    [result.get("authMethods"), result.get("auth_methods")]
        .into_iter()
        .flatten()
        .any(|v| v.as_array().is_some_and(|arr| !arr.is_empty()))
}

impl ProbeSession for AcpAuthProbeSession {
    fn start(&mut self, now: Instant) -> Vec<ProbeStep> {
        if self.phase != AuthProbePhase::NotStarted {
            return Vec::new();
        }
        self.phase = AuthProbePhase::AwaitingInitialize;
        self.deadline = Some(now + AUTH_PROBE_TIMEOUT);
        let org_agent_runtime::Output::Write(line) = encode_initialize(1) else {
            unreachable!("encode_initialize always returns Output::Write");
        };
        vec![ProbeStep::Write(line)]
    }

    fn on_line(&mut self, line: &[u8], _now: Instant) -> Vec<ProbeStep> {
        if matches!(
            self.phase,
            AuthProbePhase::NotStarted | AuthProbePhase::Done
        ) {
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
        match self.phase {
            AuthProbePhase::AwaitingInitialize => {
                if val.get("id").and_then(Value::as_u64) != Some(1) {
                    return Vec::new();
                }
                self.evaluate_init_response(&val)
            }
            AuthProbePhase::AwaitingSessionNew => {
                if val.get("id").and_then(Value::as_u64) != Some(2) {
                    return Vec::new();
                }
                self.evaluate_session_new_response(&val)
            }
            AuthProbePhase::NotStarted | AuthProbePhase::Done => Vec::new(),
        }
    }

    fn on_tick(&mut self, now: Instant) -> Vec<ProbeStep> {
        if self.phase == AuthProbePhase::Done {
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
        let prev = self.phase;
        self.phase = AuthProbePhase::Done;
        self.deadline = None;
        if prev == AuthProbePhase::AwaitingSessionNew && self.saw_auth_methods {
            return ProbeResult::Auth {
                signed_in: false,
                method: None,
            };
        }
        let stage = if prev == AuthProbePhase::AwaitingSessionNew {
            "session/new"
        } else {
            "initialize"
        };
        ProbeResult::Unavailable {
            reason: Unavailable::Other {
                detail: format!(
                    "{} ACP probe closed stdout before {stage} response",
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
    use crate::profiles::{GEMINI_PROFILE, PI_PROFILE};

    #[test]
    fn acp_auth_probe_requires_session_new_acceptance_and_fails_closed_on_unauth_init() {
        let now = Instant::now();
        let mut pctx = ProbeContext::new(PathBuf::from("/usr/local/bin/gemini"));
        pctx.runtime_dir = Some(SpecPath::try_from("/runtime/agent-1").unwrap());
        pctx.config_dir = Some(SpecPath::try_from("/agents/emp-1/home/.gemini").unwrap());
        let Some(ProbeRun::Conversation { plan, mut session }) =
            plan_probe(&GEMINI_PROFILE, Probe::Auth, &pctx)
        else {
            panic!("expected conversation auth probe for gemini");
        };
        assert_eq!(plan.args, vec!["--acp".to_owned()]);
        // Probe env maps GEMINI_CLI_HOME to parent of `.gemini` (no `.gemini/.gemini` nesting).
        assert!(plan
            .env_add
            .iter()
            .any(|(k, v)| { k.as_str() == "GEMINI_CLI_HOME" && v == "/agents/emp-1/home" }));

        // 1. Authenticated flow: initialize -> writes session/new -> session/new ok -> signed_in: true.
        let start_steps = session.start(now);
        assert_eq!(start_steps.len(), 1);
        assert!(matches!(start_steps[0], ProbeStep::Write(_)));

        let after_init = session.on_line(
            br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"authMethods":[{"id":"oauth","name":"OAuth"}],"agentInfo":{"name":"gemini","version":"0.24.0"}}}"#,
            now,
        );
        assert_eq!(after_init.len(), 1);
        let ProbeStep::Write(ref session_new_line) = after_init[0] else {
            panic!("initialize must probe session/new before concluding signed_in: {after_init:?}");
        };
        let req: Value = serde_json::from_slice(session_new_line.as_bytes()).unwrap();
        assert_eq!(req["id"], 2);
        assert_eq!(req["method"], "session/new");
        assert_eq!(req["params"]["cwd"], "/runtime/agent-1");

        let ok = session.on_line(
            br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"probe-sess-1"}}"#,
            now,
        );
        assert_eq!(
            ok,
            vec![ProbeStep::Done(ProbeResult::Auth {
                signed_in: true,
                method: Some("gemini-acp".to_owned()),
            })]
        );

        // 2. Standard unauthenticated ACP agent: initialize succeeds with protocolVersion: 1 +
        //    authMethods, then session/new returns -32000 Authentication required -> signed_in: false.
        let mut unauth_session = AcpAuthProbeSession::new("Gemini", "gemini-acp", ".".to_owned());
        unauth_session.start(now);
        let next = unauth_session.on_line(
            br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"authMethods":[{"id":"api-key","name":"API Key"}]}}"#,
            now,
        );
        assert!(matches!(next.as_slice(), [ProbeStep::Write(_)]));
        let denied = unauth_session.on_line(
            br#"{"jsonrpc":"2.0","id":2,"error":{"code":-32000,"message":"Authentication required"}}"#,
            now,
        );
        assert_eq!(
            denied,
            vec![ProbeStep::Done(ProbeResult::Auth {
                signed_in: false,
                method: None,
            })]
        );

        // 3. Agent advertising authMethods that closes stdout before session/new -> signed_in: false.
        let mut eof_unauth = AcpAuthProbeSession::new("Gemini", "gemini-acp", ".".to_owned());
        eof_unauth.start(now);
        let _ = eof_unauth.on_line(
            br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"authMethods":[{"id":"oauth"}]}}"#,
            now,
        );
        assert_eq!(
            eof_unauth.on_stdout_closed(now),
            ProbeResult::Auth {
                signed_in: false,
                method: None,
            }
        );

        // 4. Early initialize auth error -> immediate signed_in: false.
        let mut auth_err = AcpAuthProbeSession::new("Gemini", "gemini-acp", ".".to_owned());
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

        // 5. Pi probe maps `<home>/.pi` -> `<home>/.pi/agent`.
        let mut pi_ctx = ProbeContext::new(PathBuf::from("/usr/local/bin/pi-acp"));
        pi_ctx.config_dir = Some(SpecPath::try_from("/agents/emp-1/home/.pi").unwrap());
        let Some(ProbeRun::Conversation { plan: pi_plan, .. }) =
            plan_probe(&PI_PROFILE, Probe::Auth, &pi_ctx)
        else {
            panic!("expected conversation auth probe for pi");
        };
        assert!(pi_plan.env_add.iter().any(|(k, v)| {
            k.as_str() == "PI_CODING_AGENT_DIR" && v == "/agents/emp-1/home/.pi/agent"
        }));

        // 6. Timeout fails closed as Unavailable.
        let mut timed_out = AcpAuthProbeSession::new("Gemini", "gemini-acp", ".".to_owned());
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
