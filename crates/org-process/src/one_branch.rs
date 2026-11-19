//! One open task branch (processes.md §4.2, A54, A55): whether the rule is on ([`rule_on`]),
//! whether an agent may start a task ([`check_start`]), and own work ([`own_work`]) and the
//! guarded moves ([`guarded_move`]). Pure: the office passes in the project's effective set,
//! the agent's task branch records and the target task's exception, and keeps the records and
//! exceptions itself.

use crate::place::{MemberId, Place, ProcessKey};
use crate::resolve::{EffectiveSet, Layer, Outcome};

mod moves;

pub use moves::{guarded_move, own_work, Caller, Found, Move, OwnWork, Refused, Role, Tip, Tips};

/// The process key whose approved `off` turns the rule off (A64). If U26 splits it, this becomes
/// `one-open-branch`.
const RULE_KEY: &str = "branch-and-merge";

/// Whether the rule is on in the target task's project (§4.2 "The rule is on", with its rulings
/// on conflicts and files that fail). `set` is that project's effective set, none when it can't
/// be computed (PROC-17); `approved` says whether the `off` file in a layer (its place, path and
/// version) had its blob approved by a non-author (§5).
///
/// The rule is off only when the set was computed for a project ([`EffectiveSet::inputs`] has
/// one), turns the key off ([`EffectiveSet::turned_off`]), its entry's chain has a turned-off
/// layer and every such layer is approved, and the set has no conflict on the key and no invalid
/// or clashed file that has the key or whose key can't be read (it might have it). Any other set
/// leaves it on.
pub fn rule_on(set: Option<&EffectiveSet>, approved: impl Fn(&Layer) -> bool) -> bool {
    let Some(set) = set else {
        return true;
    };
    let is_key = |key: &ProcessKey| key.as_str() == RULE_KEY;
    let project = set
        .inputs
        .iter()
        .any(|place| matches!(place, Place::Project(_)));
    // Resolution also lists a sealed team's `off` that applies during a conflict in
    // `turned_off`, so a conflict on the key is checked on its own (§4.2's ruling).
    let conflict = set.conflicts.iter().any(|conflict| is_key(&conflict.key));
    let broken = set.problems.iter().any(|layer| match layer.outcome {
        Outcome::Invalid(_) => layer.key.as_ref().is_none_or(is_key),
        Outcome::Clashed => layer.key.as_ref().is_some_and(is_key),
        _ => false,
    });
    let turned_off = set.turned_off.iter().any(|off| is_key(&off.key));
    let offs: Vec<&Layer> = set
        .entries
        .iter()
        .filter(|entry| is_key(&entry.key))
        .flat_map(|entry| &entry.chain)
        .filter(|layer| layer.outcome == Outcome::TurnedOff)
        .collect();
    let off = project && turned_off && !offs.is_empty() && offs.into_iter().all(approved);
    conflict || broken || !off
}

/// A task's reference, as the office names it (W7, e.g. `chk-T-17`). Plain input data.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskRef(pub String);

/// A task branch record's state (§4.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum State {
    /// Reserved at the start, before the branch exists.
    Reserved,
    /// The branch exists.
    Open,
    /// A guarded move is judging it.
    Closing,
    /// Its `own_tip` is in the integration branch, or it was merged by hand.
    Merged,
    /// Closed without a merge.
    Archived,
}

/// One of the agent's task branch records (§4.1), as the office maps its row. Plain input data.
/// Only task branches have records: the office's docs, process and retro branches have none, so
/// they never reach [`check_start`] and never block (A55).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    /// The task.
    pub task: TaskRef,
    /// The branch, e.g. `task/chk-T-17-fix-login`.
    pub branch: String,
    /// The state.
    pub state: State,
    /// Where the merge flow has it, e.g. "waiting for review"; none until the merge flow
    /// supplies stages (A63).
    pub stage: Option<String>,
}

/// The start being checked: the target task, and the agent starting it as its assignee, whose
/// records [`check_start`] is given. Plain input data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// The task being started.
    pub task: TaskRef,
    /// Its assignee: the agent.
    pub assignee: MemberId,
}

/// A one-time exception on a task (§4.3, `BranchExceptionGranted`): it covers one record for this
/// task and assignee. Plain input data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exception {
    /// The task it was granted on.
    pub task: TaskRef,
    /// The assignee it was granted for.
    pub assignee: MemberId,
}

/// An allowed start, made only by [`check_start`]:
///
/// ```compile_fail,E0451
/// let start = org_process::Start { reuse: false, exception_used: true };
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Start {
    reuse: bool,
    exception_used: bool,
}

impl Start {
    /// Whether the start reuses the target task's own reserved or open record, so it creates
    /// none: that record passed the check when it was made.
    pub fn reuses_record(&self) -> bool {
        self.reuse
    }

    /// Whether the start used up the exception held for the target task and its assignee (§4.2
    /// "An exception", with ada's ruling; §4.3).
    pub fn exception_used(&self) -> bool {
        self.exception_used
    }
}

/// A refused start (`one_branch_open`), naming the first blocking record, made only by
/// [`check_start`]:
///
/// ```compile_fail,E0451
/// use org_process::{OneBranchOpen, TaskRef};
/// let open = OneBranchOpen { branch: "task/x".into(), task: TaskRef("x".into()), stage: None };
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OneBranchOpen {
    branch: String,
    task: TaskRef,
    stage: Option<String>,
}

impl OneBranchOpen {
    /// The blocking record's branch.
    pub fn branch(&self) -> &str {
        &self.branch
    }

    /// The blocking record's task.
    pub fn task(&self) -> &TaskRef {
        &self.task
    }

    /// The blocking record's stage, none until the merge flow supplies stages (A63).
    pub fn stage(&self) -> Option<&str> {
        self.stage.as_deref()
    }

    /// The refusal's first sentence (processes-ux.md): "Your branch <branch> isn't merged yet."
    /// with the stage in brackets before the full stop when there is one.
    pub fn message(&self) -> String {
        match &self.stage {
            Some(stage) => format!("Your branch {} isn't merged yet ({stage}).", self.branch),
            None => format!("Your branch {} isn't merged yet.", self.branch),
        }
    }
}

/// Where a record's state puts it among the blocking records, earliest first; none for a state
/// that never blocks.
fn blocking_rank(state: State) -> Option<u8> {
    match state {
        State::Open => Some(0),
        State::Closing => Some(1),
        State::Reserved => Some(2),
        State::Merged | State::Archived => None,
    }
}

/// The start check (§4.2 Start, with ada's rulings), at both of its points:
/// `task start` (point 1), which the office runs only when the target task has no reserved or
/// open record, and branch creation (point 2), which reuses the task's own reservation. `records`
/// are the assignee's task branch records; `exception` is the one held on the target task, if any.
///
/// - A reserved or open record of the target task allows the start as a reuse, which creates no
///   record. A closing, merged or archived one doesn't.
/// - Otherwise an exception for the target task and its assignee allows the start; when it is
///   used up is §4.2's ruling on exceptions.
/// - Otherwise, with the rule off, the start is allowed.
/// - Otherwise a reserved, open or closing record for another task blocks it: the first by state
///   (open, then closing, then reserved), then by branch, then by task. Merged and archived
///   records, and the target task's own records, never block.
pub fn check_start(
    records: &[Record],
    target: &Target,
    exception: Option<&Exception>,
    rule_on: bool,
) -> Result<Start, OneBranchOpen> {
    let own = |record: &Record| record.task == target.task;
    let reusable = |record: &Record| matches!(record.state, State::Reserved | State::Open);
    if records.iter().any(|record| own(record) && reusable(record)) {
        return Ok(Start {
            reuse: true,
            exception_used: false,
        });
    }
    let held = |e: &&Exception| e.task == target.task && e.assignee == target.assignee;
    if exception.filter(held).is_some() {
        return Ok(Start {
            reuse: false,
            exception_used: true,
        });
    }
    let allowed = Ok(Start {
        reuse: false,
        exception_used: false,
    });
    if !rule_on {
        return allowed;
    }
    let first = records
        .iter()
        .filter(|record| !own(record))
        .filter_map(|record| Some((blocking_rank(record.state)?, record)))
        .min_by(|(a, x), (b, y)| (a, &x.branch, &x.task).cmp(&(b, &y.branch, &y.task)));
    match first {
        Some((_, record)) => Err(OneBranchOpen {
            branch: record.branch.clone(),
            task: record.task.clone(),
            stage: record.stage.clone(),
        }),
        None => allowed,
    }
}

#[cfg(test)]
mod tests;
