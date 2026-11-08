//! The [`AgentRuntime`] factory trait and [`PlanError`].

use std::fmt;

use crate::{
    Capabilities, LaunchContext, LaunchPlan, Probe, ProbeContext, ProbeOutput, ProbePlan,
    ProbeResult, RuntimeId, Session, SessionSpec,
};

/// One runtime's adapter: a **stateless** factory. It never spawns and never does I/O.
///
/// Everything it needs comes in as arguments, so one instance serves every session and
/// every thread (hence `Send + Sync`).
pub trait AgentRuntime: Send + Sync {
    /// This runtime's id.
    fn id(&self) -> RuntimeId;

    /// The binary's usual name (e.g. `claude`), for discovering it on the user's `PATH` once
    /// at preflight. After that the office runs it by its recorded real path.
    fn binary_name(&self) -> &'static str;

    /// What this runtime can and can't do. The same for every session.
    fn capabilities(&self) -> Capabilities;

    /// Plans a headless session (new or resumed) and makes the [`Session`] that will speak
    /// its protocol over the spawned process's pipes.
    ///
    /// **Enforce, drop or refuse**, as [`Capabilities`] sorts the spec's parts: a droppable
    /// restriction it can't enforce (a sandbox list, the deny rules, the tool allowlist) is
    /// dropped, with its `enforces_*` field `false`, so the office has already decided,
    /// before calling this, whether to run without it; a grant it can't express is dropped;
    /// anything else it can't do, it refuses ([`PlanError`]). It never drops a restriction
    /// its capabilities say it enforces. It applies the policy's `allow` rules only if
    /// [`SessionSpec::keeps_allow_rules`] says so, and the permission mode
    /// [`SessionSpec::effective_permission_mode`] gives: when a deny-like restriction is
    /// dropped, every allow goes too, the implicit one in accepting edits included (deny wins
    /// over allow).
    ///
    /// It always adds [`LaunchContext::tmpdir`] as a writable root.
    fn plan_session(
        &self,
        spec: &SessionSpec,
        ctx: &LaunchContext,
    ) -> Result<(LaunchPlan, Box<dyn Session>), PlanError>;

    /// Plans a take-over: the same session, resumed in the runtime's own interactive UI on a
    /// pseudo-terminal of `cols` × `rows`, for a person to work in. Its bytes go to the person,
    /// so there's no [`Session`]. Refuses a one-off.
    fn plan_take_over(
        &self,
        spec: &SessionSpec,
        ctx: &LaunchContext,
        cols: u16,
        rows: u16,
    ) -> Result<LaunchPlan, PlanError>;

    /// Plans a probe, or `None` when the adapter has no plan for it: a probe it documents as
    /// optional for its runtime (e.g. no quota probe where quota comes in-stream), or one it
    /// doesn't know. The office treats `None` for a probe it requires as the runtime being
    /// unusable, never as "skip it", so this fails closed.
    fn plan_probe(&self, probe: Probe, ctx: &ProbeContext) -> Option<ProbePlan>;

    /// Reads a probe's output. Pure: the same output always gives the same result.
    fn parse_probe(&self, probe: Probe, out: &ProbeOutput) -> ProbeResult;
}

/// Why an adapter couldn't plan a launch.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlanError {
    /// This runtime can't do what was asked at all (e.g. take over a runtime with no
    /// interactive UI, or a permission mode it doesn't know). The text names the feature.
    Unsupported(&'static str),
    /// The spec or context can't be honoured as given (e.g. a one-off asked to take over, a
    /// new Claude session without [`LaunchContext::new_session_id`], an agent name the
    /// runtime can't use). The text says what's wrong; it never quotes a secret.
    InvalidSpec(String),
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlanError::Unsupported(what) => write!(f, "not supported by this runtime: {what}"),
            PlanError::InvalidSpec(why) => write!(f, "invalid session spec: {why}"),
        }
    }
}

impl std::error::Error for PlanError {}
