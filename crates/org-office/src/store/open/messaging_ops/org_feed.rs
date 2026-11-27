//! `OrgFeed`: committed organization changes replayed after a position.

use std::collections::BTreeMap;

use org_proto::msg::Meta;
use org_proto::org::{OrgChange, OrgChangedData, Position};
use org_proto::{Actor as WireActor, ActorKind};
use org_store::{Id, Outcome, StoreError};

use crate::store::open::ReadOp;
use crate::tasks::ports::TasksRead;

/// Reads committed organization receipts with `seq > after_seq` for `OrgFeed` replay (`subscribe`
/// and `/api/v1/events/stream` restart/resume semantics).
pub struct OrgChangesAfterRead {
    pub owner: Id,
    pub after_seq: u64,
    pub limit: usize,
}

impl ReadOp for OrgChangesAfterRead {
    type Out = Vec<OrgChangedData>;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        let org_snap = crate::structure::snapshot::project(snapshot, &self.owner)?;
        let teams: BTreeMap<&str, &org_proto::Team> =
            org_snap.teams.iter().map(|t| (t.id.as_str(), t)).collect();
        let members: BTreeMap<&str, &org_proto::Member> = org_snap
            .members
            .iter()
            .map(|m| (m.id.as_str(), m))
            .collect();
        let projects: BTreeMap<&str, &org_proto::Project> = org_snap
            .projects
            .iter()
            .map(|p| (p.id.as_str(), p))
            .collect();
        let receipts = snapshot.list_receipts_by_prefix("organization.")?;
        let mut out = Vec::new();
        for receipt in receipts {
            if receipt.outcome != Outcome::Completed || receipt.kind == "organization.create" {
                continue;
            }
            let val: Meta = serde_json::from_slice(&receipt.reply).unwrap_or_default();
            let seq = val
                .get("generation")
                .and_then(|v| v.as_u64())
                .unwrap_or(receipt.ordinal);
            if seq <= self.after_seq {
                continue;
            }
            let pos = Position { epoch: 1, seq };
            let actor = match receipt.actor {
                org_store::Actor::Employee(id) => Some(WireActor {
                    kind: ActorKind::Human,
                    member: id.as_str().to_owned(),
                }),
                org_store::Actor::System => None,
            };
            let changes =
                map_receipt_to_org_changes(&receipt.kind, &val, &teams, &members, &projects);
            out.push(OrgChangedData {
                position: pos,
                command_id: receipt.id.as_str().to_owned(),
                changes,
                actor,
            });
        }
        out.sort_by_key(|c| c.position.seq);
        out.truncate(self.limit);
        Ok(out)
    }
}

fn map_receipt_to_org_changes(
    kind: &str,
    reply: &Meta,
    teams: &BTreeMap<&str, &org_proto::Team>,
    members: &BTreeMap<&str, &org_proto::Member>,
    projects: &BTreeMap<&str, &org_proto::Project>,
) -> Vec<OrgChange> {
    if kind == "organization.decide_team_limit_request" {
        return reply
            .get("team_revision")
            .and_then(|_| reply.get("team_id").and_then(|v| v.as_str()))
            .and_then(|tid| teams.get(tid))
            .map(|t| vec![OrgChange::TeamUpserted { team: (*t).clone() }])
            .unwrap_or_default();
    }
    let entity_id = reply
        .get("id")
        .or_else(|| reply.get("employee_id"))
        .or_else(|| reply.get("project_id"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if entity_id.is_empty() {
        return Vec::new();
    }
    match kind {
        "organization.create_team"
        | "organization.update_team"
        | "organization.move_team"
        | "organization.set_team_max_members" => teams
            .get(entity_id)
            .map(|t| vec![OrgChange::TeamUpserted { team: (*t).clone() }])
            .unwrap_or_else(|| {
                vec![OrgChange::TeamDeleted {
                    team_id: entity_id.to_owned(),
                }]
            }),
        "organization.archive_team" => vec![OrgChange::TeamDeleted {
            team_id: entity_id.to_owned(),
        }],
        "organization.add_person"
        | "organization.update_person"
        | "organization.move_person"
        | "organization.set_may_hire"
        | "organization.hire_agent"
        | "organization.confirm_hire"
        | "organization.update_agent"
        | "organization.move_agent"
        | "organization.set_agent_status"
        | "organization.upsert_agent_execution"
        | "organization.ensure_chief_of_staff" => members
            .get(entity_id)
            .map(|m| {
                vec![OrgChange::MemberUpserted {
                    member: Box::new((*m).clone()),
                }]
            })
            .unwrap_or_else(|| {
                vec![OrgChange::MemberRemoved {
                    member_id: entity_id.to_owned(),
                }]
            }),
        "organization.deactivate_person" | "organization.offboard_agent" => {
            vec![OrgChange::MemberRemoved {
                member_id: entity_id.to_owned(),
            }]
        }
        "organization.create_project"
        | "organization.add_project_team"
        | "organization.remove_project_team" => projects
            .get(entity_id)
            .map(|p| {
                vec![OrgChange::ProjectUpserted {
                    project: (*p).clone(),
                }]
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}
