//! Declarative command rows for the organization domain: `org`, `team`, `person`, `directory`
//! (shorthand `who`), and `member`.

mod org_dir_member;
mod person;
mod schema;
mod team;

use org_command::Command;

pub use org_dir_member::{DIRECTORY_COMMANDS, MEMBER_COMMANDS, ORG_COMMANDS, WHO_ALIAS};
pub use person::PERSON_COMMANDS;
pub use team::TEAM_COMMANDS;

/// All command row slices in the organization domain, in canonical table order.
pub const ALL: &[&[Command]] = &[
    ORG_COMMANDS,
    TEAM_COMMANDS,
    PERSON_COMMANDS,
    DIRECTORY_COMMANDS,
    MEMBER_COMMANDS,
];
