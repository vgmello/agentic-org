//! Processes (`docs/design/processes.md`): how agents work, written down and inherited. Pure:
//! no SQL, git, network or clock, and no ports. It reads a process file and its place,
//! resolves which processes apply to an agent's work, keys the resolved sets for caching,
//! merges a new task's checklist, renders the "Your processes" section, checks the
//! one-open-task-branch rule (§4.2) and checks each write (§5).

#![warn(missing_docs)]

mod checklist;
mod file;
mod fingerprint;
mod one_branch;
mod place;
mod render;
mod resolve;
mod write;

pub use checklist::checklist;
pub use file::{Moments, Problem, ProblemKind, ProcessFile, Status, Step};
pub use fingerprint::{fingerprint, fingerprint_without_work, Fingerprint};
pub use one_branch::{
    check_start, guarded_move, own_work, rule_on, Caller, Exception, Found, Move, OneBranchOpen,
    OwnWork, Record, Refused, Role, Start, State, Target, TaskRef, Tip, Tips,
};
pub use place::{location, Level, Location, MemberId, Place, ProcessKey, ProjectKey, Repo, TeamId};
pub use render::{render, FileText, Moment, RenderInput, Rendered};
pub use resolve::{
    resolve, ChainError, Conflict, Drift, EffectiveSet, Entry, Input, Inputs, Kind, Layer, Outcome,
    PrivateReason, TurnedOff, Winner, Work,
};
pub use write::{
    check_write, Branch, RefusalCode, RefusalReason, WriteInput, WriteRefusal, Writer,
};

#[cfg(test)]
mod tests;
