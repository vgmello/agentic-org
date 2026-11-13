//! Processes (`docs/design/processes.md`): how agents work, written down and inherited. Pure:
//! no SQL, git, network or clock, and no ports. It reads a process file and its place, and
//! resolves which processes apply to an agent's work.

#![warn(missing_docs)]

mod file;
mod place;
mod resolve;

pub use file::{Moments, Problem, ProblemKind, ProcessFile, Status, Step};
pub use place::{location, Level, Location, MemberId, Place, ProcessKey, ProjectKey, Repo, TeamId};
pub use resolve::{
    resolve, ChainError, Conflict, Drift, EffectiveSet, Entry, Input, Inputs, Kind, Layer, Outcome,
    PrivateReason, TurnedOff, Winner, Work,
};

#[cfg(test)]
mod tests;
