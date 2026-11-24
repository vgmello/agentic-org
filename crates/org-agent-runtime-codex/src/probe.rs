use org_agent_runtime::{
    EnvName, Probe, ProbeContext, ProbeOutput, ProbePlan, ProbeResult, ProbeRun, SpecPath,
    Unavailable,
};

pub(crate) const TESTED_VERSION_MIN: &str = "0.169.0";
pub(crate) const TESTED_VERSION_MAX: &str = "0.171.0";

const MIN_SEMVER: (u64, u64, u64) = (0, 169, 0);
const MAX_SEMVER: (u64, u64, u64) = (0, 171, 0);

pub(crate) fn plan_probe(probe: Probe, ctx: &ProbeContext) -> Option<ProbeRun> {
    if ctx.config_dir.as_ref().is_some_and(SpecPath::is_root)
        || ctx.runtime_dir.as_ref().is_some_and(SpecPath::is_root)
    {
        return None;
    }
    let mut env_add = vec![(
        EnvName::try_from("CODEX_DISABLE_TELEMETRY").expect("static env"),
        "1".to_owned(),
    )];
    if let Some(cfg_dir) = &ctx.config_dir {
        env_add.push((
            EnvName::try_from("CODEX_HOME").expect("static env"),
            cfg_dir.as_str().to_owned(),
        ));
    }
    match probe {
        Probe::Version => Some(ProbeRun::OneShot(ProbePlan {
            program: ctx.runtime_path.clone(),
            args: vec!["--version".to_owned()],
            env_add,
            auth_env_names: Vec::new(),
            stdin_lines: Vec::new(),
            timeout_ms: 10_000,
        })),
        Probe::Auth => Some(ProbeRun::OneShot(ProbePlan {
            program: ctx.runtime_path.clone(),
            args: vec!["login".to_owned(), "status".to_owned()],
            env_add,
            auth_env_names: crate::plan::auth_env_names(),
            stdin_lines: Vec::new(),
            timeout_ms: 15_000,
        })),
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
                detail: "Codex reports quota in-stream; no standalone quota probe".to_owned(),
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
    if out.exit_code == Some(0) {
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        let lower = stdout.to_ascii_lowercase();
        let signed_in = !lower.is_empty() && !lower.contains("not logged in");
        return ProbeResult::Auth {
            signed_in,
            method: signed_in.then(|| "codex-login".to_owned()),
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
