//! [`Capabilities`]: where runtimes differ, so the office can adapt instead of guessing.

use crate::{CostBasis, UsageScope};

/// What a runtime can and can't do. Fixed per adapter (and version range); the office reads it
/// to choose how to act, e.g. interrupt-then-send where there's no steering.
///
/// ## Each part of the spec: enforced, dropped, or refused
/// The contract ([`crate::AgentRuntime::plan_session`]) sorts every part of the spec into one
/// of these groups:
/// - **Droppable restrictions**, each with an `enforces_*` field here: the sandbox's
///   `writable_roots`, `deny_write`, `deny_read` and `allowed_unix_sockets`, the policy's
///   `deny` rules, and `tools`. One the runtime can't enforce is **dropped** from the plan,
///   and its field is `false` (e.g. read denies under Codex's sandbox). The office checks
///   these fields **before** it plans, decides whether the session may run without that
///   restriction, and tells the owner, so a drop is declared, never silent. With
///   `sandbox.enabled: false` the office asks for no sandbox, and the four sandbox lists are
///   dropped whatever these fields say.
///   - Where a field is `true`, every item of that kind is enforced exactly: one the runtime
///     can't fully enforce (e.g. a path deny a shell tool could get around, where the sandbox
///     doesn't cover it) makes the adapter **refuse** the spec, never drop or approximate it.
///   - A restriction is never **narrowed** to fit a runtime's syntax.
/// - **Grants**, which only widen what the agent may do: the policy's `allow` rules, and the
///   read access [`crate::SessionSpec::read_only_dirs`] gives. One the runtime can't express
///   exactly is dropped, never **widened** (`Bash` + prefix `git status` never becomes all of
///   `Bash`); dropping a grant only makes the session ask more often.
///   - **Deny wins over allow, always:** when the adapter drops any restriction that denies
///     something (the policy's non-empty `deny` rules, the sandbox's non-empty `deny_write`
///     or `deny_read` because its field here is `false` or the sandbox is off, or a `tools`
///     allowlist), it **drops every allow rule too**, and applies
///     [`crate::PermissionMode::AcceptEdits`] as `Ask` (accepting edits is an implicit
///     allow), so that whatever the dropped restriction covered still reaches the approval
///     bridge, where the office applies its full policy.
///     [`crate::SessionSpec::keeps_allow_rules`] and
///     [`crate::SessionSpec::effective_permission_mode`] decide it, so every adapter applies
///     the same rule.
///   - While `writable_roots` is enforced, writability never comes from a grant: only the
///     writable roots (and the session's `TMPDIR`) are writable.
/// - **Settings the runtime must apply, or refuse the spec** ([`crate::PlanError`]): the
///   permission mode, the `workspace` as the cwd, the `model` and `effort` when given, the
///   `instructions`, a sandbox that's asked for (`enabled: true`) when the runtime has none,
///   and "only the office's MCP servers load".
/// - **Identity, applied where the runtime has the concept:** the agent's `name` is how a
///   runtime with agents (Claude, Antigravity) selects it, and must be selected there; the
///   `description` and `prompt` are presentational. A runtime with no agents (Codex) uses
///   none of them, and nothing is lost: none grants or restricts anything.
/// - **Metadata, with nothing to enforce:** `runtime` (which adapter), `kind`, and the
///   policy's `version`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    /// Urgent input can steer a running turn ([`crate::Control::Send`] with `urgent`).
    /// Otherwise the office interrupts, then sends.
    pub steer: bool,
    /// [`crate::Control::Interrupt`] stops the turn in-band and the process lives on.
    /// Otherwise interrupting means stopping the process and resuming.
    pub interrupt_in_band: bool,
    /// Interrupting an idle session still produces a [`crate::SessionEvent::TurnEnded`].
    /// (Claude's doesn't: it only acknowledges.)
    pub idle_interrupt_ends_turn: bool,
    /// Approval requests reach the office ([`crate::SessionEvent::ApprovalRequested`]).
    /// Otherwise actions that need approval are denied, and the run ends blocked.
    pub approval_bridge: bool,
    /// A [`crate::Decision::Deny`] message reaches the model.
    pub deny_message_reaches_model: bool,
    /// The runtime withdraws pending approvals itself (e.g. on interrupt), with
    /// [`crate::SessionEvent::ApprovalCancelled`].
    pub runtime_cancels_approvals: bool,
    /// The runtime can begin a turn on its own (background work finishing after a turn ended).
    pub unsolicited_turns: bool,
    /// The runtime's session id is known before the first turn
    /// ([`crate::LaunchPlan::runtime_session_id`]).
    pub session_id_known_up_front: bool,
    /// A resume works whatever the cwd (Claude). Otherwise conversations are tied to the cwd,
    /// and the office must resume in the original workspace.
    pub resume_ignores_cwd: bool,
    /// Changed instructions take effect on resume. Otherwise (Claude snapshots its system
    /// prompt) the office sends the change as a message.
    pub resume_applies_instructions: bool,
    /// The runtime reads instruction files (`AGENTS.md`, `CLAUDE.md`, ...) from the workspace
    /// on its own, so the task branch's content steers it (and is untrusted).
    pub reads_workspace_instruction_files: bool,
    /// Tools can leave the runtime's process group (Claude runs each shell call in its own
    /// session), so stopping and orphan cleanup can't rely on the group alone.
    pub tools_detach_from_group: bool,
    /// The session can be taken over in the runtime's own interactive UI.
    pub take_over: bool,
    /// [`crate::SandboxSpec::writable_roots`] (and the session's `TMPDIR`) are the only
    /// places the agent can write **by any path**: the runtime's own file tools (edit,
    /// write, patch) included, not only commands in its shell sandbox.
    ///
    /// The requirement: every write outside the writable roots is **impossible** for the
    /// file tools, or **reaches the approval bridge**, where the office denies it. An adapter
    /// whose runtime sandboxes only its shell sets this `true` only if it confines the file
    /// tools that way for every path outside the roots. Denying writes to the read-only dirs
    /// (like Claude's `Edit`/`Write` denies for each one) is only part of that: the rest of the
    /// file system needs it too. Otherwise the list is dropped.
    pub enforces_writable_roots: bool,
    /// [`crate::SandboxSpec::deny_write`] is enforced. Otherwise it's dropped, and with it
    /// every allow rule if it wasn't empty (see [`crate::SessionSpec::keeps_allow_rules`]).
    pub enforces_deny_write: bool,
    /// [`crate::SandboxSpec::deny_read`] is enforced. Otherwise it's dropped, and with it
    /// every allow rule if it wasn't empty (see [`crate::SessionSpec::keeps_allow_rules`]).
    pub enforces_deny_read: bool,
    /// The agent can connect only to [`crate::SandboxSpec::allowed_unix_sockets`]. Otherwise
    /// the list is dropped (and other sockets may be reachable).
    pub enforces_unix_socket_allowlist: bool,
    /// [`crate::SessionSpec::tools`] limits the agent's tools. Otherwise it's dropped, and
    /// with it every allow rule (see [`crate::SessionSpec::keeps_allow_rules`]).
    pub enforces_tool_allowlist: bool,
    /// The policy's `deny` rules ([`crate::PermissionPolicy::deny`]) are applied by the
    /// runtime itself, before it acts, each exactly (or the spec is refused). Otherwise they're
    /// dropped from the plan **together with every allow rule**, so the runtime asks before
    /// anything a deny would have covered, and the office applies its full policy at the
    /// approval bridge.
    pub enforces_deny_rules: bool,
    /// How the runtime's [`crate::Usage`] token counts usually count, for planning. Each
    /// event's own `token_scope` and `cost_scope` are what the office acts on.
    pub usage_scope: UsageScope,
    /// What the runtime's costs mean.
    pub cost: CostBasis,
    /// The runtime versions this adapter was tested with. Outside them the office warns but
    /// doesn't block.
    pub tested_versions: VersionRange,
}

/// An inclusive range of runtime versions, in the runtime's own version syntax. Only the
/// adapter compares versions (it knows the syntax): see
/// [`crate::ProbeResult::Version`]'s `in_tested_range`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VersionRange {
    /// The oldest tested version.
    pub min: String,
    /// The newest tested version.
    pub max: String,
}
