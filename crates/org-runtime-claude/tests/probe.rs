//! Probes: plans, and parsing realistic outputs (the auth ones from the spike's
//! `results/item00.json`).

use std::path::PathBuf;

use org_runtime::{
    AgentRuntime, Probe, ProbeContext, ProbeOutput, ProbePlan, ProbeResult, ProbeRun, Unavailable,
};
use org_runtime_claude::ClaudeRuntime;

fn ctx() -> ProbeContext {
    ProbeContext::new(PathBuf::from(
        "/opt/homebrew/Caskroom/claude-code/2.1.311/claude",
    ))
}

fn out(stdout: &str, exit: Option<i32>) -> ProbeOutput {
    ProbeOutput::new(stdout.as_bytes().to_vec(), vec![], exit)
}

/// Claude's probes are one-shot.
fn one_shot(run: Option<ProbeRun>) -> ProbePlan {
    match run {
        Some(ProbeRun::OneShot(plan)) => plan,
        other => panic!("{other:?}"),
    }
}

#[test]
fn probe_plans() {
    let rt = ClaudeRuntime::new();
    let version = one_shot(rt.plan_probe(Probe::Version, &ctx()));
    assert_eq!(version.program, ctx().runtime_path);
    assert_eq!(version.args, ["--version"]);
    assert!(version.stdin_lines.is_empty());
    let auth = one_shot(rt.plan_probe(Probe::Auth, &ctx()));
    assert_eq!(auth.args, ["auth", "status"]);
    assert!(auth.stdin_lines.is_empty());
    for plan in [&version, &auth] {
        assert!(plan.timeout_ms > 0);
        assert_eq!(plan.env_add.len(), 1);
        assert_eq!(plan.env_add[0].0.as_str(), "DISABLE_AUTOUPDATER");
    }
    // Quota comes in-stream: there's no probe, and it reads as unavailable.
    assert!(rt.plan_probe(Probe::Quota, &ctx()).is_none());
    assert!(matches!(
        rt.parse_probe(Probe::Quota, &out("", Some(0))),
        ProbeResult::Unavailable { .. }
    ));
}

#[test]
fn version_probe() {
    let rt = ClaudeRuntime::new();
    let parse = |s: &str, exit| rt.parse_probe(Probe::Version, &out(s, exit));
    assert_eq!(
        parse("2.1.311 (Claude Code)\n", Some(0)),
        ProbeResult::Version {
            raw: "2.1.311".into(),
            in_tested_range: Some(true)
        }
    );
    for other in ["2.1.312 (Claude Code)", "2.0.9 (Claude Code)", "3.0.0"] {
        assert_eq!(
            parse(other, Some(0)),
            ProbeResult::Version {
                raw: other.split(' ').next().unwrap().into(),
                in_tested_range: Some(false)
            }
        );
    }
    for unreadable in ["", "Claude Code", "v2.1.311", "2.1.x"] {
        assert_eq!(
            parse(unreadable, Some(0)),
            ProbeResult::Unavailable {
                reason: Unavailable::Version
            },
            "{unreadable:?}"
        );
    }
    assert_eq!(
        parse("", Some(127)),
        ProbeResult::Unavailable {
            reason: Unavailable::NotInstalled
        }
    );
    assert_eq!(
        parse("", None),
        ProbeResult::Unavailable {
            reason: Unavailable::NotInstalled
        }
    );
    let mut timed_out = out("", None);
    timed_out.timed_out = true;
    assert!(matches!(
        rt.parse_probe(Probe::Version, &timed_out),
        ProbeResult::Unavailable {
            reason: Unavailable::Other { .. }
        }
    ));
}

#[test]
fn auth_probe() {
    let rt = ClaudeRuntime::new();
    let parse = |s: &str, exit| rt.parse_probe(Probe::Auth, &out(s, exit));
    // As recorded (results/item00.json), with the scrubbed fields filled in.
    let signed_in = r#"{"loggedIn": true, "authMethod": "claude.ai", "apiProvider": "firstParty",
        "configDirectory": "/Users/u/.claude", "subscriptionType": "max"}"#;
    assert_eq!(
        parse(signed_in, Some(0)),
        ProbeResult::Auth {
            signed_in: true,
            method: Some("claude.ai".into())
        }
    );
    let signed_out = r#"{"loggedIn": false, "authMethod": "none", "apiProvider": "firstParty",
        "configDirectory": "/Users/u/.claude"}"#;
    assert_eq!(
        parse(signed_out, Some(1)),
        ProbeResult::Auth {
            signed_in: false,
            method: None
        }
    );
    // Exit code and JSON must agree; anything else fails closed.
    for (stdout, exit) in [
        (signed_in, Some(1)),
        (signed_out, Some(0)),
        (r#"{"authMethod": "claude.ai"}"#, Some(0)),
        ("Not logged in", Some(1)),
    ] {
        assert!(
            matches!(
                parse(stdout, exit),
                ProbeResult::Unavailable {
                    reason: Unavailable::Other { .. }
                }
            ),
            "{stdout} {exit:?}"
        );
    }
    assert_eq!(
        parse("", Some(127)),
        ProbeResult::Unavailable {
            reason: Unavailable::NotInstalled
        }
    );
}
