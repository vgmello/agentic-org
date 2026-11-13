//! Processes (`docs/design/processes.md`): how agents work, written down and inherited. Pure:
//! no SQL, git, network or clock, and no ports. It reads a process file and its place,
//! resolves which processes apply to an agent's work, keys the resolved sets for caching,
//! merges a new task's checklist and renders the "Your processes" section.

#![warn(missing_docs)]

mod checklist;
mod file;
mod fingerprint;
mod place;
mod render;
mod resolve;

pub use checklist::checklist;
pub use file::{Moments, Problem, ProblemKind, ProcessFile, Status, Step};
pub use fingerprint::{fingerprint, fingerprint_without_work, Fingerprint};
pub use place::{location, Level, Location, MemberId, Place, ProcessKey, ProjectKey, Repo, TeamId};
pub use render::{render, FileText, Moment, RenderInput, Rendered};
pub use resolve::{
    resolve, ChainError, Conflict, Drift, EffectiveSet, Entry, Input, Inputs, Kind, Layer, Outcome,
    PrivateReason, TurnedOff, Winner, Work,
};

#[cfg(test)]
mod tests;
