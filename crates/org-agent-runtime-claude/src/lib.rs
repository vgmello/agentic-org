//! # org-agent-runtime-claude: sans-I/O Claude Code adapter
//!
//! Implements the [`org_agent_runtime::AgentRuntime`] and [`org_agent_runtime::Session`]
//! contracts for Claude Code (`claude -p --input-format stream-json --output-format stream-json`).
//!
//! This crate performs **no I/O and spawns no processes**: `org-supervise` executes every
//! [`org_agent_runtime::LaunchPlan`] and [`org_agent_runtime::ProbePlan`].

#![forbid(unsafe_code)]

mod plan;
mod probe;
mod session;
mod transcript;

use org_agent_runtime::transcript::TranscriptParser;
use org_agent_runtime::{
    AgentRuntime, Capabilities, CostBasis, ImageInputMode, LaunchContext, LaunchPlan,
    MidTurnDelivery, PlanError, Probe, ProbeContext, ProbeOutput, ProbeResult, ProbeRun,
    RuntimeHostProfile, RuntimeId, Session, SessionSpec, UsageScope, VersionRange,
};

use crate::plan::{build_headless_plan, build_take_over_plan};
use crate::probe::{TESTED_VERSION_MAX, TESTED_VERSION_MIN};
use crate::session::ClaudeSession;

/// Stateless factory for Claude Code headless and take-over sessions.
#[derive(Debug, Default, Clone, Copy)]
pub struct ClaudeRuntime;

impl ClaudeRuntime {
    /// Creates a new stateless Claude Code runtime adapter.
    pub const fn new() -> Self {
        Self
    }
}

impl AgentRuntime for ClaudeRuntime {
    fn id(&self) -> RuntimeId {
        RuntimeId::try_from("claude").expect("static runtime id `claude` is valid")
    }

    fn binary_name(&self) -> &'static str {
        "claude"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            mid_turn_delivery: MidTurnDelivery::StepHook,
            image_input: ImageInputMode::InlineBase64,
            interrupt_in_band: true,
            idle_interrupt_ends_turn: false,
            approval_bridge: true,
            deny_message_reaches_model: true,
            runtime_cancels_approvals: true,
            unsolicited_turns: true,
            session_id_known_up_front: true,
            resume_ignores_cwd: true,
            resume_applies_instructions: false,
            reads_workspace_instruction_files: false,
            tools_detach_from_group: true,
            take_over: true,
            // Enforcement flags stay false until opt-in live tool-call tests prove each
            // guarantee against the real Claude binary, keeping the office approval-bridge
            // fallback active (B2). The plan still passes `--tools` and the session checks
            // `system/init` tools (narrowings only), but `can_use_tool` is not checked against
            // the allowlist and no live test proves it, so the office keeps its own check.
            enforces_writable_roots: false,
            enforces_deny_write: false,
            enforces_deny_read: false,
            enforces_unix_socket_allowlist: false,
            enforces_tool_allowlist: false,
            enforces_deny_rules: false,
            usage_scope: UsageScope::TurnDelta,
            cost: CostBasis::ListPriceEstimate,
            tested_versions: VersionRange {
                min: TESTED_VERSION_MIN.to_owned(),
                max: TESTED_VERSION_MAX.to_owned(),
            },
        }
    }

    fn plan_session(
        &self,
        spec: &SessionSpec,
        ctx: &LaunchContext,
    ) -> Result<(LaunchPlan, Box<dyn Session>), PlanError> {
        let caps = self.capabilities();
        let (plan, expected_mode, expanded_tools) = build_headless_plan(spec, ctx, &caps)?;
        let allowed_tools = expanded_tools.map(|tools| tools.into_iter().collect());
        let allowed_mcp_servers = spec
            .mcp_servers
            .keys()
            .map(|name| name.as_str().to_owned())
            .collect();
        let allowed_mcp_prefixes = spec
            .mcp_servers
            .keys()
            .map(|name| format!("mcp__{}__", name.as_str()))
            .collect();
        let session = ClaudeSession::new(
            spec.agent.name.as_str().to_owned(),
            spec.agent.description.clone(),
            expected_mode,
            allowed_tools,
            allowed_mcp_servers,
            allowed_mcp_prefixes,
        );
        Ok((plan, Box::new(session)))
    }

    fn plan_take_over(
        &self,
        spec: &SessionSpec,
        ctx: &LaunchContext,
        cols: u16,
        rows: u16,
    ) -> Result<LaunchPlan, PlanError> {
        let caps = self.capabilities();
        build_take_over_plan(spec, ctx, &caps, cols, rows)
    }

    fn plan_probe(&self, probe: Probe, ctx: &ProbeContext) -> Option<ProbeRun> {
        probe::plan_probe(probe, ctx)
    }

    fn parse_probe(&self, probe: Probe, out: &ProbeOutput) -> ProbeResult {
        probe::parse_probe(probe, out)
    }

    /// Claude Code answers a turn whose OAuth token or API key was rejected with the error as
    /// the turn's reply text (`Failed to authenticate. API Error: 401 ...`,
    /// `401 OAuth access token has expired`). Only a short reply that starts with one of
    /// those forms counts, so an agent's prose about a 401 never does.
    fn reply_signals_auth_expired(&self, reply: &str) -> bool {
        let trimmed = reply.trim();
        if trimmed.is_empty() || trimmed.len() > 1024 {
            return false;
        }
        let lower = trimmed.to_ascii_lowercase();
        (lower.starts_with("failed to authenticate")
            && (lower.contains("401")
                || lower.contains("oauth")
                || lower.contains("authentication_error")))
            || (lower.starts_with("api error: 401")
                && (lower.contains("oauth") || lower.contains("authentication_error")))
            || lower.starts_with("401 oauth access token has expired")
            || lower == "oauth access token has expired"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["anthropic", "claude-code"]
    }

    fn host_profile(&self) -> RuntimeHostProfile {
        RuntimeHostProfile {
            host_config_subdir: None,
            config_credential_files: &[],
            host_credential_files: &[".claude/.credentials.json"],
            uses_host_oauth_token_injection: true,
            uses_linux_bwrap_sandbox: true,
            needs_explicit_sandbox_ro_dirs: false,
        }
    }

    fn endpoint_env_var(&self) -> Option<&'static str> {
        Some("ANTHROPIC_BASE_URL")
    }

    fn auth_env_var(&self, subscription_cli: bool, secret: &str) -> Option<&'static str> {
        Some(if subscription_cli || secret.starts_with("sk-ant-oat") {
            "CLAUDE_CODE_OAUTH_TOKEN"
        } else {
            "ANTHROPIC_API_KEY"
        })
    }

    fn transcript_parser(&self) -> Option<&dyn TranscriptParser> {
        Some(&transcript::CLAUDE_TRANSCRIPT_PARSER)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expired(reply: &str) -> bool {
        ClaudeRuntime::new().reply_signals_auth_expired(reply)
    }

    #[test]
    fn claude_401_replies_signal_expired_credentials() {
        for reply in [
            r#"Failed to authenticate. API Error: 401 {"type":"error","error":{"type":"authentication_error","message":"OAuth token has expired."}}"#,
            "401 OAuth access token has expired",
            "  401 OAuth access token has expired. Please run /login.\n",
            "OAuth access token has expired",
            "oauth ACCESS token HAS expired",
            "Failed to authenticate: OAuth token revoked",
            r#"API Error: 401 {"type":"error","error":{"type":"authentication_error"}}"#,
            "API Error: 401 OAuth token has expired",
        ] {
            assert!(expired(reply), "{reply:?}");
        }
    }

    #[test]
    fn prose_and_other_errors_do_not_signal_expired_credentials() {
        let long_prose = format!("401 OAuth access token has expired {}", "x".repeat(1200));
        for reply in [
            "I fixed the 401 OAuth access token has expired bug in pump.rs and ran cargo test.",
            long_prose.as_str(),
            "",
            "   \n",
            "Failed to authenticate the webhook signature",
            "API Error: 401",
            "API Error: 500 {\"type\":\"error\",\"error\":{\"type\":\"api_error\"}}",
            "API Error: 429 rate_limit_error",
            "The OAuth access token has expired",
            "OAuth access token has expired, so I refreshed it.",
            "Done. All tests pass.",
        ] {
            assert!(!expired(reply), "{reply:?}");
        }
    }

    #[test]
    fn the_length_limit_counts_the_trimmed_reply() {
        let padded = format!(
            "{}401 OAuth access token has expired{}",
            " ".repeat(600),
            "\n".repeat(600)
        );
        assert!(expired(&padded));
        let at_limit = format!(
            "401 OAuth access token has expired {}",
            "x".repeat(1024 - 35)
        );
        assert_eq!(at_limit.len(), 1024);
        assert!(expired(&at_limit));
        assert!(!expired(&format!("{at_limit}x")));
    }

    #[test]
    fn host_profile_declares_claude_credentials_without_enabling_config_dir() {
        let profile = ClaudeRuntime::new().host_profile();
        assert!(profile.host_config_subdir.is_none());
        assert!(profile.config_credential_files.is_empty());
        assert_eq!(
            profile.host_credential_files,
            &[".claude/.credentials.json"]
        );
    }
}
