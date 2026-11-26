//! The office-verified relation of a sender to the agent a turn is for (`doc-78`, `doc-101` §4C,
//! `INSTR-5`).
//!
//! One resolver serves every turn that names its sender (queued wakes, direct instructions and
//! hiring requests alike), so a `[verified: ...]` badge means the same thing wherever it appears.
//! It reads office state only; nothing a caller claims about the sender is trusted.

use std::collections::{HashMap, HashSet};

use org_office::organization_family::records::{Employee, EmployeeKind};
use org_office::Office;

/// The relation of `sender_id` to `target`, from office state: `owner`; `reports_to` (in the
/// target's reporting chain); `team_lead` (leads the target's home team or a team above it);
/// `person` or `peer` (another agent) for any other employee, where `sender` is the record the
/// office loaded for `sender_id`; and `unknown` for a sender that is neither the owner nor a
/// loaded employee. It never guesses `person` for a sender the office cannot vouch for.
pub(crate) fn verified_sender_relation(
    office: &Office,
    target: &Employee,
    sender_id: &str,
    sender: Option<&Employee>,
) -> &'static str {
    if sender_id == office.owner_member_id() {
        return "owner";
    }
    if sender_in_reports_to_chain(office, target, sender_id) {
        return "reports_to";
    }
    if sender_in_ancestor_team_leads(office, target, sender_id) {
        return "team_lead";
    }
    match sender.map(|e| e.kind) {
        Some(EmployeeKind::Person) => "person",
        Some(EmployeeKind::Agent) => "peer",
        None => "unknown",
    }
}

pub(crate) fn sender_in_reports_to_chain(
    office: &Office,
    target: &Employee,
    sender_id: &str,
) -> bool {
    let mut visited = HashSet::new();
    visited.insert(target.id.as_str().to_owned());
    let mut cursor = target
        .reports_to_employee_id
        .as_ref()
        .map(|id| id.as_str().to_owned());
    while let Some(mgr_id) = cursor {
        if mgr_id == sender_id {
            return true;
        }
        if !visited.insert(mgr_id.clone()) {
            break;
        }
        cursor = office
            .load_employee(&mgr_id)
            .ok()
            .flatten()
            .and_then(|e| e.reports_to_employee_id)
            .map(|id| id.as_str().to_owned());
    }
    false
}

pub(crate) fn sender_in_ancestor_team_leads(
    office: &Office,
    target: &Employee,
    sender_id: &str,
) -> bool {
    let Ok(snap) = office.org_snapshot() else {
        return false;
    };
    let by_id: HashMap<&str, (&Option<String>, &Option<String>)> = snap
        .teams
        .iter()
        .map(|t| (t.id.as_str(), (&t.lead_member_id, &t.owner_team_id)))
        .collect();
    let mut visited = HashSet::new();
    let mut cursor = Some(target.home_team_id.as_str());
    while let Some(team_id) = cursor {
        if !visited.insert(team_id) {
            break;
        }
        let Some((lead, parent)) = by_id.get(team_id) else {
            break;
        };
        if lead.as_deref() == Some(sender_id) {
            return true;
        }
        cursor = parent.as_deref();
    }
    false
}
