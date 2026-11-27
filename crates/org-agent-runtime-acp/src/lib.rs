//! # org-agent-runtime-acp: unified sans-I/O JSON-RPC 2.0 ACP runtime crate
//!
//! Implements [`org_agent_runtime::AgentRuntime`], [`org_agent_runtime::Session`],
//! [`org_agent_runtime::ProbeSession`], and [`org_agent_runtime::TranscriptParser`] over
//! JSON-RPC 2.0 Agent Client Protocol (ACP) stdio for thin declarative vendor profiles:
//!
//! - [`AgyRuntime`] — Google Antigravity (`agy acp --stdio`)
//! - [`GeminiRuntime`] — Google Gemini CLI (`gemini --acp`)
//! - [`KimiRuntime`] — Moonshot Kimi Code CLI (`kimi acp`)
//! - [`PiRuntime`] — Pi ACP adapter (`pi-acp`)
//!
//! This crate performs **no I/O and spawns no processes**: `org-supervise` executes every
//! [`org_agent_runtime::LaunchPlan`] and [`org_agent_runtime::ProbePlan`].

#![forbid(unsafe_code)]

mod plan;
mod probe;
mod profile;
mod profiles;
mod protocol;
mod session;
mod transcript;

pub use crate::profile::{
    AcpProfile, AuthProbeStrategy, ExpectedMode, ModeMapping, StateDirStrategy,
    SystemPromptDelivery, TakeOverStrategy,
};
pub use crate::profiles::{
    AgyRuntime, GeminiRuntime, KimiRuntime, PiRuntime, AGY_CONFIG_CREDENTIAL_FILES,
    GEMINI_CONFIG_CREDENTIAL_FILES, GEMINI_HOST_CREDENTIAL_FILES, KIMI_CONFIG_CREDENTIAL_FILES,
    KIMI_HOST_CREDENTIAL_FILES, PI_CONFIG_CREDENTIAL_FILES, PI_HOST_CREDENTIAL_FILES,
};

#[cfg(test)]
mod tests {
    use org_agent_runtime::AgentRuntime;

    use super::{AgyRuntime, GeminiRuntime, KimiRuntime, PiRuntime};

    #[test]
    fn claude_auth_error_replies_are_ordinary_replies_for_all_acp_runtimes() {
        let runtimes: [&dyn AgentRuntime; 4] = [
            &AgyRuntime::new(),
            &GeminiRuntime::new(),
            &KimiRuntime::new(),
            &PiRuntime::new(),
        ];
        for runtime in runtimes {
            for reply in [
                r#"Failed to authenticate. API Error: 401 {"type":"error","error":{"type":"authentication_error","message":"OAuth token has expired."}}"#,
                "401 OAuth access token has expired",
                "OAuth access token has expired",
                "API Error: 401 OAuth token has expired",
            ] {
                assert!(
                    !runtime.reply_signals_auth_expired(reply),
                    "{}: {reply:?}",
                    runtime.id()
                );
            }
        }
    }
}
