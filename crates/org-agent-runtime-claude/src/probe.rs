use serde_json::Value;

use org_agent_runtime::{
    EnvName, Probe, ProbeContext, ProbeOutput, ProbePlan, ProbeResult, ProbeRun, Unavailable,
};

pub(crate) const TESTED_VERSION_MIN: &str = "2.1.0";
pub(crate) const TESTED_VERSION_MAX: &str = "2.99.999";

const MIN_SEMVER: (u64, u64, u64) = (2, 1, 0);
const MAX_SEMVER: (u64, u64, u64) = (2, 99, 999);

pub(crate) fn plan_probe(probe: Probe, ctx: &ProbeContext) -> Option<ProbeRun> {
    let mut env_add = vec![(
        EnvName::try_from("DISABLE_AUTOUPDATER").expect("static env name is valid"),
        "1".to_owned(),
    )];
    if let Some(cfg_dir) = &ctx.config_dir {
        env_add.push((
            EnvName::try_from("CLAUDE_CONFIG_DIR").expect("static env name is valid"),
            cfg_dir.as_str().to_owned(),
        ));
    }

    match probe {
        Probe::Version => Some(ProbeRun::OneShot(ProbePlan {
            program: ctx.runtime_path.clone(),
            args: vec!["--version".to_owned()],
            env_add,
            stdin_lines: Vec::new(),
            timeout_ms: 10_000,
        })),
        Probe::Auth => Some(ProbeRun::OneShot(ProbePlan {
            program: ctx.runtime_path.clone(),
            args: vec!["auth".to_owned(), "status".to_owned()],
            env_add,
            stdin_lines: Vec::new(),
            timeout_ms: 15_000,
        })),
        // Claude reports 5-hour and 7-day windows in-stream via `rate_limit_event`.
        Probe::Quota => None,
        _ => None,
    }
}

pub(crate) fn parse_probe(probe: Probe, out: &ProbeOutput) -> ProbeResult {
    if out.timed_out {
        return ProbeResult::Unavailable {
            reason: Unavailable::Other {
                detail: "probe timed out".to_owned(),
            },
        };
    }

    match probe {
        Probe::Version => parse_version(out),
        Probe::Auth => parse_auth(out),
        Probe::Quota => ProbeResult::Unavailable {
            reason: Unavailable::Other {
                detail: "Claude reports quota in-stream; no standalone quota probe".to_owned(),
            },
        },
        _ => ProbeResult::Unavailable {
            reason: Unavailable::Other {
                detail: "unsupported probe".to_owned(),
            },
        },
    }
}

fn parse_version(out: &ProbeOutput) -> ProbeResult {
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
    let in_tested_range = extract_semver(&raw).map(|v| v >= MIN_SEMVER && v <= MAX_SEMVER);
    ProbeResult::Version {
        raw,
        in_tested_range,
    }
}

fn parse_auth(out: &ProbeOutput) -> ProbeResult {
    let stdout = String::from_utf8_lossy(&out.stdout);
    if let Some(json_obj) = extract_json_object(&stdout) {
        if let Ok(val) = serde_json::from_str::<Value>(json_obj) {
            if let Some(logged_in) = val.get("loggedIn").and_then(Value::as_bool) {
                let method = build_auth_method(&val);
                return ProbeResult::Auth {
                    signed_in: logged_in,
                    method,
                };
            }
        }
    }

    if out.exit_code != Some(0) {
        return ProbeResult::Auth {
            signed_in: false,
            method: None,
        };
    }

    ProbeResult::Unavailable {
        reason: Unavailable::Other {
            detail: "unrecognized Claude auth status output".to_owned(),
        },
    }
}

fn build_auth_method(val: &Value) -> Option<String> {
    let auth_method = val.get("authMethod").and_then(Value::as_str);
    let sub_type = val.get("subscriptionType").and_then(Value::as_str);
    match (auth_method, sub_type) {
        (Some(m), Some(s)) if !s.is_empty() => Some(format!("{m} ({s})")),
        (Some(m), _) => Some(m.to_owned()),
        _ => None,
    }
}

fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end >= start {
        Some(&text[start..=end])
    } else {
        None
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
