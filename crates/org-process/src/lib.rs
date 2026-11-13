//! Processes (`docs/design/processes.md`): how agents work, written down and inherited. Pure:
//! no SQL, git, network or clock, and no ports. This slice reads a process file and its place;
//! resolving which processes apply comes next.

#![warn(missing_docs)]

mod file;
mod place;

pub use file::{read, Moments, Problem, ProblemKind, ProcessFile, Status, Step};
pub use place::{location, Level, MemberId, Place, ProcessKey, ProjectKey, Repo, TeamId};

#[cfg(test)]
mod tests;
