use org_proto::org::*;
use org_proto::{ApiError, ErrorCode, Request};
use serde_json::json;

use super::memory::MemState;
use super::*;

const NOW: &str = "2026-11-08T07:00:00.000Z";
const OWNER: &str = "OWNER100000000000000000000";

/// An organization being driven command by command: every decision is applied to the state and
/// kept in a log, as HQ does.
struct Org {
    state: MemState,
    log: Vec<Vec<OrgChange>>,
    next: u32,
}

impl Org {
    fn new() -> Self {
        let mut org = Org {
            state: MemState::default(),
            log: vec![],
            next: 0,
        };
        let first = owner_bootstrap(OWNER, "Owner", NOW).unwrap();
        org.commit(first);
        org.state.projects.insert("p1".into());
        org.state.offices.insert("office1".into());
        org
    }

    fn commit(&mut self, changes: Vec<OrgChange>) {
        for change in &changes {
            apply(&mut self.state, change).unwrap();
        }
        self.log.push(changes);
        assert_consistent(&self.state);
    }

    fn run(&mut self, command: OrgCommand) -> Result<Vec<OrgChange>, ApiError> {
        let changes = decide(&self.state, NOW, &command)?;
        self.commit(changes.clone());
        Ok(changes)
    }

    fn id(&mut self) -> String {
        self.next += 1;
        format!("cmd-{}", self.next)
    }

    fn team(
        &mut self,
        id: &str,
        name: &str,
        owner: Option<&str>,
    ) -> Result<Vec<OrgChange>, ApiError> {
        let command_id = self.id();
        self.run(OrgCommand::TeamCreate(TeamCreateParams {
            command_id,
            team_id: id.into(),
            name: name.into(),
            owner_team_id: owner.map(Into::into),
        }))
    }

    fn person(&mut self, id: &str, name: &str) -> Result<Vec<OrgChange>, ApiError> {
        let command_id = self.id();
        self.run(OrgCommand::MemberAddPerson(MemberAddPersonParams {
            command_id,
            member_id: id.into(),
            name: name.into(),
            contact: None,
        }))
    }

    fn join(
        &mut self,
        membership: &str,
        team: &str,
        member: &str,
    ) -> Result<Vec<OrgChange>, ApiError> {
        let command_id = self.id();
        self.run(OrgCommand::TeamAddMember(TeamAddMemberParams {
            command_id,
            membership_id: membership.into(),
            team_id: team.into(),
            member_id: member.into(),
            title_in_team: None,
        }))
    }

    fn lead(&mut self, team: &str, lead: Option<&str>) -> Result<Vec<OrgChange>, ApiError> {
        let command_id = self.id();
        let expected_version = self.state.teams.get(team).map_or(1, |t| t.version);
        self.run(OrgCommand::TeamSetLead(TeamSetLeadParams {
            command_id,
            team_id: team.into(),
            expected_version,
            lead_member_id: lead.map(Into::into),
        }))
    }

    fn reports(&mut self, member: &str, to: Option<&str>) -> Result<Vec<OrgChange>, ApiError> {
        let command_id = self.id();
        let expected_version = self.state.members.get(member).map_or(1, |m| m.version);
        self.run(OrgCommand::MemberSetReportsTo(MemberSetReportsToParams {
            command_id,
            member_id: member.into(),
            expected_version,
            reports_to_member_id: to.map(Into::into),
        }))
    }

    fn remove(&mut self, member: &str) -> Result<Vec<OrgChange>, ApiError> {
        let command_id = self.id();
        let expected_version = self.state.members.get(member).map_or(1, |m| m.version);
        self.run(OrgCommand::MemberRemove(MemberRemoveParams {
            command_id,
            member_id: member.into(),
            expected_version,
        }))
    }

    /// An agent, placed straight into the state (agent commands come in a later change).
    fn agent(&mut self, id: &str, handle: &str, status: AgentStatus) {
        self.state.members.insert(
            id.into(),
            Member {
                id: id.into(),
                kind: MemberKind::Agent,
                name: format!("Agent {id}"),
                contact: None,
                owner: false,
                office_id: Some("office-1".into()),
                agent: Some(AgentProfile {
                    role: "engineer".into(),
                    title: "Engineer".into(),
                    focus: None,
                    runtime: AgentRuntime::Codex,
                    model: None,
                    run_mode: None,
                    schedule: None,
                    home: None,
                    messaging_name: handle.into(),
                    status,
                    offboarded_at: None,
                }),
                reports_to_member_id: None,
                created_at: Some(NOW.into()),
                version: 1,
            },
        );
    }
}

/// Nothing refers to something that is gone, and the structure keeps its shape.
fn assert_consistent(s: &MemState) {
    for team in s.teams.values() {
        if let Some(owner) = &team.owner_team_id {
            assert!(
                s.teams.contains_key(owner),
                "team {} has no owner team",
                team.id
            );
        }
        if let Some(lead) = &team.lead_member_id {
            assert!(
                s.memberships
                    .values()
                    .any(|m| m.team_id == team.id && &m.member_id == lead),
                "the lead of {} is not a member of it",
                team.id
            );
            assert!(!s.members[lead].owner, "the owner leads {}", team.id);
        }
    }
    for m in s.memberships.values() {
        assert!(
            s.teams.contains_key(&m.team_id),
            "membership {} of no team",
            m.id
        );
        assert!(
            s.members.contains_key(&m.member_id),
            "membership {} of no one",
            m.id
        );
    }
    for link in s.project_teams.values() {
        assert!(
            s.teams.contains_key(&link.team_id),
            "link {} to no team",
            link.id
        );
    }
    for member in s.members.values() {
        if let Some(boss) = &member.reports_to_member_id {
            assert!(
                s.members.contains_key(boss),
                "{} reports to no one",
                member.id
            );
            assert!(
                !s.members[boss].owner,
                "{} reports to the owner by id",
                member.id
            );
        }
    }
    assert_eq!(s.members.values().filter(|m| m.owner).count(), 1);
}

fn code<T: std::fmt::Debug>(result: Result<T, ApiError>) -> ErrorCode {
    result.expect_err("expected an error").code
}

fn team_of(changes: &[OrgChange]) -> Team {
    changes
        .iter()
        .find_map(|c| match c {
            OrgChange::TeamUpserted { team } => Some(team.clone()),
            _ => None,
        })
        .expect("no team in the changes")
}

fn member_of(changes: &[OrgChange], id: &str) -> Member {
    changes
        .iter()
        .find_map(|c| match c {
            OrgChange::MemberUpserted { member } if member.id == id => Some(member.clone()),
            _ => None,
        })
        .expect("no such member in the changes")
}

// ---- the organization starts with its owner ---------------------------------------------

#[test]
fn a_new_organization_starts_with_its_owner_and_nothing_else() {
    let org = Org::new();
    let owner = &org.state.members[OWNER];
    assert!(owner.owner);
    assert_eq!(owner.kind, MemberKind::Person);
    assert_eq!(owner.version, 1);
    assert_eq!(owner.created_at.as_deref(), Some(NOW));
    assert!(org.state.teams.is_empty() && org.state.memberships.is_empty());
    assert_eq!(org.log.len(), 1);
}

// ---- teams ------------------------------------------------------------------------------

#[test]
fn teams_nest_and_names_are_trimmed() {
    let mut org = Org::new();
    let created = org.team("t1", "  Product  ", None).unwrap();
    assert_eq!(
        created,
        vec![OrgChange::TeamUpserted {
            team: Team {
                id: "t1".into(),
                name: "Product".into(),
                owner_team_id: None,
                lead_member_id: None,
                version: 1,
            }
        }]
    );
    let child = team_of(&org.team("t2", "Design", Some("t1")).unwrap());
    assert_eq!(child.owner_team_id.as_deref(), Some("t1"));
}

#[test]
fn sibling_team_names_clash_ignoring_case_but_not_across_owners() {
    let mut org = Org::new();
    org.team("t1", "Product", None).unwrap();
    org.team("t2", "Platform", None).unwrap();
    // Top-level teams are siblings of each other.
    assert_eq!(
        code(org.team("t3", "PRODUCT", None)),
        ErrorCode::AlreadyExists
    );
    // The same name under different owners is fine.
    org.team("t4", "Design", Some("t1")).unwrap();
    org.team("t5", "Design", Some("t2")).unwrap();
    assert_eq!(
        code(org.team("t6", "design", Some("t1"))),
        ErrorCode::AlreadyExists
    );
}

#[test]
fn team_create_errors_and_their_precedence() {
    let mut org = Org::new();
    org.team("t1", "Product", None).unwrap();
    assert_eq!(code(org.team("t9", "X", Some("nope"))), ErrorCode::NotFound);
    assert_eq!(
        code(org.team("t1", "Other", None)),
        ErrorCode::AlreadyExists
    );
    assert_eq!(code(org.team("t9", "   ", None)), ErrorCode::InvalidName);
    assert_eq!(
        code(org.team("t9", "a\u{7}b", None)),
        ErrorCode::InvalidName
    );
    assert_eq!(
        code(org.team("bad id!", "X", None)),
        ErrorCode::InvalidRequest
    );
    assert_eq!(code(org.team("", "X", None)), ErrorCode::InvalidRequest);
    // not_found comes before invalid_name, which comes before already_exists.
    assert_eq!(
        code(org.team("t1", "  ", Some("nope"))),
        ErrorCode::NotFound
    );
    assert_eq!(
        code(org.team("bad id!", "  ", None)),
        ErrorCode::InvalidRequest
    );
    assert_eq!(code(org.team("t1", "  ", None)), ErrorCode::InvalidName);
    // None of the failures changed anything.
    assert_eq!(org.state.teams.len(), 1);
}

#[test]
fn rename_bumps_the_version_and_checks_the_version_and_siblings() {
    let mut org = Org::new();
    org.team("t1", "Product", None).unwrap();
    org.team("t2", "Platform", None).unwrap();
    let rename = |org: &mut Org, id: &str, version: u64, name: &str| {
        let command_id = org.id();
        org.run(OrgCommand::TeamRename(TeamRenameParams {
            command_id,
            team_id: id.into(),
            expected_version: version,
            name: name.into(),
        }))
    };
    let renamed = team_of(&rename(&mut org, "t1", 1, "Products").unwrap());
    assert_eq!((renamed.name.as_str(), renamed.version), ("Products", 2));
    assert_eq!(
        code(rename(&mut org, "t1", 1, "Again")),
        ErrorCode::Conflict
    );
    assert_eq!(
        code(rename(&mut org, "t1", 2, "platform")),
        ErrorCode::AlreadyExists
    );
    // Recapitalizing its own name is not a clash.
    assert_eq!(
        team_of(&rename(&mut org, "t1", 2, "PRODUCTS").unwrap()).name,
        "PRODUCTS"
    );
    assert_eq!(code(rename(&mut org, "nope", 1, "X")), ErrorCode::NotFound);
    // invalid_name comes before conflict.
    assert_eq!(
        code(rename(&mut org, "t1", 99, "  ")),
        ErrorCode::InvalidName
    );
}

#[test]
fn moving_a_team_cannot_make_a_loop() {
    let mut org = Org::new();
    org.team("a", "A", None).unwrap();
    org.team("b", "B", Some("a")).unwrap();
    org.team("c", "C", Some("b")).unwrap();
    let mv = |org: &mut Org, id: &str, to: Option<&str>| {
        let command_id = org.id();
        let expected_version = org.state.teams[id].version;
        org.run(OrgCommand::TeamMove(TeamMoveParams {
            command_id,
            team_id: id.into(),
            expected_version,
            owner_team_id: to.map(Into::into),
        }))
    };
    assert_eq!(code(mv(&mut org, "a", Some("a"))), ErrorCode::Cycle);
    assert_eq!(code(mv(&mut org, "a", Some("c"))), ErrorCode::Cycle);
    assert_eq!(code(mv(&mut org, "b", Some("c"))), ErrorCode::Cycle);
    assert_eq!(code(mv(&mut org, "c", Some("nope"))), ErrorCode::NotFound);
    // Valid moves, including to the top level.
    let moved = team_of(&mv(&mut org, "c", Some("a")).unwrap());
    assert_eq!(
        (moved.owner_team_id.as_deref(), moved.version),
        (Some("a"), 2)
    );
    assert_eq!(
        team_of(&mv(&mut org, "c", None).unwrap()).owner_team_id,
        None
    );
    // A stale version is a conflict, and cycle is reported before it.
    let command_id = org.id();
    let stale = org.run(OrgCommand::TeamMove(TeamMoveParams {
        command_id,
        team_id: "b".into(),
        expected_version: 9,
        owner_team_id: Some("c".into()),
    }));
    assert_eq!(code(stale), ErrorCode::Conflict);
    let command_id = org.id();
    let stale_cycle = org.run(OrgCommand::TeamMove(TeamMoveParams {
        command_id,
        team_id: "a".into(),
        expected_version: 9,
        owner_team_id: Some("b".into()),
    }));
    assert_eq!(code(stale_cycle), ErrorCode::Cycle);
}

#[test]
fn moving_a_team_onto_a_sibling_with_the_same_name_is_refused() {
    let mut org = Org::new();
    org.team("top", "Design", None).unwrap();
    org.team("p", "Product", None).unwrap();
    org.team("inner", "design", Some("p")).unwrap();
    let command_id = org.id();
    let moved = org.run(OrgCommand::TeamMove(TeamMoveParams {
        command_id,
        team_id: "inner".into(),
        expected_version: 1,
        owner_team_id: None,
    }));
    assert_eq!(code(moved), ErrorCode::AlreadyExists);
}

#[test]
fn a_teams_lead_must_be_a_member_and_never_the_owner() {
    let mut org = Org::new();
    org.team("t1", "Product", None).unwrap();
    org.person("ANA00000000000000000000000", "Ana").unwrap();
    assert_eq!(
        code(org.lead("t1", Some("ANA00000000000000000000000"))),
        ErrorCode::NotAMember
    );
    org.join("m1", "t1", "ANA00000000000000000000000").unwrap();
    let led = team_of(&org.lead("t1", Some("ANA00000000000000000000000")).unwrap());
    assert_eq!(
        (led.lead_member_id.as_deref(), led.version),
        (Some("ANA00000000000000000000000"), 2)
    );
    assert_eq!(code(org.lead("t1", Some(OWNER))), ErrorCode::OwnerProtected);
    assert_eq!(code(org.lead("t1", Some("nobody"))), ErrorCode::NotFound);
    // Clearing the lead.
    assert_eq!(team_of(&org.lead("t1", None).unwrap()).lead_member_id, None);
}

#[test]
fn a_member_can_be_in_many_teams_and_duplicates_are_refused() {
    let mut org = Org::new();
    org.team("t1", "Product", None).unwrap();
    org.team("t2", "Platform", None).unwrap();
    org.person("ANA00000000000000000000000", "Ana").unwrap();
    org.join("m1", "t1", "ANA00000000000000000000000").unwrap();
    org.join("m2", "t2", "ANA00000000000000000000000").unwrap();
    assert_eq!(
        org.state
            .memberships_of_member("ANA00000000000000000000000")
            .len(),
        2
    );
    assert_eq!(
        code(org.join("m3", "t1", "ANA00000000000000000000000")),
        ErrorCode::AlreadyExists
    );
    assert_eq!(
        code(org.join("m1", "t2", "ANA00000000000000000000000")),
        ErrorCode::AlreadyExists
    );
    assert_eq!(code(org.join("m9", "t1", OWNER)), ErrorCode::OwnerProtected);
    assert_eq!(
        code(org.join("m9", "nope", "ANA00000000000000000000000")),
        ErrorCode::NotFound
    );
    assert_eq!(code(org.join("m9", "t1", "nobody")), ErrorCode::NotFound);
    // Adding and removing members does not change the team's version.
    assert_eq!(org.state.teams["t1"].version, 1);
}

#[test]
fn a_title_in_a_team_is_trimmed_and_limited() {
    let mut org = Org::new();
    org.team("t1", "Product", None).unwrap();
    org.person("ANA00000000000000000000000", "Ana").unwrap();
    let command_id = org.id();
    let added = org
        .run(OrgCommand::TeamAddMember(TeamAddMemberParams {
            command_id,
            membership_id: "m1".into(),
            team_id: "t1".into(),
            member_id: "ANA00000000000000000000000".into(),
            title_in_team: Some("  frontend lead ".into()),
        }))
        .unwrap();
    assert!(
        matches!(&added[0], OrgChange::MembershipUpserted { membership }
        if membership.title_in_team.as_deref() == Some("frontend lead") && membership.version == 1)
    );
    for bad in ["", "   ", &"x".repeat(AGENT_TEXT_MAX_CHARS + 1), "a\u{0}b"] {
        let command_id = org.id();
        let result = org.run(OrgCommand::TeamSetTitle(TeamSetTitleParams {
            command_id,
            membership_id: "m1".into(),
            expected_version: 1,
            title_in_team: Some(bad.into()),
        }));
        assert_eq!(code(result), ErrorCode::InvalidRequest, "{bad:?}");
    }
    let command_id = org.id();
    let retitled = org
        .run(OrgCommand::TeamSetTitle(TeamSetTitleParams {
            command_id,
            membership_id: "m1".into(),
            expected_version: 1,
            title_in_team: None,
        }))
        .unwrap();
    assert!(
        matches!(&retitled[0], OrgChange::MembershipUpserted { membership }
        if membership.title_in_team.is_none() && membership.version == 2)
    );
}

#[test]
fn leaving_a_team_ends_leading_it_in_the_same_event() {
    let mut org = Org::new();
    org.team("t1", "Product", None).unwrap();
    org.person("ANA00000000000000000000000", "Ana").unwrap();
    org.person("BO000000000000000000000000", "Bo").unwrap();
    org.join("m1", "t1", "ANA00000000000000000000000").unwrap();
    org.join("m2", "t1", "BO000000000000000000000000").unwrap();
    org.lead("t1", Some("ANA00000000000000000000000")).unwrap();
    let leave = |org: &mut Org, membership: &str| {
        let command_id = org.id();
        let expected_version = org
            .state
            .memberships
            .get(membership)
            .map_or(1, |m| m.version);
        org.run(OrgCommand::TeamRemoveMember(TeamRemoveMemberParams {
            command_id,
            membership_id: membership.into(),
            expected_version,
        }))
    };
    // A member who is not the lead leaves: the team is untouched.
    let changes = leave(&mut org, "m2").unwrap();
    assert_eq!(
        changes,
        vec![OrgChange::MembershipRemoved {
            membership_id: "m2".into()
        }]
    );
    assert_eq!(org.state.teams["t1"].version, 2);
    // The lead leaves: one event removes the membership and clears the lead.
    let changes = leave(&mut org, "m1").unwrap();
    assert_eq!(changes.len(), 2);
    let team = team_of(&changes);
    assert_eq!((team.lead_member_id, team.version), (None, 3));
    assert_eq!(code(leave(&mut org, "m1")), ErrorCode::NotFound);
}

#[test]
fn deleting_a_team_needs_it_empty_of_teams_and_takes_its_memberships_and_links() {
    let mut org = Org::new();
    org.team("a", "A", None).unwrap();
    org.team("b", "B", Some("a")).unwrap();
    org.person("ANA00000000000000000000000", "Ana").unwrap();
    org.join("m1", "b", "ANA00000000000000000000000").unwrap();
    let link = |org: &mut Org, id: &str, team: &str| {
        let command_id = org.id();
        org.run(OrgCommand::ProjectAddTeam(ProjectAddTeamParams {
            command_id,
            project_team_id: id.into(),
            project_id: "p1".into(),
            team_id: team.into(),
        }))
    };
    link(&mut org, "l1", "b").unwrap();
    link(&mut org, "l2", "a").unwrap();
    let delete = |org: &mut Org, id: &str| {
        let command_id = org.id();
        let expected_version = org.state.teams.get(id).map_or(1, |t| t.version);
        org.run(OrgCommand::TeamDelete(TeamDeleteParams {
            command_id,
            team_id: id.into(),
            expected_version,
        }))
    };
    assert_eq!(code(delete(&mut org, "a")), ErrorCode::NotEmpty);
    let changes = delete(&mut org, "b").unwrap();
    assert_eq!(
        changes,
        vec![
            OrgChange::MembershipRemoved {
                membership_id: "m1".into()
            },
            OrgChange::ProjectTeamRemoved {
                project_team_id: "l1".into()
            },
            OrgChange::TeamDeleted {
                team_id: "b".into()
            },
        ]
    );
    // The project and the person survive; the project simply has one team fewer.
    assert!(
        org.state.project_exists("p1")
            && org.state.members.contains_key("ANA00000000000000000000000")
    );
    assert!(org.state.project_team("l2").is_some());
    delete(&mut org, "a").unwrap();
    assert!(
        org.state.project_teams.is_empty(),
        "the project is unassigned, not deleted"
    );
    assert_eq!(code(delete(&mut org, "a")), ErrorCode::NotFound);
}

// ---- people -----------------------------------------------------------------------------

#[test]
fn people_share_one_handle_namespace_ignoring_case() {
    let mut org = Org::new();
    org.person("ANA00000000000000000000000", "Ana").unwrap();
    assert_eq!(
        code(org.person("ANA20000000000000000000000", "ANA")),
        ErrorCode::AlreadyExists
    );
    assert_eq!(
        code(org.person("ANA00000000000000000000000", "Somebody")),
        ErrorCode::AlreadyExists
    );
    // "owner" is reserved for the owner, and "office" for the office, in any capitalization.
    for reserved in ["owner", "Owner", "OWNER", "office", "Office", "OFFICE"] {
        assert_eq!(
            code(org.person("X0000000000000000000000000", reserved)),
            ErrorCode::AlreadyExists,
            "{reserved}"
        );
    }
    assert_eq!(
        code(org.person("X0000000000000000000000000", "  ")),
        ErrorCode::InvalidName
    );
}

#[test]
fn the_word_owner_stays_reserved_even_after_the_owner_is_renamed() {
    let mut org = Org::new();
    // The owner's name is "Owner" at first, which would block the word by name alone. Rename
    // it: now only the reservation itself is left to stop "owner".
    let command_id = org.id();
    org.run(OrgCommand::MemberUpdatePerson(MemberUpdatePersonParams {
        command_id,
        member_id: OWNER.into(),
        expected_version: 1,
        name: Some("Vitor".into()),
        contact: None,
        clear_contact: false,
    }))
    .unwrap();
    for word in ["owner", "Owner", "OWNER", "office", "Office", "OFFICE"] {
        assert_eq!(
            code(org.person("X0000000000000000000000000", word)),
            ErrorCode::AlreadyExists,
            "{word}"
        );
    }
    // Another person cannot be renamed to it either.
    org.person("ANA00000000000000000000000", "Ana").unwrap();
    let command_id = org.id();
    let rename = org.run(OrgCommand::MemberUpdatePerson(MemberUpdatePersonParams {
        command_id,
        member_id: "ANA00000000000000000000000".into(),
        expected_version: 1,
        name: Some("owner".into()),
        contact: None,
        clear_contact: false,
    }));
    assert_eq!(code(rename), ErrorCode::AlreadyExists);
}

#[test]
fn a_persons_name_cannot_take_an_agents_handle_in_either_direction() {
    let mut org = Org::new();
    org.agent("a1", "server-dev", AgentStatus::Active);
    for clash in ["server-dev", "Server-Dev", "SERVER-DEV"] {
        assert_eq!(
            code(org.person("P0000000000000000000000000", clash)),
            ErrorCode::AlreadyExists,
            "{clash}"
        );
    }
    org.person("ANA00000000000000000000000", "Ana").unwrap();
    let rename = |org: &mut Org, to: &str| {
        let command_id = org.id();
        org.run(OrgCommand::MemberUpdatePerson(MemberUpdatePersonParams {
            command_id,
            member_id: "ANA00000000000000000000000".into(),
            expected_version: org.state.members["ANA00000000000000000000000"].version,
            name: Some(to.into()),
            contact: None,
            clear_contact: false,
        }))
    };
    assert_eq!(
        code(rename(&mut org, "Server-Dev")),
        ErrorCode::AlreadyExists
    );
    assert_eq!(code(rename(&mut org, "owner")), ErrorCode::AlreadyExists);
    // An agent's display name is not a handle.
    org.person("P2000000000000000000000000", "Agent a1")
        .unwrap();
}

#[test]
fn updating_a_person_keeps_ones_own_name_and_the_owner_may_rename_itself() {
    let mut org = Org::new();
    org.person("ANA00000000000000000000000", "Ana").unwrap();
    org.person("BO000000000000000000000000", "Bo").unwrap();
    let update =
        |org: &mut Org, id: &str, name: Option<&str>, contact: Option<&str>, clear: bool| {
            let command_id = org.id();
            let expected_version = org.state.members[id].version;
            org.run(OrgCommand::MemberUpdatePerson(MemberUpdatePersonParams {
                command_id,
                member_id: id.into(),
                expected_version,
                name: name.map(Into::into),
                contact: contact.map(Into::into),
                clear_contact: clear,
            }))
        };
    // Recapitalizing its own name is fine; another person's name is not.
    assert_eq!(
        member_of(
            &update(
                &mut org,
                "ANA00000000000000000000000",
                Some("ANA"),
                None,
                false
            )
            .unwrap(),
            "ANA00000000000000000000000"
        )
        .name,
        "ANA"
    );
    assert_eq!(
        code(update(
            &mut org,
            "ANA00000000000000000000000",
            Some("bo"),
            None,
            false
        )),
        ErrorCode::AlreadyExists
    );
    // Contact: set, kept when not given, cleared.
    let m = member_of(
        &update(
            &mut org,
            "BO000000000000000000000000",
            None,
            Some(" bo@example.com "),
            false,
        )
        .unwrap(),
        "BO000000000000000000000000",
    );
    assert_eq!(m.contact.as_deref(), Some("bo@example.com"));
    let m = member_of(
        &update(
            &mut org,
            "BO000000000000000000000000",
            Some("Bo B"),
            None,
            false,
        )
        .unwrap(),
        "BO000000000000000000000000",
    );
    assert_eq!(
        (m.name.as_str(), m.contact.as_deref()),
        ("Bo B", Some("bo@example.com"))
    );
    assert_eq!(
        member_of(
            &update(&mut org, "BO000000000000000000000000", None, None, true).unwrap(),
            "BO000000000000000000000000"
        )
        .contact,
        None
    );
    assert_eq!(
        code(update(
            &mut org,
            "BO000000000000000000000000",
            None,
            Some("x"),
            true
        )),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        code(update(
            &mut org,
            "BO000000000000000000000000",
            None,
            Some("   "),
            false
        )),
        ErrorCode::InvalidRequest
    );
    // The owner edits its own record and keeps the reserved word as its handle.
    let m = member_of(
        &update(&mut org, OWNER, Some("Vitor"), None, false).unwrap(),
        OWNER,
    );
    assert!(m.owner && m.name == "Vitor");
    update(&mut org, OWNER, Some("owner"), None, false).unwrap();
    // Agents are not edited here; stale versions conflict.
    org.agent("a1", "river", AgentStatus::Active);
    assert_eq!(
        code(update(&mut org, "a1", Some("X"), None, false)),
        ErrorCode::InvalidRequest
    );
    let command_id = org.id();
    let stale = org.run(OrgCommand::MemberUpdatePerson(MemberUpdatePersonParams {
        command_id,
        member_id: "ANA00000000000000000000000".into(),
        expected_version: 99,
        name: None,
        contact: None,
        clear_contact: false,
    }));
    assert_eq!(code(stale), ErrorCode::Conflict);
}

// ---- reporting lines --------------------------------------------------------------------

#[test]
fn reporting_lines_have_no_loops_and_the_owner_reports_to_no_one() {
    let mut org = Org::new();
    for (id, name) in [
        ("A0000000000000000000000000", "A"),
        ("B0000000000000000000000000", "B"),
        ("C0000000000000000000000000", "C"),
    ] {
        org.person(id, name).unwrap();
    }
    org.reports(
        "B0000000000000000000000000",
        Some("A0000000000000000000000000"),
    )
    .unwrap();
    org.reports(
        "C0000000000000000000000000",
        Some("B0000000000000000000000000"),
    )
    .unwrap();
    assert_eq!(
        code(org.reports(
            "A0000000000000000000000000",
            Some("A0000000000000000000000000")
        )),
        ErrorCode::Cycle
    );
    assert_eq!(
        code(org.reports(
            "A0000000000000000000000000",
            Some("C0000000000000000000000000")
        )),
        ErrorCode::Cycle
    );
    assert_eq!(
        code(org.reports("A0000000000000000000000000", Some("nobody"))),
        ErrorCode::NotFound
    );
    assert_eq!(
        code(org.reports(OWNER, Some("A0000000000000000000000000"))),
        ErrorCode::OwnerProtected
    );
    assert_eq!(code(org.reports(OWNER, None)), ErrorCode::OwnerProtected);
    // Reporting to the owner is stored as reporting to no one: one form for each line.
    let m = member_of(
        &org.reports("C0000000000000000000000000", Some(OWNER))
            .unwrap(),
        "C0000000000000000000000000",
    );
    assert_eq!(m.reports_to_member_id, None);
    assert_eq!(
        member_of(
            &org.reports(
                "C0000000000000000000000000",
                Some("A0000000000000000000000000")
            )
            .unwrap(),
            "C0000000000000000000000000"
        )
        .reports_to_member_id
        .as_deref(),
        Some("A0000000000000000000000000")
    );
    assert_eq!(code(org.reports("nobody", None)), ErrorCode::NotFound);
}

#[test]
fn nobody_reports_to_or_from_an_offboarded_agent() {
    let mut org = Org::new();
    org.person("ANA00000000000000000000000", "Ana").unwrap();
    org.agent("gone", "gone", AgentStatus::Offboarded);
    assert_eq!(
        code(org.reports("ANA00000000000000000000000", Some("gone"))),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        code(org.reports("gone", Some("ANA00000000000000000000000"))),
        ErrorCode::InvalidRequest
    );
}

// ---- removing a member ------------------------------------------------------------------

#[test]
fn removing_a_member_ends_its_relations_and_reports_go_to_its_manager() {
    let mut org = Org::new();
    org.team("t1", "Product", None).unwrap();
    org.team("t2", "Platform", None).unwrap();
    for (id, name) in [
        ("BOSS0000000000000000000000", "Boss"),
        ("MID00000000000000000000000", "Mid"),
        ("LOW00000000000000000000000", "Low"),
    ] {
        org.person(id, name).unwrap();
    }
    org.reports(
        "MID00000000000000000000000",
        Some("BOSS0000000000000000000000"),
    )
    .unwrap();
    org.reports(
        "LOW00000000000000000000000",
        Some("MID00000000000000000000000"),
    )
    .unwrap();
    org.join("m1", "t1", "MID00000000000000000000000").unwrap();
    org.join("m2", "t2", "MID00000000000000000000000").unwrap();
    org.lead("t1", Some("MID00000000000000000000000")).unwrap();

    let before_team = org.state.teams["t1"].version;
    let before_low = org.state.members["LOW00000000000000000000000"].version;
    let changes = org.remove("MID00000000000000000000000").unwrap();

    // With a manager: the report now reports to the removed member's own manager.
    let low = member_of(&changes, "LOW00000000000000000000000");
    assert_eq!(
        (low.reports_to_member_id.as_deref(), low.version),
        (Some("BOSS0000000000000000000000"), before_low + 1)
    );
    // The led team lost its lead, with a new version; the other team is untouched.
    let t1 = team_of(&changes);
    assert_eq!((t1.lead_member_id, t1.version), (None, before_team + 1));
    assert_eq!(org.state.teams["t2"].version, 1);
    assert!(org.state.memberships.is_empty());
    assert!(
        matches!(changes.last(), Some(OrgChange::MemberRemoved { member_id }) if member_id == "MID00000000000000000000000")
    );
    assert!(!org.state.members.contains_key("MID00000000000000000000000"));
    // Its handle is free again.
    org.person("MID20000000000000000000000", "Mid").unwrap();
}

#[test]
fn removing_a_member_with_no_manager_sends_its_reports_to_the_owner() {
    let mut org = Org::new();
    org.person("MID00000000000000000000000", "Mid").unwrap();
    org.person("LOW00000000000000000000000", "Low").unwrap();
    org.reports(
        "LOW00000000000000000000000",
        Some("MID00000000000000000000000"),
    )
    .unwrap();
    let changes = org.remove("MID00000000000000000000000").unwrap();
    assert_eq!(
        member_of(&changes, "LOW00000000000000000000000").reports_to_member_id,
        None
    );
}

#[test]
fn the_owner_cannot_be_removed_and_other_removals_are_checked() {
    let mut org = Org::new();
    org.person("ANA00000000000000000000000", "Ana").unwrap();
    org.agent("gone", "gone", AgentStatus::Offboarded);
    assert_eq!(code(org.remove(OWNER)), ErrorCode::OwnerProtected);
    assert_eq!(code(org.remove("nobody")), ErrorCode::NotFound);
    // An offboarded agent keeps its record and handle for good.
    assert_eq!(code(org.remove("gone")), ErrorCode::InvalidRequest);
    let command_id = org.id();
    let stale = org.run(OrgCommand::MemberRemove(MemberRemoveParams {
        command_id,
        member_id: "ANA00000000000000000000000".into(),
        expected_version: 9,
    }));
    assert_eq!(code(stale), ErrorCode::Conflict);
    assert!(org.state.members.contains_key("ANA00000000000000000000000"));
}

#[test]
fn nobody_new_can_join_a_team_or_lead_it_once_offboarded() {
    let mut org = Org::new();
    org.team("t1", "Product", None).unwrap();
    org.agent("gone", "gone", AgentStatus::Offboarded);
    assert_eq!(
        code(org.join("m1", "t1", "gone")),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        code(org.lead("t1", Some("gone"))),
        ErrorCode::InvalidRequest
    );
}

// ---- projects ---------------------------------------------------------------------------

#[test]
fn several_teams_can_share_a_project_and_links_are_their_own_entities() {
    let mut org = Org::new();
    org.team("t1", "Product", None).unwrap();
    org.team("t2", "Platform", None).unwrap();
    let link = |org: &mut Org, id: &str, project: &str, team: &str| {
        let command_id = org.id();
        org.run(OrgCommand::ProjectAddTeam(ProjectAddTeamParams {
            command_id,
            project_team_id: id.into(),
            project_id: project.into(),
            team_id: team.into(),
        }))
    };
    link(&mut org, "l1", "p1", "t1").unwrap();
    link(&mut org, "l2", "p1", "t2").unwrap();
    assert_eq!(
        code(link(&mut org, "l3", "p1", "t1")),
        ErrorCode::AlreadyExists
    );
    assert_eq!(
        code(link(&mut org, "l1", "p1", "t2")),
        ErrorCode::AlreadyExists
    );
    assert_eq!(
        code(link(&mut org, "l9", "nope", "t1")),
        ErrorCode::NotFound
    );
    assert_eq!(
        code(link(&mut org, "l9", "p1", "nope")),
        ErrorCode::NotFound
    );
    let command_id = org.id();
    let stale = org.run(OrgCommand::ProjectRemoveTeam(ProjectRemoveTeamParams {
        command_id,
        project_team_id: "l1".into(),
        expected_version: 5,
        areas_to_team_id: None,
    }));
    assert_eq!(code(stale), ErrorCode::Conflict);
    let command_id = org.id();
    let removed = org
        .run(OrgCommand::ProjectRemoveTeam(ProjectRemoveTeamParams {
            command_id,
            project_team_id: "l1".into(),
            expected_version: 1,
            areas_to_team_id: None,
        }))
        .unwrap();
    assert_eq!(
        removed,
        vec![OrgChange::ProjectTeamRemoved {
            project_team_id: "l1".into()
        }]
    );
    // Removing the link and adding it again gives a new entity with its own id.
    link(&mut org, "l1-again", "p1", "t1").unwrap();
    assert!(org.state.project_exists("p1"));
}

// ---- cross-cutting ----------------------------------------------------------------------

#[test]
fn replaying_the_log_on_an_empty_state_rebuilds_exactly_the_same_state() {
    let mut org = Org::new();
    org.team("a", "A", None).unwrap();
    org.team("b", "B", Some("a")).unwrap();
    org.person("ANA00000000000000000000000", "Ana").unwrap();
    org.person("BO000000000000000000000000", "Bo").unwrap();
    org.join("m1", "b", "ANA00000000000000000000000").unwrap();
    org.join("m2", "b", "BO000000000000000000000000").unwrap();
    org.lead("b", Some("ANA00000000000000000000000")).unwrap();
    org.reports(
        "BO000000000000000000000000",
        Some("ANA00000000000000000000000"),
    )
    .unwrap();
    org.remove("ANA00000000000000000000000").unwrap();
    org.person("CY000000000000000000000000", "Cy").unwrap();
    let projects = org.state.projects.clone();

    let mut rebuilt = MemState::default();
    for change in org.log.iter().flatten() {
        apply(&mut rebuilt, change).unwrap();
    }
    rebuilt.projects = projects; // projects are not made by these commands yet
    rebuilt.offices = org.state.offices.clone(); // nor offices
    assert_eq!(rebuilt, org.state);
}

#[test]
fn every_upsert_raises_the_version_by_exactly_one_and_failures_change_nothing() {
    let mut org = Org::new();
    org.team("t1", "Product", None).unwrap();
    org.person("ANA00000000000000000000000", "Ana").unwrap();
    org.join("m1", "t1", "ANA00000000000000000000000").unwrap();
    org.lead("t1", Some("ANA00000000000000000000000")).unwrap();
    let snapshot = org.state.clone();

    // A batch of failing commands leaves the state exactly as it was.
    assert!(org.team("t1", "Dup", None).is_err());
    assert!(org.person("ANA00000000000000000000000", "Ana").is_err());
    assert!(org.lead("t1", Some(OWNER)).is_err());
    assert!(org.remove(OWNER).is_err());
    assert_eq!(org.state, snapshot);

    // A cascade bumps each touched entity once, and only those.
    org.person("BO000000000000000000000000", "Bo").unwrap();
    org.reports(
        "BO000000000000000000000000",
        Some("ANA00000000000000000000000"),
    )
    .unwrap();
    let (team, bo) = (
        org.state.teams["t1"].version,
        org.state.members["BO000000000000000000000000"].version,
    );
    let changes = org.remove("ANA00000000000000000000000").unwrap();
    assert_eq!(team_of(&changes).version, team + 1);
    assert_eq!(
        member_of(&changes, "BO000000000000000000000000").version,
        bo + 1
    );
    assert_eq!(org.state.members[OWNER].version, 1);
}

#[test]
fn requests_are_read_as_commands_and_unknown_or_malformed_ones_are_told_apart() {
    let request = Request::new::<TeamCreate>(
        1,
        &TeamCreateParams {
            command_id: "c1".into(),
            team_id: "t1".into(),
            name: "Product".into(),
            owner_team_id: None,
        },
    )
    .unwrap();
    let command = OrgCommand::parse(&request).unwrap().expect("a team.create");
    assert_eq!(command.command_id(), "c1");
    assert!(matches!(command, OrgCommand::TeamCreate(ref p) if p.name == "Product"));

    // Not an organization command this build handles.
    let hello = Request {
        id: 2,
        method: "hello".into(),
        params: json!({}),
        principal: None,
    };
    assert!(OrgCommand::parse(&hello).unwrap().is_none());
    // A known method with params of the wrong shape.
    let bad = Request {
        id: 3,
        method: "team.create".into(),
        params: json!({"name": 5}),
        principal: None,
    };
    assert_eq!(code(OrgCommand::parse(&bad)), ErrorCode::InvalidRequest);
}

#[test]
fn a_change_from_a_newer_build_is_refused_not_silently_dropped() {
    let mut state = MemState::default();
    assert_eq!(apply(&mut state, &OrgChange::Unknown), Err(UnknownChange));
    assert_eq!(state, MemState::default());
}

#[test]
fn ids_are_short_and_plain() {
    let mut org = Org::new();
    let long = "x".repeat(ID_MAX_CHARS + 1);
    assert_eq!(code(org.team(&long, "X", None)), ErrorCode::InvalidRequest);
    org.team(&"x".repeat(ID_MAX_CHARS), "Y", None).unwrap();
    assert_eq!(code(org.team("a b", "Z", None)), ErrorCode::InvalidRequest);
    assert_eq!(code(org.team("é", "Z", None)), ErrorCode::InvalidRequest);
    org.team("01ARZ3NDEKTSV4RRFFQ69G5FAV", "Z", None).unwrap();
}

// ---- review follow-ups: stale versions, precedence, ids, agents ---------------------------

/// A small organization: teams t1 (Product) > t2 (Design), people ann and bob in t2.
fn staffed() -> Org {
    let mut org = Org::new();
    org.team("t1", "Product", None).unwrap();
    org.team("t2", "Design", Some("t1")).unwrap();
    org.person("ANN00000000000000000000000", "Ann").unwrap();
    org.person("BOB00000000000000000000000", "Bob").unwrap();
    org.join("ms-ann", "t2", "ANN00000000000000000000000")
        .unwrap();
    org.join("ms-bob", "t2", "BOB00000000000000000000000")
        .unwrap();
    org
}

fn message_of<T: std::fmt::Debug>(result: Result<T, ApiError>) -> String {
    result.expect_err("expected an error").message
}

#[test]
fn a_taken_id_is_told_apart_from_a_duplicate_relation() {
    let mut org = staffed();
    org.team("t3", "Other", None).unwrap();
    // A fresh pair under a taken membership id: the id is the problem.
    let taken = org.join("ms-ann", "t3", "BOB00000000000000000000000");
    assert_eq!(code(taken.clone()), ErrorCode::AlreadyExists);
    assert!(message_of(taken).contains("id is taken"));
    // A taken pair under a fresh id: the pair is.
    let twice = org.join("ms-new", "t2", "ANN00000000000000000000000");
    assert_eq!(code(twice.clone()), ErrorCode::AlreadyExists);
    assert!(message_of(twice).contains("already in that team"));

    let link = |org: &mut Org, id: &str, team: &str| {
        let command_id = org.id();
        org.run(OrgCommand::ProjectAddTeam(ProjectAddTeamParams {
            command_id,
            project_team_id: id.into(),
            project_id: "p1".into(),
            team_id: team.into(),
        }))
    };
    link(&mut org, "l1", "t2").unwrap();
    let taken = link(&mut org, "l1", "t3");
    assert_eq!(code(taken.clone()), ErrorCode::AlreadyExists);
    assert!(message_of(taken).contains("id is taken"));
    let twice = link(&mut org, "l2", "t2");
    assert_eq!(code(twice.clone()), ErrorCode::AlreadyExists);
    assert!(message_of(twice).contains("already works"));

    let taken = org.person("ANN00000000000000000000000", "Anna");
    assert_eq!(code(taken.clone()), ErrorCode::AlreadyExists);
    assert!(message_of(taken).contains("member id is taken"));
    let taken = org.team("t2", "Elsewhere", None);
    assert!(message_of(taken).contains("team id is taken"));
}

#[test]
fn ids_made_by_the_client_are_checked_on_every_command_that_takes_one() {
    let mut org = staffed();
    assert_eq!(
        code(org.join("bad id", "t1", "ANN00000000000000000000000")),
        ErrorCode::InvalidRequest
    );
    assert_eq!(code(org.person("bad id", "Zed")), ErrorCode::InvalidRequest);
    let command_id = org.id();
    let linked = org.run(OrgCommand::ProjectAddTeam(ProjectAddTeamParams {
        command_id,
        project_team_id: "x".repeat(65),
        project_id: "p1".into(),
        team_id: "t1".into(),
    }));
    assert_eq!(code(linked), ErrorCode::InvalidRequest);
    let command_id = org.id();
    let moved = org.run(OrgCommand::MemberMove(MemberMoveParams {
        command_id,
        membership_id: "ms-ann".into(),
        expected_version: 1,
        new_membership_id: "".into(),
        to_team_id: "t1".into(),
        title_in_team: None,
    }));
    assert_eq!(code(moved), ErrorCode::InvalidRequest);
    // Nothing of the above got in.
    assert_eq!(org.state.memberships.len(), 2);
    assert!(org.state.project_teams.is_empty());
}

#[test]
fn moving_a_team_compares_its_stored_name_by_key() {
    let mut org = Org::new();
    org.team("t1", "Product", None).unwrap();
    org.team("t2", "Platform", None).unwrap();
    org.team("a", "design", Some("t1")).unwrap();
    org.team("b", "  DESIGN ", Some("t2")).unwrap();
    let command_id = org.id();
    let moved = org.run(OrgCommand::TeamMove(TeamMoveParams {
        command_id,
        team_id: "b".into(),
        expected_version: 1,
        owner_team_id: Some("t1".into()),
    }));
    assert_eq!(code(moved), ErrorCode::AlreadyExists);
    // Moving to where it already is clashes with nobody but itself.
    let command_id = org.id();
    org.run(OrgCommand::TeamMove(TeamMoveParams {
        command_id,
        team_id: "b".into(),
        expected_version: 1,
        owner_team_id: Some("t2".into()),
    }))
    .unwrap();
}

#[test]
fn a_stale_version_is_a_conflict_on_every_command_that_checks_one() {
    let mut org = staffed();
    org.lead("t2", Some("ANN00000000000000000000000")).unwrap();
    let link_id = org.id();
    org.run(OrgCommand::ProjectAddTeam(ProjectAddTeamParams {
        command_id: link_id,
        project_team_id: "l1".into(),
        project_id: "p1".into(),
        team_id: "t2".into(),
    }))
    .unwrap();
    let before = org.state.clone();
    let stale = 7;
    let commands = vec![
        OrgCommand::TeamSetLead(TeamSetLeadParams {
            command_id: "c".into(),
            team_id: "t2".into(),
            expected_version: stale,
            lead_member_id: Some("BOB00000000000000000000000".into()),
        }),
        OrgCommand::TeamDelete(TeamDeleteParams {
            command_id: "c".into(),
            team_id: "t2".into(),
            expected_version: stale,
        }),
        OrgCommand::TeamRemoveMember(TeamRemoveMemberParams {
            command_id: "c".into(),
            membership_id: "ms-bob".into(),
            expected_version: stale,
        }),
        OrgCommand::TeamSetTitle(TeamSetTitleParams {
            command_id: "c".into(),
            membership_id: "ms-bob".into(),
            expected_version: stale,
            title_in_team: Some("Designer".into()),
        }),
        OrgCommand::MemberSetReportsTo(MemberSetReportsToParams {
            command_id: "c".into(),
            member_id: "BOB00000000000000000000000".into(),
            expected_version: stale,
            reports_to_member_id: Some("ANN00000000000000000000000".into()),
        }),
        OrgCommand::MemberUpdatePerson(MemberUpdatePersonParams {
            command_id: "c".into(),
            member_id: "BOB00000000000000000000000".into(),
            expected_version: stale,
            name: Some("Robert".into()),
            contact: None,
            clear_contact: false,
        }),
        OrgCommand::MemberRemove(MemberRemoveParams {
            command_id: "c".into(),
            member_id: "BOB00000000000000000000000".into(),
            expected_version: stale,
        }),
        OrgCommand::MemberMove(MemberMoveParams {
            command_id: "c".into(),
            membership_id: "ms-bob".into(),
            expected_version: stale,
            new_membership_id: "ms-new".into(),
            to_team_id: "t1".into(),
            title_in_team: None,
        }),
        OrgCommand::ProjectRemoveTeam(ProjectRemoveTeamParams {
            command_id: "c".into(),
            project_team_id: "l1".into(),
            expected_version: stale,
            areas_to_team_id: None,
        }),
    ];
    for command in commands {
        let result = decide(&org.state, NOW, &command);
        assert_eq!(
            result.expect_err(&format!("{command:?}")).code,
            ErrorCode::Conflict,
            "{command:?}"
        );
    }
    assert_eq!(org.state, before, "deciding changes nothing");
}

#[test]
fn updating_a_person_raises_its_version_and_keeps_what_was_not_given() {
    let mut org = staffed();
    let command_id = org.id();
    let changes = org
        .run(OrgCommand::MemberUpdatePerson(MemberUpdatePersonParams {
            command_id,
            member_id: "ANN00000000000000000000000".into(),
            expected_version: 1,
            name: None,
            contact: Some(" ann@example.com ".into()),
            clear_contact: false,
        }))
        .unwrap();
    let ann = member_of(&changes, "ANN00000000000000000000000");
    assert_eq!(ann.version, 2);
    assert_eq!(ann.name, "Ann");
    assert_eq!(ann.contact.as_deref(), Some("ann@example.com"));
}

#[test]
fn a_domain_rule_is_told_before_a_stale_version_and_after_a_bad_value() {
    let mut org = staffed();
    let stale = 9;
    let team_set_lead = |org: &mut Org, lead: &str| {
        let command_id = org.id();
        org.run(OrgCommand::TeamSetLead(TeamSetLeadParams {
            command_id,
            team_id: "t2".into(),
            expected_version: stale,
            lead_member_id: Some(lead.into()),
        }))
    };
    assert_eq!(
        code(team_set_lead(&mut org, OWNER)),
        ErrorCode::OwnerProtected
    );
    org.person("CAT00000000000000000000000", "Cat").unwrap();
    assert_eq!(
        code(team_set_lead(&mut org, "CAT00000000000000000000000")),
        ErrorCode::NotAMember
    );
    assert_eq!(
        code(team_set_lead(&mut org, "ANN00000000000000000000000")),
        ErrorCode::Conflict
    );

    let command_id = org.id();
    let deleted = org.run(OrgCommand::TeamDelete(TeamDeleteParams {
        command_id,
        team_id: "t1".into(),
        expected_version: stale,
    }));
    assert_eq!(code(deleted), ErrorCode::NotEmpty);

    let command_id = org.id();
    let moved = org.run(OrgCommand::TeamMove(TeamMoveParams {
        command_id,
        team_id: "t1".into(),
        expected_version: stale,
        owner_team_id: Some("t2".into()),
    }));
    assert_eq!(code(moved), ErrorCode::Cycle);

    org.reports(
        "BOB00000000000000000000000",
        Some("ANN00000000000000000000000"),
    )
    .unwrap();
    let command_id = org.id();
    let looped = org.run(OrgCommand::MemberSetReportsTo(MemberSetReportsToParams {
        command_id,
        member_id: "ANN00000000000000000000000".into(),
        expected_version: stale,
        reports_to_member_id: Some("BOB00000000000000000000000".into()),
    }));
    assert_eq!(code(looped), ErrorCode::Cycle);

    let command_id = org.id();
    let removed = org.run(OrgCommand::MemberRemove(MemberRemoveParams {
        command_id,
        member_id: OWNER.into(),
        expected_version: stale,
    }));
    assert_eq!(code(removed), ErrorCode::OwnerProtected);

    // ...and a bad value is told before a domain rule.
    let command_id = org.id();
    let bad = org.run(OrgCommand::TeamAddMember(TeamAddMemberParams {
        command_id,
        membership_id: "m9".into(),
        team_id: "t1".into(),
        member_id: OWNER.into(),
        title_in_team: Some("  ".into()),
    }));
    assert_eq!(code(bad), ErrorCode::InvalidRequest);
}

#[test]
fn active_agents_are_members_leads_and_managers_and_removing_one_frees_its_handle() {
    let mut org = staffed();
    org.agent("ag", "Pixel", AgentStatus::Active);
    org.join("ms-ag", "t2", "ag").unwrap();
    org.lead("t2", Some("ag")).unwrap();
    org.reports("BOB00000000000000000000000", Some("ag"))
        .unwrap();
    assert_eq!(org.state.teams["t2"].lead_member_id.as_deref(), Some("ag"));
    // Its handle is taken while it is here, whatever its display name.
    assert_eq!(
        code(org.person("P2000000000000000000000000", "pixel")),
        ErrorCode::AlreadyExists
    );

    let changes = org.remove("ag").unwrap();
    assert!(changes.contains(&OrgChange::MembershipRemoved {
        membership_id: "ms-ag".into()
    }));
    assert_eq!(team_of(&changes).lead_member_id, None);
    assert_eq!(team_of(&changes).version, 3);
    assert_eq!(
        member_of(&changes, "BOB00000000000000000000000").reports_to_member_id,
        None
    );
    assert!(!org.state.members.contains_key("ag"));
    // The handle is free again.
    org.person("P2000000000000000000000000", "pixel").unwrap();
}

#[test]
fn moving_a_member_ends_one_membership_and_starts_another_in_one_event() {
    let mut org = staffed();
    org.lead("t2", Some("ANN00000000000000000000000")).unwrap();
    let command_id = org.id();
    let changes = org
        .run(OrgCommand::MemberMove(MemberMoveParams {
            command_id,
            membership_id: "ms-ann".into(),
            expected_version: 1,
            new_membership_id: "ms-ann-2".into(),
            to_team_id: "t1".into(),
            title_in_team: Some(" Head ".into()),
        }))
        .unwrap();
    assert_eq!(
        changes[..2],
        [
            OrgChange::MembershipRemoved {
                membership_id: "ms-ann".into()
            },
            OrgChange::MembershipUpserted {
                membership: Membership {
                    id: "ms-ann-2".into(),
                    team_id: "t1".into(),
                    member_id: "ANN00000000000000000000000".into(),
                    title_in_team: Some("Head".into()),
                    version: 1,
                }
            }
        ]
    );
    // Leaving the team she led ends the lead, in the same event.
    let old = team_of(&changes);
    assert_eq!(
        (old.id.as_str(), old.lead_member_id, old.version),
        ("t2", None, 3)
    );
    assert_eq!(changes.len(), 3);
    assert!(org.state.memberships.contains_key("ms-ann-2"));
    assert!(!org.state.memberships.contains_key("ms-ann"));
}

#[test]
fn a_move_that_leads_nothing_changes_no_team_and_bad_moves_are_refused() {
    let mut org = staffed();
    let mv = |org: &mut Org, membership: &str, new: &str, to: &str| {
        let command_id = org.id();
        org.run(OrgCommand::MemberMove(MemberMoveParams {
            command_id,
            membership_id: membership.into(),
            expected_version: 1,
            new_membership_id: new.into(),
            to_team_id: to.into(),
            title_in_team: None,
        }))
    };
    assert_eq!(code(mv(&mut org, "nope", "n", "t1")), ErrorCode::NotFound);
    assert_eq!(
        code(mv(&mut org, "ms-ann", "n", "nope")),
        ErrorCode::NotFound
    );
    // To the team she is already in.
    assert_eq!(
        code(mv(&mut org, "ms-ann", "n", "t2")),
        ErrorCode::AlreadyExists
    );
    // To a team another membership id is taken.
    assert_eq!(
        code(mv(&mut org, "ms-ann", "ms-bob", "t1")),
        ErrorCode::AlreadyExists
    );
    // Already in the target through another membership.
    org.join("ms-ann-t1", "t1", "ANN00000000000000000000000")
        .unwrap();
    assert_eq!(
        code(mv(&mut org, "ms-ann", "n", "t1")),
        ErrorCode::AlreadyExists
    );
    let changes = mv(&mut org, "ms-bob", "ms-bob-2", "t1").unwrap();
    assert!(!changes
        .iter()
        .any(|c| matches!(c, OrgChange::TeamUpserted { .. })));
}

#[test]
fn the_owner_bootstrap_checks_its_inputs() {
    const OWNER_ID: &str = "O1000000000000000000000000";
    // A member id is exactly 26 letters and digits: 25 and 27 are refused, and so is `is_valid_id`'s
    // wider alphabet.
    for bad in [
        "bad id",
        "o1",
        &OWNER_ID[1..],
        &format!("{OWNER_ID}0"),
        "O1-00000000000000000000000",
    ] {
        assert_eq!(
            code(owner_bootstrap(bad, "Owner", NOW)),
            ErrorCode::InvalidRequest,
            "{bad}"
        );
    }
    assert_eq!(
        code(owner_bootstrap(OWNER_ID, "  ", NOW)),
        ErrorCode::InvalidName
    );
    assert_eq!(
        code(owner_bootstrap(OWNER_ID, "a\u{7}b", NOW)),
        ErrorCode::InvalidName
    );
    let changes = owner_bootstrap(OWNER_ID, " Vi ", NOW).unwrap();
    assert_eq!(member_of(&changes, OWNER_ID).name, "Vi");
}

#[test]
fn name_key_matches_the_key_of_a_parsed_name() {
    assert_eq!(name_key("  Design "), "design");
    assert_eq!(name_key("DESIGN"), name_key("design"));
}

// ---- second review: titles, created_at, loops in bad data, more precedence ------------------

#[test]
fn a_move_does_not_carry_the_old_title_over() {
    let mut org = staffed();
    let command_id = org.id();
    org.run(OrgCommand::TeamSetTitle(TeamSetTitleParams {
        command_id,
        membership_id: "ms-ann".into(),
        expected_version: 1,
        title_in_team: Some("Head of design".into()),
    }))
    .unwrap();
    let command_id = org.id();
    org.run(OrgCommand::MemberMove(MemberMoveParams {
        command_id,
        membership_id: "ms-ann".into(),
        expected_version: 2,
        new_membership_id: "ms-ann-2".into(),
        to_team_id: "t1".into(),
        title_in_team: None,
    }))
    .unwrap();
    assert_eq!(org.state.memberships["ms-ann-2"].title_in_team, None);
}

#[test]
fn a_new_person_has_the_time_it_was_added_and_a_trimmed_contact() {
    let mut org = Org::new();
    let command_id = org.id();
    let changes = org
        .run(OrgCommand::MemberAddPerson(MemberAddPersonParams {
            command_id,
            member_id: "ZED00000000000000000000000".into(),
            name: " Zed ".into(),
            contact: Some(" zed@example.com ".into()),
        }))
        .unwrap();
    let zed = member_of(&changes, "ZED00000000000000000000000");
    assert_eq!(zed.created_at.as_deref(), Some(NOW));
    assert_eq!(zed.contact.as_deref(), Some("zed@example.com"));
    assert_eq!(
        (zed.version, zed.owner, zed.name.as_str()),
        (1, false, "Zed")
    );
}

#[test]
fn a_blank_name_is_not_told_to_be_an_organization_name() {
    let mut org = Org::new();
    let message = message_of(org.team("t1", "  ", None));
    assert!(
        !message.to_lowercase().contains("organization"),
        "{message}"
    );
}

#[test]
fn walks_over_loops_already_in_bad_data_end() {
    // Bad data (decide never makes it): t1 and t2 own each other, a and b report to each other.
    let mut state = MemState::default();
    let team = |id: &str, owner: &str| Team {
        id: id.into(),
        name: id.into(),
        owner_team_id: Some(owner.into()),
        lead_member_id: None,
        version: 1,
    };
    state.teams.insert("t1".into(), team("t1", "t2"));
    state.teams.insert("t2".into(), team("t2", "t1"));
    state.teams.insert(
        "t3".into(),
        Team {
            owner_team_id: None,
            ..team("t3", "x")
        },
    );
    let mut org = Org::new();
    org.state = state;
    let moved = decide(
        &org.state,
        NOW,
        &OrgCommand::TeamMove(TeamMoveParams {
            command_id: "c".into(),
            team_id: "t3".into(),
            expected_version: 1,
            owner_team_id: Some("t1".into()),
        }),
    );
    assert!(moved.is_ok(), "{moved:?}");

    let mut org = Org::new();
    org.person("A0000000000000000000000000", "Ann").unwrap();
    org.person("B0000000000000000000000000", "Bob").unwrap();
    org.person("C0000000000000000000000000", "Cat").unwrap();
    for (who, boss) in [
        ("A0000000000000000000000000", "B0000000000000000000000000"),
        ("B0000000000000000000000000", "A0000000000000000000000000"),
    ] {
        org.state.members.get_mut(who).unwrap().reports_to_member_id = Some(boss.into());
    }
    let reported = decide(
        &org.state,
        NOW,
        &OrgCommand::MemberSetReportsTo(MemberSetReportsToParams {
            command_id: "c".into(),
            member_id: "C0000000000000000000000000".into(),
            expected_version: 1,
            reports_to_member_id: Some("A0000000000000000000000000".into()),
        }),
    );
    assert!(reported.is_ok(), "{reported:?}");
}

#[test]
fn a_stale_version_never_hides_a_missing_thing_or_a_bad_value() {
    let mut org = staffed();
    let stale = 9;
    let mv = |to: &str, new: &str, title: Option<&str>| {
        decide(
            &org.state,
            NOW,
            &OrgCommand::MemberMove(MemberMoveParams {
                command_id: "c".into(),
                membership_id: "ms-ann".into(),
                expected_version: stale,
                new_membership_id: new.into(),
                to_team_id: to.into(),
                title_in_team: title.map(Into::into),
            }),
        )
    };
    assert_eq!(code(mv("nope", "n", None)), ErrorCode::NotFound);
    assert_eq!(code(mv("t1", "bad id", None)), ErrorCode::InvalidRequest);
    assert_eq!(code(mv("t1", "n", Some(" "))), ErrorCode::InvalidRequest);
    assert_eq!(code(mv("t1", "n", None)), ErrorCode::Conflict);

    let title = decide(
        &org.state,
        NOW,
        &OrgCommand::TeamSetTitle(TeamSetTitleParams {
            command_id: "c".into(),
            membership_id: "ms-ann".into(),
            expected_version: stale,
            title_in_team: Some(" ".into()),
        }),
    );
    assert_eq!(code(title), ErrorCode::InvalidRequest);

    org.agent("ag", "Pixel", AgentStatus::Active);
    let update = |member: &str, name: Option<&str>| {
        decide(
            &org.state,
            NOW,
            &OrgCommand::MemberUpdatePerson(MemberUpdatePersonParams {
                command_id: "c".into(),
                member_id: member.into(),
                expected_version: stale,
                name: name.map(Into::into),
                contact: None,
                clear_contact: false,
            }),
        )
    };
    assert_eq!(
        code(update("ANN00000000000000000000000", Some("  "))),
        ErrorCode::InvalidName
    );
    assert_eq!(code(update("ag", None)), ErrorCode::InvalidRequest);
    assert_eq!(code(update("nope", None)), ErrorCode::NotFound);
    assert_eq!(
        code(update("ANN00000000000000000000000", Some("Anna"))),
        ErrorCode::Conflict
    );
}

#[test]
fn a_missing_thing_is_told_before_a_bad_id_and_a_bad_value_before_a_bad_name() {
    let mut org = staffed();
    // not_found before invalid_request.
    assert_eq!(
        code(org.join("bad id", "nope", "ANN00000000000000000000000")),
        ErrorCode::NotFound
    );
    assert_eq!(code(org.join("bad id", "t1", "nope")), ErrorCode::NotFound);
    // invalid_request before invalid_name.
    let command_id = org.id();
    let added = org.run(OrgCommand::MemberAddPerson(MemberAddPersonParams {
        command_id,
        member_id: "bad id".into(),
        name: "  ".into(),
        contact: None,
    }));
    assert_eq!(code(added), ErrorCode::InvalidRequest);
    let command_id = org.id();
    let added = org.run(OrgCommand::MemberAddPerson(MemberAddPersonParams {
        command_id,
        member_id: "ZED00000000000000000000000".into(),
        name: "  ".into(),
        contact: Some(" ".into()),
    }));
    assert_eq!(code(added), ErrorCode::InvalidRequest);
}

#[test]
fn what_is_not_there_is_not_found_on_every_command_that_names_it() {
    let org = staffed();
    let lead = decide(
        &org.state,
        NOW,
        &OrgCommand::TeamSetLead(TeamSetLeadParams {
            command_id: "c".into(),
            team_id: "nope".into(),
            expected_version: 1,
            lead_member_id: None,
        }),
    );
    assert_eq!(code(lead), ErrorCode::NotFound);
    let title = decide(
        &org.state,
        NOW,
        &OrgCommand::TeamSetTitle(TeamSetTitleParams {
            command_id: "c".into(),
            membership_id: "nope".into(),
            expected_version: 1,
            title_in_team: None,
        }),
    );
    assert_eq!(code(title), ErrorCode::NotFound);
    let unlink = decide(
        &org.state,
        NOW,
        &OrgCommand::ProjectRemoveTeam(ProjectRemoveTeamParams {
            command_id: "c".into(),
            project_team_id: "nope".into(),
            expected_version: 1,
            areas_to_team_id: None,
        }),
    );
    assert_eq!(code(unlink), ErrorCode::NotFound);
}

// ---- third review: conflict before already_exists, the owner's display name ----------------

#[test]
fn a_stale_version_is_told_before_a_name_or_id_that_is_taken() {
    let mut org = staffed();
    org.team("t3", "Research", Some("t1")).unwrap();
    let stale = 9;
    let rename = decide(
        &org.state,
        NOW,
        &OrgCommand::TeamRename(TeamRenameParams {
            command_id: "c".into(),
            team_id: "t3".into(),
            expected_version: stale,
            name: "design".into(),
        }),
    );
    assert_eq!(code(rename), ErrorCode::Conflict);
    let moved = decide(
        &org.state,
        NOW,
        &OrgCommand::TeamMove(TeamMoveParams {
            command_id: "c".into(),
            team_id: "t2".into(),
            expected_version: stale,
            owner_team_id: None,
        }),
    );
    // Moving "Design" to the top level clashes with nothing, so a stale version is the only
    // problem; make a top-level "Design" and it is still the version that is told.
    assert_eq!(code(moved), ErrorCode::Conflict);
    org.team("t4", "Design", None).unwrap();
    let moved = decide(
        &org.state,
        NOW,
        &OrgCommand::TeamMove(TeamMoveParams {
            command_id: "c".into(),
            team_id: "t2".into(),
            expected_version: stale,
            owner_team_id: None,
        }),
    );
    assert_eq!(code(moved), ErrorCode::Conflict);
    let update = decide(
        &org.state,
        NOW,
        &OrgCommand::MemberUpdatePerson(MemberUpdatePersonParams {
            command_id: "c".into(),
            member_id: "ANN00000000000000000000000".into(),
            expected_version: stale,
            name: Some("Bob".into()),
            contact: None,
            clear_contact: false,
        }),
    );
    assert_eq!(code(update), ErrorCode::Conflict);
    let moved = decide(
        &org.state,
        NOW,
        &OrgCommand::MemberMove(MemberMoveParams {
            command_id: "c".into(),
            membership_id: "ms-ann".into(),
            expected_version: stale,
            new_membership_id: "ms-bob".into(),
            to_team_id: "t1".into(),
            title_in_team: None,
        }),
    );
    assert_eq!(code(moved), ErrorCode::Conflict);
}

#[test]
fn a_new_reporting_line_raises_the_version_by_one() {
    let mut org = staffed();
    let changes = org
        .reports(
            "BOB00000000000000000000000",
            Some("ANN00000000000000000000000"),
        )
        .unwrap();
    let bob = member_of(&changes, "BOB00000000000000000000000");
    assert_eq!(
        (bob.version, bob.reports_to_member_id.as_deref()),
        (2, Some("ANN00000000000000000000000"))
    );
}

#[test]
fn a_reporting_line_to_someone_missing_is_not_found_before_anything_else() {
    let mut org = staffed();
    org.agent("OLD00000000000000000000000", "Old", AgentStatus::Offboarded);
    let command_id = org.id();
    let result = org.run(OrgCommand::MemberSetReportsTo(MemberSetReportsToParams {
        command_id,
        member_id: "OLD00000000000000000000000".into(),
        expected_version: 1,
        reports_to_member_id: Some("nope".into()),
    }));
    assert_eq!(code(result), ErrorCode::NotFound);
}

#[test]
fn a_bad_contact_is_told_before_a_bad_name_in_a_person_update() {
    let org = staffed();
    let result = decide(
        &org.state,
        NOW,
        &OrgCommand::MemberUpdatePerson(MemberUpdatePersonParams {
            command_id: "c".into(),
            member_id: "ANN00000000000000000000000".into(),
            expected_version: 1,
            name: Some("  ".into()),
            contact: Some(" ".into()),
            clear_contact: false,
        }),
    );
    assert_eq!(code(result), ErrorCode::InvalidRequest);
}

#[test]
fn the_owners_display_name_is_not_a_handle() {
    let mut org = staffed();
    let rename = |org: &mut Org, member: &str, name: &str| {
        let command_id = org.id();
        let expected_version = org.state.members[member].version;
        org.run(OrgCommand::MemberUpdatePerson(MemberUpdatePersonParams {
            command_id,
            member_id: member.into(),
            expected_version,
            name: Some(name.into()),
            contact: None,
            clear_contact: false,
        }))
    };
    // The owner may take the name of another person, or the word "owner" for display...
    rename(&mut org, OWNER, "Ann").unwrap();
    rename(&mut org, OWNER, "Vi").unwrap();
    // ...and a new person may reuse the owner's display name.
    org.person("VI200000000000000000000000", "Vi").unwrap();
    // But the word `owner` is still nobody else's.
    assert_eq!(
        code(rename(&mut org, "BOB00000000000000000000000", "Owner")),
        ErrorCode::AlreadyExists
    );
    assert_eq!(
        code(org.person("P9000000000000000000000000", "owner")),
        ErrorCode::AlreadyExists
    );
    // And people still clash with each other.
    assert_eq!(
        code(org.person("P1000000000000000000000000", "vi")),
        ErrorCode::AlreadyExists
    );
}

#[test]
fn an_offboarded_agents_messaging_name_stays_taken_for_a_new_person_and_a_rename() {
    let mut org = staffed();
    org.agent(
        "OLD00000000000000000000000",
        "Pixel",
        AgentStatus::Offboarded,
    );
    assert_eq!(
        code(org.person("P2000000000000000000000000", "pixel")),
        ErrorCode::AlreadyExists
    );
    // A person cannot be renamed onto it either.
    let command_id = org.id();
    let expected_version = org.state.members["ANN00000000000000000000000"].version;
    let renamed = org.run(OrgCommand::MemberUpdatePerson(MemberUpdatePersonParams {
        command_id,
        member_id: "ANN00000000000000000000000".into(),
        expected_version,
        name: Some("PIXEL".into()),
        contact: None,
        clear_contact: false,
    }));
    assert_eq!(code(renamed), ErrorCode::AlreadyExists);
}

#[test]
fn a_view_that_returns_the_owner_for_a_handle_still_does_not_block_its_display_name() {
    let mut org = staffed();
    // The owner is displayed as "Vi"; a store that wrongly answered handle lookups with the
    // owner must not make "Vi" unavailable to a new person.
    let command_id = org.id();
    let expected_version = org.state.members[OWNER].version;
    org.run(OrgCommand::MemberUpdatePerson(MemberUpdatePersonParams {
        command_id,
        member_id: OWNER.into(),
        expected_version,
        name: Some("Vi".into()),
        contact: None,
        clear_contact: false,
    }))
    .unwrap();
    org.state.leaky_handles = true;
    org.person("VI200000000000000000000000", "Vi").unwrap();
}

#[test]
fn a_member_id_is_exactly_26_letters_and_digits() {
    let good = "ANA00000000000000000000000";
    let mut org = Org::new();
    for bad in [
        &good[1..],
        &format!("{good}0"),
        "ana",
        "ANA-0000000000000000000000",
        "",
    ] {
        assert_eq!(
            code(org.person(bad, "Ana")),
            ErrorCode::InvalidRequest,
            "{bad:?}"
        );
    }
    assert!(org.person(good, "Ana").is_ok());
}

#[test]
fn a_person_cannot_be_named_like_a_member_id_or_a_run() {
    let mut org = Org::new();
    let looks_like_an_id = "BOB00000000000000000000000";
    for bad in [
        looks_like_an_id,
        &looks_like_an_id.to_lowercase(),
        " BOB00000000000000000000000 ",
        "run:1",
        "RUN: later",
        "\u{212A}0000000000000000000000000", // Kelvin sign lower-cases to `k`: 26 alphanumerics
    ] {
        assert_eq!(
            code(org.person("ANA00000000000000000000000", bad)),
            ErrorCode::InvalidName,
            "{bad:?}"
        );
    }
    // Close, but not the shape: 25 and 27 characters, or `run` without the colon.
    assert!(org
        .person("ANA00000000000000000000000", "BOB0000000000000000000000")
        .is_ok());
    assert!(org
        .person("ANB00000000000000000000000", "BOB000000000000000000000000")
        .is_ok());
    assert!(org.person("ANC00000000000000000000000", "runner").is_ok());
}

#[test]
fn renaming_a_person_to_an_address_shape_is_refused_but_the_owner_may_display_anything() {
    let mut org = Org::new();
    org.person("ANA00000000000000000000000", "Ana").unwrap();
    let rename = |org: &mut Org, id: &str, name: &str| {
        let command_id = org.id();
        let expected_version = org.state.members[id].version;
        org.run(OrgCommand::MemberUpdatePerson(MemberUpdatePersonParams {
            command_id,
            member_id: id.into(),
            expected_version,
            name: Some(name.into()),
            contact: None,
            clear_contact: false,
        }))
    };

    assert_eq!(
        code(rename(
            &mut org,
            "ANA00000000000000000000000",
            "BOB00000000000000000000000"
        )),
        ErrorCode::InvalidName
    );
    assert_eq!(
        code(rename(&mut org, "ANA00000000000000000000000", "run:7")),
        ErrorCode::InvalidName
    );
    // The owner's display name is not a handle.
    assert!(rename(&mut org, OWNER, "BOB00000000000000000000000").is_ok());
}

// ---- agents -----------------------------------------------------------------------------

/// A member id for tests: `n` padded with zeros to the 26 characters an id has.
fn mid(n: &str) -> String {
    format!("{n:0<26}")
}

fn profile(handle: &str) -> AgentProfile {
    AgentProfile {
        role: "software engineer".into(),
        title: "Senior Engineer".into(),
        focus: None,
        runtime: AgentRuntime::ClaudeCode,
        model: None,
        run_mode: None,
        schedule: None,
        home: None,
        messaging_name: handle.into(),
        status: AgentStatus::Active,
        offboarded_at: None,
    }
}

impl Org {
    fn hire(&mut self, id: &str, handle: &str) -> Result<Vec<OrgChange>, ApiError> {
        self.hire_with(id, "Pixel", "office1", profile(handle))
    }

    fn hire_with(
        &mut self,
        id: &str,
        name: &str,
        office: &str,
        profile: AgentProfile,
    ) -> Result<Vec<OrgChange>, ApiError> {
        let command_id = self.id();
        self.run(OrgCommand::MemberAddAgent(MemberAddAgentParams {
            command_id,
            member_id: id.into(),
            name: name.into(),
            office_id: office.into(),
            profile,
        }))
    }

    fn version_of(&self, id: &str) -> u64 {
        self.state.members.get(id).map_or(1, |m| m.version)
    }

    fn edit_agent(
        &mut self,
        id: &str,
        change: impl FnOnce(&mut MemberUpdateAgentParams),
    ) -> Result<Vec<OrgChange>, ApiError> {
        let command_id = self.id();
        let mut params = MemberUpdateAgentParams {
            command_id,
            member_id: id.into(),
            expected_version: self.version_of(id),
            name: None,
            role: None,
            title: None,
            focus: None,
            clear_focus: false,
            runtime: None,
            model: None,
            clear_model: false,
            home: None,
            clear_home: false,
            messaging_name: None,
        };
        change(&mut params);
        self.run(OrgCommand::MemberUpdateAgent(params))
    }

    fn set_status(&mut self, id: &str, status: AgentStatus) -> Result<Vec<OrgChange>, ApiError> {
        let command_id = self.id();
        let expected_version = self.version_of(id);
        self.run(OrgCommand::MemberSetAgentStatus(
            MemberSetAgentStatusParams {
                command_id,
                member_id: id.into(),
                expected_version,
                status,
            },
        ))
    }

    fn offboard(&mut self, id: &str) -> Result<Vec<OrgChange>, ApiError> {
        let command_id = self.id();
        let expected_version = self.version_of(id);
        self.run(OrgCommand::MemberOffboard(MemberOffboardParams {
            command_id,
            member_id: id.into(),
            expected_version,
        }))
    }

    fn run_mode(
        &mut self,
        id: &str,
        run_mode: Option<RunMode>,
        schedule: Option<Schedule>,
    ) -> Result<Vec<OrgChange>, ApiError> {
        let command_id = self.id();
        let expected_version = self.version_of(id);
        self.run(OrgCommand::MemberSetRunMode(MemberSetRunModeParams {
            command_id,
            member_id: id.into(),
            expected_version,
            run_mode,
            schedule,
        }))
    }
}

fn agent_of(changes: &[OrgChange], id: &str) -> AgentProfile {
    member_of(changes, id).agent.expect("an agent")
}

#[test]
fn an_agent_is_added_with_its_profile_trimmed_and_the_time_it_was_hired() {
    let mut org = Org::new();
    let id = mid("AG1");
    let mut p = profile("pixel.bot");
    p.role = "  software engineer ".into();
    p.focus = Some(" payments ".into());
    p.model = Some("claude-sonnet-5-5".into());
    p.home = Some("/Users/dev/agents/pixel".into());
    p.run_mode = Some(RunMode::Scheduled);
    p.schedule = Some(Schedule {
        spec: " weekdays at 9 ".into(),
        time_zone: "Europe/Lisbon".into(),
    });
    p.status = AgentStatus::Paused;

    let changes = org.hire_with(&id, "  Pixel ", "office1", p).unwrap();

    let [OrgChange::MemberUpserted { member }] = &changes[..] else {
        panic!("{changes:?}")
    };
    assert_eq!(
        (
            member.kind,
            member.name.as_str(),
            member.owner,
            member.version
        ),
        (MemberKind::Agent, "Pixel", false, 1)
    );
    assert_eq!(member.office_id.as_deref(), Some("office1"));
    assert_eq!(member.created_at.as_deref(), Some(NOW));
    assert_eq!(member.contact, None);
    assert_eq!(member.reports_to_member_id, None);
    let agent = member.agent.as_ref().unwrap();
    assert_eq!(agent.role, "software engineer");
    assert_eq!(agent.focus.as_deref(), Some("payments"));
    assert_eq!(agent.status, AgentStatus::Paused);
    assert_eq!(agent.messaging_name, "pixel.bot");
    assert_eq!(agent.schedule.as_ref().unwrap().spec, "weekdays at 9");
    assert_eq!(agent.offboarded_at, None);
    // Addressed by its handle, in any case.
    assert_eq!(
        org.state.member_with_handle("pixel.bot").map(|m| m.id),
        Some(id)
    );
}

#[test]
fn an_agent_needs_a_known_office_a_member_id_and_a_profile_inside_its_limits() {
    let mut org = Org::new();
    let id = mid("AG1");
    let bad = |change: &dyn Fn(&mut AgentProfile)| {
        let mut p = profile("pixel");
        change(&mut p);
        p
    };
    // The office comes first (not_found), before anything else is looked at.
    let mut p = profile("pixel");
    p.role = String::new();
    assert_eq!(
        code(org.hire_with("short", "Pixel", "nowhere", p)),
        ErrorCode::NotFound
    );
    // A member id is exactly 26 letters and digits.
    for id in [
        "short",
        &mid("AG1")[1..],
        &format!("{}0", mid("AG1")),
        "has-dash",
    ] {
        let got = org.hire_with(id, "Pixel", "office1", profile("pixel"));
        if id == mid("AG1") {
            continue;
        }
        assert_eq!(code(got), ErrorCode::InvalidRequest, "{id}");
    }
    let cases: Vec<(&str, AgentProfile)> = vec![
        ("empty role", bad(&|p| p.role = " ".into())),
        ("empty title", bad(&|p| p.title = String::new())),
        ("control char", bad(&|p| p.title = "a\u{7}b".into())),
        (
            "long role",
            bad(&|p| p.role = "r".repeat(AGENT_TEXT_MAX_CHARS + 1)),
        ),
        ("empty focus", bad(&|p| p.focus = Some("  ".into()))),
        ("empty model", bad(&|p| p.model = Some(String::new()))),
        (
            "unknown runtime",
            bad(&|p| p.runtime = AgentRuntime::Unknown),
        ),
        (
            "codex spelled Other",
            bad(&|p| p.runtime = AgentRuntime::Other("Codex".into())),
        ),
        (
            "claude code spelled Other",
            bad(&|p| p.runtime = AgentRuntime::Other("claude-code".into())),
        ),
        (
            "claude code with a space",
            bad(&|p| p.runtime = AgentRuntime::Other("Claude Code".into())),
        ),
        (
            "empty other",
            bad(&|p| p.runtime = AgentRuntime::Other(String::new())),
        ),
        ("status unknown", bad(&|p| p.status = AgentStatus::Unknown)),
        (
            "status offboarded",
            bad(&|p| p.status = AgentStatus::Offboarded),
        ),
        (
            "offboarded_at given",
            bad(&|p| p.offboarded_at = Some(NOW.into())),
        ),
        (
            "run mode unknown",
            bad(&|p| p.run_mode = Some(RunMode::Unknown)),
        ),
        (
            "scheduled without a schedule",
            bad(&|p| p.run_mode = Some(RunMode::Scheduled)),
        ),
        (
            "a schedule without scheduled",
            bad(&|p| {
                p.schedule = Some(Schedule {
                    spec: "daily".into(),
                    time_zone: "UTC".into(),
                })
            }),
        ),
        (
            "relative home",
            bad(&|p| p.home = Some("agents/pixel".into())),
        ),
        ("home with ..", bad(&|p| p.home = Some("/a/../b".into()))),
        ("home with NUL", bad(&|p| p.home = Some("/a\u{0}b".into()))),
        (
            "long home",
            bad(&|p| p.home = Some(format!("/{}", "h".repeat(AGENT_HOME_MAX_BYTES)))),
        ),
    ];
    for (what, p) in cases {
        assert_eq!(
            code(org.hire_with(&id, "Pixel", "office1", p)),
            ErrorCode::InvalidRequest,
            "{what}"
        );
    }
    // Nothing was added; a named runtime, `Other` with a name of its own, and `..` inside a name
    // (not a component) are fine.
    assert!(!org.state.members.contains_key(&id));
    let mut ok = profile("pixel");
    ok.runtime = AgentRuntime::Other("aider".into());
    ok.home = Some("/a/b..c".into());
    assert!(org.hire_with(&id, "Pixel", "office1", ok).is_ok());
}

#[test]
fn a_schedule_has_a_spec_and_a_time_zone_that_can_never_be_a_path() {
    let mut org = Org::new();
    for (zone, fine) in [
        ("Europe/Lisbon", true),
        ("UTC", true),
        ("Etc/GMT+1", true),
        ("America/Argentina/Buenos_Aires", true),
        ("", false),
        ("/Europe", false),
        ("Europe/", false),
        ("Europe//Lisbon", false),
        ("../etc", false),
        ("Europe/..", false),
        ("a.b", false),
        ("Europe Lisbon", false),
        (&"z".repeat(AGENT_TIME_ZONE_MAX_CHARS + 1), false),
    ] {
        let mut p = profile("pixel");
        p.run_mode = Some(RunMode::Scheduled);
        p.schedule = Some(Schedule {
            spec: "daily".into(),
            time_zone: zone.into(),
        });
        let got = org.hire_with(&mid("AG1"), "Pixel", "office1", p);
        if fine {
            got.unwrap();
            org.remove(&mid("AG1")).unwrap();
        } else {
            assert_eq!(code(got), ErrorCode::InvalidRequest, "{zone:?}");
        }
    }
    let mut p = profile("pixel");
    p.run_mode = Some(RunMode::Scheduled);
    p.schedule = Some(Schedule {
        spec: " ".into(),
        time_zone: "UTC".into(),
    });
    assert_eq!(
        code(org.hire_with(&mid("AG1"), "Pixel", "office1", p)),
        ErrorCode::InvalidRequest
    );
}

#[test]
fn an_agents_handle_has_a_format_and_one_namespace_with_people_and_the_reserved_words() {
    let mut org = Org::new();
    org.person(&mid("PE1"), "Ana").unwrap();
    org.hire(&mid("AG1"), "Pixel").unwrap();
    // The format and the address shape: invalid_name.
    let looks_like_an_id = mid("ABC");
    for bad in [
        "",
        "-x",
        ".x",
        "a b",
        "é",
        "a/b",
        &"h".repeat(AGENT_MESSAGING_NAME_MAX_CHARS + 1),
        looks_like_an_id.as_str(),
        "run:1",
        " pixel2",
    ] {
        assert_eq!(
            code(org.hire(&mid("AG2"), bad)),
            ErrorCode::InvalidName,
            "{bad:?}"
        );
    }
    // Taken by an agent, a person (either case), or reserved: already_exists.
    for taken in [
        "pixel", "PIXEL", "ana", "ANA", "owner", "Owner", "office", "OFFICE",
    ] {
        assert_eq!(
            code(org.hire(&mid("AG2"), taken)),
            ErrorCode::AlreadyExists,
            "{taken}"
        );
    }
    // The other way round: a person may not take an agent's handle.
    assert_eq!(
        code(org.person(&mid("PE2"), "pixel")),
        ErrorCode::AlreadyExists
    );
    // The owner's display name is not a handle.
    org.hire(&mid("AG2"), "Owner2.bot").unwrap();
    // The display name of an agent is a name, not a handle: two agents may share one.
    org.hire_with(&mid("AG3"), "Pixel", "office1", profile("pixel3"))
        .unwrap();
}

#[test]
fn adding_an_agent_tells_the_errors_in_the_contracts_order() {
    let mut org = Org::new();
    org.hire(&mid("AG1"), "pixel").unwrap();
    let mut p = profile("pixel"); // taken
    p.role = String::new(); // and a bad profile
                            // invalid_request first,
    assert_eq!(
        code(org.hire_with(&mid("AG1"), "", "office1", p)),
        ErrorCode::InvalidRequest
    );
    // then invalid_name (the display name before the handle),
    assert_eq!(
        code(org.hire_with(&mid("AG1"), "  ", "office1", profile("pixel"))),
        ErrorCode::InvalidName
    );
    assert_eq!(
        code(org.hire_with(&mid("AG1"), "Fine", "office1", profile("-bad"))),
        ErrorCode::InvalidName
    );
    // then already_exists: the id before the handle.
    assert_eq!(
        code(org.hire_with(&mid("AG1"), "Fine", "office1", profile("other"))),
        ErrorCode::AlreadyExists
    );
}

#[test]
fn an_agent_is_edited_field_by_field_and_each_edit_raises_the_version() {
    let mut org = Org::new();
    let id = mid("AG1");
    let mut p = profile("pixel");
    p.focus = Some("payments".into());
    p.model = Some("m".into());
    p.home = Some("/h".into());
    org.hire_with(&id, "Pixel", "office1", p).unwrap();

    let changes = org
        .edit_agent(&id, |p| {
            p.name = Some(" Pixel Two ".into());
            p.role = Some("reviewer".into());
            p.title = Some(" Principal ".into());
            p.runtime = Some(AgentRuntime::Codex);
            p.messaging_name = Some("PIXEL".into()); // another capitalisation of its own
            p.clear_focus = true;
            p.clear_model = true;
            p.home = Some("/new/home".into());
        })
        .unwrap();

    let member = member_of(&changes, &id);
    assert_eq!((member.name.as_str(), member.version), ("Pixel Two", 2));
    let agent = member.agent.unwrap();
    assert_eq!(
        (
            agent.role.as_str(),
            agent.title.as_str(),
            agent.messaging_name.as_str()
        ),
        ("reviewer", "Principal", "PIXEL")
    );
    assert_eq!(agent.runtime, AgentRuntime::Codex);
    assert_eq!((agent.focus, agent.model), (None, None));
    assert_eq!(agent.home.as_deref(), Some("/new/home"));
    // The rest is untouched.
    assert_eq!(agent.status, AgentStatus::Active);
    assert_eq!(org.state.members[&id].office_id.as_deref(), Some("office1"));
    // Clearing what is not there, and setting what is, are accepted.
    org.edit_agent(&id, |p| p.clear_home = true).unwrap();
    org.edit_agent(&id, |p| p.role = Some("reviewer".into()))
        .unwrap();
    assert_eq!(org.state.members[&id].version, 4);
}

#[test]
fn editing_an_agent_refuses_what_the_contract_refuses() {
    let mut org = Org::new();
    let id = mid("AG1");
    org.hire(&id, "pixel").unwrap();
    org.hire(&mid("AG2"), "other").unwrap();
    org.person(&mid("PE1"), "Ana").unwrap();
    // Nothing to change, a field with its clear flag, values outside their limits.
    assert_eq!(code(org.edit_agent(&id, |_| {})), ErrorCode::InvalidRequest);
    assert_eq!(
        code(org.edit_agent(&id, |p| {
            p.focus = Some("x".into());
            p.clear_focus = true;
        })),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        code(org.edit_agent(&id, |p| {
            p.home = Some("/h".into());
            p.clear_home = true;
        })),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        code(org.edit_agent(&id, |p| {
            p.model = Some("m".into());
            p.clear_model = true;
        })),
        ErrorCode::InvalidRequest
    );
    for change in [
        (|p: &mut MemberUpdateAgentParams| p.role = Some(" ".into())) as fn(&mut _),
        |p| p.title = Some(String::new()),
        |p| p.focus = Some("  ".into()),
        |p| p.model = Some(String::new()),
        |p| p.runtime = Some(AgentRuntime::Unknown),
        |p| p.runtime = Some(AgentRuntime::Other("CODEX".into())),
        |p| p.home = Some("relative".into()),
    ] {
        assert_eq!(code(org.edit_agent(&id, change)), ErrorCode::InvalidRequest);
    }
    // A person is not edited here; a missing member is not found.
    assert_eq!(
        code(org.edit_agent(&mid("PE1"), |p| p.role = Some("x".into()))),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        code(org.edit_agent(&mid("NOPE"), |p| p.role = Some("x".into()))),
        ErrorCode::NotFound
    );
    // Names: the format, then a stale version, then a taken handle.
    assert_eq!(
        code(org.edit_agent(&id, |p| p.name = Some(" ".into()))),
        ErrorCode::InvalidName
    );
    assert_eq!(
        code(org.edit_agent(&id, |p| p.messaging_name = Some("-no".into()))),
        ErrorCode::InvalidName
    );
    assert_eq!(
        code(org.edit_agent(&id, |p| {
            p.messaging_name = Some("OTHER".into());
            p.expected_version = 99;
        })),
        ErrorCode::Conflict,
        "a stale version is told before a taken handle"
    );
    for taken in ["OTHER", "ana", "owner", "office"] {
        assert_eq!(
            code(org.edit_agent(&id, |p| p.messaging_name = Some(taken.into()))),
            ErrorCode::AlreadyExists,
            "{taken}"
        );
    }
    assert_eq!(
        code(org.edit_agent(&id, |p| {
            p.role = Some("x".into());
            p.expected_version = 99;
        })),
        ErrorCode::Conflict
    );
    // None of the refusals changed anything.
    assert_eq!(org.state.members[&id].version, 1);
}

#[test]
fn an_agent_is_paused_and_resumed_and_only_to_those_two() {
    let mut org = Org::new();
    let id = mid("AG1");
    org.hire(&id, "pixel").unwrap();
    org.person(&mid("PE1"), "Ana").unwrap();

    let paused = org.set_status(&id, AgentStatus::Paused).unwrap();
    assert_eq!(agent_of(&paused, &id).status, AgentStatus::Paused);
    assert_eq!(member_of(&paused, &id).version, 2);
    // The same status is accepted and still counts as an edit.
    org.set_status(&id, AgentStatus::Paused).unwrap();
    org.set_status(&id, AgentStatus::Active).unwrap();
    assert_eq!(org.state.members[&id].version, 4);

    for refused in [AgentStatus::Offboarded, AgentStatus::Unknown] {
        assert_eq!(
            code(org.set_status(&id, refused)),
            ErrorCode::InvalidRequest,
            "{refused:?}"
        );
    }
    assert_eq!(
        code(org.set_status(&mid("PE1"), AgentStatus::Paused)),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        code(org.set_status(&mid("NOPE"), AgentStatus::Paused)),
        ErrorCode::NotFound
    );
    let command_id = org.id();
    assert_eq!(
        code(org.run(OrgCommand::MemberSetAgentStatus(
            MemberSetAgentStatusParams {
                command_id,
                member_id: id.clone(),
                expected_version: 1,
                status: AgentStatus::Paused,
            }
        ))),
        ErrorCode::Conflict
    );
}

#[test]
fn offboarding_ends_an_agents_relations_and_keeps_its_record_and_its_handle() {
    let mut org = Org::new();
    let (boss, agent, report) = (mid("BOSS"), mid("AG1"), mid("REP"));
    org.person(&boss, "Bea").unwrap();
    org.hire(&agent, "pixel").unwrap();
    org.person(&report, "Rex").unwrap();
    org.team("t1", "Platform", None).unwrap();
    org.team("t2", "Design", None).unwrap();
    org.join("m1", "t1", &agent).unwrap();
    org.join("m2", "t2", &agent).unwrap();
    org.join("m3", "t2", &report).unwrap();
    org.lead("t1", Some(&agent)).unwrap();
    org.reports(&agent, Some(&boss)).unwrap();
    org.reports(&report, Some(&agent)).unwrap();
    // An agent that reported to it and was offboarded earlier: its record is history.
    let gone = mid("GONE");
    org.hire(&gone, "gone").unwrap();
    org.reports(&gone, Some(&agent)).unwrap();
    org.offboard(&gone).unwrap();
    let gone_before = org.state.members[&gone].clone();
    let before = org.state.members[&agent].version;

    let changes = org.offboard(&agent).unwrap();
    // It is left alone: it still reports to the agent, at the same version, and is not in the
    // changes at all.
    assert_eq!(org.state.members[&gone], gone_before);
    assert!(!changes
        .iter()
        .any(|c| matches!(c, OrgChange::MemberUpserted { member } if member.id == gone)));

    // Its memberships end, the team it led loses its lead, whoever reported to it now reports
    // to its manager, and the record stays, offboarded, one version up.
    assert!(changes.contains(&OrgChange::MembershipRemoved {
        membership_id: "m1".into()
    }));
    assert!(changes.contains(&OrgChange::MembershipRemoved {
        membership_id: "m2".into()
    }));
    assert_eq!(org.state.teams["t1"].lead_member_id, None);
    assert_eq!(org.state.teams["t1"].version, 3); // created, led, lead cleared
    assert_eq!(
        org.state.members[&report].reports_to_member_id.as_deref(),
        Some(boss.as_str())
    );
    let member = &org.state.members[&agent];
    assert_eq!(member.version, before + 1);
    let profile = member.agent.as_ref().unwrap();
    assert_eq!(profile.status, AgentStatus::Offboarded);
    assert_eq!(profile.offboarded_at.as_deref(), Some(NOW));
    assert_eq!(profile.messaging_name, "pixel");
    // The last change is the agent itself.
    assert!(
        matches!(changes.last(), Some(OrgChange::MemberUpserted { member }) if member.id == agent)
    );

    // Its handle stays reserved for good, and nobody may name it any more.
    assert_eq!(
        code(org.hire(&mid("AG2"), "pixel")),
        ErrorCode::AlreadyExists
    );
    assert_eq!(
        code(org.hire(&mid("AG2"), "PIXEL")),
        ErrorCode::AlreadyExists
    );
    assert_eq!(
        code(org.person(&mid("PE9"), "pixel")),
        ErrorCode::AlreadyExists
    );
    assert_eq!(
        code(org.join("m9", "t1", &agent)),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        code(org.lead("t1", Some(&agent))),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        code(org.reports(&report, Some(&agent))),
        ErrorCode::InvalidRequest
    );
    // It is history: no second offboarding, no edits, no status, no run mode, no removal.
    assert_eq!(code(org.offboard(&agent)), ErrorCode::InvalidRequest);
    assert_eq!(
        code(org.edit_agent(&agent, |p| p.role = Some("x".into()))),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        code(org.set_status(&agent, AgentStatus::Active)),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        code(org.run_mode(&agent, Some(RunMode::AlwaysOn), None)),
        ErrorCode::InvalidRequest
    );
    assert_eq!(code(org.remove(&agent)), ErrorCode::InvalidRequest);
}

#[test]
fn offboarding_checks_who_and_which_version() {
    let mut org = Org::new();
    org.person(&mid("PE1"), "Ana").unwrap();
    org.hire(&mid("AG1"), "pixel").unwrap();
    assert_eq!(code(org.offboard(&mid("PE1"))), ErrorCode::InvalidRequest);
    assert_eq!(code(org.offboard(&mid("NOPE"))), ErrorCode::NotFound);
    let command_id = org.id();
    assert_eq!(
        code(org.run(OrgCommand::MemberOffboard(MemberOffboardParams {
            command_id,
            member_id: mid("AG1"),
            expected_version: 7,
        }))),
        ErrorCode::Conflict
    );
    assert_eq!(org.state.members[&mid("AG1")].version, 1);
}

#[test]
fn an_agents_run_mode_is_set_with_its_schedule_or_cleared() {
    let mut org = Org::new();
    let id = mid("AG1");
    org.hire(&id, "pixel").unwrap();
    let schedule = Schedule {
        spec: " weekdays at 9 ".into(),
        time_zone: "Europe/Lisbon".into(),
    };

    for mode in [RunMode::AlwaysOn, RunMode::OnCall] {
        let changes = org.run_mode(&id, Some(mode), None).unwrap();
        assert_eq!(agent_of(&changes, &id).run_mode, Some(mode));
    }
    let scheduled = org
        .run_mode(&id, Some(RunMode::Scheduled), Some(schedule.clone()))
        .unwrap();
    let profile = agent_of(&scheduled, &id);
    assert_eq!(profile.run_mode, Some(RunMode::Scheduled));
    assert_eq!(profile.schedule.unwrap().spec, "weekdays at 9");
    // Leaving the run mode out takes the agent out of the office's control, schedule and all.
    let freed = org.run_mode(&id, None, None).unwrap();
    assert_eq!(agent_of(&freed, &id).run_mode, None);
    assert_eq!(agent_of(&freed, &id).schedule, None);
    assert_eq!(org.state.members[&id].version, 5);
    // The mode it has is accepted.
    org.run_mode(&id, None, None).unwrap();

    for (mode, schedule) in [
        (Some(RunMode::Unknown), None),
        (Some(RunMode::Scheduled), None),
        (Some(RunMode::OnCall), Some(schedule.clone())),
        (None, Some(schedule.clone())),
    ] {
        assert_eq!(
            code(org.run_mode(&id, mode, schedule)),
            ErrorCode::InvalidRequest
        );
    }
    org.person(&mid("PE1"), "Ana").unwrap();
    assert_eq!(
        code(org.run_mode(&mid("PE1"), Some(RunMode::AlwaysOn), None)),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        code(org.run_mode(&mid("NOPE"), Some(RunMode::AlwaysOn), None)),
        ErrorCode::NotFound
    );
    let command_id = org.id();
    assert_eq!(
        code(
            org.run(OrgCommand::MemberSetRunMode(MemberSetRunModeParams {
                command_id,
                member_id: id.clone(),
                expected_version: 1,
                run_mode: Some(RunMode::AlwaysOn),
                schedule: None,
            }))
        ),
        ErrorCode::Conflict
    );
}

#[test]
fn a_person_edit_refuses_an_agent_and_an_agent_edit_refuses_a_person() {
    let mut org = Org::new();
    org.hire(&mid("AG1"), "pixel").unwrap();
    let command_id = org.id();
    let got = org.run(OrgCommand::MemberUpdatePerson(MemberUpdatePersonParams {
        command_id,
        member_id: mid("AG1"),
        expected_version: 1,
        name: Some("x".into()),
        contact: None,
        clear_contact: false,
    }));
    assert_eq!(code(got), ErrorCode::InvalidRequest);
}

#[test]
fn the_agent_commands_are_read_from_their_requests() {
    use org_proto::Call;
    let command_id = "cmd-1".to_string();
    let requests = [
        Request::new::<MemberAddAgent>(
            1,
            &MemberAddAgentParams {
                command_id: command_id.clone(),
                member_id: mid("AG1"),
                name: "Pixel".into(),
                office_id: "office1".into(),
                profile: profile("pixel"),
            },
        )
        .unwrap(),
        Request::new::<MemberOffboard>(
            2,
            &MemberOffboardParams {
                command_id: command_id.clone(),
                member_id: mid("AG1"),
                expected_version: 1,
            },
        )
        .unwrap(),
        Request::new::<MemberSetAgentStatus>(
            3,
            &MemberSetAgentStatusParams {
                command_id: command_id.clone(),
                member_id: mid("AG1"),
                expected_version: 1,
                status: AgentStatus::Paused,
            },
        )
        .unwrap(),
        Request::new::<MemberSetRunMode>(
            4,
            &MemberSetRunModeParams {
                command_id: command_id.clone(),
                member_id: mid("AG1"),
                expected_version: 1,
                run_mode: None,
                schedule: None,
            },
        )
        .unwrap(),
    ];
    let methods = [
        MemberAddAgent::METHOD,
        MemberOffboard::METHOD,
        MemberSetAgentStatus::METHOD,
        MemberSetRunMode::METHOD,
    ];
    for (request, method) in requests.iter().zip(methods) {
        let command = OrgCommand::parse(request).unwrap().expect(method);
        assert_eq!(command.command_id(), "cmd-1");
        assert_eq!(command.method_and_params().0, method);
    }
    // The update has a request of its own, with its optional fields left out.
    let update = Request {
        id: 5,
        method: MemberUpdateAgent::METHOD.to_owned(),
        params: json!({"command_id": "cmd-1", "member_id": mid("AG1"), "expected_version": 1, "role": "x"}),
        principal: None,
    };
    assert!(matches!(
        OrgCommand::parse(&update).unwrap(),
        Some(OrgCommand::MemberUpdateAgent(_))
    ));
    // Params that do not decode are invalid_request, not an unknown method.
    let bad = Request {
        id: 6,
        method: MemberOffboard::METHOD.to_owned(),
        params: json!({"nope": 1}),
        principal: None,
    };
    assert_eq!(
        OrgCommand::parse(&bad).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
}

#[test]
fn the_edges_of_an_agents_home_time_zone_and_handle_are_exact() {
    let mut org = Org::new();
    let try_hire = |org: &mut Org, change: &dyn Fn(&mut AgentProfile)| {
        let mut p = profile("pixel");
        change(&mut p);
        let got = org.hire_with(&mid("AG1"), "Pixel", "office1", p);
        if got.is_ok() {
            org.remove(&mid("AG1")).unwrap();
        }
        got.map(|_| ())
    };
    let home = |h: String| move |p: &mut AgentProfile| p.home = Some(h.clone());
    let zone = |z: &str| {
        let z = z.to_owned();
        move |p: &mut AgentProfile| {
            p.run_mode = Some(RunMode::Scheduled);
            p.schedule = Some(Schedule {
                spec: "daily".into(),
                time_zone: z.clone(),
            });
        }
    };
    let handle = |h: &str| {
        let h = h.to_owned();
        move |p: &mut AgentProfile| p.messaging_name = h.clone()
    };

    // home: a `..` component anywhere (also first or last) is refused, not a substring check;
    // the limit is 1024 BYTES, inclusive.
    for bad in ["/..", "/a/..", "/../a", "/a/../b"] {
        assert_eq!(
            code(try_hire(&mut org, &home(bad.into()))),
            ErrorCode::InvalidRequest,
            "{bad}"
        );
    }
    assert!(try_hire(
        &mut org,
        &home(format!("/{}", "h".repeat(AGENT_HOME_MAX_BYTES - 1)))
    )
    .is_ok());
    assert_eq!(
        code(try_hire(
            &mut org,
            &home(format!("/{}", "h".repeat(AGENT_HOME_MAX_BYTES)))
        )),
        ErrorCode::InvalidRequest
    );
    // 600 two-byte characters are 600 characters but 1201 bytes with the slash.
    assert_eq!(
        code(try_hire(&mut org, &home(format!("/{}", "é".repeat(600))))),
        ErrorCode::InvalidRequest
    );
    assert!(try_hire(&mut org, &home("/a b/ü".into())).is_ok());

    // time zone: 64 characters inclusive; `-` and `+` allowed; no backslash, no non-ASCII.
    assert!(try_hire(&mut org, &zone(&"z".repeat(AGENT_TIME_ZONE_MAX_CHARS))).is_ok());
    assert!(try_hire(&mut org, &zone("Etc/GMT-5")).is_ok());
    for bad in ["Europe\\Lisbon", "Europe/Lisboà", "Europe/Lis:bon"] {
        assert_eq!(
            code(try_hire(&mut org, &zone(bad))),
            ErrorCode::InvalidRequest,
            "{bad}"
        );
    }

    // handle: 64 characters inclusive; `-` `_` `.` allowed inside; `:` not.
    assert!(try_hire(
        &mut org,
        &handle(&"h".repeat(AGENT_MESSAGING_NAME_MAX_CHARS))
    )
    .is_ok());
    for fine in ["a-b", "a_b", "a.b", "9lives"] {
        assert!(try_hire(&mut org, &handle(fine)).is_ok(), "{fine}");
    }
    assert_eq!(
        code(try_hire(&mut org, &handle("a:b"))),
        ErrorCode::InvalidName
    );
}

#[test]
fn an_agents_display_name_is_a_name_not_a_handle() {
    let mut org = Org::new();
    // The display name may look like a member id or a run address: only the handle is addressed.
    org.hire_with(&mid("AG1"), "run:1", "office1", profile("pixel"))
        .unwrap();
    org.hire_with(
        &mid("AG2"),
        "ABCDEFGHIJKLMNOPQRSTUVWXYZ",
        "office1",
        profile("other"),
    )
    .unwrap();
    org.edit_agent(&mid("AG1"), |p| p.name = Some("run:2".into()))
        .unwrap();
    org.edit_agent(&mid("AG2"), |p| {
        p.name = Some("ZYXWVUTSRQPONMLKJIHGFEDCBA".into())
    })
    .unwrap();
}

#[test]
fn a_runtime_spelled_as_other_is_refused_for_every_named_runtime() {
    let mut org = Org::new();
    for spelled in [
        "Antigravity",
        "ANTIGRAVITY",
        "claude_code",
        "Claude-Code",
        "codex",
    ] {
        let mut p = profile("pixel");
        p.runtime = AgentRuntime::Other(spelled.into());
        assert_eq!(
            code(org.hire_with(&mid("AG1"), "Pixel", "office1", p)),
            ErrorCode::InvalidRequest,
            "{spelled}"
        );
    }
}

#[test]
fn clearing_a_field_alone_is_an_edit_and_removes_it() {
    let mut org = Org::new();
    let id = mid("AG1");
    let mut p = profile("pixel");
    p.focus = Some("f".into());
    p.model = Some("m".into());
    p.home = Some("/h".into());
    org.hire_with(&id, "Pixel", "office1", p).unwrap();

    // Each clear flag alone is a call that gives a field.
    let changes = org.edit_agent(&id, |p| p.clear_model = true).unwrap();
    assert_eq!(agent_of(&changes, &id).model, None);
    let changes = org.edit_agent(&id, |p| p.clear_home = true).unwrap();
    assert_eq!(agent_of(&changes, &id).home, None);
    let changes = org.edit_agent(&id, |p| p.clear_focus = true).unwrap();
    assert_eq!(agent_of(&changes, &id).focus, None);
    assert_eq!(org.state.members[&id].version, 4);
}

#[test]
fn removing_a_member_outright_re_points_everyone_who_reported_to_it_even_offboarded_agents() {
    let mut org = Org::new();
    let (boss, mid_manager, old) = (mid("BOSS"), mid("MID"), mid("OLDA"));
    org.person(&boss, "Bea").unwrap();
    org.person(&mid_manager, "Max").unwrap();
    org.reports(&mid_manager, Some(&boss)).unwrap();
    org.hire(&old, "old").unwrap();
    org.reports(&old, Some(&mid_manager)).unwrap();
    org.offboard(&old).unwrap();

    org.remove(&mid_manager).unwrap();

    // member.remove is a hard delete: nobody may point at the deleted member.
    assert_eq!(
        org.state.members[&old].reports_to_member_id.as_deref(),
        Some(boss.as_str())
    );
}

#[test]
fn an_agents_office_id_is_checked_for_its_shape_before_it_is_looked_up() {
    let mut org = Org::new();
    for bad in ["", "has space", "bad!", &"o".repeat(ID_MAX_CHARS + 1)] {
        assert_eq!(
            code(org.hire_with(&mid("AG1"), "Pixel", bad, profile("pixel"))),
            ErrorCode::InvalidRequest,
            "{bad:?}"
        );
    }
    // A well-formed office that is not there is not_found.
    assert_eq!(
        code(org.hire_with(&mid("AG1"), "Pixel", "nowhere", profile("pixel"))),
        ErrorCode::NotFound
    );
}

#[test]
fn an_agent_edit_tells_a_bad_value_before_a_stale_version() {
    let mut org = Org::new();
    let id = mid("AG1");
    org.hire(&id, "pixel").unwrap();

    // invalid_name, invalid_request: each comes before `conflict`.
    assert_eq!(
        code(org.edit_agent(&id, |p| {
            p.name = Some(" ".into());
            p.expected_version = 99;
        })),
        ErrorCode::InvalidName
    );
    assert_eq!(
        code(org.edit_agent(&id, |p| {
            p.messaging_name = Some("-bad".into());
            p.expected_version = 99;
        })),
        ErrorCode::InvalidName
    );
    let command_id = org.id();
    assert_eq!(
        code(org.run(OrgCommand::MemberSetAgentStatus(
            MemberSetAgentStatusParams {
                command_id,
                member_id: id.clone(),
                expected_version: 99,
                status: AgentStatus::Offboarded,
            }
        ))),
        ErrorCode::InvalidRequest
    );
    let command_id = org.id();
    assert_eq!(
        code(
            org.run(OrgCommand::MemberSetRunMode(MemberSetRunModeParams {
                command_id,
                member_id: id.clone(),
                expected_version: 99,
                run_mode: Some(RunMode::Scheduled),
                schedule: None,
            }))
        ),
        ErrorCode::InvalidRequest
    );
    // And a good value with a stale version is the conflict.
    assert_eq!(
        code(org.edit_agent(&id, |p| {
            p.role = Some("x".into());
            p.expected_version = 99;
        })),
        ErrorCode::Conflict
    );
    assert_eq!(org.state.members[&id].version, 1);
}
