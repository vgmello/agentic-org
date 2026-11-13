//! Processes (`docs/design/processes.md`): how agents work, written down and inherited. Pure:
//! no SQL, git, network or clock, and no ports. This slice maps a file's path to its place;
//! reading the file and resolving which processes apply come next.

#![warn(missing_docs)]

mod place;

pub use place::{location, Level, MemberId, Place, ProcessKey, ProjectKey, Repo, TeamId};

#[cfg(test)]
mod tests;
