//! Processes (`docs/design/processes.md`): how agents work, written down and inherited. Pure:
//! no SQL, git, network or clock, and no ports. It reads a process file and its place, resolves
//! which processes apply to an agent's work, and checks the one-open-task-branch rule (§4.2).

#![warn(missing_docs)]

mod file;
mod one_branch;
mod place;
mod resolve;

pub use file::{Moments, Problem, ProblemKind, ProcessFile, Status, Step};
pub use one_branch::{
    check_start, guarded_move, own_work, rule_on, Caller, Exception, Found, Move, OneBranchOpen,
    OwnWork, Record, Refused, Role, Start, State, Target, TaskRef, Tip, Tips,
};
pub use place::{location, Level, Location, MemberId, Place, ProcessKey, ProjectKey, Repo, TeamId};
pub use resolve::{
    resolve, ChainError, Conflict, Drift, EffectiveSet, Entry, Input, Inputs, Kind, Layer, Outcome,
    PrivateReason, TurnedOff, Winner, Work,
};

#[cfg(test)]
mod tests;
