use org_store::{
    testkit::{id, MemoryEngine, MemoryTx},
    CorePort, CoreRead, Sha256, Timestamp,
};

use super::*;
use crate::organization::{
    ports::{OrganizationRead, RecordRef},
    records::{
        Employee, EmployeeKind, EmployeeStatus, Office, OfficeCode, Organization, Repository,
        TeamAllocation, TeamCounter, TeamStatus,
    },
};

const ORG: u64 = 1;
const OFFICE: u64 = 2;
const REPOSITORY: u64 = 3;
const TOP: u64 = 4;
const ANA: u64 = 5;
const HR: u64 = 6;
const CHILD: u64 = 7;

fn files() -> OrganizationFiles {
    OrganizationFiles::new(id(ORG))
}

fn meta() -> Meta {
    Meta::created(&By::Employee(id(ANA)), Timestamp(1_700_000_000_000))
}

fn team(n: u64, number: u64, parent: Option<u64>, lead: Option<u64>) -> Team {
    Team {
        id: id(n),
        name: format!("Team {n}"),
        number,
        parent_team_id: parent.map(id),
        lead_employee_id: lead.map(id),
        status: TeamStatus::Active,
        max_members: 6,
        meta: meta(),
    }
}

fn employee(n: u64, handle: &str, kind: EmployeeKind, home: u64) -> Employee {
    Employee {
        id: id(n),
        handle: handle.into(),
        kind,
        name: handle.to_uppercase(),
        home_team_id: id(home),
        role_id: None,
        reports_to_employee_id: None,
        status: EmployeeStatus::Active,
        contact: None,
        title: None,
        focus: None,
        role_text: None,
        title_in_team: None,
        offboarded_at: None,
        may_hire: kind == EmployeeKind::Person,
        builtin_key: None,
        meta: meta(),
        body: String::new(),
    }
}

fn allocation(number: u64, team: u64) -> TeamAllocation {
    TeamAllocation {
        organization_id: id(ORG),
        number,
        team_id: id(team),
        allocated_at: "2023-11-14T22:13:20.000Z".into(),
    }
}

fn organization() -> Organization {
    Organization {
        id: id(ORG),
        slug: "example".into(),
        name: "Example".into(),
        organization_repository_id: id(REPOSITORY),
        state_branch: "org/state".into(),
        meta: meta(),
    }
}

fn repository() -> Repository {
    Repository {
        id: id(REPOSITORY),
        key: Some("organization".into()),
        kind: RepositoryKind::Remote,
        remote_url: Some("https://example.test/org.git".into()),
        meta: meta(),
    }
}

fn office(n: u64, slug: &str, role: OfficeRole) -> Office {
    Office {
        id: id(n),
        slug: slug.into(),
        name: slug.to_uppercase(),
        role,
        meta: meta(),
    }
}

fn code(code: &str, office: u64) -> OfficeCode {
    OfficeCode {
        code: code.into(),
        office_id: id(office),
        issued_at: "2023-11-14T22:13:20.000Z".into(),
        retired_at: None,
    }
}

fn counter(last_number: u64) -> TeamCounter {
    TeamCounter {
        organization_id: id(ORG),
        last_number,
    }
}

/// Applies one file as the store does: the rows, then the source hash.
fn put(tx: &mut MemoryTx, path: &DurablePath, bytes: &[u8]) -> Result<(), ProjectionError> {
    Projector::<MemoryEngine>::apply(&files(), tx, path, Some(bytes))?;
    tx.set_source(path, Some(Sha256::of(bytes)))?;
    Ok(())
}

fn remove(tx: &mut MemoryTx, path: &DurablePath) {
    Projector::<MemoryEngine>::apply(&files(), tx, path, None).unwrap();
    tx.set_source(path, None).unwrap();
}

fn put_team(tx: &mut MemoryTx, chain: &[u64], record: &Team) {
    let chain: Vec<Id> = chain.iter().copied().map(id).collect();
    let path = records::team_path(&chain).unwrap();
    put(tx, &path, &records::encode_team(record)).unwrap();
}

fn put_employee(tx: &mut MemoryTx, chain: &[u64], record: &Employee) {
    let chain: Vec<Id> = chain.iter().copied().map(id).collect();
    let path = records::employee_path(&chain, record.kind, &record.id).unwrap();
    put(tx, &path, &records::encode_employee(record)).unwrap();
}

fn put_allocation(tx: &mut MemoryTx, record: &TeamAllocation) {
    let path = records::team_allocation_path(&id(ORG), record.number);
    put(tx, &path, &records::encode_team_allocation(record)).unwrap();
}

fn put_counter(tx: &mut MemoryTx, record: &TeamCounter) {
    let bytes = records::encode_team_counter(record);
    put(tx, &records::team_counter_path(&id(ORG)), &bytes).unwrap();
}

/// A whole, valid organization: one office, a top-level team led by Ana, HR, a child team.
fn seeded() -> MemoryTx {
    let mut tx = MemoryEngine::new().begin().unwrap();
    let bytes = records::encode_organization(&organization());
    put(&mut tx, &records::organization_path(), &bytes).unwrap();
    let bytes = records::encode_repositories(&[repository()]);
    put(&mut tx, &records::repositories_path(), &bytes).unwrap();
    let bytes = records::encode_office(&office(OFFICE, "hq", OfficeRole::Hq));
    put(&mut tx, &records::office_path(&id(OFFICE)), &bytes).unwrap();
    let bytes = records::encode_office_codes(&[code("A", OFFICE)]);
    put(&mut tx, &records::office_codes_path(), &bytes).unwrap();
    put_team(&mut tx, &[TOP], &team(TOP, 1, None, Some(ANA)));
    put_team(&mut tx, &[TOP, CHILD], &team(CHILD, 2, Some(TOP), None));
    put_employee(
        &mut tx,
        &[TOP],
        &employee(ANA, "ana", EmployeeKind::Person, TOP),
    );
    put_employee(
        &mut tx,
        &[TOP],
        &employee(HR, "hr", EmployeeKind::Agent, TOP),
    );
    put_counter(&mut tx, &counter(2));
    put_allocation(&mut tx, &allocation(1, TOP));
    put_allocation(&mut tx, &allocation(2, CHILD));
    tx
}

fn check(tx: &MemoryTx) -> Result<(), ProjectionError> {
    Projector::<MemoryEngine>::validate(&files(), tx)
}

/// The path and the reason `validate` refuses with.
fn refusal(tx: &MemoryTx) -> (String, String) {
    let error = check(tx).unwrap_err();
    assert!(error.refuses_rebuild);
    (
        error.path.map(String::from).unwrap_or_default(),
        error.reason,
    )
}

fn top_path() -> String {
    records::team_path(&[id(TOP)]).unwrap().into()
}

#[test]
fn every_file_becomes_its_rows_and_a_removed_file_takes_them_away() {
    let mut tx = seeded();
    check(&tx).unwrap();
    assert_eq!(tx.organization().unwrap(), Some(organization()));
    assert_eq!(tx.repositories().unwrap(), [repository()]);
    assert_eq!(
        tx.office(&id(OFFICE)).unwrap(),
        Some(office(OFFICE, "hq", OfficeRole::Hq))
    );
    assert_eq!(tx.offices().unwrap().len(), 1);
    assert_eq!(
        tx.office_code(&id(OFFICE)).unwrap(),
        Some(code("A", OFFICE))
    );
    assert_eq!(tx.office_code(&id(TOP)).unwrap(), None);
    assert_eq!(
        tx.team(&id(TOP)).unwrap(),
        Some(team(TOP, 1, None, Some(ANA)))
    );
    assert_eq!(tx.team_by_number(2).unwrap().map(|t| t.id), Some(id(CHILD)));
    assert_eq!(tx.team_by_number(3).unwrap(), None);
    assert_eq!(tx.teams().unwrap().len(), 2);
    assert_eq!(
        tx.team_chain(&id(CHILD)).unwrap(),
        Some(vec![id(TOP), id(CHILD)])
    );
    assert_eq!(tx.team_chain(&id(ANA)).unwrap(), None);
    assert_eq!(
        tx.employee(&id(HR)).unwrap(),
        Some(employee(HR, "hr", EmployeeKind::Agent, TOP))
    );
    assert_eq!(tx.employees().unwrap().len(), 2);
    assert!(tx.handle_in_use("ana").unwrap());
    assert!(!tx.handle_in_use("bo").unwrap());
    assert_eq!(tx.team_counter().unwrap(), Some(counter(2)));
    assert_eq!(
        tx.team_allocations().unwrap(),
        [allocation(1, TOP), allocation(2, CHILD)]
    );
    let reference = |kind: &str, n: u64| RecordRef {
        kind: kind.into(),
        id: id(n),
        is_deleted: false,
    };
    assert_eq!(
        tx.record_refs().unwrap(),
        [
            reference("organization", ORG),
            reference("team", TOP),
            reference("team", CHILD)
        ]
    );
    // No cursor yet: generation zero.
    assert_eq!(tx.generation().unwrap(), 0);

    // A put replaces the row whole.
    let mut renamed = team(CHILD, 2, Some(TOP), None);
    renamed.name = "Renamed".into();
    put_team(&mut tx, &[TOP, CHILD], &renamed);
    assert_eq!(tx.team(&id(CHILD)).unwrap(), Some(renamed));
    assert_eq!(tx.teams().unwrap().len(), 2);

    // Every file removed: every row gone, and an empty projection is valid.
    let state = DurablePath::parse("state").unwrap();
    for path in tx.sources_under(&state).unwrap() {
        remove(&mut tx, &path);
    }
    assert_eq!(tx.organization().unwrap(), None);
    assert!(tx.repositories().unwrap().is_empty());
    assert!(tx.offices().unwrap().is_empty());
    assert!(tx.office_codes().unwrap().is_empty());
    assert!(tx.teams().unwrap().is_empty());
    assert!(tx.employees().unwrap().is_empty());
    assert_eq!(tx.team_counter().unwrap(), None);
    assert!(tx.team_allocations().unwrap().is_empty());
    assert!(tx.record_refs().unwrap().is_empty());
    check(&tx).unwrap();
}

#[test]
fn the_projector_owns_its_paths_and_no_others() {
    let owns = |text: &str| {
        let path = DurablePath::parse(text).unwrap();
        Projector::<MemoryEngine>::owns(&files(), &path)
    };
    assert!(owns("state/organization.yaml"));
    assert!(owns("state/identity/office-codes.yaml"));
    assert!(owns(&top_path()));
    assert!(!owns(
        "state/teams/01900000-0000-7000-8000-000000000004/work/tasks/a.md"
    ));
    assert!(!owns(
        "recovery/commands/01900000-0000-7000-8000-000000000004.json"
    ));
    let mut tx = MemoryEngine::new().begin().unwrap();
    let other = DurablePath::parse("state/roles/a.md").unwrap();
    let error = Projector::<MemoryEngine>::apply(&files(), &mut tx, &other, None).unwrap_err();
    assert_eq!(error.reason, "not an organization file");
}

#[test]
fn a_file_that_disagrees_with_its_path_or_does_not_decode_is_refused_and_writes_nothing() {
    let mut tx = MemoryEngine::new().begin().unwrap();
    let mut refused = |path: DurablePath, bytes: Vec<u8>| {
        let error = put(&mut tx, &path, &bytes).unwrap_err();
        assert!(error.refuses_rebuild);
        assert_eq!(error.path, Some(path));
        error.reason
    };
    let chain = |teams: &[u64]| teams.iter().copied().map(id).collect::<Vec<_>>();

    // Another organization's record, counter and allocation.
    let mut other = organization();
    other.id = id(0x99);
    assert_eq!(
        refused(
            records::organization_path(),
            records::encode_organization(&other)
        ),
        "id does not match the path"
    );
    let foreign = TeamCounter {
        organization_id: id(0x99),
        last_number: 1,
    };
    assert_eq!(
        refused(
            records::team_counter_path(&id(0x99)),
            records::encode_team_counter(&foreign)
        ),
        "the organization does not match the path"
    );
    assert_eq!(
        refused(
            records::team_counter_path(&id(ORG)),
            records::encode_team_counter(&foreign)
        ),
        "scope_id does not match the path"
    );
    assert_eq!(
        refused(
            records::team_allocation_path(&id(ORG), 3),
            records::encode_team_allocation(&allocation(2, CHILD))
        ),
        "number does not match the path"
    );
    assert_eq!(
        refused(
            records::team_allocation_path(&id(0x99), 2),
            records::encode_team_allocation(&allocation(2, CHILD))
        ),
        "the organization does not match the path"
    );

    assert_eq!(
        refused(
            records::office_path(&id(TOP)),
            records::encode_office(&office(OFFICE, "hq", OfficeRole::Hq))
        ),
        "id does not match the path"
    );

    // A team in another team's folder, under the wrong parent, or at the top with one.
    let child = records::encode_team(&team(CHILD, 2, Some(TOP), None));
    assert_eq!(
        refused(
            records::team_path(&chain(&[TOP, HR])).unwrap(),
            child.clone()
        ),
        "id does not match the path"
    );
    assert_eq!(
        refused(
            records::team_path(&chain(&[HR, CHILD])).unwrap(),
            child.clone()
        ),
        "parent_team_id does not match the path"
    );
    assert_eq!(
        refused(records::team_path(&chain(&[CHILD])).unwrap(), child),
        "parent_team_id does not match the path"
    );
    let top = records::encode_team(&team(TOP, 1, None, None));
    assert_eq!(
        refused(records::team_path(&chain(&[CHILD, TOP])).unwrap(), top),
        "parent_team_id does not match the path"
    );

    // A profile under another ID, in the other kind's folder, or outside its home team.
    let ana = employee(ANA, "ana", EmployeeKind::Person, TOP);
    let bytes = records::encode_employee(&ana);
    let at = |teams: &[u64], kind, n| records::employee_path(&chain(teams), kind, &id(n)).unwrap();
    assert_eq!(
        refused(at(&[TOP], EmployeeKind::Person, HR), bytes.clone()),
        "id does not match the path"
    );
    assert_eq!(
        refused(at(&[TOP], EmployeeKind::Agent, ANA), bytes.clone()),
        "kind does not match the path"
    );
    assert_eq!(
        refused(at(&[TOP, CHILD], EmployeeKind::Person, ANA), bytes),
        "home_team_id does not match the path"
    );

    // Bytes that are not the record name the field.
    assert_eq!(
        refused(records::organization_path(), b"id: 7\n".to_vec()),
        "schema_version: missing"
    );
    assert_eq!(
        refused(records::repositories_path(), b"\xff".to_vec()),
        "not UTF-8"
    );
    assert!(tx.organization().unwrap().is_none() && tx.teams().unwrap().is_empty());
    assert!(tx.employees().unwrap().is_empty() && tx.team_counter().unwrap().is_none());
}

#[test]
fn validation_refuses_a_missing_organization_record() {
    let mut tx = seeded();
    remove(&mut tx, &records::organization_path());
    assert_eq!(
        refusal(&tx),
        (
            "state/organization.yaml".into(),
            "the organization record is missing".into()
        )
    );
}

#[test]
fn validation_refuses_a_lead_outside_the_team() {
    // Ana's home is the top-level team; the child team cannot be led by her.
    let mut tx = seeded();
    put_team(
        &mut tx,
        &[TOP, CHILD],
        &team(CHILD, 2, Some(TOP), Some(ANA)),
    );
    let (path, reason) = refusal(&tx);
    assert!(
        path.ends_with(&format!("/teams/{}/team.yaml", id(CHILD))),
        "{path}"
    );
    assert_eq!(reason, "its lead is not an employee of the team");

    // Nor by nobody on record.
    let mut tx = seeded();
    put_team(&mut tx, &[TOP], &team(TOP, 1, None, Some(0x77)));
    assert_eq!(
        refusal(&tx),
        (top_path(), "its lead is not an employee of the team".into())
    );
}

#[test]
fn validation_refuses_what_no_single_file_shows() {
    let case = |change: &dyn Fn(&mut MemoryTx)| {
        let mut tx = seeded();
        change(&mut tx);
        refusal(&tx)
    };
    let is = |path: String, reason: &str| (path, reason.to_owned());
    let organization_path = || String::from(records::organization_path());
    let counter_path = || String::from(records::team_counter_path(&id(ORG)));
    let rewrite = |tx: &mut MemoryTx, path: DurablePath, bytes: Vec<u8>| {
        put(tx, &path, &bytes).unwrap();
    };

    // The organization's repository.
    assert_eq!(
        case(&|tx| remove(tx, &records::repositories_path())),
        is(
            organization_path(),
            "organization_repository_id names no remote repository"
        )
    );
    let local = Repository {
        kind: RepositoryKind::Local,
        remote_url: None,
        ..repository()
    };
    assert_eq!(
        case(&|tx| rewrite(
            tx,
            records::repositories_path(),
            records::encode_repositories(std::slice::from_ref(&local))
        ))
        .1,
        "organization_repository_id names no remote repository"
    );

    // Offices: exactly one HQ, distinct slugs, codes that name an office.
    assert_eq!(
        case(&|tx| remove(tx, &records::office_path(&id(OFFICE)))).1,
        "0 offices have the role hq; exactly one does"
    );
    let second = |slug: &'static str, role| {
        move |tx: &mut MemoryTx| {
            let bytes = records::encode_office(&office(0x20, slug, role));
            put(tx, &records::office_path(&id(0x20)), &bytes).unwrap();
        }
    };
    assert_eq!(
        case(&second("second", OfficeRole::Hq)).1,
        "2 offices have the role hq; exactly one does"
    );
    assert_eq!(
        case(&second("hq", OfficeRole::Satellite)).1,
        "another office has this slug"
    );
    let mut satellite = seeded();
    second("lisbon", OfficeRole::Satellite)(&mut satellite);
    check(&satellite).unwrap();
    assert_eq!(
        case(&|tx| rewrite(
            tx,
            records::office_codes_path(),
            records::encode_office_codes(&[code("A", OFFICE), code("B", 0x20)])
        )),
        is(
            records::office_codes_path().into(),
            "code B names no office"
        )
    );

    // Teams: a parent on record, one number each, allocated to it.
    let orphan_path = records::team_path(&[id(0x30), id(0x31)]).unwrap();
    let orphan = records::encode_team(&team(0x31, 3, Some(0x30), None));
    assert_eq!(
        case(&|tx| rewrite(tx, orphan_path.clone(), orphan.clone())).1,
        format!(
            "team {} has no chain of parents to a top-level team",
            id(0x31)
        )
    );
    assert_eq!(
        case(&|tx| put_team(tx, &[TOP, CHILD], &team(CHILD, 1, Some(TOP), None))).1,
        "another team has this number"
    );
    assert_eq!(
        case(&|tx| put_team(tx, &[TOP, CHILD], &team(CHILD, 3, Some(TOP), None))).1,
        "its number is not allocated to it"
    );
    assert_eq!(
        case(&|tx| put_allocation(tx, &allocation(2, TOP))).1,
        "its number is not allocated to it"
    );

    // The counter and the allocations.
    assert_eq!(
        case(&|tx| put_counter(tx, &counter(1))),
        is(counter_path(), "behind an allocation")
    );
    assert_eq!(
        case(&|tx| remove(tx, &records::team_counter_path(&id(ORG)))),
        is(counter_path(), "the team counter is missing")
    );
    assert_eq!(
        case(&|tx| {
            put_counter(tx, &counter(3));
            put_allocation(tx, &allocation(3, 0x31));
        }),
        is(
            records::team_allocation_path(&id(ORG), 3).into(),
            "names no team with this number"
        )
    );
    // A counter ahead of its allocations is a gap, and gaps are allowed.
    let mut ahead = seeded();
    put_counter(&mut ahead, &counter(9));
    check(&ahead).unwrap();

    // Employees: a home team on record, one handle each, a reporting line that ends.
    let stray = records::employee_path(&[id(0x30)], EmployeeKind::Agent, &id(0x32)).unwrap();
    let homeless = records::encode_employee(&employee(0x32, "bo", EmployeeKind::Agent, 0x30));
    assert_eq!(
        case(&|tx| rewrite(tx, stray.clone(), homeless.clone())).1,
        format!("employee {} has no home team", id(0x32))
    );
    assert_eq!(
        case(&|tx| put_employee(tx, &[TOP], &employee(0x32, "ana", EmployeeKind::Agent, TOP))).1,
        "another employee has this handle"
    );
    let reporting = |n: u64, handle: &str, to: u64| Employee {
        reports_to_employee_id: Some(id(to)),
        ..employee(n, handle, EmployeeKind::Agent, TOP)
    };
    assert_eq!(
        case(&|tx| put_employee(tx, &[TOP], &reporting(0x32, "bo", 0x77))).1,
        "reports to nobody on record"
    );
    assert_eq!(
        case(&|tx| {
            put_employee(tx, &[TOP], &reporting(0x32, "bo", 0x33));
            put_employee(tx, &[TOP], &reporting(0x33, "cy", 0x32));
        })
        .1,
        "its reporting line is a cycle"
    );
    let mut line = seeded();
    put_employee(&mut line, &[TOP], &reporting(0x32, "bo", ANA));
    put_employee(&mut line, &[TOP], &reporting(0x33, "cy", 0x32));
    check(&line).unwrap();

    // Attribution to an employee on record.
    let by_nobody = Team {
        meta: Meta::created(&By::Employee(id(0x77)), Timestamp(0)),
        ..team(CHILD, 2, Some(TOP), None)
    };
    assert_eq!(
        case(&|tx| put_team(tx, &[TOP, CHILD], &by_nobody)).1,
        "attributed to nobody on record"
    );
    let by_system = Team {
        meta: Meta::created(&By::System, Timestamp(0)),
        ..team(CHILD, 2, Some(TOP), None)
    };
    let mut system = seeded();
    put_team(&mut system, &[TOP, CHILD], &by_system);
    check(&system).unwrap();
}

#[test]
fn validation_refuses_a_file_outside_the_folder_its_parent_chain_names() {
    // The child team's file also sits under a team that is not its parent's place.
    let mut tx = seeded();
    put_team(
        &mut tx,
        &[TOP, CHILD, 0x40],
        &team(0x40, 3, Some(CHILD), None),
    );
    put_counter(&mut tx, &counter(3));
    put_allocation(&mut tx, &allocation(3, 0x40));
    check(&tx).unwrap();
    // A second copy of the grandchild, in a folder that matches its own parent field but
    // not where that parent is.
    let stale = records::team_path(&[id(CHILD), id(0x40)]).unwrap();
    let bytes = records::encode_team(&team(0x40, 3, Some(CHILD), None));
    put(&mut tx, &stale, &bytes).unwrap();
    assert_eq!(
        refusal(&tx),
        (
            stale.into(),
            "not in the folder its parent chain names".into()
        )
    );

    // A profile left behind in a team the employee no longer belongs to.
    let mut tx = seeded();
    put_employee(
        &mut tx,
        &[TOP, CHILD],
        &employee(HR, "hr", EmployeeKind::Agent, CHILD),
    );
    let (path, reason) = refusal(&tx);
    assert_eq!(reason, "not in the folder its parent chain names");
    assert_eq!(
        path,
        String::from(records::employee_path(&[id(TOP)], EmployeeKind::Agent, &id(HR)).unwrap())
    );
}
