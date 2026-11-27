//! Thin [`SandboxRunner`] & [`SandboxPlan`] confinement seam and [`LocalOsSandbox`] adapter
//! (`TASK-233`, `TASK-256`).
//!
//! Isolates all host-OS confinement planning (such as Linux `bwrap` directory pre-creation,
//! absent-path deny pruning, and post-launch sibling isolation) inside one designated adapter
//! module, keeping session launch, spec building, installation, wake draining, and the session
//! I/O pump free of concrete OS sandbox primitives.

mod bwrap_isolation;
mod bwrap_pruning;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use org_agent_runtime::{RuntimeHostProfile, SessionSpec};
use org_office::Office;
use org_proto::{ApiError, ErrorCode};

use bwrap_isolation::{
    enforce_bwrap_sibling_isolation, ensure_bwrap_deny_dirs_with,
    has_unmasked_bwrap_sibling_in_office,
};
use bwrap_pruning::{apply_linux_claude_bwrap_pruning, is_linux_bwrap_sandbox};

use super::office_masks::OfficeMasks;
use super::SessionManager;

#[cfg(test)]
pub(crate) use bwrap_isolation::{
    enforce_bwrap_sibling_isolation as enforce_bwrap_sibling_isolation_for_test,
    ensure_bwrap_deny_dirs,
};
#[cfg(test)]
pub(crate) use bwrap_pruning::{
    apply_linux_claude_bwrap_pruning as apply_linux_claude_bwrap_pruning_for_test,
    pattern_exists_for_bwrap, prune_nonexistent_bwrap_denies,
};

/// Confinement mode selected by a [`SandboxRunner`] for a session launch.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxConfinementMode {
    /// Session runs without OS sandbox confinement (`sandbox_enabled == false`).
    Unsandboxed,
    /// Native host-OS confinement provided directly by the platform/runtime without bwrap
    /// mount-mask pre-creation or absent-deny pruning.
    HostNative,
    /// Linux `bwrap` directory-mask confinement requiring pre-created `0700` deny directories,
    /// absent-path pruning, and post-launch sibling staleness checks (`TASK-144`, `TASK-166`).
    LinuxBwrap,
}

/// Opaque confinement plan produced by [`SandboxRunner::prepare_launch`] and carried across
/// session launch, live handles, and staleness checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxPlan {
    mode: SandboxConfinementMode,
    masked_siblings: Option<BTreeSet<String>>,
    office_masks_digest: Option<String>,
    prune_absent_denies: bool,
}

impl SandboxPlan {
    fn new(
        mode: SandboxConfinementMode,
        masked_siblings: Option<BTreeSet<String>>,
        prune_absent_denies: bool,
    ) -> Self {
        Self {
            mode,
            masked_siblings,
            office_masks_digest: None,
            prune_absent_denies,
        }
    }

    pub fn unsandboxed() -> Self {
        Self::new(SandboxConfinementMode::Unsandboxed, None, false)
    }

    pub fn host_native() -> Self {
        Self::new(SandboxConfinementMode::HostNative, None, false)
    }

    pub fn linux_bwrap(masked_siblings: BTreeSet<String>) -> Self {
        Self::new(
            SandboxConfinementMode::LinuxBwrap,
            Some(masked_siblings),
            true,
        )
    }

    /// Attaches a masked sibling snapshot to this plan (for custom [`SandboxRunner`] adapters).
    pub fn with_masked_siblings(mut self, masked_siblings: Option<BTreeSet<String>>) -> Self {
        self.masked_siblings = masked_siblings;
        self
    }

    /// Attaches a launch-time office-masks topology digest (`TASK-245`) so warm-session reuse
    /// detects co-hosted sibling organizations or satellites created after launch.
    pub fn with_office_masks_digest(mut self, digest: Option<String>) -> Self {
        self.office_masks_digest = digest;
        self
    }

    /// Sets whether [`SandboxRunner::adapt_session_spec`] should prune absent deny paths.
    pub fn with_prune_absent_denies(mut self, prune: bool) -> Self {
        self.prune_absent_denies = prune;
        self
    }

    /// Selected confinement mode.
    pub const fn mode(&self) -> SandboxConfinementMode {
        self.mode
    }

    /// Masked sibling employee IDs captured at launch preparation, when mount-mask tracking applies.
    pub fn masked_siblings(&self) -> Option<&BTreeSet<String>> {
        self.masked_siblings.as_ref()
    }

    /// Launch-time office-masks topology digest, when tracked for warm-session staleness checks.
    pub fn office_masks_digest(&self) -> Option<&str> {
        self.office_masks_digest.as_deref()
    }

    /// Whether absent concrete deny paths should be pruned when adapting a session spec.
    pub const fn prunes_absent_denies(&self) -> bool {
        self.prune_absent_denies
    }

    /// Pure in-memory check: returns `true` when this plan tracks masked siblings and any sibling
    /// in `other_employees` (other than `self_employee_id`) is not masked.
    pub fn has_unmasked_sibling<'a>(
        &self,
        self_employee_id: &str,
        other_employees: impl IntoIterator<Item = &'a str>,
    ) -> bool {
        let Some(masked) = self.masked_siblings.as_ref() else {
            return false;
        };
        other_employees
            .into_iter()
            .any(|other| other != self_employee_id && !masked.contains(other))
    }
}

/// Inputs supplied to [`SandboxRunner::prepare_launch`].
pub struct SandboxPrepareContext<'a> {
    pub office: &'a Office,
    pub office_socket: &'a Path,
    pub agent_os_state_roots: &'a [PathBuf],
    pub employee_id: &'a str,
    pub sandbox_enabled: bool,
    pub host_profile: RuntimeHostProfile,
}

/// Inputs supplied to [`SandboxRunner::verify_before_spawn`].
pub struct SandboxVerifyContext<'a> {
    pub office: &'a Office,
    pub employee_id: &'a str,
    /// Snapshot of currently live or starting employee IDs in the session registry.
    pub active_employees: &'a [String],
}

/// Consumer-oriented confinement seam isolating OS-local sandbox planning and lifecycle checks
/// (`TASK-233`).
pub trait SandboxRunner: Send + Sync {
    /// Selects confinement mode, prepares required OS confinement state (such as `0700` deny
    /// directories), and returns an opaque [`SandboxPlan`] for this launch.
    fn prepare_launch(&self, ctx: &SandboxPrepareContext<'_>) -> Result<SandboxPlan, ApiError>;

    /// Applies confinement-specific adjustments (such as absent-path deny pruning) to `spec`.
    fn adapt_session_spec(&self, plan: &SandboxPlan, spec: &mut SessionSpec);

    /// Runs fail-closed confinement verification right before spawning a child or pump thread.
    fn verify_before_spawn(
        &self,
        ctx: &SandboxVerifyContext<'_>,
        plan: &SandboxPlan,
    ) -> Result<(), ApiError>;

    /// Returns `true` when `plan` is stale for `employee_id` in `office` and the warm handle must
    /// be recycled before serving another turn.
    fn is_plan_stale(&self, office: &Office, employee_id: &str, plan: &SandboxPlan) -> bool;

    /// Ensures no live or starting sibling session can observe `starting_emp_id`'s newly prepared
    /// workspace or binding artifacts before launch proceeds.
    fn enforce_sibling_isolation(
        &self,
        mgr: &SessionManager,
        starting_emp_id: &str,
    ) -> Result<(), ApiError>;
}

/// Default local-OS [`SandboxRunner`] implementing host-native confinement and Linux `bwrap`
/// directory-mask isolation (`TASK-144`, `TASK-163`, `TASK-166`).
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalOsSandbox;

impl SandboxRunner for LocalOsSandbox {
    fn prepare_launch(&self, ctx: &SandboxPrepareContext<'_>) -> Result<SandboxPlan, ApiError> {
        if !ctx.sandbox_enabled {
            return Ok(SandboxPlan::unsandboxed());
        }
        let (plan, track_absent) = if is_linux_bwrap_sandbox(ctx.sandbox_enabled, ctx.host_profile)
        {
            let masked = ensure_bwrap_deny_dirs_with(
                ctx.office,
                ctx.office_socket,
                ctx.agent_os_state_roots,
                ctx.employee_id,
            )?;
            (SandboxPlan::linux_bwrap(masked), true)
        } else {
            (SandboxPlan::host_native(), false)
        };
        let digest = OfficeMasks::of(ctx.office)
            .topology_digest(track_absent)
            .ok_or_else(|| {
                ApiError::new(
                    ErrorCode::StorageError,
                    "could not compute sandbox office mask topology",
                )
            })?;
        Ok(plan.with_office_masks_digest(Some(digest)))
    }

    fn adapt_session_spec(&self, plan: &SandboxPlan, spec: &mut SessionSpec) {
        if plan.prunes_absent_denies() {
            apply_linux_claude_bwrap_pruning(spec);
        }
    }

    fn verify_before_spawn(
        &self,
        ctx: &SandboxVerifyContext<'_>,
        plan: &SandboxPlan,
    ) -> Result<(), ApiError> {
        if plan.masked_siblings().is_none() && plan.office_masks_digest().is_none() {
            return Ok(());
        }
        let sibling_raced = plan.has_unmasked_sibling(
            ctx.employee_id,
            ctx.active_employees.iter().map(String::as_str),
        ) || plan.masked_siblings().is_some_and(|masked| {
            has_unmasked_bwrap_sibling_in_office(ctx.office, ctx.employee_id, masked)
        });
        if sibling_raced {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "a newly hired or starting sibling agent is not masked in this sandbox; retry launch",
            ));
        }
        if is_office_masks_digest_stale(ctx.office, plan) {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "co-hosted office sandbox topology changed while preparing launch; retry launch",
            ));
        }
        Ok(())
    }

    fn is_plan_stale(&self, office: &Office, employee_id: &str, plan: &SandboxPlan) -> bool {
        plan.masked_siblings()
            .is_some_and(|masked| has_unmasked_bwrap_sibling_in_office(office, employee_id, masked))
            || is_office_masks_digest_stale(office, plan)
    }

    fn enforce_sibling_isolation(
        &self,
        mgr: &SessionManager,
        starting_emp_id: &str,
    ) -> Result<(), ApiError> {
        enforce_bwrap_sibling_isolation(mgr, starting_emp_id)
    }
}

fn is_office_masks_digest_stale(office: &Office, plan: &SandboxPlan) -> bool {
    let Some(expected) = plan.office_masks_digest() else {
        return false;
    };
    let track_absent_static = plan.mode() == SandboxConfinementMode::LinuxBwrap;
    match OfficeMasks::of(office).topology_digest(track_absent_static) {
        Some(current) => current != expected,
        None => true, // Fail closed on unexpected filesystem I/O error.
    }
}
