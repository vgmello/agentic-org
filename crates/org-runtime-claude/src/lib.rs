//! # org-runtime-claude: the Claude Code adapter
//!
//! Implements [`org_runtime::AgentRuntime`] for Claude Code's headless stream-json mode
//! (`claude -p --input-format stream-json --output-format stream-json`), tested against
//! Claude Code **2.1.311** through the recorded transcripts of the S0 spike
//! (`spikes/claude/`).
//!
//! - [`ClaudeRuntime`] is the stateless factory: launch plans (argv, the generated
//!   `settings.json`, `mcp.json` and `instructions.md`, the stop plan), probes and
//!   [`Capabilities`].
//! - The session it makes is a pure state machine over the stream-json protocol (see the
//!   `session` module's docs for the mapping).
//!
//! ## Isolation
//! Agents use the user's real `~/.claude`, so the sign-in works, isolated by three
//! switches: `--setting-sources ""` (no settings, instruction files, skills, agents or
//! commands from anywhere), the office's own `--settings` file (`autoMemoryEnabled: false`,
//! `permissions.disableBypassPermissionsMode: "disable"`, the policy and the sandbox), and
//! `--strict-mcp-config` with the office's `--mcp-config`. Every agent's tools are denied
//! reading `~/.claude` (all agents' transcripts), `~/.claude.json` and `/tmp/cc-socks`
//! (every Claude process's messaging socket), by the sandbox (Bash) and by `Read` rules
//! (the file tools).
//!
//! ## Evidence
//! What rests on the spike's recorded runs, and what was only read from the CLI binary, is
//! noted where it's used, as **\[binary\]** for the latter.

#![deny(unsafe_code)]
#![warn(missing_docs)]

mod approval;
mod outcome;
mod plan;
mod probe;
mod ratelimit;
mod rules;
mod session;
mod usage;

use org_runtime::{
    AgentRuntime, Capabilities, CostBasis, LaunchContext, LaunchPlan, PlanError, Probe,
    ProbeContext, ProbeOutput, ProbeResult, ProbeRun, RuntimeId, Session, SessionSpec, UsageScope,
    VersionRange,
};

/// This adapter's runtime id.
pub const RUNTIME_ID: &str = "claude";

/// The Claude Code versions this adapter was tested with.
pub const TESTED_VERSION: &str = "2.1.311";

/// The Claude Code adapter. Stateless: one instance serves every session.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClaudeRuntime;

impl ClaudeRuntime {
    /// The adapter.
    pub fn new() -> Self {
        ClaudeRuntime
    }
}

impl AgentRuntime for ClaudeRuntime {
    fn id(&self) -> RuntimeId {
        RuntimeId::try_from(RUNTIME_ID).expect("the runtime id is a valid token")
    }

    fn binary_name(&self) -> &'static str {
        "claude"
    }

    /// Claude Code 2.1.311's capabilities, per the spike (`spikes/claude/README.md`):
    /// - no steering: urgent input interrupts, then sends; the interrupt is in-band and an
    ///   idle one only acknowledges (item 3);
    /// - approvals bridge with `--permission-prompt-tool stdio`, a deny's message reaches the
    ///   model, and Claude withdraws a pending approval on interrupt (item 2);
    /// - background work can start a turn after a `result` (fact 12);
    /// - the conversation id is chosen up front (`--session-id`), a resume works from any
    ///   cwd, and the system prompt is a snapshot, so changed instructions don't apply on
    ///   resume (item 4);
    /// - no instruction file is read from the workspace under `--setting-sources ""`
    ///   (item 5, fact 3);
    /// - each Bash call runs in its own session and process group (item 7);
    /// - take-over resumes the conversation in the TUI (item 10);
    /// - the sandbox enforces the writable roots, `denyWrite` (items 8, 9) and the socket
    ///   allowlist (item 8); read denies (`sandbox.filesystem.denyRead` plus `Read` rules)
    ///   are **\[binary\]** and design-decided, not yet run (S1 verifies them); permission deny
    ///   rules apply (item 5) and the agent's `tools` limit the main thread (item 6).
    ///   **Note for the office:** Claude's sandbox opens parts of a linked worktree's
    ///   `work.git` by itself (`HEAD`, `packed-refs`, `info/`, `description`, `FETCH_HEAD`,
    ///   `worktrees/<id>/gitdir`; spike fact 4), and its cwd; only `denyWrite` entries (the
    ///   spec's `deny_write`) take those back, so the office must list them.
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            steer: false,
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
            enforces_writable_roots: true,
            enforces_deny_write: true,
            enforces_deny_read: true,
            enforces_unix_socket_allowlist: true,
            enforces_tool_allowlist: true,
            enforces_deny_rules: true,
            usage_scope: UsageScope::TurnDelta,
            cost: CostBasis::ListPriceEstimate,
            tested_versions: VersionRange {
                min: TESTED_VERSION.to_owned(),
                max: TESTED_VERSION.to_owned(),
            },
        }
    }

    fn plan_session(
        &self,
        spec: &SessionSpec,
        ctx: &LaunchContext,
    ) -> Result<(LaunchPlan, Box<dyn Session>), PlanError> {
        let planned = plan::plan(spec, ctx, &self.capabilities(), plan::Mode::Headless)?;
        let session = session::ClaudeSession::new(
            spec.agent.name.as_str(),
            planned.reported_mode,
            planned.plan.runtime_session_id.clone(),
            planned.writable,
        );
        Ok((planned.plan, Box::new(session)))
    }

    fn plan_take_over(
        &self,
        spec: &SessionSpec,
        ctx: &LaunchContext,
        cols: u16,
        rows: u16,
    ) -> Result<LaunchPlan, PlanError> {
        let mode = plan::Mode::TakeOver { cols, rows };
        Ok(plan::plan(spec, ctx, &self.capabilities(), mode)?.plan)
    }

    fn plan_probe(&self, probe: Probe, ctx: &ProbeContext) -> Option<ProbeRun> {
        probe::plan(probe, ctx)
    }

    fn parse_probe(&self, probe: Probe, out: &ProbeOutput) -> ProbeResult {
        probe::parse(probe, out, &self.capabilities().tested_versions)
    }
}
