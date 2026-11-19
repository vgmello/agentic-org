//! A new task's checklist (processes.md §3.4 Checklists, PROC-12): the project's default
//! checklist, then the steps of the project's processes (O ⊕ T ⊕ J, never G).

use crate::file::{Status, Step};
use crate::place::Level;
use crate::resolve::{EffectiveSet, Winner};

/// The checklist `task.create` gives a task: `default` first, then the checklist steps of each
/// key's winning file in `set`, ordered by the winner's level (organization, team, project), then
/// by key. A private (agent-level) winner adds nothing, nor does a winner that isn't `active`
/// (a set from [`crate::resolve`] has none: a turned-off key or a conflict with nothing applying
/// has no winner, and a conflict adds only the version that applies). A step whose text, trimmed
/// of leading and trailing whitespace, equals an earlier step's (letter case and inner spaces
/// counting) is merged into that earlier one: it keeps its place and text, and is required if any
/// copy is. Steps of `default` merge the same way.
pub fn checklist(default: &[Step], set: &EffectiveSet) -> Vec<Step> {
    let mut winners: Vec<&Winner> = set
        .entries
        .iter()
        .filter_map(|entry| entry.winner.as_ref())
        .filter(|w| w.file.place.level() != Level::Agent)
        .filter(|w| w.file.status == Status::Active)
        .collect();
    winners.sort_by(|a, b| {
        (a.file.place.level(), &a.file.key).cmp(&(b.file.place.level(), &b.file.key))
    });
    let added = winners.into_iter().flat_map(|w| &w.file.checklist);
    let mut steps: Vec<Step> = Vec::new();
    for step in default.iter().chain(added) {
        match steps.iter_mut().find(|s| s.text.trim() == step.text.trim()) {
            Some(earlier) => earlier.required |= step.required,
            None => steps.push(step.clone()),
        }
    }
    steps
}

#[cfg(test)]
mod tests;
