//! Records a snapshot moves to another team chain (HQ's `MoveTeam`, document 01): a moved
//! team's `team.yaml`, and every profile, `execution.md`, hire request and process document
//! homed in its subtree, each now under the new chain while this satellite still holds it at
//! the old one (`TASK-224`).
//!
//! The projection keys those records by ID, not by path: removing the old path after the
//! snapshot wrote the new one would remove the record it just wrote (a team, with the tasks
//! and profiles that name it). So the old path's removal goes in the batch that writes the new
//! one, just before it, as HQ's own `MoveTeam` entry orders them, and reconciliation never
//! removes it again. Task folder files are not among them: the replicated-files command moves
//! a moved team's tasks itself (`doc-77`).

use std::collections::{BTreeMap, BTreeSet};

/// The record a team-homed file holds wherever its team stands: the team whose folder it is
/// in, and its path below that folder. `None` outside `state/teams/`, for a path with nothing
/// below its team's folder, and for a task folder file.
pub(super) fn team_homed_record(path: &str) -> Option<(&str, &str)> {
    let mut rest = path.strip_prefix("state/")?;
    let mut team = None;
    while let Some(below) = rest.strip_prefix("teams/") {
        let (id, after) = below.split_once('/')?;
        if id.is_empty() {
            return None;
        }
        team = Some(id);
        rest = after;
    }
    let team = team?;
    let task_file = org_office::task_id_for_work_path(path).is_some();
    (!rest.is_empty() && !task_file).then_some((team, rest))
}

/// For each path the snapshot writes, the local paths of the same team-homed record at
/// another chain, which the snapshot does not write: each is one `may_remove` lets
/// reconciliation remove, so this only moves a removal earlier, never adds one.
pub(super) fn superseded_paths<'a>(
    local: impl IntoIterator<Item = &'a String>,
    incoming: &BTreeSet<String>,
    may_remove: impl Fn(&str) -> bool,
) -> BTreeMap<String, Vec<String>> {
    let written: BTreeMap<(&str, &str), &String> = incoming
        .iter()
        .filter_map(|path| team_homed_record(path).map(|record| (record, path)))
        .collect();
    let mut superseded: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for old in local {
        if incoming.contains(old) || !may_remove(old) {
            continue;
        }
        let Some(new) = team_homed_record(old).and_then(|record| written.get(&record)) else {
            continue;
        };
        superseded
            .entry((*new).clone())
            .or_default()
            .push(old.clone());
    }
    superseded
}
