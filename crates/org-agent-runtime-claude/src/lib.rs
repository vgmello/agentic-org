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

use org_agent_runtime::{
    AgentRuntime, Capabilities, CostBasis, LaunchContext, LaunchPlan, MidTurnDelivery, PlanError,
    Probe, ProbeContext, ProbeOutput, ProbeResult, ProbeRun, RuntimeId, Session, SessionSpec,
    UsageScope, VersionRange,
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
}
