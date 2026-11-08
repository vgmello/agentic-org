use org_proto::org::*;
use org_proto::{ApiError, ErrorCode, Request};
use serde_json::json;

use super::memory::MemState;
use super::*;

const NOW: &str = "2026-11-08T07:00:00.000Z";
const OWNER: &str = "owner-1";

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
    org.person("ana", "Ana").unwrap();
    assert_eq!(code(org.lead("t1", Some("ana"))), ErrorCode::NotAMember);
    org.join("m1", "t1", "ana").unwrap();
    let led = team_of(&org.lead("t1", Some("ana")).unwrap());
    assert_eq!(
        (led.lead_member_id.as_deref(), led.version),
        (Some("ana"), 2)
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
    org.person("ana", "Ana").unwrap();
    org.join("m1", "t1", "ana").unwrap();
    org.join("m2", "t2", "ana").unwrap();
    assert_eq!(org.state.memberships_of_member("ana").len(), 2);
    assert_eq!(code(org.join("m3", "t1", "ana")), ErrorCode::AlreadyExists);
    assert_eq!(code(org.join("m1", "t2", "ana")), ErrorCode::AlreadyExists);
    assert_eq!(code(org.join("m9", "t1", OWNER)), ErrorCode::OwnerProtected);
    assert_eq!(code(org.join("m9", "nope", "ana")), ErrorCode::NotFound);
    assert_eq!(code(org.join("m9", "t1", "nobody")), ErrorCode::NotFound);
    // Adding and removing members does not change the team's version.
    assert_eq!(org.state.teams["t1"].version, 1);
}

#[test]
fn a_title_in_a_team_is_trimmed_and_limited() {
    let mut org = Org::new();
    org.team("t1", "Product", None).unwrap();
    org.person("ana", "Ana").unwrap();
    let command_id = org.id();
    let added = org
        .run(OrgCommand::TeamAddMember(TeamAddMemberParams {
            command_id,
            membership_id: "m1".into(),
            team_id: "t1".into(),
            member_id: "ana".into(),
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
    org.person("ana", "Ana").unwrap();
    org.person("bo", "Bo").unwrap();
    org.join("m1", "t1", "ana").unwrap();
    org.join("m2", "t1", "bo").unwrap();
    org.lead("t1", Some("ana")).unwrap();
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
    org.person("ana", "Ana").unwrap();
    org.join("m1", "b", "ana").unwrap();
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
    assert!(org.state.project_exists("p1") && org.state.members.contains_key("ana"));
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
    org.person("ana", "Ana").unwrap();
    assert_eq!(code(org.person("ana2", "ANA")), ErrorCode::AlreadyExists);
    assert_eq!(
        code(org.person("ana", "Somebody")),
        ErrorCode::AlreadyExists
    );
    // "owner" is reserved for the owner, in any capitalization.
    for reserved in ["owner", "Owner", "OWNER"] {
        assert_eq!(
            code(org.person("x", reserved)),
            ErrorCode::AlreadyExists,
            "{reserved}"
        );
    }
    assert_eq!(code(org.person("x", "  ")), ErrorCode::InvalidName);
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
    for word in ["owner", "Owner", "OWNER"] {
        assert_eq!(
            code(org.person("x", word)),
            ErrorCode::AlreadyExists,
            "{word}"
        );
    }
    // Another person cannot be renamed to it either.
    org.person("ana", "Ana").unwrap();
    let command_id = org.id();
    let rename = org.run(OrgCommand::MemberUpdatePerson(MemberUpdatePersonParams {
        command_id,
        member_id: "ana".into(),
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
            code(org.person("p", clash)),
            ErrorCode::AlreadyExists,
            "{clash}"
        );
    }
    org.person("ana", "Ana").unwrap();
    let rename = |org: &mut Org, to: &str| {
        let command_id = org.id();
        org.run(OrgCommand::MemberUpdatePerson(MemberUpdatePersonParams {
            command_id,
            member_id: "ana".into(),
            expected_version: org.state.members["ana"].version,
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
    org.person("p2", "Agent a1").unwrap();
}

#[test]
fn updating_a_person_keeps_ones_own_name_and_the_owner_may_rename_itself() {
    let mut org = Org::new();
    org.person("ana", "Ana").unwrap();
    org.person("bo", "Bo").unwrap();
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
            &update(&mut org, "ana", Some("ANA"), None, false).unwrap(),
            "ana"
        )
        .name,
        "ANA"
    );
    assert_eq!(
        code(update(&mut org, "ana", Some("bo"), None, false)),
        ErrorCode::AlreadyExists
    );
    // Contact: set, kept when not given, cleared.
    let m = member_of(
        &update(&mut org, "bo", None, Some(" bo@example.com "), false).unwrap(),
        "bo",
    );
    assert_eq!(m.contact.as_deref(), Some("bo@example.com"));
    let m = member_of(
        &update(&mut org, "bo", Some("Bo B"), None, false).unwrap(),
        "bo",
    );
    assert_eq!(
        (m.name.as_str(), m.contact.as_deref()),
        ("Bo B", Some("bo@example.com"))
    );
    assert_eq!(
        member_of(&update(&mut org, "bo", None, None, true).unwrap(), "bo").contact,
        None
    );
    assert_eq!(
        code(update(&mut org, "bo", None, Some("x"), true)),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        code(update(&mut org, "bo", None, Some("   "), false)),
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
        member_id: "ana".into(),
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
    for (id, name) in [("a", "A"), ("b", "B"), ("c", "C")] {
        org.person(id, name).unwrap();
    }
    org.reports("b", Some("a")).unwrap();
    org.reports("c", Some("b")).unwrap();
    assert_eq!(code(org.reports("a", Some("a"))), ErrorCode::Cycle);
    assert_eq!(code(org.reports("a", Some("c"))), ErrorCode::Cycle);
    assert_eq!(code(org.reports("a", Some("nobody"))), ErrorCode::NotFound);
    assert_eq!(
        code(org.reports(OWNER, Some("a"))),
        ErrorCode::OwnerProtected
    );
    assert_eq!(code(org.reports(OWNER, None)), ErrorCode::OwnerProtected);
    // Reporting to the owner is stored as reporting to no one: one form for each line.
    let m = member_of(&org.reports("c", Some(OWNER)).unwrap(), "c");
    assert_eq!(m.reports_to_member_id, None);
    assert_eq!(
        member_of(&org.reports("c", Some("a")).unwrap(), "c")
            .reports_to_member_id
            .as_deref(),
        Some("a")
    );
    assert_eq!(code(org.reports("nobody", None)), ErrorCode::NotFound);
}

#[test]
fn nobody_reports_to_or_from_an_offboarded_agent() {
    let mut org = Org::new();
    org.person("ana", "Ana").unwrap();
    org.agent("gone", "gone", AgentStatus::Offboarded);
    assert_eq!(
        code(org.reports("ana", Some("gone"))),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        code(org.reports("gone", Some("ana"))),
        ErrorCode::InvalidRequest
    );
}

// ---- removing a member ------------------------------------------------------------------

#[test]
fn removing_a_member_ends_its_relations_and_reports_go_to_its_manager() {
    let mut org = Org::new();
    org.team("t1", "Product", None).unwrap();
    org.team("t2", "Platform", None).unwrap();
    for (id, name) in [("boss", "Boss"), ("mid", "Mid"), ("low", "Low")] {
        org.person(id, name).unwrap();
    }
    org.reports("mid", Some("boss")).unwrap();
    org.reports("low", Some("mid")).unwrap();
    org.join("m1", "t1", "mid").unwrap();
    org.join("m2", "t2", "mid").unwrap();
    org.lead("t1", Some("mid")).unwrap();

    let before_team = org.state.teams["t1"].version;
    let before_low = org.state.members["low"].version;
    let changes = org.remove("mid").unwrap();

    // With a manager: the report now reports to the removed member's own manager.
    let low = member_of(&changes, "low");
    assert_eq!(
        (low.reports_to_member_id.as_deref(), low.version),
        (Some("boss"), before_low + 1)
    );
    // The led team lost its lead, with a new version; the other team is untouched.
    let t1 = team_of(&changes);
    assert_eq!((t1.lead_member_id, t1.version), (None, before_team + 1));
    assert_eq!(org.state.teams["t2"].version, 1);
    assert!(org.state.memberships.is_empty());
    assert!(
        matches!(changes.last(), Some(OrgChange::MemberRemoved { member_id }) if member_id == "mid")
    );
    assert!(!org.state.members.contains_key("mid"));
    // Its handle is free again.
    org.person("mid2", "Mid").unwrap();
}

#[test]
fn removing_a_member_with_no_manager_sends_its_reports_to_the_owner() {
    let mut org = Org::new();
    org.person("mid", "Mid").unwrap();
    org.person("low", "Low").unwrap();
    org.reports("low", Some("mid")).unwrap();
    let changes = org.remove("mid").unwrap();
    assert_eq!(member_of(&changes, "low").reports_to_member_id, None);
}

#[test]
fn the_owner_cannot_be_removed_and_other_removals_are_checked() {
    let mut org = Org::new();
    org.person("ana", "Ana").unwrap();
    org.agent("gone", "gone", AgentStatus::Offboarded);
    assert_eq!(code(org.remove(OWNER)), ErrorCode::OwnerProtected);
    assert_eq!(code(org.remove("nobody")), ErrorCode::NotFound);
    // An offboarded agent keeps its record and handle for good.
    assert_eq!(code(org.remove("gone")), ErrorCode::InvalidRequest);
    let command_id = org.id();
    let stale = org.run(OrgCommand::MemberRemove(MemberRemoveParams {
        command_id,
        member_id: "ana".into(),
        expected_version: 9,
    }));
    assert_eq!(code(stale), ErrorCode::Conflict);
    assert!(org.state.members.contains_key("ana"));
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
    }));
    assert_eq!(code(stale), ErrorCode::Conflict);
    let command_id = org.id();
    let removed = org
        .run(OrgCommand::ProjectRemoveTeam(ProjectRemoveTeamParams {
            command_id,
            project_team_id: "l1".into(),
            expected_version: 1,
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
    org.person("ana", "Ana").unwrap();
    org.person("bo", "Bo").unwrap();
    org.join("m1", "b", "ana").unwrap();
    org.join("m2", "b", "bo").unwrap();
    org.lead("b", Some("ana")).unwrap();
    org.reports("bo", Some("ana")).unwrap();
    org.remove("ana").unwrap();
    org.person("cy", "Cy").unwrap();
    let projects = org.state.projects.clone();

    let mut rebuilt = MemState::default();
    for change in org.log.iter().flatten() {
        apply(&mut rebuilt, change).unwrap();
    }
    rebuilt.projects = projects; // projects are not made by these commands yet
    assert_eq!(rebuilt, org.state);
}

#[test]
fn every_upsert_raises_the_version_by_exactly_one_and_failures_change_nothing() {
    let mut org = Org::new();
    org.team("t1", "Product", None).unwrap();
    org.person("ana", "Ana").unwrap();
    org.join("m1", "t1", "ana").unwrap();
    org.lead("t1", Some("ana")).unwrap();
    let snapshot = org.state.clone();

    // A batch of failing commands leaves the state exactly as it was.
    assert!(org.team("t1", "Dup", None).is_err());
    assert!(org.person("ana", "Ana").is_err());
    assert!(org.lead("t1", Some(OWNER)).is_err());
    assert!(org.remove(OWNER).is_err());
    assert_eq!(org.state, snapshot);

    // A cascade bumps each touched entity once, and only those.
    org.person("bo", "Bo").unwrap();
    org.reports("bo", Some("ana")).unwrap();
    let (team, bo) = (
        org.state.teams["t1"].version,
        org.state.members["bo"].version,
    );
    let changes = org.remove("ana").unwrap();
    assert_eq!(team_of(&changes).version, team + 1);
    assert_eq!(member_of(&changes, "bo").version, bo + 1);
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
    };
    assert!(OrgCommand::parse(&hello).unwrap().is_none());
    // A known method with params of the wrong shape.
    let bad = Request {
        id: 3,
        method: "team.create".into(),
        params: json!({"name": 5}),
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
    org.person("ann", "Ann").unwrap();
    org.person("bob", "Bob").unwrap();
    org.join("ms-ann", "t2", "ann").unwrap();
    org.join("ms-bob", "t2", "bob").unwrap();
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
    let taken = org.join("ms-ann", "t3", "bob");
    assert_eq!(code(taken.clone()), ErrorCode::AlreadyExists);
    assert!(message_of(taken).contains("id is taken"));
    // A taken pair under a fresh id: the pair is.
    let twice = org.join("ms-new", "t2", "ann");
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

    let taken = org.person("ann", "Anna");
    assert_eq!(code(taken.clone()), ErrorCode::AlreadyExists);
    assert!(message_of(taken).contains("member id is taken"));
    let taken = org.team("t2", "Elsewhere", None);
    assert!(message_of(taken).contains("team id is taken"));
}

#[test]
fn ids_made_by_the_client_are_checked_on_every_command_that_takes_one() {
    let mut org = staffed();
    assert_eq!(
        code(org.join("bad id", "t1", "ann")),
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
    org.lead("t2", Some("ann")).unwrap();
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
            lead_member_id: Some("bob".into()),
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
            member_id: "bob".into(),
            expected_version: stale,
            reports_to_member_id: Some("ann".into()),
        }),
        OrgCommand::MemberUpdatePerson(MemberUpdatePersonParams {
            command_id: "c".into(),
            member_id: "bob".into(),
            expected_version: stale,
            name: Some("Robert".into()),
            contact: None,
            clear_contact: false,
        }),
        OrgCommand::MemberRemove(MemberRemoveParams {
            command_id: "c".into(),
            member_id: "bob".into(),
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
            member_id: "ann".into(),
            expected_version: 1,
            name: None,
            contact: Some(" ann@example.com ".into()),
            clear_contact: false,
        }))
        .unwrap();
    let ann = member_of(&changes, "ann");
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
    org.person("cat", "Cat").unwrap();
    assert_eq!(code(team_set_lead(&mut org, "cat")), ErrorCode::NotAMember);
    assert_eq!(code(team_set_lead(&mut org, "ann")), ErrorCode::Conflict);

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

    org.reports("bob", Some("ann")).unwrap();
    let command_id = org.id();
    let looped = org.run(OrgCommand::MemberSetReportsTo(MemberSetReportsToParams {
        command_id,
        member_id: "ann".into(),
        expected_version: stale,
        reports_to_member_id: Some("bob".into()),
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
    org.reports("bob", Some("ag")).unwrap();
    assert_eq!(org.state.teams["t2"].lead_member_id.as_deref(), Some("ag"));
    // Its handle is taken while it is here, whatever its display name.
    assert_eq!(code(org.person("p2", "pixel")), ErrorCode::AlreadyExists);

    let changes = org.remove("ag").unwrap();
    assert!(changes.contains(&OrgChange::MembershipRemoved {
        membership_id: "ms-ag".into()
    }));
    assert_eq!(team_of(&changes).lead_member_id, None);
    assert_eq!(team_of(&changes).version, 3);
    assert_eq!(member_of(&changes, "bob").reports_to_member_id, None);
    assert!(!org.state.members.contains_key("ag"));
    // The handle is free again.
    org.person("p2", "pixel").unwrap();
}

#[test]
fn moving_a_member_ends_one_membership_and_starts_another_in_one_event() {
    let mut org = staffed();
    org.lead("t2", Some("ann")).unwrap();
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
                    member_id: "ann".into(),
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
    org.join("ms-ann-t1", "t1", "ann").unwrap();
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
    assert_eq!(
        code(owner_bootstrap("bad id", "Owner", NOW)),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        code(owner_bootstrap("o1", "  ", NOW)),
        ErrorCode::InvalidName
    );
    assert_eq!(
        code(owner_bootstrap("o1", "a\u{7}b", NOW)),
        ErrorCode::InvalidName
    );
    let changes = owner_bootstrap("o1", " Vi ", NOW).unwrap();
    assert_eq!(member_of(&changes, "o1").name, "Vi");
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
            member_id: "zed".into(),
            name: " Zed ".into(),
            contact: Some(" zed@example.com ".into()),
        }))
        .unwrap();
    let zed = member_of(&changes, "zed");
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
    org.person("a", "Ann").unwrap();
    org.person("b", "Bob").unwrap();
    org.person("c", "Cat").unwrap();
    for (who, boss) in [("a", "b"), ("b", "a")] {
        org.state.members.get_mut(who).unwrap().reports_to_member_id = Some(boss.into());
    }
    let reported = decide(
        &org.state,
        NOW,
        &OrgCommand::MemberSetReportsTo(MemberSetReportsToParams {
            command_id: "c".into(),
            member_id: "c".into(),
            expected_version: 1,
            reports_to_member_id: Some("a".into()),
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
    assert_eq!(code(update("ann", Some("  "))), ErrorCode::InvalidName);
    assert_eq!(code(update("ag", None)), ErrorCode::InvalidRequest);
    assert_eq!(code(update("nope", None)), ErrorCode::NotFound);
    assert_eq!(code(update("ann", Some("Anna"))), ErrorCode::Conflict);
}

#[test]
fn a_missing_thing_is_told_before_a_bad_id_and_a_bad_value_before_a_bad_name() {
    let mut org = staffed();
    // not_found before invalid_request.
    assert_eq!(code(org.join("bad id", "nope", "ann")), ErrorCode::NotFound);
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
        member_id: "zed".into(),
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
            member_id: "ann".into(),
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
    let changes = org.reports("bob", Some("ann")).unwrap();
    let bob = member_of(&changes, "bob");
    assert_eq!(
        (bob.version, bob.reports_to_member_id.as_deref()),
        (2, Some("ann"))
    );
}

#[test]
fn a_reporting_line_to_someone_missing_is_not_found_before_anything_else() {
    let mut org = staffed();
    org.agent("old", "Old", AgentStatus::Offboarded);
    let command_id = org.id();
    let result = org.run(OrgCommand::MemberSetReportsTo(MemberSetReportsToParams {
        command_id,
        member_id: "old".into(),
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
            member_id: "ann".into(),
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
    org.person("vi2", "Vi").unwrap();
    // But the word `owner` is still nobody else's.
    assert_eq!(
        code(rename(&mut org, "bob", "Owner")),
        ErrorCode::AlreadyExists
    );
    assert_eq!(code(org.person("p9", "owner")), ErrorCode::AlreadyExists);
    // And people still clash with each other.
    assert_eq!(code(org.person("p10", "vi")), ErrorCode::AlreadyExists);
}
