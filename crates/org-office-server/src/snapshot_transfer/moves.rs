//! Records a snapshot moves to another team chain: HQ's `MoveTeam` (document 01) moves a
//! team's `team.yaml` and every profile, `execution.md`, hire request, process and standard
//! homed in its subtree; `MovePerson` and `MoveAgent` move an employee's profile (and an
//! agent's `execution.md`) to another home team. Each is now at a new path while this satellite
//! still holds it at the old one (`TASK-224`).
//!
//! The projection keys those records by ID, not by path: removing the old path after the
//! snapshot wrote the new one would remove the record it just wrote (a team, with the tasks
//! and profiles that name it). So the old path's removal goes in the batch that writes the new
//! one, just before it, as HQ's own move entry orders them, and reconciliation never removes
//! it again. Task folder files are not among them: the replicated-files command moves a moved
//! team's tasks itself (`doc-77`).

use std::collections::{BTreeMap, BTreeSet};

use org_office::organization_family::records::{kind_of, Kind};
use org_office::processes::records::{parse_path, StructureDocPath};
use org_store::DurablePath;

/// What a team-homed record is wherever its team stands: its kind and its ID.
pub(super) type RecordKey = (&'static str, String);

/// The record a team-homed file holds wherever its team stands, as the owning families'
/// parsers read the canonical durable path: a team's `team.yaml` is its team, a person's or an
/// agent's profile its employee, and an `execution.md`, a hire request, a process or a standard
/// its own ID. `None` for text that is not a canonical durable path (`..`, `.`, an empty
/// segment, a trailing `/`), an ID that is not a stable ID, a task folder file, and every
/// record not homed in a team.
pub(super) fn team_homed_record(path: &str) -> Option<RecordKey> {
    let path = DurablePath::parse(path).ok()?;
    if let Some(kind) = kind_of(&path) {
        return match kind {
            Kind::Team(chain) => chain.last().map(|team| ("team", team.as_str().to_owned())),
            Kind::Employee(_, _, id) => Some(("employee", id.as_str().to_owned())),
            Kind::AgentExecution(_, id) => Some(("agent_execution", id.as_str().to_owned())),
            Kind::HireRequest(_, id) => Some(("hire", id.as_str().to_owned())),
            _ => None,
        };
    }
    match parse_path(&path)? {
        StructureDocPath::Team(_, kind, id) => Some((kind.as_str(), id.as_str().to_owned())),
        StructureDocPath::Organization(..) | StructureDocPath::Project(..) => None,
    }
}

/// For each path the snapshot writes, the local paths of the same team-homed record elsewhere,
/// which the snapshot does not write, each named once: each is one `may_remove` lets
/// reconciliation remove, so this only moves a removal earlier, never adds one. A record the
/// snapshot writes at more than one path is left to reconciliation: which write moves it is
/// not known.
pub(super) fn superseded_paths<'a>(
    local: impl IntoIterator<Item = &'a String>,
    incoming: &BTreeSet<String>,
    may_remove: impl Fn(&str) -> bool,
) -> BTreeMap<String, Vec<String>> {
    let mut written: BTreeMap<RecordKey, Option<&String>> = BTreeMap::new();
    for path in incoming {
        if let Some(record) = team_homed_record(path) {
            written
                .entry(record)
                .and_modify(|new| *new = None)
                .or_insert(Some(path));
        }
    }
    let mut superseded: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for old in local {
        if incoming.contains(old) || !may_remove(old) {
            continue;
        }
        let Some(new) =
            team_homed_record(old).and_then(|record| written.get(&record).copied().flatten())
        else {
            continue;
        };
        superseded
            .entry(new.clone())
            .or_default()
            .insert(old.clone());
    }
    superseded
        .into_iter()
        .map(|(new, old)| (new, old.into_iter().collect()))
        .collect()
}
