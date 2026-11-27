//! Where an agent sits in its organization, by name: its home team and the leads above it
//! (`doc-27` §1 part 5). Names come from organization records people edit, so each one is
//! normalized to one plain line before it reaches a prompt.

use std::collections::BTreeSet;

use org_proto::org::OrgSnapshot;
use org_redact::sanitize::{sanitize_untrusted_meta, MAX_META_FIELD_BYTES};

/// The most teams walked up from the home team (a guard against a malformed cycle).
const MAX_TEAM_DEPTH: usize = 32;

/// One lead above the agent: the team they lead and who they are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeadLink {
    pub team_name: String,
    pub lead_name: String,
    pub lead_handle: Option<String>,
}

impl LeadLink {
    /// `Name (@handle), lead of Team`.
    pub fn render(&self) -> String {
        let handle = self
            .lead_handle
            .as_deref()
            .map(|h| format!(" (@{h})"))
            .unwrap_or_default();
        format!("{}{handle}, lead of {}", self.lead_name, self.team_name)
    }
}

/// Where an agent sits: its home team's name and the leads above it, nearest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Placement {
    pub home_team_name: String,
    /// Whether the agent leads its home team.
    pub leads_home_team: bool,
    /// The leads of the home team and of every team above it, nearest first. The agent itself
    /// is never listed.
    pub leads: Vec<LeadLink>,
    /// Who the agent reports to, when set: `Name (@handle)`.
    pub reports_to: Option<String>,
}

fn clean(raw: &str) -> String {
    sanitize_untrusted_meta(raw, MAX_META_FIELD_BYTES)
}

fn member_label(snapshot: &OrgSnapshot, member_id: &str) -> (String, Option<String>) {
    match snapshot.members.iter().find(|m| m.id == member_id) {
        Some(member) => (
            clean(&member.name),
            member
                .handle
                .as_deref()
                .map(clean)
                .filter(|h| !h.is_empty()),
        ),
        None => (clean(member_id), None),
    }
}

/// Resolves `employee_id`'s placement from one consistent `snapshot`: its home team by name,
/// the lead of that team and of each team above it, and who it reports to. A team missing from
/// the snapshot ends the walk; its ID stands in for its name.
pub fn resolve_placement(
    snapshot: &OrgSnapshot,
    employee_id: &str,
    home_team_id: &str,
    reports_to_id: Option<&str>,
) -> Placement {
    let mut placement = Placement {
        home_team_name: clean(home_team_id),
        ..Placement::default()
    };
    let mut seen = BTreeSet::new();
    let mut next = Some(home_team_id.to_owned());
    while let Some(team_id) = next.take() {
        if seen.len() >= MAX_TEAM_DEPTH || !seen.insert(team_id.clone()) {
            break;
        }
        let Some(team) = snapshot.teams.iter().find(|t| t.id == team_id) else {
            break;
        };
        let team_name = clean(&team.name);
        if team_id == home_team_id {
            placement.home_team_name = team_name.clone();
        }
        match team.lead_member_id.as_deref() {
            Some(lead) if lead == employee_id => {
                placement.leads_home_team |= team_id == home_team_id;
            }
            Some(lead) => {
                let (lead_name, lead_handle) = member_label(snapshot, lead);
                placement.leads.push(LeadLink {
                    team_name,
                    lead_name,
                    lead_handle,
                });
            }
            None => {}
        }
        next = team.owner_team_id.clone();
    }
    placement.reports_to = reports_to_id.map(|id| {
        let (name, handle) = member_label(snapshot, id);
        match handle {
            Some(handle) => format!("{name} (@{handle})"),
            None => name,
        }
    });
    placement
}
