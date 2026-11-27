//! # org-office-sessions: production agent session runner, supervision, and CLI enrollment
//!
//! Composes `org-agent-runtime` and its adapters, `org-supervise`, `org-pki::wrap`,
//! `org-platform::process`, `org-session-instructions`, `org-session-presets`,
//! `org-subscription-usage`, `org-turn-attachments`, `org-redact`, and `org-office` to implement:
//! - Headless supervised agent sessions in private `0700` agent homes (`TASK-42` / C3)
//! - Permission presets, default-on sandbox, and escalation policy (`ADR-0032` §3 / `ENROLL-7`)
//! - Standing agent instructions in static-to-volatile prefix order, the generated `agent-os`
//!   command catalogue, fenced turn objectives and the command drift guard (`doc-27` / `FC-171`)
//! - Office-owned sub-agents and cross-turn background task tracking & reaping (`ADR-0025`)
//! - Per-run cryptographic `AGENT_OS_SESSION_TOKEN` & kernel descendant verification (`CLI-12`, `SUB-3`)
//! - External (BYOA) agent leases, Ed25519 challenge-response and RAM-only fast tokens under a
//!   single-active-lease invariant (`TASK-137`)
//! - Restart continuation & stall watchdog (`ADR-0030` / `LIVE-9..13`)
//! - Browser-driven provider CLI enrollment & wrapped credential custody (`ADR-0032` / `ENROLL-1..6`)
//! - Live turn controls: a person's interrupt and mid-turn steer of a warm session, and the
//!   post-redaction [`SessionStreamHub`] of coarse turn activity (`TASK-103`)

#![forbid(unsafe_code)]

pub mod binding;
pub mod credentials;
mod employee_locks;
mod external;
mod external_sessions;
mod host_auth;
pub(crate) use org_session_instructions as instructions;
mod lifecycle;
pub mod manager;
pub mod partition;
pub(crate) use org_session_presets as preset;
mod provenance;
mod pump;
pub mod recovery;
pub(crate) use org_redact as redact;
pub mod router;
pub mod runtime_registry;
pub mod stream_hub;
pub mod subagents;
pub(crate) use org_subscription_usage as subscription_usage;
pub mod transcript;
pub(crate) use org_turn_attachments as turn_attachments;
mod turn_attachments_adapter;
pub mod wake;

pub use binding::{
    AcceptedCommandRecord, ActiveRunBinding, BindingMode, BindingRejection, LocalPeerPidVerifier,
    PeerIdentity, RunBindingRegistry, RunBindingVerifier, VerifiedRunBinding,
};
pub use credentials::{
    CliEnrollmentDriver, CredentialMetadata, CredentialVault, EnrollmentDriver, EnrollmentManager,
    EnrollmentOutcome, EnrollmentPrompt, EnrollmentVaultError,
};
pub use external::{ExternalLeaseRecord, EXTERNAL_LEASE_VERSION};
pub use lifecycle::spawn_wake_retry_loop;
pub use manager::{
    LocalOsSandbox, PreparedWorktreeNotice, SandboxConfinementMode, SandboxPlan,
    SandboxPrepareContext, SandboxRunner, SandboxVerifyContext, SessionManager,
    SessionManagerConfig, TaskWorktreePreparer, WorktreeSandboxRefreshOutcome,
    MID_TURN_SANDBOX_REFRESH_HINT,
};
pub use recovery::{
    ActiveTurnRecord, PendingStartRecord, ProcessExitOracle, RecoveryAction, RecoveryReport,
    SessionStore, StallWatchdog, DEFAULT_STALL_TIMEOUT, MAX_CONSECUTIVE_STALLS,
    MAX_RESUME_ATTEMPTS,
};
pub use router::{HqPermitAcquireSpec, HqPermitCoordinator};
pub use runtime_registry::{RuntimeRegistry, RuntimeRegistryError};
pub use stream_hub::{
    SessionActivity, SessionStreamEvent, SessionStreamFrame, SessionStreamHub, SessionStreamKey,
    SessionStreamSubscription, StreamFilter, TurnEnd,
};
pub use subagents::{
    build_general_subagent_definition, subset_tools, BackgroundKind, BackgroundRegistry,
    BackgroundTaskState, BackgroundWake, SubagentDefinition,
};
pub use turn_attachments::{
    TurnAttachmentResolveError, TurnAttachmentResolver, TurnDeliveryAttachment, TurnDeliveryScope,
    MAX_INLINE_IMAGES_PER_TURN,
};
pub use turn_attachments_adapter::{map_office_delivery_result, to_office_delivery_scope};
pub use wake::{
    compose_coalesced_wake_objective, compose_mid_turn_notice_batch, sanitize_untrusted_body,
    MidTurnLimits, MidTurnRateTracker, MidTurnSelection, PendingAskAnswer, PendingWakeItem,
    PendingWakeQueueRecord, SuspendedAskEntry, SuspendedStateRecord, WakeHoldReason, WakeKind,
    WakeOutcome, WakeQuotaRecord, DEFAULT_HOURLY_WAKE_LIMIT, DEFAULT_MID_TURN_COALESCE_AGENT,
    DEFAULT_MID_TURN_COALESCE_PERSON, DEFAULT_MID_TURN_HOURLY_AGENT_CAP,
    DEFAULT_MID_TURN_PER_TURN_AGENT_CAP, DEFAULT_MID_TURN_PER_TURN_HARD_CAP,
    DEFAULT_MID_TURN_SENDER_COOLDOWN, DEFAULT_SUSPEND_GRACE, DEFAULT_WAKE_WINDOW,
    MAX_MID_TURN_BATCH_BYTES, MAX_WAKE_ITEM_BODY_BYTES,
};

#[cfg(test)]
mod tests;
