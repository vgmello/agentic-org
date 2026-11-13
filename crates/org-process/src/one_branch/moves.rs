//! Own work and the guarded moves (processes.md §4.2 "Guarded moves", decision 11): whether a
//! task's branch holds the agent's unmerged work ([`own_work`]), and whether a caller may close,
//! reassign or unassign the task ([`guarded_move`]). Pure: the office takes the copies, judges
//! each against `base_sha` and the integration branch in `mirror.git`, checks the caller's role
//! and binding (decision 15) and settles `forwarded_to_owner` before these run.

use super::{OneBranchOpen, Record, State};

/// One copy the office took for a guarded move, judged against the record's `base_sha` and the
/// fetched integration branch. Plain input data; [`Tip::judge`] maps the office's three facts to
/// it.
///
/// A sha equal to `base_sha` is [`Tip::AtBase`] whatever else is true of it: the base came from
/// the integration branch (decision 14), so it carries none of the agent's work, and §4.2 counts
/// only a tip that "differs from it". So the other variants are all shas that differ from the
/// base, and "in the integration branch" or "descends from the base" is never asked of the base
/// itself. The shas stay with the office, which stores them for §4.2's re-check.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tip {
    /// There is no such copy, e.g. no branch, no recorded `own_tip` or no remote head.
    Missing,
    /// The copy failed, or its sha couldn't be read or judged.
    Unreadable,
    /// The sha is `base_sha`.
    AtBase,
    /// Another sha, in the integration branch.
    Integrated,
    /// Another sha, descending from `base_sha` and not in the integration branch: own work.
    Beyond,
    /// Another sha, neither descending from `base_sha` nor in the integration branch (a reset
    /// onto someone else's branch, §4.1): it fails closed as own work ([`own_work`]).
    Unrelated,
}

impl Tip {
    /// The tip of a sha the office has judged: `at_base` (it equals `base_sha`),
    /// `descends_from_base` and `in_integration`. `at_base` decides alone (see [`Tip`]); then
    /// `in_integration`; then `descends_from_base`.
    pub fn judge(at_base: bool, descends_from_base: bool, in_integration: bool) -> Tip {
        match (at_base, in_integration, descends_from_base) {
            (true, _, _) => Tip::AtBase,
            (false, true, _) => Tip::Integrated,
            (false, false, true) => Tip::Beyond,
            (false, false, false) => Tip::Unrelated,
        }
    }
}

/// Every copy §4.2 judges for a guarded move. Plain input data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tips {
    /// The `work.git` branch, copied to `refs/office/task/<b>`.
    pub branch: Tip,
    /// The worktree's HEAD, copied to `refs/office/task-head/<b>`.
    pub head: Tip,
    /// The record's `own_tip`; [`Tip::Missing`] while none is recorded.
    pub own_tip: Tip,
    /// The remote head; [`Tip::Missing`] when the branch was never pushed.
    pub remote: Tip,
}

/// What [`own_work`] found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Found {
    /// No own work, and not merged: the record is archived.
    None,
    /// `own_tip` is in the integration branch and no copy has work beyond it.
    Merged,
    /// Own unmerged work.
    Some,
}

/// The judgement a guarded move rests on, made only by [`own_work`]:
///
/// ```compile_fail,E0423
/// let own = org_process::OwnWork(org_process::Found::None);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OwnWork(Found);

impl OwnWork {
    /// What was found.
    pub fn found(&self) -> Found {
        self.0
    }
}

/// Own unmerged work (§4.2 "Own unmerged work", and "The record is merged instead only if").
/// `uncommitted` says the worktree has uncommitted changes, non-ignored untracked files included;
/// the office passes true when it couldn't tell.
///
/// - [`Found::Some`] when there are uncommitted changes, the branch or HEAD copy is missing or
///   unreadable, or any tip is unreadable, [`Tip::Beyond`] or [`Tip::Unrelated`]. A missing
///   `own_tip` or remote head is not own work: §4.1's record has `own_tip?`, a branch never
///   pushed has no remote head, and §4.2's "missing branch" is the `work.git` branch. An
///   unrelated tip (history from outside the base, e.g. a reset onto another branch) fails
///   closed, whether or not `own_tip` is merged (§4.2's reading, for ada's confirmation).
/// - Otherwise [`Found::Merged`] when `own_tip` is [`Tip::Integrated`]: every copy is then at the
///   base, missing, or in the integration branch, so none has work beyond it.
/// - Otherwise [`Found::None`].
pub fn own_work(tips: &Tips, uncommitted: bool) -> OwnWork {
    let all = [tips.branch, tips.head, tips.own_tip, tips.remote];
    let failed = |tip: Tip| matches!(tip, Tip::Missing | Tip::Unreadable);
    if uncommitted
        || failed(tips.branch)
        || failed(tips.head)
        || all.contains(&Tip::Unreadable)
        || all.contains(&Tip::Beyond)
        || all.contains(&Tip::Unrelated)
    {
        return OwnWork(Found::Some);
    }
    if tips.own_tip == Tip::Integrated {
        OwnWork(Found::Merged)
    } else {
        OwnWork(Found::None)
    }
}

/// The caller's role in the task's project, as the office has checked it with its binding
/// (decision 15). Plain input data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    /// The organization's owner.
    Owner,
    /// A lead of one of the project's teams.
    Lead,
    /// Anyone else.
    Other,
}

/// Who asks for the guarded move. Plain input data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Caller {
    /// The caller's role.
    pub role: Role,
    /// Whether the caller is the record's assignee.
    pub assignee: bool,
}

/// An accepted guarded move, made only by [`guarded_move`]:
///
/// ```compile_fail,E0451
/// let done = org_process::Move { state: org_process::State::Archived, over_own_work: true };
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Move {
    state: State,
    over_own_work: bool,
}

impl Move {
    /// The record's new state: [`State::Merged`] or [`State::Archived`].
    pub fn state(&self) -> State {
        self.state
    }

    /// Whether the move went over own unmerged work: the office records it on the task, the
    /// branch is archived (§4.5) and the agent freed.
    pub fn over_own_work(&self) -> bool {
        self.over_own_work
    }
}

/// A refused guarded move.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// `one_branch_open`, to the assignee, naming the record's branch.
    OneBranchOpen(OneBranchOpen),
    /// `not_authorized`.
    NotAuthorized,
}

/// A guarded move (§4.2 "Guarded moves", decision 11) of the task whose record is `record`, by
/// `caller`, judged by `own`. The office has already checked the caller's role and binding
/// (decision 15) and settled `forwarded_to_owner` for a hand-run lead, so neither is decided here.
///
/// - No own work: the move goes through and archives the record; `own_tip` merged: it goes
///   through and marks the record merged.
/// - Own work: the owner, also on their own task, or a lead who isn't the assignee makes the move
///   over the work. Decision 11 names "a project lead (not the assignee) or the owner", and §4.3
///   exempts only the owner from the own-task bar; so a lead who is the assignee is the assignee
///   here. The assignee otherwise gets [`Refused::OneBranchOpen`] naming the record, and anyone
///   else [`Refused::NotAuthorized`].
pub fn guarded_move(caller: Caller, own: &OwnWork, record: &Record) -> Result<Move, Refused> {
    let state = match own.0 {
        Found::None => State::Archived,
        Found::Merged => State::Merged,
        Found::Some => {
            let may = match caller.role {
                Role::Owner => true,
                Role::Lead => !caller.assignee,
                Role::Other => false,
            };
            if may {
                return Ok(Move {
                    state: State::Archived,
                    over_own_work: true,
                });
            }
            if !caller.assignee {
                return Err(Refused::NotAuthorized);
            }
            return Err(Refused::OneBranchOpen(OneBranchOpen {
                branch: record.branch.clone(),
                task: record.task.clone(),
                stage: record.stage.clone(),
            }));
        }
    };
    Ok(Move {
        state,
        over_own_work: false,
    })
}

#[cfg(test)]
mod tests;
