//! The mock office's organizations: what `org.snapshot` answers until the real office does.
//!
//! TEMPORARY: the data is invented. An organization's structure starts as the sample below unless
//! `AGENTIC_ORG_MOCK_ORG=empty` asks for an owner and nothing else; the mock does not keep it between
//! launches yet, so the setting applies each time it is served. Everything uses the real
//! `org_proto::org` types, so only the transport changes when the real office lands.

use org_proto::org::{
    AgentProfile, AgentRuntime, AgentStatus, Member, MemberKind, Membership, OrgSnapshot, Position,
    ProjectSummary, ProjectTeam, RunMode, Team,
};

/// Which organization a new mock organization starts as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seed {
    Sample,
    Empty,
}

impl Seed {
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("empty") => Self::Empty,
            _ => Self::Sample,
        }
    }

    /// The snapshot a new organization starts with. `hq_office_id` is where the agents are placed.
    pub fn snapshot(self, hq_office_id: &str) -> OrgSnapshot {
        match self {
            Self::Sample => sample_org(hq_office_id),
            Self::Empty => empty_org(),
        }
    }
}

fn team(id: &str, name: &str, owner: Option<&str>, lead: Option<&str>) -> Team {
    Team {
        id: id.into(),
        name: name.into(),
        owner_team_id: owner.map(Into::into),
        lead_member_id: lead.map(Into::into),
        version: 1,
    }
}

fn person(id: &str, name: &str, reports_to: Option<&str>) -> Member {
    Member {
        id: id.into(),
        kind: MemberKind::Person,
        name: name.into(),
        contact: None,
        owner: false,
        office_id: None,
        agent: None,
        reports_to_member_id: reports_to.map(Into::into),
        created_at: None,
        version: 1,
    }
}

fn agent(
    id: &str,
    name: &str,
    office_id: Option<&str>,
    reports_to: Option<&str>,
    role: &str,
) -> Member {
    Member {
        kind: MemberKind::Agent,
        office_id: office_id.map(Into::into),
        agent: Some(AgentProfile {
            role: role.into(),
            title: role.into(),
            focus: None,
            runtime: AgentRuntime::ClaudeCode,
            model: None,
            run_mode: Some(RunMode::OnCall),
            schedule: None,
            home: None,
            messaging_name: id.into(),
            status: AgentStatus::Active,
            offboarded_at: None,
        }),
        ..person(id, name, reports_to)
    }
}

fn joined(n: usize, team_id: &str, member_id: &str, title: Option<&str>) -> Membership {
    Membership {
        id: format!("ms-{n}"),
        team_id: team_id.into(),
        member_id: member_id.into(),
        title_in_team: title.map(Into::into),
        version: 1,
    }
}

fn owner() -> Member {
    Member {
        owner: true,
        ..person("you", "You", None)
    }
}

/// An organization with an owner and nothing else.
pub fn empty_org() -> OrgSnapshot {
    OrgSnapshot {
        position: Position { epoch: 1, seq: 1 },
        teams: vec![],
        members: vec![owner()],
        memberships: vec![],
        projects: vec![],
        project_teams: vec![],
    }
}

/// An invented organization; none of it is from a real workspace.
pub fn sample_org(hq: &str) -> OrgSnapshot {
    let h = Some(hq);
    let members = vec![
        owner(),
        Member {
            contact: Some("avery@example.com".into()),
            ..person("avery", "Avery Stone", None)
        },
        person("hana", "Hana Weiss", Some("avery")),
        person("leo", "Leo Brandt", Some("hana")),
        agent("sam", "Sam Okafor", h, Some("avery"), "software engineer"),
        agent("noor", "Noor Haddad", h, Some("sam"), "web engineer"),
        agent("kai", "Kai Lindqvist", h, Some("sam"), "platform engineer"),
        agent("mira", "Mira Castillo", h, Some("sam"), "software engineer"),
        agent("juno", "Juno Park", h, Some("avery"), "designer"),
        agent("pax", "Pax Moreau", h, Some("juno"), "designer"),
        // Not placed in an office.
        agent("ines", "Inês Duarte", None, Some("hana"), "people partner"),
        agent("quinn", "Quinn Adeyemi", h, None, "researcher"),
    ];
    let teams = vec![
        team("eng", "Engineering", None, Some("sam")),
        team("web", "Web", Some("eng"), Some("noor")),
        team("platform", "Platform", Some("eng"), Some("kai")),
        team("design", "Design", None, Some("juno")),
        team("people", "People", None, Some("hana")),
    ];
    let memberships = vec![
        joined(1, "eng", "sam", Some("Staff engineer")),
        joined(2, "web", "noor", None),
        joined(3, "web", "mira", None),
        joined(4, "platform", "kai", None),
        joined(5, "platform", "mira", None),
        joined(6, "design", "juno", None),
        joined(7, "design", "pax", None),
        joined(8, "design", "noor", None),
        joined(9, "people", "hana", None),
        joined(10, "people", "leo", None),
        joined(11, "people", "ines", None),
    ];
    let project = |id: &str, name: &str| ProjectSummary {
        id: id.into(),
        name: name.into(),
    };
    let project_team = |id: &str, project_id: &str, team_id: &str| ProjectTeam {
        id: id.into(),
        project_id: project_id.into(),
        team_id: team_id.into(),
        version: 1,
    };
    OrgSnapshot {
        position: Position { epoch: 1, seq: 1 },
        teams,
        members,
        memberships,
        projects: vec![
            project("atlas", "Atlas"),
            project("beacon", "Beacon"),
            project("onboarding", "Onboarding kit"),
            project("scratch", "Scratchpad"),
        ],
        project_teams: vec![
            project_team("pt-1", "atlas", "platform"),
            project_team("pt-2", "beacon", "web"),
            project_team("pt-3", "beacon", "design"),
            project_team("pt-4", "onboarding", "people"),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn ids<'a>(it: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
        it.collect()
    }

    #[test]
    fn the_seed_is_read_from_the_environment_value() {
        assert_eq!(Seed::parse(Some("empty")), Seed::Empty);
        assert_eq!(Seed::parse(Some("anything else")), Seed::Sample);
        assert_eq!(Seed::parse(None), Seed::Sample);
    }

    #[test]
    fn an_empty_organization_is_its_owner_alone() {
        let s = Seed::Empty.snapshot("office-1");
        assert_eq!(ids(s.members.iter().map(|m| m.id.as_str())), ["you"]);
        assert!(s.members[0].owner && s.members[0].kind == MemberKind::Person);
        assert!(s.teams.is_empty() && s.memberships.is_empty());
    }

    #[test]
    fn the_sample_is_consistent() {
        let s = Seed::Sample.snapshot("office-1");
        let members: HashSet<_> = s.members.iter().map(|m| m.id.as_str()).collect();
        let teams: HashSet<_> = s.teams.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(members.len(), s.members.len(), "member ids are unique");
        assert_eq!(teams.len(), s.teams.len(), "team ids are unique");
        // Exactly one owner, in no team, reporting to no one.
        assert_eq!(s.members.iter().filter(|m| m.owner).count(), 1);
        for m in &s.memberships {
            assert!(
                teams.contains(m.team_id.as_str()),
                "{} is in a known team",
                m.id
            );
            assert!(members.contains(m.member_id.as_str()));
            assert!(!s.members.iter().any(|x| x.owner && x.id == m.member_id));
        }
        for t in &s.teams {
            if let Some(owner) = &t.owner_team_id {
                assert!(teams.contains(owner.as_str()));
            }
            if let Some(lead) = &t.lead_member_id {
                assert!(
                    s.memberships
                        .iter()
                        .any(|m| &m.member_id == lead && m.team_id == t.id),
                    "the lead of {} is a member of it",
                    t.id
                );
            }
        }
        for m in &s.members {
            if let Some(boss) = &m.reports_to_member_id {
                assert!(members.contains(boss.as_str()));
            }
        }
        for pt in &s.project_teams {
            assert!(teams.contains(pt.team_id.as_str()));
            assert!(s.projects.iter().any(|p| p.id == pt.project_id));
        }
    }

    #[test]
    fn agents_are_placed_in_the_hq_office_except_the_one_left_unplaced() {
        let s = sample_org("office-9");
        let agents: Vec<_> = s
            .members
            .iter()
            .filter(|m| m.kind == MemberKind::Agent)
            .collect();
        assert_eq!(agents.len(), 8);
        assert!(agents.iter().all(|m| m.agent.is_some()));
        assert_eq!(
            agents
                .iter()
                .filter(|m| m.office_id.as_deref() == Some("office-9"))
                .count(),
            7
        );
        assert!(agents
            .iter()
            .any(|m| m.name == "Inês Duarte" && m.office_id.is_none()));
        // People are never placed.
        assert!(s
            .members
            .iter()
            .filter(|m| m.kind == MemberKind::Person)
            .all(|m| m.office_id.is_none()));
    }

    #[test]
    fn handles_are_unique_ignoring_case_across_all_members() {
        let s = sample_org("o");
        let mut seen = HashSet::new();
        for m in &s.members {
            let handle = m
                .agent
                .as_ref()
                .map_or(m.name.as_str(), |a| a.messaging_name.as_str());
            assert!(seen.insert(handle.to_lowercase()), "{handle} is used twice");
        }
    }

    #[test]
    fn the_snapshot_serialises_to_the_wire_shape_the_ui_reads() {
        let json = serde_json::to_value(sample_org("o")).unwrap();
        let sam = json["members"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["id"] == "sam")
            .unwrap();
        assert_eq!(sam["kind"], "agent");
        assert_eq!(sam["office_id"], "o");
        assert_eq!(sam["reports_to_member_id"], "avery");
        let you = &json["members"][0];
        assert_eq!(you["owner"], true);
        assert!(you.get("office_id").is_none(), "absent, not null");
        assert_eq!(json["position"]["epoch"], 1);
    }
}
