//! # org-office-sessions: production agent session runner, supervision, and CLI enrollment
//!
//! Composes `org-agent-runtime` and its adapters, `org-supervise`, `org-pki::wrap`,
//! `org-platform::process`, and `org-office` (the `agent-os` command table is injected by the
//! office binary, see [`install_command_catalogue`]) to implement:
//! - Headless supervised agent sessions in private `0700` agent homes (`TASK-42` / C3)
//! - Permission presets, default-on sandbox, and escalation policy (`ADR-0032` §3 / `ENROLL-7`)
//! - Standing agent instructions in static-to-volatile prefix order, the generated `agent-os`
//!   command catalogue, fenced turn objectives and the command drift guard (`doc-27` / `FC-171`)
//! - Office-owned sub-agents and cross-turn background task tracking & reaping (`ADR-0025`)
//! - Per-run cryptographic `AGENT_OS_SESSION_TOKEN` & kernel descendant verification (`CLI-12`, `SUB-3`)
//! - Restart continuation & stall watchdog (`ADR-0030` / `LIVE-9..13`)
//! - Browser-driven provider CLI enrollment & wrapped credential custody (`ADR-0032` / `ENROLL-1..6`)

#![forbid(unsafe_code)]

pub mod binding;
pub mod credentials;
mod employee_locks;
mod host_auth;
pub mod instructions;
mod lifecycle;
pub mod manager;
pub mod preset;
mod provenance;
mod pump;
pub mod recovery;
pub mod redact;
pub mod router;
pub mod runtime_registry;
pub mod subagents;
pub mod subscription_usage;
pub mod transcript;
pub mod wake;

pub use binding::{AcceptedCommandRecord, ActiveRunBinding, BindingRejection, RunBindingRegistry};
pub use credentials::{
    CliEnrollmentDriver, CredentialMetadata, CredentialVault, EnrollmentDriver, EnrollmentManager,
    EnrollmentOutcome, EnrollmentPrompt, EnrollmentVaultError,
};
pub use instructions::{
    assemble_standing_instructions, compose_turn_objective, install_command_catalogue,
    verify_command_references, CommandCatalogue, InstructionInput, StandingInstructions,
    TurnTrigger, BUILTIN_INSTRUCTIONS_MAX_BYTES, SAME_USER_ISOLATION_NOTICE,
};
pub use lifecycle::spawn_wake_retry_loop;
pub use manager::{SessionManager, SessionManagerConfig};
pub use preset::{
    build_grants_summary, build_session_policy, decide_escalation, decide_escalation_with_roots,
    decide_escalation_with_tools, EscalationDisposition, PresetConfig, PresetError,
};
pub use recovery::{
    ActiveTurnRecord, PendingStartRecord, ProcessExitOracle, RecoveryAction, RecoveryReport,
    SessionStore, StallWatchdog, DEFAULT_STALL_TIMEOUT, MAX_CONSECUTIVE_STALLS,
    MAX_RESUME_ATTEMPTS,
};
pub use router::{HqPermitAcquireSpec, HqPermitCoordinator};
pub use runtime_registry::{RuntimeRegistry, RuntimeRegistryError};
pub use subagents::{
    build_general_subagent_definition, subset_tools, BackgroundKind, BackgroundRegistry,
    BackgroundTaskState, BackgroundWake, SubagentDefinition,
};
pub use subscription_usage::{
    HttpUsageResponse, ProviderSubscriptionUsageCollector, SubscriptionUsageCollectOutcome,
    SubscriptionUsageCollectRequest, SubscriptionUsageCollector, SubscriptionUsageTransport,
    SystemHttpsUsageTransport, UsageTransportError,
};
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
