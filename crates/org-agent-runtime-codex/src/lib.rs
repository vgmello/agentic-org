//! # org-agent-runtime-codex: sans-I/O OpenAI Codex (GPT) adapter
//!
//! Implements the [`org_agent_runtime::AgentRuntime`] and [`org_agent_runtime::Session`]
//! contracts for Codex CLI (`codex app-server --stdio`).
//!
//! This crate performs **no I/O and spawns no processes**: `org-supervise` executes every
//! [`org_agent_runtime::LaunchPlan`] and [`org_agent_runtime::ProbePlan`].

#![forbid(unsafe_code)]

mod plan;
mod probe;
mod protocol;
mod session;

use org_agent_runtime::{
    AgentRuntime, Capabilities, CostBasis, LaunchContext, LaunchPlan, MidTurnDelivery, PlanError,
    Probe, ProbeContext, ProbeOutput, ProbeResult, ProbeRun, RuntimeId, Session, SessionSpec,
    UsageScope, VersionRange,
};

use crate::plan::{build_headless_plan, build_take_over_plan};
use crate::probe::{TESTED_VERSION_MAX, TESTED_VERSION_MIN};
use crate::session::CodexSession;

/// Stateless factory for Codex (`codex app-server --stdio`) headless and take-over sessions.
#[derive(Debug, Default, Clone, Copy)]
pub struct CodexRuntime;

impl CodexRuntime {
    /// Creates a new stateless Codex CLI runtime adapter.
    pub const fn new() -> Self {
        Self
    }
}

impl AgentRuntime for CodexRuntime {
    fn id(&self) -> RuntimeId {
        RuntimeId::try_from("codex").expect("static runtime id `codex` is valid")
    }

    fn binary_name(&self) -> &'static str {
        "codex"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            mid_turn_delivery: MidTurnDelivery::NativeSteer,
            interrupt_in_band: true,
            idle_interrupt_ends_turn: false,
            approval_bridge: true,
            deny_message_reaches_model: true,
            runtime_cancels_approvals: true,
            unsolicited_turns: false,
            session_id_known_up_front: false,
            resume_ignores_cwd: false,
            resume_applies_instructions: true,
            reads_workspace_instruction_files: false,
            tools_detach_from_group: true,
            take_over: true,
            // Enforcement flags stay false until live-binary tests verify each guarantee,
            // keeping the office approval-bridge fallback active (B2).
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
        let (plan, thread_cfg) = build_headless_plan(spec, ctx, &caps)?;
        let session = CodexSession::new(thread_cfg);
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_auth_error_replies_are_ordinary_replies_for_codex() {
        // Codex has no in-band expired-credentials reply, so the default applies and
        // Claude's 401 forms are ordinary reply text.
        let runtime = CodexRuntime::new();
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
