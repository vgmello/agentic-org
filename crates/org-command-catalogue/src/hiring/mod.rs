//! Declarative command rows for the hiring domain: `role`, `hr`, and `agent`.

mod agent;
mod agent_sessions;
mod hr;
mod role;
mod schema;

use org_command::Command;

pub use agent::AGENT_COMMANDS;
pub use agent_sessions::AGENT_SESSIONS_COMMANDS;
pub use hr::HR_COMMANDS;
pub use role::ROLE_COMMANDS;

/// All command row slices in the hiring domain, in canonical table order.
pub const ALL: &[&[Command]] = &[
    ROLE_COMMANDS,
    HR_COMMANDS,
    AGENT_COMMANDS,
    AGENT_SESSIONS_COMMANDS,
];
