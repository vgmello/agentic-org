//! # org-agent-runtime-agy: sans-I/O Google Antigravity (`agy acp --stdio`) adapter
//!
//! Implements the [`org_agent_runtime::AgentRuntime`] and [`org_agent_runtime::Session`]
//! contracts for Antigravity CLI over its native Agent Client Protocol (`agy acp --stdio`,
//! verified live on `agy 1.5.16–1.5.17`).
//!
//! This crate performs **no I/O and spawns no processes**: `org-supervise` executes every
//! [`org_agent_runtime::LaunchPlan`] and [`org_agent_runtime::ProbePlan`].

#![forbid(unsafe_code)]

mod plan;
mod probe;
mod protocol;
mod session;
mod transcript;

use org_agent_runtime::transcript::TranscriptParser;
use org_agent_runtime::{
    AgentRuntime, Capabilities, CostBasis, ImageInputMode, LaunchContext, LaunchPlan,
    MidTurnDelivery, PlanError, Probe, ProbeContext, ProbeOutput, ProbeResult, ProbeRun,
    RuntimeHostProfile, RuntimeId, Session, SessionSpec, UsageScope, VersionRange,
};

pub use crate::plan::AGY_CONFIG_CREDENTIAL_FILES;
use crate::plan::{build_headless_plan, build_take_over_plan};
use crate::probe::{TESTED_VERSION_MAX, TESTED_VERSION_MIN};
use crate::session::AgySession;

/// Stateless factory for Antigravity (`agy acp --stdio`) headless and take-over sessions.
#[derive(Debug, Default, Clone, Copy)]
pub struct AgyRuntime;

impl AgyRuntime {
    /// Creates a new stateless Antigravity ACP runtime adapter.
    pub const fn new() -> Self {
        Self
    }
}

impl AgentRuntime for AgyRuntime {
    fn id(&self) -> RuntimeId {
        RuntimeId::try_from("agy").expect("static runtime id `agy` is valid")
    }

    fn binary_name(&self) -> &'static str {
        "agy"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            mid_turn_delivery: MidTurnDelivery::NativeSteer,
            image_input: ImageInputMode::StagedFileOnly,
            interrupt_in_band: true,
            idle_interrupt_ends_turn: false,
            approval_bridge: true,
            // ACP `session/request_permission` selects an optionId ("allow"/"deny"); no free-form
            // deny reason string is carried in the ACP Selected outcome.
            deny_message_reaches_model: false,
            runtime_cancels_approvals: true,
            unsolicited_turns: false,
            session_id_known_up_front: false,
            resume_ignores_cwd: false,
            resume_applies_instructions: true,
            reads_workspace_instruction_files: false,
            tools_detach_from_group: true,
            take_over: true,
            // Enforcement flags remain false until live-binary confinement tests prove every
            // guarantee, keeping the office approval-bridge fallback active (B2).
            enforces_writable_roots: false,
            enforces_deny_write: false,
            enforces_deny_read: false,
            enforces_unix_socket_allowlist: false,
            enforces_tool_allowlist: false,
            enforces_deny_rules: false,
            usage_scope: UsageScope::TurnDelta,
            cost: CostBasis::None,
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
        let (plan, cfg) = build_headless_plan(spec, ctx, &caps)?;
        Ok((plan, Box::new(AgySession::new(cfg))))
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

    fn aliases(&self) -> &'static [&'static str] {
        &["google", "antigravity", "antigravity-cli", "gemini"]
    }

    fn host_profile(&self) -> RuntimeHostProfile {
        RuntimeHostProfile {
            host_config_subdir: Some(".gemini"),
            config_credential_files: AGY_CONFIG_CREDENTIAL_FILES,
            host_credential_files: &[],
            uses_host_oauth_token_injection: false,
            uses_mount_mask_sandbox: false,
            needs_explicit_sandbox_ro_dirs: true,
        }
    }

    fn endpoint_env_var(&self) -> Option<&'static str> {
        Some("GOOGLE_GEMINI_BASE_URL")
    }

    fn auth_env_var(&self, subscription_cli: bool, _secret: &str) -> Option<&'static str> {
        Some(if subscription_cli {
            "AGY_AUTH_TOKEN"
        } else {
            "GEMINI_API_KEY"
        })
    }

    fn transcript_parser(&self) -> Option<&dyn TranscriptParser> {
        Some(&transcript::AGY_TRANSCRIPT_PARSER)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_auth_error_replies_are_ordinary_replies_for_agy() {
        // Antigravity has no in-band expired-credentials reply, so the default applies and
        // Claude's 401 forms are ordinary reply text.
        let runtime = AgyRuntime::new();
        for reply in [
            r#"Failed to authenticate. API Error: 401 {"type":"error","error":{"type":"authentication_error","message":"OAuth token has expired."}}"#,
            "401 OAuth access token has expired",
            "OAuth access token has expired",
            "API Error: 401 OAuth token has expired",
        ] {
            assert!(!runtime.reply_signals_auth_expired(reply), "{reply:?}");
        }
    }
}
