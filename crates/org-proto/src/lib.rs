//! # org-proto: the contract
//!
//! Request/response and event types spoken between the desktop app (via `org-client`),
//! the agent CLI, and an office server. Serde types; TypeScript is generated from them
//! for the app. **The most depended-on crate: keep it small, stable, additive-only.**
//! Every change needs ada's review (see AGENTS.md).
