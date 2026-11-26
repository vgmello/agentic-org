use org_proto::org::{AgentProfile, AgentRuntime, Position};

use super::*;
use org_proto::org::RunMode;

const NOW: &str = "2026-11-08T12:00:00.000Z";
const OFFICE: ThisOffice<'static> = ThisOffice {
    id: "office1",
    name: "HQ",
};

fn id(n: &str) -> String {
    format!("{n:0<26}")
}

fn person(n: &str, name: &str) -> Member {
    Member {
        id: id(n),
        kind: MemberKind::Person,
        name: name.into(),
        handle: None,
        home_team_id: None,
        role_id: None,
        title_in_team: None,
        contact: None,
        owner: false,
        office_id: None,
        agent: None,
        reports_to_member_id: None,
        created_at: None,
        version: 1,
    }
}

fn owner() -> Member {
    Member {
        owner: true,
        ..person("OWNER", "Vinicius")
    }
}

fn agent(n: &str, name: &str, handle: &str, status: AgentStatus) -> Member {
    Member {
        kind: MemberKind::Agent,
        office_id: Some("office1".into()),
        agent: Some(AgentProfile {
            role: "engineer".into(),
            title: "Senior Engineer".into(),
            focus: None,
            runtime: AgentRuntime::ClaudeCode,
            model: None,
            run_mode: None,
            schedule: None,
            home: None,
            messaging_name: handle.into(),
            status,
            offboarded_at: None,
        }),
        ..person(n, name)
    }
}

fn team(tid: &str, name: &str, owner_team: Option<&str>) -> Team {
    Team {
        id: tid.into(),
        name: name.into(),
        owner_team_id: owner_team.map(Into::into),
        lead_member_id: None,
        version: 1,
    }
}

/// An owner, a person (Ana), two agents (Pixel active, Gone offboarded), two teams.
fn org() -> OrgSnapshot {
    let mut pixel = agent("PIXEL", "Pixel the designer", "pixel", AgentStatus::Active);
    pixel.home_team_id = Some("t-design".into());
    pixel.title_in_team = Some("Lead designer".into());
    pixel.reports_to_member_id = Some(id("ANA"));
    let mut ana = person("ANA", "Ana");
    ana.home_team_id = Some("t-eng".into());
    ana.title_in_team = Some("Manager".into());
    ana.reports_to_member_id = None;
    OrgSnapshot {
        position: Position { epoch: 1, seq: 9 },
        teams: vec![
            team("t-design", "Design", None),
            team("t-eng", "Engineering", None),
            team("t-eng-sub", "Design", Some("t-eng")),
        ],
        members: vec![
            owner(),
            ana,
            pixel,
            agent("GONE", "Gone", "gone", AgentStatus::Offboarded),
        ],
        projects: vec![],
        project_teams: vec![],
    }
}

fn ask(params: WhoParams) -> Result<Vec<String>, ErrorCode> {
    who(&org(), OFFICE, NOW, &NothingLive, &params)
        .map(|r| r.members.into_iter().map(|c| c.id).collect())
        .map_err(|e| e.code)
}

fn all() -> Vec<DirectoryCard> {
    who(&org(), OFFICE, NOW, &NothingLive, &WhoParams::default())
        .unwrap()
        .members
}

#[test]
fn everyone_but_the_offboarded_is_listed_by_name_then_id() {
    let names: Vec<_> = all().into_iter().map(|c| c.name).collect();
    assert_eq!(names, ["Ana", "Pixel the designer", "Vinicius"]);
}

#[test]
fn a_card_has_the_handle_kind_title_teams_manager_and_office() {
    let cards = all();
    let pixel = cards.iter().find(|c| c.id == id("PIXEL")).unwrap();
    assert_eq!(pixel.handle.as_deref(), Some("pixel"));
    assert_eq!(pixel.kind, MemberKind::Agent);
    assert_eq!(pixel.agent_status, Some(AgentStatus::Active));
    assert_eq!(pixel.role_title.as_deref(), Some("Senior Engineer"));
    assert!(!pixel.owner);
    // Home team with its title.
    let teams: Vec<_> = pixel
        .teams
        .iter()
        .map(|t| (t.name.as_str(), t.title_in_team.as_deref()))
        .collect();
    assert_eq!(teams, [("Design", Some("Lead designer"))]);
    assert_eq!(
        pixel.reports_to.as_ref().unwrap().handle.as_deref(),
        Some("Ana")
    );
    assert_eq!(pixel.reports_to.as_ref().unwrap().name, "Ana");
    assert_eq!(
        pixel.office,
        Some(OfficeRef {
            id: "office1".into(),
            name: "HQ".into()
        })
    );
    assert_eq!(pixel.presence, MemberPresence::Available);
}

#[test]
fn the_owners_handle_is_owner_and_a_person_has_no_title_agent_status_or_office() {
    let cards = all();
    let owner = cards.iter().find(|c| c.owner).unwrap();
    assert_eq!(owner.handle.as_deref(), Some("owner"));
    assert_eq!(owner.name, "Vinicius");
    assert_eq!(owner.kind, MemberKind::Person);
    assert!(owner.teams.is_empty() && owner.reports_to.is_none());
    let ana = cards.iter().find(|c| c.id == id("ANA")).unwrap();
    assert_eq!(ana.handle.as_deref(), Some("Ana"));
    assert_eq!(
        (&ana.agent_status, &ana.role_title, &ana.office),
        (&None, &None, &None)
    );
    assert_eq!(ana.teams.len(), 1);
    assert_eq!(ana.teams[0].title_in_team.as_deref(), Some("Manager"));
}

#[test]
fn a_member_is_named_by_owner_then_id_then_handle_ignoring_case() {
    let pixel = Ok(vec![id("PIXEL")]);
    let member = |m: &str| {
        ask(WhoParams {
            member: Some(m.into()),
            ..WhoParams::default()
        })
    };
    assert_eq!(member("owner"), Ok(vec![id("OWNER")]));
    assert_eq!(member("OWNER"), Ok(vec![id("OWNER")]));
    assert_eq!(member(&id("PIXEL")), pixel);
    assert_eq!(member("pixel"), pixel);
    assert_eq!(member("PiXeL"), pixel);
    assert_eq!(member("Ana"), Ok(vec![id("ANA")]));
    assert_eq!(member("ana"), Ok(vec![id("ANA")]));
    // A display name is not a handle, and neither is the owner's name.
    assert_eq!(member("Pixel the designer"), Err(ErrorCode::NotFound));
    assert_eq!(member("Vinicius"), Err(ErrorCode::NotFound));
    assert_eq!(member("nobody"), Err(ErrorCode::NotFound));
    // An offboarded agent is not in the directory.
    assert_eq!(member("gone"), Err(ErrorCode::NotFound));
    assert_eq!(member(&id("GONE")), Err(ErrorCode::NotFound));
}

#[test]
fn an_id_beats_a_handle_and_the_office_is_never_a_member() {
    // A person whose NAME equals another member's id cannot capture the lookup by id.
    let mut snapshot = org();
    snapshot
        .members
        .push(person("TRICK", &id("ANA").to_lowercase()));
    assert_eq!(
        resolve_member(&snapshot, &id("ANA"))
            .unwrap()
            .map(|m| m.id.clone()),
        Some(id("ANA"))
    );
    assert!(resolve_member(&snapshot, "office").unwrap().is_none());
    assert!(resolve_member(&snapshot, "OFFICE").unwrap().is_none());
    assert_eq!(
        ask(WhoParams {
            member: Some("Office".into()),
            ..WhoParams::default()
        }),
        Err(ErrorCode::InvalidRequest)
    );
}

#[test]
fn resolve_returns_an_offboarded_agent_as_stored_because_its_handle_stays_reserved() {
    let snapshot = org();
    let gone = resolve_member(&snapshot, "gone").unwrap().unwrap();
    assert!(is_offboarded(gone));
}

#[test]
fn team_by_id_or_by_name_and_kind_filters_combine() {
    let team = |t: &str| {
        ask(WhoParams {
            team: Some(t.into()),
            ..WhoParams::default()
        })
    };
    // By id, and by name ignoring case.
    assert_eq!(team("t-eng"), Ok(vec![id("ANA")]));
    assert_eq!(team("engineering"), Ok(vec![id("ANA")]));
    assert_eq!(team("t-design"), Ok(vec![id("PIXEL")]));
    // "Design" names two teams (one under Engineering): ambiguous, the id works.
    assert_eq!(team("Design"), Err(ErrorCode::InvalidRequest));
    assert_eq!(team("t-eng-sub"), Ok(vec![]));
    assert_eq!(team("Nope"), Err(ErrorCode::NotFound));
    // Kind: agents only; people include the owner.
    let kind = |k| {
        ask(WhoParams {
            kind: Some(k),
            ..WhoParams::default()
        })
    };
    assert_eq!(kind(MemberKind::Agent), Ok(vec![id("PIXEL")]));
    assert_eq!(kind(MemberKind::Person), Ok(vec![id("ANA"), id("OWNER")]));
    assert_eq!(kind(MemberKind::Unknown), Err(ErrorCode::InvalidRequest));
    // Filters narrow each other; a member outside the team is an empty list, not an error.
    let both = |m: &str, t: &str| {
        ask(WhoParams {
            member: Some(m.into()),
            team: Some(t.into()),
            kind: None,
        })
    };
    assert_eq!(both("pixel", "t-design"), Ok(vec![id("PIXEL")]));
    assert_eq!(both("ana", "t-design"), Ok(vec![]));
    assert_eq!(
        ask(WhoParams {
            member: Some("pixel".into()),
            team: Some("t-eng".into()),
            kind: Some(MemberKind::Person),
        }),
        Ok(vec![])
    );
}

#[test]
fn the_errors_come_in_the_documented_order() {
    let long = "x".repeat(MAX_HANDLE_BYTES + 1);
    let at_limit = "x".repeat(MAX_HANDLE_BYTES);
    let params = |member: Option<&str>, team: Option<&str>, kind| WhoParams {
        member: member.map(Into::into),
        team: team.map(Into::into),
        kind,
    };
    // too_large first, whatever else is wrong.
    assert_eq!(
        ask(params(Some(&long), Some("Nope"), Some(MemberKind::Unknown))),
        Err(ErrorCode::TooLarge)
    );
    assert_eq!(
        ask(params(None, Some(&long), None)),
        Err(ErrorCode::TooLarge)
    );
    assert_eq!(
        ask(params(Some(&at_limit), None, None)),
        Err(ErrorCode::NotFound)
    );
    // then invalid_request (kind, office) before not_found.
    assert_eq!(
        ask(params(Some("nobody"), None, Some(MemberKind::Unknown))),
        Err(ErrorCode::InvalidRequest)
    );
    assert_eq!(
        ask(params(Some("office"), Some("Nope"), None)),
        Err(ErrorCode::InvalidRequest)
    );
    // then the member before the team.
    assert_eq!(
        ask(params(Some("nobody"), Some("Design"), None)),
        Err(ErrorCode::NotFound)
    );
    assert_eq!(
        ask(params(Some("pixel"), Some("Design"), None)),
        Err(ErrorCode::InvalidRequest)
    );
}

struct Live(Vec<(String, MemberRuntime)>);
impl RuntimeView for Live {
    fn runtime_of(&self, member_id: &str) -> Option<MemberRuntime> {
        self.0
            .iter()
            .find(|(id, _)| id == member_id)
            .map(|(_, r)| r.clone())
    }
}

fn runtime(presence: MemberPresence, status: Option<(&str, &str)>) -> MemberRuntime {
    MemberRuntime {
        presence,
        last_seen_at: Some("2026-11-08T11:59:00.000Z".into()),
        status: status.map(|(text, _)| text.to_owned()),
        status_expires_at: status.map(|(_, at)| at.to_owned()),
        current_task_id: Some("task-1".into()),
    }
}

#[test]
fn presence_and_status_come_from_the_runtime_and_a_lapsed_status_is_absent() {
    let live = Live(vec![
        (
            id("PIXEL"),
            runtime(
                MemberPresence::Busy,
                Some(("Drafting the ADR", "2026-11-08T13:00:00.000Z")),
            ),
        ),
        (
            id("ANA"),
            runtime(
                MemberPresence::Online,
                Some(("back soon", "2026-11-08T11:00:00.000Z")), // lapsed an hour ago
            ),
        ),
    ]);
    let cards = who(&org(), OFFICE, NOW, &live, &WhoParams::default())
        .unwrap()
        .members;
    let pixel = cards.iter().find(|c| c.id == id("PIXEL")).unwrap();
    assert_eq!(pixel.presence, MemberPresence::Busy);
    assert_eq!(pixel.status.as_deref(), Some("Drafting the ADR"));
    assert_eq!(
        pixel.status_expires_at.as_deref(),
        Some("2026-11-08T13:00:00.000Z")
    );
    assert_eq!(pixel.current_task_id.as_deref(), Some("task-1"));
    assert_eq!(
        pixel.last_seen_at.as_deref(),
        Some("2026-11-08T11:59:00.000Z")
    );
    let ana = cards.iter().find(|c| c.id == id("ANA")).unwrap();
    assert_eq!(ana.presence, MemberPresence::Online);
    assert_eq!((&ana.status, &ana.status_expires_at), (&None, &None));
    // Exactly at the expiry the line has lapsed.
    let at_expiry = Live(vec![(
        id("PIXEL"),
        runtime(MemberPresence::Idle, Some(("x", NOW))),
    )]);
    let cards = who(&org(), OFFICE, NOW, &at_expiry, &WhoParams::default())
        .unwrap()
        .members;
    assert_eq!(
        cards.iter().find(|c| c.id == id("PIXEL")).unwrap().status,
        None
    );
}

#[test]
fn active_agents_without_a_live_run_are_available_and_paused_agents_are_paused() {
    let mut snapshot = org();
    let presence_with = |snapshot: &mut OrgSnapshot, mode: Option<RunMode>| {
        snapshot
            .members
            .iter_mut()
            .find(|m| m.id == id("PIXEL"))
            .unwrap()
            .agent
            .as_mut()
            .unwrap()
            .run_mode = mode;
        let cards = who(snapshot, OFFICE, NOW, &NothingLive, &WhoParams::default())
            .unwrap()
            .members;
        let presence = |n: &str| cards.iter().find(|c| c.id == id(n)).unwrap().presence;
        (presence("PIXEL"), presence("ANA"), presence("OWNER"))
    };
    // ADR-0028 (AVAIL-4): active agents with no live run show Available regardless of run_mode.
    for mode in [
        Some(RunMode::OnCall),
        Some(RunMode::Scheduled),
        Some(RunMode::AlwaysOn),
        None,
    ] {
        assert_eq!(
            presence_with(&mut snapshot, mode),
            (
                MemberPresence::Available,
                MemberPresence::Offline,
                MemberPresence::Offline
            ),
            "{mode:?}"
        );
    }
    snapshot
        .members
        .iter_mut()
        .find(|m| m.id == id("PIXEL"))
        .unwrap()
        .agent
        .as_mut()
        .unwrap()
        .status = AgentStatus::Paused;
    assert_eq!(presence_with(&mut snapshot, None).0, MemberPresence::Paused);
}

#[test]
fn an_agent_in_another_office_shows_its_office_id_and_a_record_without_a_profile_has_no_handle() {
    let mut snapshot = org();
    for member in &mut snapshot.members {
        if member.id == id("PIXEL") {
            member.office_id = Some("elsewhere".into());
        }
    }
    snapshot.members.push(Member {
        kind: MemberKind::Agent,
        ..person("BARE", "Bare record")
    });
    let cards = who(&snapshot, OFFICE, NOW, &NothingLive, &WhoParams::default())
        .unwrap()
        .members;
    let pixel = cards.iter().find(|c| c.id == id("PIXEL")).unwrap();
    assert_eq!(pixel.office.as_ref().unwrap().name, "elsewhere");
    let bare = cards.iter().find(|c| c.id == id("BARE")).unwrap();
    assert_eq!(bare.handle, None);
    assert_eq!(bare.agent_status, Some(AgentStatus::Unknown));
    assert_eq!(bare.role_title, None);
}

// ---- the review's cases -----------------------------------------------------------------

#[test]
fn cards_are_ordered_by_name_ignoring_case_then_by_id() {
    let mut snapshot = org();
    // Pushed so that the order they are stored in is NOT the order they must come out in.
    snapshot.members.push(person("AAA", "ana")); // "ana" == "Ana" ignoring case; AAA < ANA
    snapshot.members.push(person("ZED", "Bob"));
    snapshot.members.push(person("BOB", "bob")); // BOB < ZED
    let cards = who(&snapshot, OFFICE, NOW, &NothingLive, &WhoParams::default())
        .unwrap()
        .members;
    let order: Vec<_> = cards
        .iter()
        .map(|c| (c.name.as_str(), c.id.as_str()))
        .collect();
    assert_eq!(
        order,
        [
            ("ana", id("AAA").as_str()), // the same name as Ana: the id breaks the tie
            ("Ana", id("ANA").as_str()),
            ("bob", id("BOB").as_str()),
            ("Bob", id("ZED").as_str()),
            ("Pixel the designer", id("PIXEL").as_str()),
            ("Vinicius", id("OWNER").as_str()),
        ]
    );
}

#[test]
fn a_cards_home_team_carries_its_name_and_title() {
    let snapshot = org();
    let cards = who(&snapshot, OFFICE, NOW, &NothingLive, &WhoParams::default())
        .unwrap()
        .members;
    let pixel = cards.iter().find(|c| c.id == id("PIXEL")).unwrap();
    let names: Vec<_> = pixel.teams.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["Design"]);
}

#[test]
fn handles_compare_ignoring_case_for_any_script_and_keep_the_case_they_were_given() {
    let mut snapshot = org();
    snapshot
        .members
        .push(agent("REMY", "Remy", "Remy", AgentStatus::Active));
    snapshot.members.push(person("EVA", "Éva"));
    let found = |name: &str| {
        resolve_member(&snapshot, name)
            .unwrap()
            .map(|m| m.id.clone())
    };
    for name in ["remy", "REMY", "Remy", "rEmY"] {
        assert_eq!(found(name), Some(id("REMY")), "{name}");
    }
    for name in ["Éva", "éva", "ÉVA", "éVA"] {
        assert_eq!(found(name), Some(id("EVA")), "{name}");
    }
    let cards = who(&snapshot, OFFICE, NOW, &NothingLive, &WhoParams::default())
        .unwrap()
        .members;
    let remy = cards.iter().find(|c| c.id == id("REMY")).unwrap();
    assert_eq!(remy.handle.as_deref(), Some("Remy")); // as given, not folded
}

#[test]
fn the_word_owner_names_the_owner_even_when_a_member_has_that_id_and_office_names_no_one() {
    let mut snapshot = org();
    let mut sneaky = person("X", "Sneaky");
    sneaky.id = "owner".into(); // not a valid member id, but a stored record might be
    snapshot.members.push(sneaky);
    // A person named "Office" and an agent whose handle is "office" (HQ refuses both; a
    // damaged organization could hold them): neither is found by the reserved word.
    snapshot.members.push(person("OFF", "Office"));
    snapshot
        .members
        .push(agent("OFFA", "Office agent", "office", AgentStatus::Active));
    assert_eq!(
        resolve_member(&snapshot, "owner")
            .unwrap()
            .map(|m| m.id.clone()),
        Some(id("OWNER"))
    );
    for word in ["office", "OFFICE", "Office"] {
        assert!(resolve_member(&snapshot, word).unwrap().is_none(), "{word}");
    }
}

#[test]
fn the_input_is_trimmed_like_the_handles_it_is_compared_with() {
    let snapshot = org();
    let found = |name: &str| {
        resolve_member(&snapshot, name)
            .unwrap()
            .map(|m| m.id.clone())
    };
    assert_eq!(found(" pixel "), Some(id("PIXEL")));
    assert_eq!(found("\t\npixel\r\n"), Some(id("PIXEL")));
    // Only ASCII whitespace is trimmed: a no-break space or an ideographic space is text.
    assert_eq!(found("\u{a0}pixel"), None);
    assert_eq!(found("pixel\u{3000}"), None);
    assert_eq!(found(" owner "), Some(id("OWNER")));
    assert_eq!(found(&format!(" {} ", id("ANA"))), Some(id("ANA")));
    assert!(resolve_member(&snapshot, " office ").unwrap().is_none());
    assert_eq!(
        ask(WhoParams {
            member: Some(" office ".into()),
            ..WhoParams::default()
        }),
        Err(ErrorCode::InvalidRequest)
    );
}

#[test]
fn two_owners_fail_closed_instead_of_picking_one() {
    let mut snapshot = org();
    snapshot.members.push(Member {
        id: id("OWNER2"),
        ..owner()
    });
    assert_eq!(resolve_member(&snapshot, "owner"), Err(AmbiguousHandle));
}

#[test]
fn two_members_with_one_handle_fail_closed_instead_of_picking_one() {
    let mut snapshot = org();
    snapshot
        .members
        .push(agent("TWIN", "Pixel twin", "PIXEL", AgentStatus::Active));
    assert_eq!(resolve_member(&snapshot, "pixel"), Err(AmbiguousHandle));
    // Their ids still name them, and `who` says it cannot answer rather than guess.
    assert!(resolve_member(&snapshot, &id("TWIN")).unwrap().is_some());
    let got = who(
        &snapshot,
        OFFICE,
        NOW,
        &NothingLive,
        &WhoParams {
            member: Some("pixel".into()),
            ..WhoParams::default()
        },
    );
    assert_eq!(got.unwrap_err().code, ErrorCode::StorageError);
}

#[test]
fn a_home_team_that_is_gone_is_left_off_the_card() {
    let mut snapshot = org();
    snapshot
        .members
        .iter_mut()
        .find(|m| m.id == id("ANA"))
        .unwrap()
        .home_team_id = Some("t-gone".into());
    let cards = who(&snapshot, OFFICE, NOW, &NothingLive, &WhoParams::default())
        .unwrap()
        .members;
    let ana = cards.iter().find(|c| c.id == id("ANA")).unwrap();
    assert!(ana.teams.is_empty());
}

#[test]
fn a_manager_who_is_the_owner_is_no_manager() {
    let mut snapshot = org();
    snapshot
        .members
        .iter_mut()
        .find(|m| m.id == id("ANA"))
        .unwrap()
        .reports_to_member_id = Some(id("OWNER"));
    let cards = who(&snapshot, OFFICE, NOW, &NothingLive, &WhoParams::default())
        .unwrap()
        .members;
    assert!(cards
        .iter()
        .find(|c| c.id == id("ANA"))
        .unwrap()
        .reports_to
        .is_none());
}

#[test]
fn an_office_run_agent_that_reports_offline_still_reads_asleep() {
    let mut snapshot = org();
    snapshot
        .members
        .iter_mut()
        .find(|m| m.id == id("PIXEL"))
        .unwrap()
        .agent
        .as_mut()
        .unwrap()
        .run_mode = Some(RunMode::OnCall);
    // The run just ended: the runtime remembers when it was last seen, and active agents stay Available.
    let live = Live(vec![(id("PIXEL"), runtime(MemberPresence::Offline, None))]);
    let cards = who(&snapshot, OFFICE, NOW, &live, &WhoParams::default())
        .unwrap()
        .members;
    let pixel = cards.iter().find(|c| c.id == id("PIXEL")).unwrap();
    assert_eq!(pixel.presence, MemberPresence::Available);
    assert!(pixel.last_seen_at.is_some());
    // A live run reports Busy, and an agent without a run_mode is also Available when idle.
    let busy = Live(vec![(id("PIXEL"), runtime(MemberPresence::Busy, None))]);
    let cards = who(&snapshot, OFFICE, NOW, &busy, &WhoParams::default())
        .unwrap()
        .members;
    assert_eq!(
        cards.iter().find(|c| c.id == id("PIXEL")).unwrap().presence,
        MemberPresence::Busy
    );
    snapshot
        .members
        .iter_mut()
        .find(|m| m.id == id("PIXEL"))
        .unwrap()
        .agent
        .as_mut()
        .unwrap()
        .run_mode = None;
    let cards = who(&snapshot, OFFICE, NOW, &live, &WhoParams::default())
        .unwrap()
        .members;
    assert_eq!(
        cards.iter().find(|c| c.id == id("PIXEL")).unwrap().presence,
        MemberPresence::Available
    );
}

#[test]
fn a_status_lapses_by_the_instant_not_by_the_text_of_its_time() {
    let status_of = |expires: Option<&str>| {
        let live = Live(vec![(
            id("PIXEL"),
            MemberRuntime {
                presence: MemberPresence::Busy,
                last_seen_at: None,
                status: Some("working".into()),
                status_expires_at: expires.map(Into::into),
                current_task_id: None,
            },
        )]);
        let cards = who(&org(), OFFICE, NOW, &live, &WhoParams::default())
            .unwrap()
            .members;
        cards
            .into_iter()
            .find(|c| c.id == id("PIXEL"))
            .unwrap()
            .status
    };
    let live = Some("working".to_owned());
    // NOW is 12:00:00.000Z. The same instant in other spellings has lapsed...
    assert_eq!(status_of(Some("2026-11-08T12:00:00Z")), None);
    assert_eq!(status_of(Some("2026-11-08T14:00:00+02:00")), None);
    // ...and a later one in another spelling has not (13:00Z, written with an offset).
    assert_eq!(status_of(Some("2026-11-08T14:00:00.001+01:00")), live);
    assert_eq!(status_of(Some("2026-11-08T12:00:00.001Z")), live);
    // An expiry that cannot be read counts as lapsed; none at all never lapses.
    assert_eq!(status_of(Some("tomorrow")), None);
    assert_eq!(status_of(Some("")), None);
    assert_eq!(status_of(None), live);
}
