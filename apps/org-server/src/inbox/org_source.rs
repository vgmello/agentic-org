//! Read-only org projection; one snapshot supplies both counts and the filtered page.
use super::{Keyset, Query, Source, SourceFailure, SourceRead};
use org_proto::inbox::{
    Action, InboxClears, InboxCounts, InboxItem, InboxKind, InboxState, Open, OpenTarget,
};
use org_proto::{AgentStatus, MemberKind};

type SnapshotFn = Box<dyn Fn() -> Result<org_proto::OrgSnapshot, SourceFailure> + Send + Sync>;

pub(super) struct OrgSource {
    snapshot: SnapshotFn,
    office_id: String,
}

impl OrgSource {
    pub(super) fn new(snapshot: SnapshotFn, office_id: String) -> Self {
        Self {
            snapshot,
            office_id,
        }
    }
}

impl Source for OrgSource {
    fn name(&self) -> &'static str {
        "org"
    }

    fn read(&self, query: &Query<'_>) -> Result<SourceRead, SourceFailure> {
        let snapshot = (self.snapshot)().map_err(|_| SourceFailure)?;
        // Only known current agents create Problems; uncertain profiles never nag.
        // No office assignment is authoritative org data. A foreign office may be a live
        // satellite. OFC-10 must record removal at HQ (or reclaim to None) before we add
        // that predicate; never infer removal from process status or reachability.
        let members: Vec<_> = snapshot
            .members
            .iter()
            .filter(|member| {
                member.kind == MemberKind::Agent
                    && member.agent.as_ref().is_some_and(|profile| {
                        matches!(profile.status, AgentStatus::Active | AgentStatus::Paused)
                    })
                    && member.office_id.is_none()
            })
            .collect();
        let count = members.len() as u64;
        let counts = InboxCounts {
            problems: count,
            total: count,
            ..InboxCounts::default()
        };
        let mut items = Vec::new();
        if query.state == InboxState::Open
            && query
                .kinds
                .is_none_or(|kinds| kinds.contains(&InboxKind::Problem))
        {
            for member in members {
                let teams: Vec<_> = snapshot
                    .memberships
                    .iter()
                    .filter(|membership| membership.member_id == member.id)
                    .map(|membership| membership.team_id.as_str())
                    .collect();
                if query.team.is_some_and(|team| !teams.contains(&team))
                    || query.project.is_some_and(|project| {
                        !snapshot.project_teams.iter().any(|link| {
                            link.project_id == project && teams.contains(&link.team_id.as_str())
                        })
                    })
                {
                    continue;
                }
                let item = InboxItem {
                    reference: format!("{}:org:{}:unplaced", self.office_id, member.id),
                    source: "org".into(),
                    kind: InboxKind::Problem,
                    member: Some(member.id.clone()),
                    project: None,
                    team: None,
                    summary: format!("Place {} in an office", member.name),
                    context: None,
                    recommendation: None,
                    options: None,
                    created_at_ms: None,
                    expires_at_ms: None,
                    state: InboxState::Open,
                    handled_at_ms: None,
                    outcome: None,
                    clears: InboxClears::Placed,
                    dismissable: false,
                    actions: vec![Action::Open {
                        label: "Open".into(),
                        open: Open {
                            target: OpenTarget::Member,
                            id: member.id.clone(),
                        },
                    }],
                };
                if query.after.is_none_or(|after| after.admits(&item)) {
                    items.push(item);
                }
            }
            items.sort_by_key(|item| Keyset::of(item).reference);
            items.truncate(query.limit.min(500));
        }
        Ok(SourceRead { items, counts })
    }
}

#[cfg(test)]
mod tests;
