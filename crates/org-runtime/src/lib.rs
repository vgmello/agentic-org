//! # org-runtime: the agent runtime port
//!
//! The contract every agent runtime adapter implements (`org-runtime-claude`,
//! `org-runtime-codex`, `org-runtime-agy`), and the office's session layer
//! (`org-office-sessions`) drives. See `docs/design/agent-sessions.md`, decision 2.
//!
//! **A contract: keep it small, stable, additive-only.** Every change goes through ada
//! (see AGENTS.md). Two adapters are built against it independently.
//!
//! ## The shape
//! - [`AgentRuntime`] is a stateless factory per runtime. From a [`SessionSpec`] (what the
//!   office asks for, deterministic and hashed) and a [`LaunchContext`] (per-run facts) it
//!   builds a [`LaunchPlan`] (what to spawn: argv, files, env additions, how to stop it) and
//!   a [`Session`]. It also plans and parses [`Probe`]s (version, sign-in, quota).
//! - [`Session`] is **sans-I/O**: a pure state machine. It never spawns, reads, writes,
//!   sleeps or reads the clock. A pump (in `org-office-sessions`) owns the child's pipes:
//!   it splits stdout with [`LineFramer`], feeds each frame in, writes every
//!   [`Output::Write`] to stdin, forwards every [`Output::Event`], and calls
//!   [`Session::on_tick`] at [`Session::next_deadline`]. Time is an input (`now: Instant`)
//!   on every call, so protocol timeouts are pure and testable from fixtures.
//! - [`SessionEvent`] is the normalized stream every runtime maps onto, with [`Usage`],
//!   [`RateLimit`] and [`TurnOutcome`]. [`Capabilities`] says where runtimes differ.
//! - [`encode_line`] and [`LineFramer`] are the one encoder and framer every adapter and the
//!   pump share, so line handling can't drift between runtimes.
//!
//! The adapter never spawns, so `org-supervise` stays the one place that does. Adapters
//! depend only on this crate.
//!
//! ## Invariants
//! What the types enforce:
//! - **No bypass mode.** [`PermissionMode`] has no bypass variant, and a request for one
//!   doesn't decode.
//! - **Tool input is never rewritten.** A [`Decision`] allows or denies the call as the
//!   runtime asked for it; there's no way to send changed input (Claude's `updatedInput`).
//! - **No per-run values reach the spec; no env values reach the adapter.** The session
//!   token, the session mark, the session's `TMPDIR` and the conversation to resume aren't
//!   fields of [`SessionSpec`], which is stored and hashed; the adapter gets environment
//!   variable **names** only ([`LaunchContext::office_env_names`]).
//! - **Nothing structured can break out of where it's written.** Every path, pattern, name and
//!   token in the spec, the launch context or a plan that reaches argv, a config file, a rule
//!   or an environment name is a validated type
//!   ([`SpecPath`], [`PathPattern`], [`CommandPrefix`], [`AgentName`], [`Token`],
//!   [`RuntimeId`], [`ToolName`], [`RuleTool`], [`McpServerName`], [`EnvName`],
//!   [`SessionId`], [`PlainId`]), checked when made and when decoded, and the plan-only
//!   [`RelPath`] and [`CleanupPath`], checked when made (they're never decoded). The one
//!   exception is the runtime's binary ([`LaunchContext::runtime_path`]), the real path the
//!   office recorded at preflight.
//! - **Every protocol line is encoded, never formatted.** [`Output::Write`] and
//!   [`ProbePlan::stdin_lines`] carry a [`Line`], which only [`encode_line`] makes.
//! - **Escalations never ride on policy.** Every [`ApprovalRequest`] carries an
//!   [`Escalation`]: the office always denies [`Escalation::SandboxBypass`] (the fixed denies
//!   win, with no override in phase 1), and sends [`Escalation::PermissionChange`] and
//!   [`Escalation::Unknown`] to the owner with a warning, never answering them from policy.
//! - **Redaction is possible everywhere.** [`SessionEvent::redact`] reaches every free-text
//!   string, including inside JSON values. Events carry no raw runtime messages: tool input
//!   is their only JSON, and `redact` reaches all of it.
//!
//! What adapters must do (the types can't stop it, so reviews check it):
//! - Never put a runtime's skip-permissions switch in [`LaunchPlan::args`] or
//!   [`LaunchPlan::env_add`].
//! - Enforce, drop or refuse each part of the spec as [`Capabilities`] sorts them: drop a
//!   restriction only where its `enforces_*` field says so, and refuse what it can't do.
//! - Apply the policy's `allow` rules only if [`SessionSpec::keeps_allow_rules`] says so
//!   (deny wins over allow).
//! - Turn a session id from the runtime that isn't a valid [`SessionId`] into a
//!   [`SessionEvent::ProtocolError`].
//! - Always add [`LaunchContext::tmpdir`] as a writable root.
//! - List a [`StopPlan`]'s graceful steps ([`StopStep::CloseStdin`]) before its signals. They
//!   keep their full grace; the first signal the supervisor sends is always `SIGTERM` to the
//!   runtime's own process, before the plan's signal steps.
//! - Refuse a spec whose `workspace` or a writable root is `/` (a stored spec with one
//!   doesn't decode; one built in code reaches the adapter, which checks
//!   [`SpecPath::is_root`] and returns [`PlanError::InvalidSpec`]).
//! - Set every [`ApprovalRequest::escalation`] from what the runtime asks (see
//!   [`Escalation`]).
//! - Write every deny rule so it beats a matching allow, whatever the rules' order.
//! - Run the `testkit` checks, [`Session`]'s and the plan's, in the adapter's own tests.
//! - Name every [`CleanupPath`] by its real path (`/private/tmp/...`, not `/tmp/...` on
//!   macOS): the supervisor follows no symlink, so a path through one is never cleaned up.
//! - Never put a secret in argv, a [`PlannedFile`], an event or an error text.
//! - Decline runtime requests that need content back (see [`ApprovalRequest`]).
//!
//! What the office must do: keep secrets out of the spec's free text, which may reach argv or
//! files ([`SessionSpec::instructions`], [`AgentIdentity::description`] and
//! [`AgentIdentity::prompt`], [`McpServer::args`]).
//!
//! ## Conformance testkit
//! With the `testkit` feature, the `testkit` module checks the [`Session`] contract rules
//! against any session, driven by the adapter's own fixture lines. Every adapter runs it in
//! its tests (as a dev-dependency with `features = ["testkit"]`).
//!
//! ## Deferred: `RuntimeHost`
//! The design names an optional `RuntimeHost`, a shared host process serving several
//! sessions. Every runtime runs one process per session today (Codex included: one
//! `app-server` per session), so nothing would use it, and it isn't defined here. It will be
//! added, additively, if a runtime ever shares one process between sessions.
//!
//! ## Compatibility rules
//! - Public enums that cross the boundary are `#[non_exhaustive]`: match them with a
//!   wildcard arm, and meet a variant you don't know as a restriction you can't honour,
//!   never by guessing:
//!   - an unknown [`PermissionMode`] or [`SessionKind`]: refuse the spec
//!     ([`PlanError::Unsupported`]);
//!   - an unknown [`RuleScope`] in a **deny** rule: refuse the spec; in an **allow** rule:
//!     drop that grant (dropping a grant only makes the session ask more);
//!   - an unknown [`Control`] or [`Decision`]: refuse it ([`ControlError::Unsupported`]);
//!   - an unknown [`Probe`]: `plan_probe` returns `None` (it has no plan for it) and
//!     `parse_probe` reports it [`Unavailable`]. Otherwise `plan_probe` returns `None` only
//!     for a probe the adapter documents as optional for its runtime (e.g. no quota probe
//!     where quota comes in-stream);
//!   - on the office's side, `None` from `plan_probe` for a probe the office **requires**
//!     means the runtime is unusable ([`Unavailable`]), never "skip it", so an adapter
//!     that can't plan a probe fails closed. The required set is the office's, not taken
//!     from adapters: [`Probe::Version`] and [`Probe::Auth`] for every runtime, and
//!     [`Probe::Quota`] only for a runtime whose quota the office polls;
//!   - on the office's side, an unknown [`PlanError`], [`ControlError`], [`ProbeResult`] or
//!     [`Unavailable`]: a failure, or the runtime unusable, never a success;
//!   - a runtime-side value that decoded as `Unknown` (an event, outcome, status, ...):
//!     treat it as unknown, never as some known value.
//! - Structs that may grow are `#[non_exhaustive]`, with a constructor: [`ApprovalRequest`]
//!   ([`ApprovalRequest::new`]), [`Usage`] ([`Usage::new`]), [`RateLimit`]
//!   ([`RateLimit::new`]), [`ProbeContext`] ([`ProbeContext::new`]) and [`ProbeOutput`]
//!   ([`ProbeOutput::new`]). Build them that way and set the public fields after, so a field
//!   added later isn't a breaking change.
//! - Every other public struct is **exhaustive on purpose**, so a new field is a breaking
//!   change that makes every builder and reader decide about it:
//!   - [`SessionSpec`] and its parts ([`AgentIdentity`], [`PermissionPolicy`], [`Rule`],
//!     [`SandboxSpec`], [`McpServer`]): a new field may be a restriction, and the office must
//!     set it and every adapter enforce, drop or refuse it;
//!   - [`LaunchContext`]: a new per-run value is one the office must supply;
//!   - [`LaunchPlan`], [`PlannedFile`], [`PlannedDir`], [`StopPlan`], [`ProbePlan`]: the
//!     supervisor must carry out every field;
//!   - [`Capabilities`] (and [`VersionRange`]): every adapter must declare a new capability
//!     rather than default it;
//!   - [`ExitInfo`]: it mirrors the OS's exit status, which won't grow.
//! - The enums that instruct the pump or the supervisor are **exhaustive** on purpose:
//!   [`Output`], [`Frame`], [`FileLocation`], [`Stdio`], [`StopStep`], [`StopSignal`],
//!   [`SignalTarget`], [`ProbeRun`] and [`ProbeStep`]. Their reader can't safely skip a variant it doesn't know, so a new one
//!   is a breaking change, and the reader's build fails until it handles it.
//! - Values that originate in a runtime (events, usage, rate limits) decode unknown enum
//!   values as `Unknown`, and an unknown event type as [`SessionEvent::Unknown`], so a newer
//!   writer never breaks an older reader. Values the office writes ([`SessionSpec`] and its
//!   parts) don't: an unknown permission mode, session kind or field fails to decode, because
//!   guessing there would weaken isolation.
//! - [`SessionSpec::spec_hash`] is part of the contract: changing the canonical form or the
//!   hash changes every stored hash.

#![deny(unsafe_code)]
#![warn(missing_docs)]

mod caps;
mod event;
mod line;
mod path;
mod plan;
mod probe;
mod runtime;
mod session;
mod spec;
mod token;
mod usage;

#[cfg(feature = "testkit")]
pub mod testkit;

pub use caps::{Capabilities, VersionRange};
pub use event::{
    Activity, ApprovalId, ApprovalKind, ApprovalRequest, Escalation, ExitInfo, LimitKind,
    NoticeKind, SessionEvent, StartFailure, TurnOutcome,
};
pub use line::{encode_line, Frame, Line, LineFramer, MAX_LINE_BYTES};
pub use path::{PathPattern, RelPath, SpecPath, SpecPathError};
pub use plan::{
    CleanupPath, FileLocation, LaunchPlan, PlannedDir, PlannedFile, ProbePlan, SignalTarget, Stdio,
    StopPlan, StopSignal, StopStep,
};
pub use probe::{
    Probe, ProbeContext, ProbeOutput, ProbeResult, ProbeRun, ProbeSession, ProbeStep, Unavailable,
};
pub use runtime::{AgentRuntime, PlanError};
pub use session::{Control, ControlError, Decision, Output, Session};
pub use spec::{
    AgentIdentity, LaunchContext, McpServer, PermissionMode, PermissionPolicy, Rule, RuleScope,
    SandboxSpec, SessionKind, SessionSpec,
};
pub use token::{
    AgentName, CommandPrefix, EnvName, InvalidValue, McpServerName, PlainId, RuleTool, RuntimeId,
    SessionId, Token, ToolName,
};
pub use usage::{CostBasis, LimitStatus, RateLimit, Usage, UsageScope, Window};
