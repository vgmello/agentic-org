//! Declarative command rows for the messaging domain: `msg`, `channel`, and `status`.

mod ask;
mod channel;
mod msg;
mod schema;
mod status;

use org_command::Command;

pub use ask::MSG_ASK_COMMANDS;
pub use channel::CHANNEL_COMMANDS;
pub use msg::{MSG_READ_COMMANDS, MSG_SEND_COMMANDS};
pub use status::STATUS_COMMANDS;

/// All command row slices in the messaging domain, in canonical table order.
pub const ALL: &[&[Command]] = &[
    MSG_SEND_COMMANDS,
    MSG_ASK_COMMANDS,
    MSG_READ_COMMANDS,
    CHANNEL_COMMANDS,
    STATUS_COMMANDS,
];
