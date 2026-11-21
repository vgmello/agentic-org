//! # org-office-sessions: production agent session runner, supervision, and CLI enrollment
//!
//! Composes `org-agent-runtime`, `org-agent-runtime-claude`, `org-supervise`, `org-pki::wrap`,
//! `org-platform::process`, `org-office`, and `org-agent-cli::table` to implement:
//! - Headless supervised agent sessions in private `0700` agent homes (`TASK-42` / C3)
//! - Permission presets, default-on sandbox, and escalation policy (`ADR-0032` §3 / `ENROLL-7`)
//! - Standing 7-part agent instructions & generated `agent-os` command catalogue (`doc-27` / `FC-171`)
//! - Office-owned sub-agents and cross-turn background task tracking & reaping (`ADR-0025`)
//! - Per-run cryptographic `AGENT_OS_SESSION_TOKEN` & kernel descendant verification (`CLI-12`, `SUB-3`)
//! - Restart continuation & stall watchdog (`ADR-0030` / `LIVE-9..13`)
//! - Browser-driven provider CLI enrollment & wrapped credential custody (`ADR-0032` / `ENROLL-1..6`)

#![deny(unsafe_code)]

pub mod binding;
pub mod credentials;
pub mod instructions;
pub mod manager;
pub mod preset;
mod pump;
pub mod recovery;
pub mod runtime_registry;
pub mod subagents;

pub use binding::{AcceptedCommandRecord, ActiveRunBinding, BindingRejection, RunBindingRegistry};
pub use credentials::{
    CliEnrollmentDriver, CredentialMetadata, CredentialVault, EnrollmentDriver, EnrollmentManager,
    EnrollmentOutcome, EnrollmentPrompt, EnrollmentVaultError,
};
pub use instructions::{
    assemble_standing_instructions, compose_turn_objective, verify_command_references,
    InstructionInput, TurnTrigger, BUILTIN_INSTRUCTIONS_MAX_BYTES, SAME_USER_ISOLATION_NOTICE,
};
pub use manager::{SessionManager, SessionManagerConfig};
pub use preset::{
    build_grants_summary, build_session_policy, decide_escalation, EscalationDisposition,
    PresetConfig, PresetError,
};
pub use recovery::{
    ActiveTurnRecord, PendingStartRecord, ProcessExitOracle, RecoveryAction, RecoveryReport,
    SessionStore, StallWatchdog, DEFAULT_STALL_TIMEOUT, MAX_CONSECUTIVE_STALLS,
    MAX_RESUME_ATTEMPTS,
};
pub use runtime_registry::{RuntimeRegistry, RuntimeRegistryError};
pub use subagents::{
    build_general_subagent_definition, subset_tools, BackgroundKind, BackgroundRegistry,
    BackgroundTaskState, BackgroundWake, SubagentDefinition,
};

#[cfg(test)]
mod tests;
