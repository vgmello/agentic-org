//! The structure changes after creation, on any engine: one test per rule of the data
//! contract (documents 01, 04 and 06) as the B2 plan maps them (section 4, R2-R21), with
//! the owner's decisions of 2026-11-19 (Q1 a team forest, Q2 and Q3 refusals naming what
//! to clear, Q4 no limit on people, Q5 archive only an empty team).

use super::*;

/// Teams 0x21 and 0x22 under the top-level team.
fn two_teams<E: OrganizationEngine, G>(open: &dyn Fn() -> (Arc<E>, G)) -> Bench<E, G> {
    let bench = Bench::created(open);
    bench.created_team(2, 0x21, TEAM);
    bench.created_team(3, 0x22, TEAM);
    bench
}

/// The owner creates `command`'s team, under operation `op`.
fn create<E: OrganizationEngine, G>(
    bench: &Bench<E, G>,
    op: u64,
    command: &CreateTeam,
) -> TeamCreated {
    let reply = bench.owner(op, command).expect("a decided creation");
    assert_eq!(reply.outcome, Outcome::Completed, "{:?}", rejection(&reply));
    serde_json::from_slice(&reply.bytes).expect("a team reply")
}

fn named(team: u64, parent: Option<u64>, name: &str) -> CreateTeam {
    CreateTeam {
        id: id(team),
        name: name.into(),
        parent: parent.map(id),
    }
}

fn rename(bench: &Bench<impl OrganizationEngine, impl Sized>, team: u64, name: &str) -> UpdateTeam {
    UpdateTeam {
        name: Some(name.into()),
        ..UpdateTeam::new(id(team), bench.team(team).meta.revision)
    }
}

fn lead(bench: &Bench<impl OrganizationEngine, impl Sized>, team: u64, lead: u64) -> UpdateTeam {
    UpdateTeam {
        lead: Some(id(lead)),
        ..UpdateTeam::new(id(team), bench.team(team).meta.revision)
    }
}

fn manager(
    bench: &Bench<impl OrganizationEngine, impl Sized>,
    person: u64,
    manager: u64,
) -> UpdatePerson {
    UpdatePerson {
        manager: Edit::Set(id(manager)),
        ..UpdatePerson::new(id(person), bench.employee(person).meta.revision)
    }
}

fn deactivation(
    bench: &Bench<impl OrganizationEngine, impl Sized>,
    person: u64,
) -> DeactivatePerson {
    DeactivatePerson {
        id: id(person),
        expected_revision: bench.employee(person).meta.revision,
    }
}

fn team_file(chain: &[u64]) -> String {
    let chain: Vec<Id> = chain.iter().map(|n| id(*n)).collect();
    records::team_path(&chain)
        .expect("a path")
        .as_str()
        .to_owned()
}

fn profile_file(chain: &[u64], kind: EmployeeKind, employee: u64) -> String {
    let chain: Vec<Id> = chain.iter().map(|n| id(*n)).collect();
    let path = records::employee_path(&chain, kind, &id(employee));
    path.expect("a path").as_str().to_owned()
}

/// R2: a team keeps its number through a move and an archive; numbers are never given
/// again, and the allocations stand.
pub fn archive_and_move_keep_the_number<E: OrganizationEngine, G>(open: &dyn Fn() -> (Arc<E>, G)) {
    let bench = two_teams(open);
    let allocations = bench.read(|snapshot| snapshot.team_allocations());
    bench.done(4, &bench.move_team(0x22, 0x21));
    assert_eq!(bench.team(0x22).number, 3);
    bench.done(5, &bench.archive(0x22));
    let archived = bench.team(0x22);
    assert_eq!(
        (archived.number, archived.status),
        (3, TeamStatus::Archived)
    );
    bench.read(|snapshot| {
        assert_eq!(snapshot.team_allocations()?, allocations);
        assert_eq!(snapshot.team_by_number(3)?.map(|t| t.id), Some(id(0x22)));
        Ok(())
    });
    assert_eq!(bench.created_team(6, 0x23, TEAM).number, 4);
}

/// R3: a team never moves under itself or a team below it.
pub fn moving_a_team_cannot_make_a_loop<E: OrganizationEngine, G>(open: &dyn Fn() -> (Arc<E>, G)) {
    let bench = Bench::created(open);
    bench.created_team(2, 0x21, TEAM);
    bench.created_team(3, 0x22, 0x21);
    bench.created_team(4, 0x23, 0x22);
    let files = bench.state_files();
    for (op, parent) in [(5, 0x23), (6, 0x22), (7, 0x21)] {
        let refused = bench.refused(op, &bench.move_team(0x21, parent));
        assert_eq!(refused.code, RejectCode::Cycle, "{parent:x}");
    }
    let refused = bench.refused(8, &bench.move_team(0x22, 0x23));
    assert_eq!(refused.code, RejectCode::Cycle);
    assert_eq!(text(&bench.state_files()), text(&files));
    // Under a sibling's subtree is fine.
    bench.created_team(9, 0x24, TEAM);
    bench.done(10, &bench.move_team(0x22, 0x24));
}

/// R3: the depth limit counts the whole moved subtree, and a team is only made where an
/// employee's profile still fits.
pub fn a_move_past_the_depth_limit_is_refused<E: OrganizationEngine, G>(
    open: &dyn Fn() -> (Arc<E>, G),
) {
    let bench = Bench::created(open);
    // The deepest chain a creation allows.
    let mut chain = vec![TEAM];
    let deepest = loop {
        let team = 0x30 + chain.len() as u64;
        let parent = *chain.last().expect("a parent");
        let reply = bench
            .create_team(100 + team, team, parent)
            .expect("decided");
        if reply.outcome == Outcome::Rejected {
            assert_eq!(rejection(&reply).code, RejectCode::InvalidRequest);
            break chain.len();
        }
        chain.push(team);
        assert!(chain.len() <= crate::organization::ports::MAX_TEAM_DEPTH);
    };
    assert!(deepest >= 8, "{deepest}");
    let bottom = *chain.last().expect("the deepest team");
    let above_bottom = chain[deepest - 2];
    // Every team takes a person and an agent's profile would fit as well.
    bench.add_person(200, 0x60, bottom);

    // A two-team subtree under the team above the bottom: one level too deep.
    bench.created_team(201, 0x50, TEAM);
    bench.created_team(202, 0x51, 0x50);
    let files = bench.state_files();
    let refused = bench.refused(203, &bench.move_team(0x50, above_bottom));
    assert_eq!(refused.code, RejectCode::InvalidRequest);
    assert_eq!(text(&bench.state_files()), text(&files));
    // Its leaf alone fits there.
    bench.done(204, &bench.move_team(0x51, above_bottom));
    assert_eq!(
        bench.read(|s| s.team_chain(&id(0x51))).map(|c| c.len()),
        Some(deepest)
    );
    // A team with a person in it moves to the deepest level: the profile fits there.
    bench.add_person(205, 0x61, 0x50);
    bench.done(206, &bench.move_team(0x50, above_bottom));
    let profile = bench.read(|s| s.team_chain(&id(0x50))).expect("a chain");
    let path = records::employee_path(&profile, EmployeeKind::Person, &id(0x61));
    assert!(bench
        .state_files()
        .contains_key(path.expect("a path").as_str()));
}

/// R4: a move rewrites the team, every team below it and every profile homed in its
/// subtree, in one entry, with nothing left at the old paths; a rebuild agrees.
pub fn a_subtree_move_rewrites_every_path_once<E: OrganizationEngine, G>(
    open: &dyn Fn() -> (Arc<E>, G),
) {
    let bench = Bench::created(open);
    bench.created_team(2, 0x21, TEAM);
    bench.created_team(3, 0x22, 0x21);
    bench.created_team(4, 0x23, TEAM);
    bench.add_person(5, 0x31, 0x21);
    bench.add_person(6, 0x32, 0x22);
    // An agent homed below the moved team (no command places one before C3).
    let mut agent = bench.employee(HR);
    agent.id = id(0x3b);
    agent.handle = "agent3b".into();
    agent.builtin_key = None;
    agent.home_team_id = id(0x22);
    let agent_path = profile_file(&[TEAM, 0x21, 0x22], EmployeeKind::Agent, 0x3b);
    let placed = Change::new(Op::CreateTeam.ack())
        .put(durable(&agent_path), records::encode_employee(&agent));
    assert_eq!(
        bench.write(placed).map(|r| r.outcome),
        Ok(Outcome::Completed)
    );
    let before = bench.state_files();
    let generation = bench.generation();
    let moved_b = bench.team(0x22);

    let reply = bench.done(7, &bench.move_team(0x21, 0x23));
    assert_eq!((reply.revision, reply.generation), (2, generation + 1));
    let after = bench.state_files();

    let old = [
        team_file(&[TEAM, 0x21]),
        team_file(&[TEAM, 0x21, 0x22]),
        profile_file(&[TEAM, 0x21], EmployeeKind::Person, 0x31),
        profile_file(&[TEAM, 0x21, 0x22], EmployeeKind::Person, 0x32),
        agent_path.clone(),
    ];
    let new = [
        team_file(&[TEAM, 0x23, 0x21]),
        team_file(&[TEAM, 0x23, 0x21, 0x22]),
        profile_file(&[TEAM, 0x23, 0x21], EmployeeKind::Person, 0x31),
        profile_file(&[TEAM, 0x23, 0x21, 0x22], EmployeeKind::Person, 0x32),
        profile_file(&[TEAM, 0x23, 0x21, 0x22], EmployeeKind::Agent, 0x3b),
    ];
    for path in &old {
        assert!(!after.contains_key(path), "{path} is still there");
    }
    let old_prefix = format!("state/teams/{}/teams/{}/", id(TEAM), id(0x21));
    assert!(!after.keys().any(|path| path.starts_with(&old_prefix)));
    // The moved team changed; everything below it is the same bytes at a new path.
    let a = records::decode_team(&after[&new[0]]).expect("the moved team");
    assert_eq!(a.parent_team_id, Some(id(0x23)));
    assert_eq!(a.meta.revision, 2);
    for (from, to) in old.iter().zip(&new).skip(1) {
        assert_eq!(after.get(to), before.get(from), "{to}");
    }
    assert_eq!(bench.team(0x22), moved_b);
    // Nothing else changed, and no file was added or lost.
    assert_eq!(after.len(), before.len());
    for (path, bytes) in &before {
        if !old.contains(path) {
            assert_eq!(after.get(path), Some(bytes), "{path}");
        }
    }
    bench.read(|snapshot| {
        let chain = snapshot.team_chain(&id(0x22))?;
        assert_eq!(chain, Some(vec![id(TEAM), id(0x23), id(0x21), id(0x22)]));
        Ok(())
    });

    let rows_before = rows(&bench);
    let bench = bench.rebuilt(open).expect("a rebuild after the move");
    assert!(
        rows(&bench) == rows_before,
        "{:?}\n{rows_before:?}",
        rows(&bench)
    );
    assert_eq!(text(&bench.state_files()), text(&after));
}

/// R5 and Q1: the organization is a forest; the owner makes more top-level teams, which
/// can move and be archived, but the creation's team never is.
pub fn the_first_team_stays_top_level<E: OrganizationEngine, G>(open: &dyn Fn() -> (Arc<E>, G)) {
    let bench = Bench::created(open);
    let top = create(&bench, 2, &named(0x30, None, "Second"));
    assert_eq!(top.number, 2);
    let path = team_file(&[0x30]);
    assert_eq!(path, format!("state/teams/{}/team.yaml", id(0x30)));
    assert!(bench.state_files().contains_key(&path));
    assert_eq!(bench.team(0x30).parent_team_id, None);
    // Top-level names clash ignoring case, with the creation's team ("Example") too.
    let clash = bench.refused(3, &named(0x31, None, "EXAMPLE"));
    assert_eq!(clash.code, RejectCode::AlreadyExists);

    let moved = bench.refused(4, &bench.move_team(TEAM, 0x30));
    assert_eq!(moved.code, RejectCode::InvalidRequest);
    let archived = bench.refused(5, &bench.archive(TEAM));
    assert_eq!(archived.code, RejectCode::InvalidRequest);
    assert_eq!(bench.team(TEAM).status, TeamStatus::Active);

    // Another top-level team moves under the first, and is archived.
    bench.done(6, &bench.move_team(0x30, TEAM));
    assert_eq!(bench.team(0x30).parent_team_id, Some(id(TEAM)));
    bench.done(7, &bench.archive(0x30));
}

/// R6: a team's name is one line of at most 200 characters, unique among the active
/// teams of one parent ignoring case; archived siblings and other parents do not clash.
pub fn sibling_team_names_clash_ignoring_case_but_not_across_parents<E: OrganizationEngine, G>(
    open: &dyn Fn() -> (Arc<E>, G),
) {
    let bench = Bench::created(open);
    create(&bench, 2, &named(0x21, Some(TEAM), "Core"));
    let code = |op: u64, command: &dyn Fn() -> CreateTeam| bench.refused(op, &command()).code;
    assert_eq!(
        code(3, &|| named(0x22, Some(TEAM), "core")),
        RejectCode::AlreadyExists
    );
    // Under another parent the same name is fine.
    create(&bench, 4, &named(0x22, Some(0x21), "core"));
    // Lines and lengths.
    let long = "x".repeat(201);
    for (op, name) in [(5, ""), (6, " Core"), (7, "two\nlines"), (8, long.as_str())] {
        let refused = bench.refused(op, &named(0x23, Some(TEAM), name));
        assert_eq!(refused.code, RejectCode::InvalidName, "{name:?}");
    }
    let longest = "y".repeat(200);
    create(&bench, 9, &named(0x23, Some(TEAM), &longest));
    // A rename into a clash, and a move into one.
    let renamed = bench.refused(10, &rename(&bench, 0x23, "CORE"));
    assert_eq!(renamed.code, RejectCode::AlreadyExists);
    let refused = bench.refused(11, &rename(&bench, 0x23, "a\tb"));
    assert_eq!(refused.code, RejectCode::InvalidName);
    let moved = bench.refused(12, &bench.move_team(0x22, TEAM));
    assert_eq!(moved.code, RejectCode::AlreadyExists);
    // A rename to its own name is an edit that still counts.
    let same = bench.done(13, &rename(&bench, 0x21, "Core"));
    assert_eq!(same.revision, 2);
    // An archived sibling keeps its name without blocking it.
    bench.done(14, &bench.move_team(0x22, 0x23));
    bench.done(15, &bench.archive(0x22));
    bench.done(16, &bench.archive(0x21));
    create(&bench, 17, &named(0x24, Some(TEAM), "core"));
}

/// R7: an archived team takes no new team, move or person, and is not changed again.
pub fn archived_teams_take_nothing_new<E: OrganizationEngine, G>(open: &dyn Fn() -> (Arc<E>, G)) {
    let bench = two_teams(open);
    bench.created_team(4, 0x23, TEAM);
    bench.add_person(5, 0x31, 0x22);
    bench.done(6, &bench.archive(0x21));
    let files = bench.state_files();
    let invalid = |op: u64, refused: OrgRejection| {
        assert_eq!(
            refused.code,
            RejectCode::InvalidRequest,
            "op {op}: {refused:?}"
        );
    };
    invalid(7, bench.refused(7, &team_command(0x24, 0x21)));
    invalid(8, bench.refused(8, &bench.move_team(0x23, 0x21)));
    invalid(9, bench.refused(9, &person_command(0x32, 0x21)));
    invalid(10, bench.refused(10, &bench.move_person(0x31, 0x21)));
    invalid(11, bench.refused(11, &rename(&bench, 0x21, "Renamed")));
    invalid(12, bench.refused(12, &bench.move_team(0x21, 0x23)));
    invalid(13, bench.refused(13, &bench.archive(0x21)));
    assert_eq!(text(&bench.state_files()), text(&files));
}

/// R7 and Q5: only an empty team is archived: no active child team and no employee who
/// is not inactive. Archived child teams and inactive employees stay with it.
pub fn archive_needs_an_empty_team<E: OrganizationEngine, G>(open: &dyn Fn() -> (Arc<E>, G)) {
    let bench = Bench::created(open);
    bench.created_team(2, 0x21, TEAM);
    bench.created_team(3, 0x22, 0x21);
    bench.add_person(4, 0x31, 0x21);
    let refused = bench.refused(5, &bench.archive(0x21));
    assert_eq!(refused.code, RejectCode::NotEmpty);
    assert!(
        refused.message.contains("1 active team and 1 employee:"),
        "{refused:?}"
    );
    bench.done(6, &bench.archive(0x22));
    let refused = bench.refused(7, &bench.archive(0x21));
    assert_eq!(refused.code, RejectCode::NotEmpty);
    bench.deactivate(8, 0x31);
    // An active child team alone keeps it too.
    bench.created_team(10, 0x23, 0x21);
    let refused = bench.refused(11, &bench.archive(0x21));
    assert_eq!(refused.code, RejectCode::NotEmpty);
    assert!(
        refused.message.contains("1 active team and 0 employees:"),
        "{refused:?}"
    );
    bench.done(12, &bench.archive(0x23));
    let archived = bench.done(9, &bench.archive(0x21));
    assert_eq!(archived.revision, 2);
    let team = bench.team(0x21);
    assert_eq!((team.status, team.number), (TeamStatus::Archived, 2));
    assert_eq!(bench.employee(0x31).home_team_id, id(0x21));
}

/// R8: a lead is a direct, active employee of the team (its home team is the team),
/// person or agent; the appointment is set, replaced and cleared by the owner.
pub fn a_teams_lead_is_a_direct_active_employee<E: OrganizationEngine, G>(
    open: &dyn Fn() -> (Arc<E>, G),
) {
    let bench = two_teams(open);
    bench.add_person(4, 0x31, 0x21);
    bench.add_person(5, 0x32, 0x21);
    bench.add_person(6, 0x33, 0x22);
    bench.deactivate(7, 0x32);
    let code = |op: u64, lead_of: u64| bench.refused(op, &lead(&bench, 0x21, lead_of)).code;
    // The owner's home is the top-level team; 0x33's is another team; 0x32 is inactive.
    assert_eq!(code(8, CREATOR), RejectCode::InvalidRequest);
    assert_eq!(code(9, 0x33), RejectCode::InvalidRequest);
    assert_eq!(code(10, 0x32), RejectCode::InvalidRequest);
    assert_eq!(code(11, 0x99), RejectCode::NotFound);
    assert_eq!(bench.team(0x21).lead_employee_id, None);

    let led = bench.done(12, &lead(&bench, 0x21, 0x31));
    assert_eq!(led.revision, 2);
    assert_eq!(bench.team(0x21).lead_employee_id, Some(id(0x31)));
    // An agent leads too: the HR lead's home is the top-level team.
    bench.done(13, &lead(&bench, TEAM, HR));
    assert_eq!(bench.team(TEAM).lead_employee_id, Some(id(HR)));
    // Keeping the lead: an update without one leaves it.
    bench.done(14, &rename(&bench, 0x21, "Kept"));
    assert_eq!(bench.team(0x21).lead_employee_id, Some(id(0x31)));
    bench.set_lead(15, 0x21, None);
    assert_eq!(bench.team(0x21).lead_employee_id, None);
    // Appointing and clearing at once is a malformed command.
    let both = UpdateTeam {
        lead: Some(id(0x31)),
        clear_lead: true,
        ..UpdateTeam::new(id(0x21), bench.team(0x21).meta.revision)
    };
    bench.unrecorded(16, &Actor::Employee(id(CREATOR)), &both);
}

/// R9 and Q3: a team's lead does not move to another team until the appointment is
/// cleared or another lead appointed; the refusal names the team.
pub fn moving_a_lead_is_refused_until_the_appointment_is_cleared<E: OrganizationEngine, G>(
    open: &dyn Fn() -> (Arc<E>, G),
) {
    let bench = two_teams(open);
    bench.add_person(4, 0x31, 0x21);
    bench.add_person(5, 0x32, 0x21);
    bench.set_lead(6, 0x21, Some(0x31));
    let files = bench.state_files();
    let refused = bench.refused(7, &bench.move_person(0x31, 0x22));
    assert_eq!(refused.code, RejectCode::InvalidRequest);
    assert!(refused.message.contains("TEAM-2"), "{refused:?}");
    // The owner leads the top-level team: the same.
    let owner = bench.refused(8, &bench.move_person(CREATOR, 0x22));
    assert!(owner.message.contains("TEAM-1"), "{owner:?}");
    assert_eq!(text(&bench.state_files()), text(&files));

    // Another lead appointed: the former one moves.
    bench.set_lead(9, 0x21, Some(0x32));
    bench.done(10, &bench.move_person(0x31, 0x22));
    assert_eq!(bench.employee(0x31).home_team_id, id(0x22));
    // Cleared: the lead moves.
    bench.set_lead(11, 0x21, None);
    bench.done(12, &bench.move_person(0x32, 0x22));
}

/// R10: an employee has one home team, which exists and is active, for an addition and
/// for a move; the profile lives in that team's folder.
pub fn a_person_needs_an_active_home_team<E: OrganizationEngine, G>(
    open: &dyn Fn() -> (Arc<E>, G),
) {
    let bench = two_teams(open);
    assert_eq!(
        bench.refused(4, &person_command(0x31, 0x99)).code,
        RejectCode::NotFound
    );
    bench.add_person(5, 0x31, 0x21);
    let path = profile_file(&[TEAM, 0x21], EmployeeKind::Person, 0x31);
    assert!(bench.state_files().contains_key(&path));
    assert_eq!(
        bench.refused(6, &bench.move_person(0x31, 0x99)).code,
        RejectCode::NotFound
    );
    assert_eq!(
        bench.refused(7, &bench.move_person(0x31, 0x21)).code,
        RejectCode::AlreadyExists
    );
    let moved = bench.done(8, &bench.move_person(0x31, 0x22));
    assert_eq!(moved.revision, 2);
    let files = bench.state_files();
    assert!(!files.contains_key(&path));
    let to = profile_file(&[TEAM, 0x22], EmployeeKind::Person, 0x31);
    let record = records::decode_employee(&files[&to]).expect("the moved profile");
    assert_eq!(record.home_team_id, id(0x22));
    // An agent is not moved here: agents change through HR.
    let refused = bench.refused(9, &bench.move_person(HR, 0x22));
    assert_eq!(refused.code, RejectCode::InvalidRequest);
}

/// R11: one handle namespace for the organization, over every status; a handle is
/// lowercase, not reserved and not shaped like an address.
pub fn people_share_one_handle_namespace<E: OrganizationEngine, G>(open: &dyn Fn() -> (Arc<E>, G)) {
    let bench = two_teams(open);
    let with = |person: u64, handle: &str| AddPerson {
        handle: handle.into(),
        ..person_command(person, 0x21)
    };
    let code = |op: u64, command: AddPerson| bench.refused(op, &command).code;
    // The creator's and the HR lead's handles are taken.
    assert_eq!(code(4, with(0x31, "ana")), RejectCode::AlreadyExists);
    assert_eq!(code(5, with(0x31, "hr")), RejectCode::AlreadyExists);
    for (op, handle) in [
        (6, "here"),
        (7, "Bo"),
        (8, "bo@example.test"),
        (9, ""),
        (10, "bo."),
    ] {
        assert_eq!(
            code(op, with(0x31, handle)),
            RejectCode::InvalidName,
            "{handle:?}"
        );
    }
    bench.done(11, &with(0x31, "bo"));
    assert_eq!(code(12, with(0x32, "bo")), RejectCode::AlreadyExists);
    // An ID that is another record's.
    for (op, taken) in [(13, 0x21), (14, CREATOR), (15, 0x31)] {
        let command = AddPerson {
            id: id(taken),
            ..with(0x32, "cy")
        };
        assert_eq!(code(op, command), RejectCode::AlreadyExists, "{taken:x}");
    }
}

/// R11: an inactive person's handle stays theirs.
pub fn an_inactive_persons_handle_stays_taken<E: OrganizationEngine, G>(
    open: &dyn Fn() -> (Arc<E>, G),
) {
    let bench = two_teams(open);
    bench.add_person(4, 0x31, 0x21);
    bench.deactivate(5, 0x31);
    let again = AddPerson {
        handle: "p31".into(),
        ..person_command(0x32, 0x22)
    };
    assert_eq!(bench.refused(6, &again).code, RejectCode::AlreadyExists);
    assert!(bench.read(|snapshot| snapshot.handle_in_use("p31")));
}

/// R12: deactivating sets `inactive` and `offboarded_at` and keeps the handle, the home
/// team and the profile's path; an inactive person changes no further.
pub fn deactivate_sets_inactive_and_offboarded_at_and_keeps_the_handle<E: OrganizationEngine, G>(
    open: &dyn Fn() -> (Arc<E>, G),
) {
    let bench = two_teams(open);
    bench.add_person(4, 0x31, 0x21);
    let path = profile_file(&[TEAM, 0x21], EmployeeKind::Person, 0x31);
    let done = bench.deactivate(5, 0x31);
    assert_eq!(done.revision, 2);
    let person = bench.employee(0x31);
    assert_eq!(person.status, EmployeeStatus::Inactive);
    assert_eq!(person.offboarded_at, Some(records::time_text(CREATION_AT)));
    assert_eq!(
        (person.handle.as_str(), &person.home_team_id),
        ("p31", &id(0x21))
    );
    let files = bench.state_files();
    assert_eq!(records::decode_employee(&files[&path]), Ok(person));

    let code = |op: u64, refused: OrgRejection| {
        assert_eq!(
            refused.code,
            RejectCode::InvalidRequest,
            "op {op}: {refused:?}"
        );
    };
    code(6, bench.refused(6, &deactivation(&bench, 0x31)));
    code(7, bench.refused(7, &UpdatePerson::new(id(0x31), 2)));
    code(8, bench.refused(8, &bench.move_person(0x31, 0x22)));
    // An agent is not deactivated here.
    code(9, bench.refused(9, &deactivation(&bench, HR)));
    let hr = UpdatePerson::new(id(HR), bench.employee(HR).meta.revision);
    code(10, bench.refused(10, &hr));
    assert_eq!(text(&bench.state_files()), text(&files));
}

/// R13: a reporting line names an existing employee, never the person, and never comes
/// back to the person, however far up.
pub fn reporting_lines_have_no_loops<E: OrganizationEngine, G>(open: &dyn Fn() -> (Arc<E>, G)) {
    let bench = two_teams(open);
    bench.add_person(4, 0x31, 0x21);
    bench.add_person(5, 0x32, 0x21);
    bench.add_person(6, 0x33, 0x22);
    bench.set_manager(7, 0x31, Some(0x32));
    bench.set_manager(8, 0x32, Some(0x33));
    assert_eq!(bench.employee(0x31).reports_to_employee_id, Some(id(0x32)));
    let refused = bench.refused(9, &manager(&bench, 0x33, 0x31));
    assert_eq!(refused.code, RejectCode::Cycle);
    let refused = bench.refused(10, &manager(&bench, 0x33, 0x32));
    assert_eq!(refused.code, RejectCode::Cycle);
    let refused = bench.refused(11, &manager(&bench, 0x31, 0x31));
    assert_eq!(refused.code, RejectCode::InvalidRequest);
    let refused = bench.refused(12, &manager(&bench, 0x31, 0x99));
    assert_eq!(refused.code, RejectCode::NotFound);
    // Anyone may report to an agent, across teams.
    bench.set_manager(13, 0x33, Some(HR));
    bench.set_manager(14, 0x31, None);
    assert_eq!(bench.employee(0x31).reports_to_employee_id, None);
    // With the line cut, the top of it may report to its former bottom.
    bench.set_manager(15, 0x33, Some(0x31));
}

/// R13: nobody starts to report to an inactive employee.
pub fn nobody_new_reports_to_an_inactive_person<E: OrganizationEngine, G>(
    open: &dyn Fn() -> (Arc<E>, G),
) {
    let bench = two_teams(open);
    bench.add_person(4, 0x31, 0x21);
    bench.add_person(5, 0x32, 0x21);
    bench.deactivate(6, 0x32);
    let refused = bench.refused(7, &manager(&bench, 0x31, 0x32));
    assert_eq!(refused.code, RejectCode::InvalidRequest);
    // Nor to a paused agent (one is paused here as its file would carry it).
    let mut hr = bench.employee(HR);
    hr.status = EmployeeStatus::Paused;
    let path = durable(&profile_file(&[TEAM], EmployeeKind::Agent, HR));
    let paused = Change::new(Op::CreateTeam.ack()).put(path, records::encode_employee(&hr));
    assert_eq!(
        bench.write(paused).map(|r| r.outcome),
        Ok(Outcome::Completed)
    );
    let refused = bench.refused(8, &manager(&bench, 0x31, HR));
    assert_eq!(refused.code, RejectCode::InvalidRequest);
    assert_eq!(bench.employee(0x31).reports_to_employee_id, None);
    // An active agent is fine.
    hr.status = EmployeeStatus::Active;
    let path = durable(&profile_file(&[TEAM], EmployeeKind::Agent, HR));
    let active = Change::new(Op::CreateTeam.ack()).put(path, records::encode_employee(&hr));
    assert_eq!(
        bench.write(active).map(|r| r.outcome),
        Ok(Outcome::Completed)
    );
    bench.set_manager(9, 0x31, Some(HR));
}

/// R14: the owner is never deactivated, ahead of every other rule that would refuse it.
pub fn the_owner_cannot_be_deactivated<E: OrganizationEngine, G>(open: &dyn Fn() -> (Arc<E>, G)) {
    let bench = Bench::created(open);
    // The owner also leads the top-level team: `owner_protected` comes first.
    let refused = bench.refused(2, &deactivation(&bench, CREATOR));
    assert_eq!(refused.code, RejectCode::OwnerProtected);
    assert_eq!(bench.employee(CREATOR).status, EmployeeStatus::Active);
}

/// R14 and Q2: a person who leads a team or has active direct reports is refused with
/// `not_empty`, naming what to clear; nothing changes until it is cleared.
pub fn deactivating_refuses_a_lead_or_a_manager<E: OrganizationEngine, G>(
    open: &dyn Fn() -> (Arc<E>, G),
) {
    let bench = two_teams(open);
    bench.add_person(4, 0x31, 0x21);
    bench.add_person(5, 0x32, 0x22);
    bench.add_person(6, 0x33, 0x22);
    bench.set_lead(7, 0x21, Some(0x31));
    bench.set_manager(8, 0x32, Some(0x31));
    let files = bench.state_files();
    let refused = bench.refused(9, &deactivation(&bench, 0x31));
    assert_eq!(refused.code, RejectCode::NotEmpty);
    assert!(refused.message.contains("TEAM-2"), "{refused:?}");
    assert!(refused.message.contains("manager of @p32"), "{refused:?}");
    assert_eq!(text(&bench.state_files()), text(&files));

    bench.set_lead(10, 0x21, None);
    let refused = bench.refused(11, &deactivation(&bench, 0x31));
    assert_eq!(refused.code, RejectCode::NotEmpty);
    assert!(!refused.message.contains("TEAM-2"), "{refused:?}");
    bench.set_manager(12, 0x32, None);
    bench.deactivate(13, 0x31);
    // A report who has left does not hold the manager.
    bench.set_manager(14, 0x33, Some(0x32));
    bench.deactivate(15, 0x33);
    bench.deactivate(16, 0x32);
    assert_eq!(bench.employee(0x33).reports_to_employee_id, Some(id(0x32)));
}

/// R15: a new person is an active person who may hire, with no builtin key, no manager
/// and the owner as creator.
pub fn a_new_person_may_hire<E: OrganizationEngine, G>(open: &dyn Fn() -> (Arc<E>, G)) {
    let bench = two_teams(open);
    let generation = bench.generation();
    let added = bench.done(
        4,
        &AddPerson {
            title: Some("Engineer".into()),
            contact: Some("bo@example.test".into()),
            ..person_command(0x31, 0x21)
        },
    );
    assert_eq!(added.id, id(0x31));
    assert_eq!((added.revision, added.generation), (1, generation + 1));
    let person = bench.employee(0x31);
    assert_eq!(person.kind, EmployeeKind::Person);
    assert_eq!(person.status, EmployeeStatus::Active);
    assert!(person.may_hire);
    assert_eq!(person.builtin_key, None);
    assert_eq!(person.reports_to_employee_id, None);
    assert_eq!(person.offboarded_at, None);
    assert_eq!(person.title.as_deref(), Some("Engineer"));
    assert_eq!(person.meta.created_by, By::Employee(id(CREATOR)));
    // Free text is one line.
    let two_lines = AddPerson {
        focus: Some("a\nb".into()),
        ..person_command(0x32, 0x21)
    };
    assert_eq!(
        bench.refused(5, &two_lines).code,
        RejectCode::InvalidRequest
    );
}

/// R16 and Q4: a team takes six by hiring unless changed; people are not limited, by
/// addition or by a move.
pub fn team_member_limit<E: OrganizationEngine, G>(open: &dyn Fn() -> (Arc<E>, G)) {
    let bench = two_teams(open);
    assert_eq!(bench.team(0x21).max_members, records::DEFAULT_MAX_MEMBERS);
    let files = bench.state_files();
    let file = String::from_utf8_lossy(&files[&team_file(&[TEAM, 0x21])]).into_owned();
    assert!(file.contains("max_members: 6\n"), "{file}");
    for n in 0..8 {
        bench.add_person(10 + n, 0x40 + n, 0x21);
    }
    bench.add_person(30, 0x50, 0x22);
    bench.done(31, &bench.move_person(0x50, 0x21));
    let members = bench.read(|snapshot| snapshot.employees());
    let members = members
        .iter()
        .filter(|e| e.home_team_id == id(0x21))
        .count();
    assert_eq!(members, 9);
}

/// R18: a role reference names a role record of the organization; roles are read-only,
/// so the role here is written as a file the way a repository would carry it.
pub fn a_role_reference_must_exist<E: OrganizationEngine, G>(open: &dyn Fn() -> (Arc<E>, G)) {
    let bench = two_teams(open);
    let with_role = |person: u64, role: u64| AddPerson {
        role: Some(id(role)),
        ..person_command(person, 0x21)
    };
    assert_eq!(
        bench.refused(4, &with_role(0x31, 0x70)).code,
        RejectCode::NotFound
    );
    bench.put_role(0x70, "engineer");
    assert_eq!(bench.read(|s| s.roles()).len(), 1);
    bench.done(5, &with_role(0x31, 0x70));
    assert_eq!(bench.employee(0x31).role_id, Some(id(0x70)));
    let set = |role: Edit<Id>| UpdatePerson {
        role,
        ..UpdatePerson::new(id(0x31), bench.employee(0x31).meta.revision)
    };
    assert_eq!(
        bench.refused(6, &set(Edit::Set(id(0x71)))).code,
        RejectCode::NotFound
    );
    bench.done(7, &set(Edit::Clear));
    assert_eq!(bench.employee(0x31).role_id, None);
    bench.done(8, &set(Edit::Set(id(0x70))));
    // An ID in use as a role's is no new record's.
    let taken = CreateTeam {
        id: id(0x70),
        ..team_command(0x23, TEAM)
    };
    assert_eq!(bench.refused(9, &taken).code, RejectCode::AlreadyExists);
}

/// R19: only the owner changes the structure. Anyone else (a person who leads a team, an
/// agent, the system, nobody on record) is refused unrecorded for every command, and so
/// is a command whose IDs are not stable IDs.
pub fn only_the_owner_changes_structure<E: OrganizationEngine, G>(open: &dyn Fn() -> (Arc<E>, G)) {
    let bench = two_teams(open);
    bench.add_person(4, 0x31, 0x21);
    bench.add_person(5, 0x32, 0x21);
    bench.set_lead(6, 0x21, Some(0x31));
    let files = bench.state_files();
    let strangers = [
        Actor::Employee(id(0x31)),
        Actor::Employee(id(HR)),
        Actor::Employee(id(0x99)),
        Actor::System,
    ];
    let mut op = 100;
    let mut refuse = |command: &dyn Fn(&Actor, u64) -> StoreError| {
        for by in &strangers {
            op += 1;
            let error = command(by, op);
            assert_eq!(error, StoreError::invalid(NOT_AUTHORIZED), "{by:?}");
        }
    };
    refuse(&|by, op| bench.unrecorded(op, by, &team_command(0x23, 0x21)));
    refuse(&|by, op| bench.unrecorded(op, by, &rename(&bench, 0x21, "Mine")));
    refuse(&|by, op| bench.unrecorded(op, by, &bench.move_team(0x22, 0x21)));
    refuse(&|by, op| bench.unrecorded(op, by, &bench.archive(0x22)));
    refuse(&|by, op| bench.unrecorded(op, by, &person_command(0x33, 0x21)));
    refuse(&|by, op| bench.unrecorded(op, by, &manager(&bench, 0x32, 0x31)));
    refuse(&|by, op| bench.unrecorded(op, by, &bench.move_person(0x32, 0x22)));
    refuse(&|by, op| bench.unrecorded(op, by, &deactivation(&bench, 0x32)));
    assert_eq!(text(&bench.state_files()), text(&files));

    // IDs that are no stable IDs: malformed, refused unrecorded.
    let owner = Actor::Employee(id(CREATOR));
    let bad = Id::parse("team-x").expect("a safe component");
    bench.unrecorded(
        200,
        &owner,
        &MoveTeam {
            parent: bad.clone(),
            ..bench.move_team(0x22, 0x21)
        },
    );
    bench.unrecorded(
        201,
        &owner,
        &ArchiveTeam {
            id: bad.clone(),
            expected_revision: 1,
        },
    );
    bench.unrecorded(
        202,
        &owner,
        &AddPerson {
            home_team: bad.clone(),
            ..person_command(0x33, 0x21)
        },
    );
    bench.unrecorded(
        203,
        &owner,
        &UpdatePerson {
            manager: Edit::Set(bad.clone()),
            ..UpdatePerson::new(id(0x32), 1)
        },
    );
    bench.unrecorded(
        204,
        &owner,
        &MovePerson {
            id: bad.clone(),
            expected_revision: 1,
            home_team: id(0x22),
        },
    );
    bench.unrecorded(
        205,
        &owner,
        &DeactivatePerson {
            id: bad,
            expected_revision: 1,
        },
    );
    assert_eq!(text(&bench.state_files()), text(&files));
}

/// R20: every change raises the changed record's revision by one, names the owner and
/// the instant, and leaves a receipt of its kind and actor; an edit that changes nothing
/// still counts.
pub fn every_write_raises_the_revision_by_one_and_is_receipted<E: OrganizationEngine, G>(
    open: &dyn Fn() -> (Arc<E>, G),
) {
    let bench = two_teams(open);
    bench.add_person(4, 0x31, 0x21);
    bench.add_person(5, 0x32, 0x21);
    let check = |op: u64, kind: Op, revision: u64, meta: Meta| {
        assert_eq!(meta.revision, revision, "{kind:?}");
        assert_eq!(meta.updated_by, By::Employee(id(CREATOR)), "{kind:?}");
        assert_eq!(
            meta.updated_at,
            Some(records::time_text(CREATION_AT)),
            "{kind:?}"
        );
        let receipt = bench.read(|snapshot| snapshot.receipt(&id(op)));
        let receipt = receipt.expect("a receipt");
        assert_eq!(receipt.kind, kind.kind());
        assert_eq!(receipt.actor, Actor::Employee(id(CREATOR)));
        assert_eq!(receipt.outcome, Outcome::Completed);
    };
    // Last changed by the system at no recorded instant: a change by the owner says so.
    let mut team = bench.team(0x21);
    team.meta.updated_by = By::System;
    team.meta.updated_at = None;
    let mut person = bench.employee(0x31);
    person.meta.updated_by = By::System;
    person.meta.updated_at = None;
    let change = Change::new(Op::CreateTeam.ack())
        .put(
            durable(&team_file(&[TEAM, 0x21])),
            records::encode_team(&team),
        )
        .put(
            durable(&profile_file(&[TEAM, 0x21], EmployeeKind::Person, 0x31)),
            records::encode_employee(&person),
        );
    let written = bench.write(change).expect("written");
    assert_eq!(written.outcome, Outcome::Completed);
    let generation = bench.generation();
    let r = bench.done(10, &UpdateTeam::new(id(0x21), 1));
    assert_eq!(r.revision, 2);
    check(10, Op::UpdateTeam, 2, bench.team(0x21).meta);
    bench.done(11, &bench.move_team(0x22, 0x21));
    check(11, Op::MoveTeam, 2, bench.team(0x22).meta);
    bench.done(12, &UpdatePerson::new(id(0x31), 1));
    check(12, Op::UpdatePerson, 2, bench.employee(0x31).meta);
    bench.done(13, &bench.move_person(0x31, 0x22));
    check(13, Op::MovePerson, 3, bench.employee(0x31).meta);
    bench.done(14, &deactivation(&bench, 0x31));
    check(14, Op::DeactivatePerson, 4, bench.employee(0x31).meta);
    bench.done(15, &bench.move_person(0x32, TEAM));
    bench.done(16, &bench.archive(0x22));
    check(16, Op::ArchiveTeam, 3, bench.team(0x22).meta);
    bench.add_person(17, 0x33, TEAM);
    check(17, Op::AddPerson, 1, bench.employee(0x33).meta);
    bench.created_team(18, 0x23, TEAM);
    check(18, Op::CreateTeam, 1, bench.team(0x23).meta);
    // One generation each, and a retry is the stored answer without another.
    assert_eq!(bench.generation(), generation + 9);
    let again = bench
        .owner(10, &UpdateTeam::new(id(0x21), 1))
        .expect("stored");
    assert_eq!(
        serde_json::from_slice::<Changed>(&again.bytes).ok(),
        Some(r)
    );
    assert_eq!(bench.generation(), generation + 9);
}

/// R21: a stale `expected_revision` is `conflict`, told after every other rule but
/// `already_exists`.
pub fn a_domain_rule_is_told_before_a_stale_revision<E: OrganizationEngine, G>(
    open: &dyn Fn() -> (Arc<E>, G),
) {
    let bench = two_teams(open);
    bench.created_team(4, 0x23, 0x21);
    bench.add_person(5, 0x31, 0x21);
    bench.add_person(6, 0x32, 0x21);
    bench.set_lead(7, 0x21, Some(0x31));
    bench.set_manager(8, 0x31, Some(0x32));
    let stale = 99;
    // A rule refusal at any revision: that rule.
    let cycle = MoveTeam {
        expected_revision: stale,
        ..bench.move_team(0x21, 0x23)
    };
    assert_eq!(bench.refused(10, &cycle).code, RejectCode::Cycle);
    let not_empty = ArchiveTeam {
        id: id(0x21),
        expected_revision: stale,
    };
    assert_eq!(bench.refused(11, &not_empty).code, RejectCode::NotEmpty);
    let owner = DeactivatePerson {
        id: id(CREATOR),
        expected_revision: stale,
    };
    assert_eq!(bench.refused(12, &owner).code, RejectCode::OwnerProtected);
    let loop_back = UpdatePerson {
        expected_revision: stale,
        ..manager(&bench, 0x32, 0x31)
    };
    assert_eq!(bench.refused(13, &loop_back).code, RejectCode::Cycle);
    let lead_moves = MovePerson {
        expected_revision: stale,
        ..bench.move_person(0x31, 0x22)
    };
    assert_eq!(
        bench.refused(14, &lead_moves).code,
        RejectCode::InvalidRequest
    );
    let bad_name = UpdateTeam {
        expected_revision: stale,
        ..rename(&bench, 0x22, "two\nlines")
    };
    assert_eq!(bench.refused(15, &bad_name).code, RejectCode::InvalidName);

    // Stale alone, and stale ahead of `already_exists`.
    let conflict = |op: u64, refused: OrgRejection| {
        assert_eq!(refused.code, RejectCode::Conflict, "op {op}: {refused:?}");
    };
    conflict(20, bench.refused(20, &UpdateTeam::new(id(0x22), stale)));
    let clash = UpdateTeam {
        expected_revision: stale,
        ..rename(&bench, 0x22, "Team 21")
    };
    conflict(21, bench.refused(21, &clash));
    let already_there = MoveTeam {
        expected_revision: stale,
        ..bench.move_team(0x22, TEAM)
    };
    conflict(22, bench.refused(22, &already_there));
    let own_team = MovePerson {
        expected_revision: stale,
        ..bench.move_person(0x32, 0x21)
    };
    conflict(23, bench.refused(23, &own_team));
    conflict(24, bench.refused(24, &UpdatePerson::new(id(0x32), stale)));
    let archive = ArchiveTeam {
        id: id(0x22),
        expected_revision: stale,
    };
    conflict(25, bench.refused(25, &archive));
    // At the current revision, `already_exists`.
    let clash = rename(&bench, 0x22, "Team 21");
    assert_eq!(bench.refused(26, &clash).code, RejectCode::AlreadyExists);
}

/// R17: one built-in HR lead: a second employee with `builtin_key: hr` is refused by the
/// projection, on every engine.
pub fn one_builtin_hr_lead<E: OrganizationEngine, G>(open: &dyn Fn() -> (Arc<E>, G)) {
    let bench = Bench::created(open);
    let mut second = bench.employee(HR);
    second.id = id(0x3a);
    second.handle = "hr2".into();
    let path = profile_file(&[TEAM], EmployeeKind::Agent, 0x3a);
    let faulty =
        Change::new(Op::CreateTeam.ack()).put(durable(&path), records::encode_employee(&second));
    refused_by_validation(bench, open, faulty, "another employee has this builtin_key");
}

/// The rules only `validate` holds for roles: a role reference names a role, and a key
/// names one role.
pub fn roles_are_checked_by_the_projection<E: OrganizationEngine, G>(
    open: &dyn Fn() -> (Arc<E>, G),
) {
    let bench = Bench::created(open);
    let mut person = bench.employee(CREATOR);
    person.role_id = Some(id(0x71));
    let path = profile_file(&[TEAM], EmployeeKind::Person, CREATOR);
    let faulty =
        Change::new(Op::CreateTeam.ack()).put(durable(&path), records::encode_employee(&person));
    refused_by_validation(bench, open, faulty, "its role_id names no role");

    let bench = Bench::created(open);
    bench.put_role(0x70, "engineer");
    let twin = role_record(0x71, "engineer");
    let faulty = Change::new(Op::CreateTeam.ack())
        .put(records::role_path(&id(0x71)), records::encode_role(&twin));
    refused_by_validation(bench, open, faulty, "another role has this key");

    // A team led by an inactive employee.
    let bench = Bench::created(open);
    bench.created_team(2, 0x21, TEAM);
    bench.add_person(3, 0x31, 0x21);
    let mut inactive = bench.employee(0x31);
    inactive.status = EmployeeStatus::Inactive;
    let mut team = bench.team(0x21);
    team.lead_employee_id = Some(id(0x31));
    let profile = profile_file(&[TEAM, 0x21], EmployeeKind::Person, 0x31);
    let faulty = Change::new(Op::CreateTeam.ack())
        .put(durable(&profile), records::encode_employee(&inactive))
        .put(
            durable(&team_file(&[TEAM, 0x21])),
            records::encode_team(&team),
        );
    refused_by_validation(bench, open, faulty, "its lead is inactive");
}

/// A path the suite named as text, back as a durable path.
fn durable(path: &str) -> org_store::DurablePath {
    org_store::DurablePath::parse(path).expect("a durable path")
}

/// `faulty` files break a rule `validate` holds: an engine with constraints of its own
/// fails the write; otherwise the write lands and the rebuild is refused.
fn refused_by_validation<E: OrganizationEngine, G>(
    bench: Bench<E, G>,
    open: &dyn Fn() -> (Arc<E>, G),
    faulty: Change,
    reason: &str,
) {
    let generation = bench.generation();
    match bench.write(faulty) {
        // An engine constraint stops the write: nothing is accepted.
        Err(error @ (StoreError::Internal(_) | StoreError::Projection(_))) => {
            // The engine's own constraint or reference check, not some other failure.
            let text = format!("{error:?}");
            assert!(
                ["constraint", "references", reason]
                    .iter()
                    .any(|t| text.contains(t)),
                "{text}"
            );
            assert_eq!(bench.generation(), generation);
            return;
        }
        reply => assert_eq!(reply.map(|reply| reply.outcome), Ok(Outcome::Completed)),
    }
    let error = bench.rebuilt(open).err().expect("the rebuild is refused");
    let text = error.to_string();
    assert!(text.contains(reason), "{text}");
}
