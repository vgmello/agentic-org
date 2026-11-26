//! The additions the office's switch to this store needs, on both engines: the CA records
//! in the organization's first entry, the office-local family under rebuild, the
//! clean-stop marker, `org.snapshot` and `team.create` through the native protocol's
//! mapping.

use org_proto::{
    org::{CommandAccepted, CommandStatus, MemberKind, TeamCreateParams},
    ApiError, ErrorCode,
};
use org_store::{testkit::CrashPoint, Change, Files, Operation, Sha256, Stream};

use super::*;
use crate::{
    office_local::{
        read_ca,
        records::{ca_key_path, clean_stop_path},
        CaError,
    },
    organization::{
        features::Op,
        records::{self, By, EmployeeKind, EmployeeStatus, Meta, TeamStatus},
    },
    structure::{snapshot::position, wire},
};

const HR: u64 = 0x13;

/// The state files of the suite's creation, under `durable/`: the organization family's
/// golden files and the escrow package.
fn creation_state(package: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut files = crate::organization::testkit::creation_files();
    files.insert("state/security/ca-escrow.v1.bin".into(), package.to_vec());
    files
}

/// The suite's CA key record, written out by hand from document 01's form.
fn golden_ca_key(package: &[u8]) -> String {
    format!(
        "{{\"schema_version\":1,\
\"organization_id\":\"01900000-0000-7000-8000-0000000000a2\",\
\"office_id\":\"01900000-0000-7000-8000-0000000000a3\",\
\"operation_id\":\"01900000-0000-7000-8000-000000000001\",\
\"escrow_version\":1,\
\"public_key\":\"04{}\",\
\"package_sha256\":\"{}\",\
\"wrapped_ca_key\":\"{}\"}}\n",
        "01".repeat(64),
        Sha256::of(package).to_hex(),
        "5b".repeat(64),
    )
}

fn local_folder(place: &Place) -> PathBuf {
    place
        .durable()
        .join(format!("local/offices/{}", identity().office_id))
}

fn marker_file(place: &Place) -> PathBuf {
    place
        .durable()
        .join(clean_stop_path(&identity().office_id).as_str())
}

fn runtime_folder(place: &Place) -> PathBuf {
    place.runtime().join(identity().office_id.as_str())
}

/// STORE-6 + TASK-10 (H-06) for the CA records: Entry 1 commits only the public state
/// files and escrow package (never the wrapped CA key), while `write_ca_key` writes
/// `ca-key.v1.json` atomically to local security storage outside `durable/recovery/`.
#[test]
fn creation_writes_both_ca_records_in_its_one_entry_with_their_exact_bytes() {
    for engine in ENGINES {
        let place = Place::new();
        let store = place.create(engine);
        let created = create_organization(&store, 1).unwrap();
        assert_eq!(created.outcome, Outcome::Completed, "{engine:?}");
        store.sync().unwrap();
        // One entry holds everything the creation wrote to state/.
        assert_eq!(store.status().flushed, 1, "{engine:?}");

        let material = ca_material(&identity().organization_id, CA);
        let state = tree(&place.durable().join("state"));
        let state: BTreeMap<String, Vec<u8>> = state
            .into_iter()
            .map(|(path, bytes)| (format!("state/{path}"), bytes))
            .collect();
        assert_eq!(state, creation_state(&material.package), "{engine:?}");
        // Entry 1 never writes ca-key.v1.json into local/ or durable/recovery/.
        assert!(
            tree(&local_folder(&place)).is_empty(),
            "{engine:?}: Entry 1 must not write ca-key.v1.json via journal"
        );
        // Local security storage write (done by `Office::provision_with` outside the journal).
        let record = material.record(&identity(), &id(1));
        crate::office_local::write_ca_key(&place.durable(), &identity().office_id, &record)
            .unwrap();
        let local = tree(&local_folder(&place));
        let key = String::from_utf8(local["security/ca-key.v1.json"].clone()).unwrap();
        assert_eq!(key, golden_ca_key(&material.package), "{engine:?}");
        assert_eq!(local.len(), 1, "{engine:?}: {:?}", local.keys());

        // Read back as the opening and the recovery read will.
        let files = Files::open(&place.durable()).unwrap();
        let ca = read_ca(&files, &identity().office_id)
            .unwrap()
            .expect("the CA");
        assert_eq!(ca.package, material.package);
        assert_eq!(ca.key.public_key, material.public_key);
        let (organization, office) = (identity().organization_id, identity().office_id);
        assert_eq!(ca.check(&organization, &office, &id(1)), Ok(()));
        let other_op = ca.check(&organization, &office, &id(2));
        assert!(
            matches!(other_op, Err(CaError::Mismatch(_))),
            "{other_op:?}"
        );
        store.close().unwrap();
    }
}

/// The CA material is part of what a creation's retry is matched on.
#[test]
fn a_creation_retry_with_regenerated_ca_material_conflicts_and_with_the_same_answers() {
    for engine in ENGINES {
        let place = Place::new();
        let store = place.create(engine);
        let organization = identity().organization_id;
        let created = create_organization(&store, 1).unwrap();
        let again = create_organization_with(&store, 1, ca_material(&organization, CA));
        assert_eq!(again, Ok(created.clone()), "{engine:?}");
        let regenerated = create_organization_with(&store, 1, ca_material(&organization, 2));
        assert_eq!(regenerated, Err(StoreError::Conflict), "{engine:?}");
        store.close().unwrap();

        // After a restart too: the receipt is matched, nothing is decided again.
        let store = place.reopen(engine);
        let again = create_organization_with(&store, 1, ca_material(&organization, CA));
        assert_eq!(again, Ok(created), "{engine:?}");
        let regenerated = create_organization_with(&store, 1, ca_material(&organization, 2));
        assert_eq!(regenerated, Err(StoreError::Conflict), "{engine:?}");
        assert_eq!(store.read(DirectoryGeneration).unwrap().0, 1, "{engine:?}");
        store.close().unwrap();
    }
}

/// Material that is not a CA record, or an escrow package sealed for another
/// organization, is refused before admission: nothing is recorded.
#[test]
fn ca_material_that_breaks_a_rule_is_refused_and_records_nothing() {
    for engine in ENGINES {
        let place = Place::new();
        let store = place.create(engine);
        let organization = identity().organization_id;
        let mut compressed = ca_material(&organization, CA);
        compressed.public_key[0] = 2;
        let mut short = ca_material(&organization, CA);
        short.wrapped_ca_key.truncate(8);
        let foreign = ca_material(&id(0xB2), CA);
        let mut cut = ca_material(&organization, CA);
        cut.package.pop();
        // A well-formed package sealed for another public key than the one given.
        let mut mismatched = ca_material(&organization, CA);
        mismatched.public_key = ca_material(&organization, 2).public_key;
        let cases = [compressed, short, foreign, cut, mismatched];
        for (n, bad) in cases.into_iter().enumerate() {
            let refused = create_organization_with(&store, 1, bad);
            assert!(
                matches!(refused, Err(StoreError::Invalid(_))),
                "{engine:?} {n}: {refused:?}"
            );
        }
        assert_eq!(store.read(DirectoryGeneration).unwrap().0, 0, "{engine:?}");
        // The first well-formed attempt decides afresh.
        let created = create_organization(&store, 1).unwrap();
        assert_eq!(created.outcome, Outcome::Completed, "{engine:?}");
        store.close().unwrap();
    }
}

/// M6 + TASK-10 (H-06): the wrapped CA key is ONLY in its own local security record.
/// No published state file, no projection, and no acceptance journal segment under
/// `durable/recovery/` holds it in either raw or hex form.
#[test]
fn the_wrapped_ca_key_is_in_no_state_file_and_no_projection() {
    for engine in ENGINES {
        let place = Place::new();
        let store = place.create(engine);
        create_organization(&store, 1).unwrap();
        create_team(&store, 2, 0x21).unwrap();
        store.sync().unwrap();
        let material = ca_material(&identity().organization_id, CA);
        let record = material.record(&identity(), &id(1));
        crate::office_local::write_ca_key(&place.durable(), &identity().office_id, &record)
            .unwrap();
        let wrapped = material.wrapped_ca_key;
        let hex: String = wrapped.iter().map(|b| format!("{b:02x}")).collect();
        let holds = |bytes: &[u8]| {
            let raw = bytes
                .windows(wrapped.len())
                .any(|w| w == wrapped.as_slice());
            raw || bytes.windows(hex.len()).any(|w| w == hex.as_bytes())
        };
        let dump = store.dump().join("\n");
        assert!(!holds(dump.as_bytes()), "{engine:?}: the projection");
        store.close().unwrap();
        for (path, bytes) in tree(&place.durable().join("state")) {
            assert!(!holds(&bytes), "{engine:?}: state/{path}");
        }
        for (path, bytes) in tree(&place.durable().join("recovery")) {
            assert!(!holds(&bytes), "{engine:?}: recovery/{path}");
        }
        for (path, bytes) in tree(&place.runtime()) {
            assert!(!holds(&bytes), "{engine:?}: runtime/{path}");
        }
        // Its record holds it, wrapped.
        let local = tree(&local_folder(&place));
        assert!(holds(&local["security/ca-key.v1.json"]), "{engine:?}");
    }
}

/// Rebuilding with the CA records, a team and the marker of an orderly stop gives the
/// projection the store had: the marker is not a source.
#[test]
fn a_rebuild_with_the_ca_records_and_the_marker_equals_the_projection_it_replaces() {
    for engine in ENGINES {
        let place = Place::new();
        let store = place.create(engine);
        create_organization(&store, 1).unwrap();
        create_team(&store, 2, 0x21).unwrap();
        store.sync().unwrap();
        let (dump, read) = (store.dump(), directory(&store));
        let owner = DirectorySnapshot { owner: id(CREATOR) };
        let snapshot = store.read(owner).unwrap().0;
        store.close().unwrap();
        assert!(marker_file(&place).is_file(), "{engine:?}");
        if engine == EngineChoice::Sqlite {
            std::fs::remove_dir_all(runtime_folder(&place)).unwrap();
        }

        // The memory engine rebuilds at every opening.
        let store = place.reopen(engine);
        assert!(store.stopped_cleanly(), "{engine:?}");
        assert_eq!(store.dump(), dump, "{engine:?}");
        assert_eq!(directory(&store), read, "{engine:?}");
        let owner = DirectorySnapshot { owner: id(CREATOR) };
        assert_eq!(store.read(owner).unwrap().0, snapshot, "{engine:?}");
        store.close().unwrap();
    }
}

/// A file under the office's local folder that no family owns stops the rebuild, which
/// names it.
#[test]
fn an_unowned_file_in_the_offices_local_folder_fails_the_rebuild_and_is_named() {
    for engine in ENGINES {
        let place = Place::new();
        let store = place.create(engine);
        create_organization(&store, 1).unwrap();
        store.close().unwrap();
        let material = ca_material(&identity().organization_id, CA);
        let record = material.record(&identity(), &id(1));
        crate::office_local::write_ca_key(&place.durable(), &identity().office_id, &record)
            .unwrap();
        let stray = local_folder(&place).join("security/ca-key.v2.json");
        std::fs::write(&stray, b"{}\n").unwrap();
        if engine == EngineChoice::Sqlite {
            std::fs::remove_dir_all(runtime_folder(&place)).unwrap();
        }
        let refused = open_store(&place.config(engine, Opening::Existing));
        let error = refused.err().expect("the rebuild is refused").to_string();
        let path = format!(
            "local/offices/{}/security/ca-key.v2.json",
            identity().office_id
        );
        assert!(error.contains(&path), "{engine:?}: {error}");
        assert!(error.contains("no family owns"), "{engine:?}: {error}");
        // A malformed key record names its own path.
        std::fs::remove_file(&stray).unwrap();
        let key = place
            .durable()
            .join(ca_key_path(&identity().office_id).as_str());
        let mut bytes = std::fs::read(&key).unwrap();
        bytes.pop();
        std::fs::write(&key, bytes).unwrap();
        if engine == EngineChoice::Sqlite {
            let _ = std::fs::remove_dir_all(runtime_folder(&place));
        }
        let refused = open_store(&place.config(engine, Opening::Existing));
        let error = refused.err().expect("the rebuild is refused").to_string();
        let path = ca_key_path(&identity().office_id).to_string();
        assert!(error.contains(&path), "{engine:?}: {error}");
    }
}

/// Office-local Git credential files under `security/` (`TASK-161`, `doc-35`) are excluded from
/// store rebuilds (never read into memory or rejected as unowned), while stray files beside them
/// remain refused.
#[test]
fn office_git_credential_files_survive_store_rebuild_while_stray_security_files_fail() {
    for engine in ENGINES {
        let place = Place::new();
        let store = place.create(engine);
        create_organization(&store, 1).unwrap();
        store.close().unwrap();
        let material = ca_material(&identity().organization_id, CA);
        let record = material.record(&identity(), &id(1));
        crate::office_local::write_ca_key(&place.durable(), &identity().office_id, &record)
            .unwrap();
        let sec = local_folder(&place).join("security");
        std::fs::write(sec.join("git-https-helper"), "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::write(sec.join("git-ssh-key"), "PRIVATE KEY BYTES").unwrap();
        std::fs::write(sec.join("git-known-hosts"), "github.com ssh-ed25519 AAAA\n").unwrap();
        if engine == EngineChoice::Sqlite {
            std::fs::remove_dir_all(runtime_folder(&place)).unwrap();
        }
        let reopened = open_store(&place.config(engine, Opening::Existing))
            .expect("rebuild must exclude office Git credential files");
        reopened.close().unwrap();

        // Stray sibling under security/ must still fail the rebuild.
        let stray = sec.join("ca-key.v2.json");
        std::fs::write(&stray, b"{}\n").unwrap();
        if engine == EngineChoice::Sqlite {
            std::fs::remove_dir_all(runtime_folder(&place)).unwrap();
        }
        let err = open_store(&place.config(engine, Opening::Existing))
            .err()
            .expect("stray security file must still be refused")
            .to_string();
        assert!(err.contains("security/ca-key.v2.json"), "{engine:?}: {err}");
    }
}

/// The marker: written by an orderly close with the journal it closed, reported once by
/// the next opening and removed by it; absent after a drop, a kill or a failed close.
#[test]
fn an_orderly_close_leaves_the_marker_and_the_next_opening_reports_it_once() {
    for engine in ENGINES {
        let place = Place::new();
        let store = place.create(engine);
        assert!(!store.stopped_cleanly(), "{engine:?}: a new office");
        create_organization(&store, 1).unwrap();
        create_team(&store, 2, 0x21).unwrap();
        store.close().unwrap();
        let marker = std::fs::read(marker_file(&place)).unwrap();
        let expected = format!(
            "{{\"journal_incarnation\":\"{}\",\"head_ordinal\":2}}\n",
            identity().journal_incarnation
        );
        assert_eq!(String::from_utf8(marker).unwrap(), expected, "{engine:?}");

        let store = place.reopen(engine);
        assert!(store.stopped_cleanly(), "{engine:?}");
        assert!(
            !marker_file(&place).exists(),
            "{engine:?}: removed at opening"
        );
        // Dropped, as a kill is: no marker, and the next opening is told so.
        drop(store);
        assert!(!marker_file(&place).exists(), "{engine:?}");
        let store = place.reopen(engine);
        assert!(!store.stopped_cleanly(), "{engine:?}");

        // Killed at a crash point while deciding: no marker either.
        store.close().unwrap();
        let store = place.reopen(engine);
        assert!(store.stopped_cleanly(), "{engine:?}");
        place.faults.arm(CrashPoint::DuringDecide, 1);
        assert!(create_team(&store, 3, 0x22).is_err(), "{engine:?}");
        assert!(place.faults.dead(), "{engine:?}");
        drop(store);
        place.faults.revive();
        assert!(!marker_file(&place).exists(), "{engine:?}");
        let store = place.reopen(engine);
        assert!(!store.stopped_cleanly(), "{engine:?}");
        store.close().unwrap();
    }
}

/// A marker for another journal incarnation or head, or in another form, is not clean;
/// it is removed all the same.
#[test]
fn a_marker_that_does_not_match_the_opened_journal_is_not_clean() {
    for engine in ENGINES {
        let place = Place::new();
        let store = place.create(engine);
        create_organization(&store, 1).unwrap();
        store.close().unwrap();
        let marker = |text: String| {
            std::fs::write(marker_file(&place), text).unwrap();
            let store = place.reopen(engine);
            let clean = store.stopped_cleanly();
            assert!(!marker_file(&place).exists(), "{engine:?}");
            store.close().unwrap();
            clean
        };
        let incarnation = identity().journal_incarnation;
        let good = |head: u64, of: &Id| {
            format!("{{\"journal_incarnation\":\"{of}\",\"head_ordinal\":{head}}}\n")
        };
        assert!(marker(good(1, &incarnation)), "{engine:?}");
        assert!(!marker(good(2, &incarnation)), "{engine:?}: another head");
        assert!(
            !marker(good(0, &incarnation)),
            "{engine:?}: an earlier head"
        );
        assert!(!marker(good(1, &id(0xB4))), "{engine:?}: another journal");
        assert!(!marker("torn".into()), "{engine:?}");
        assert!(
            !marker(good(1, &incarnation).replace(':', ": ")),
            "{engine:?}"
        );
    }
}

/// An opening that fails leaves the marker for the next one.
#[test]
fn an_opening_that_fails_leaves_the_marker() {
    for engine in ENGINES {
        let place = Place::new();
        let store = place.create(engine);
        create_organization(&store, 1).unwrap();
        store.close().unwrap();
        place.custody.close();
        assert!(open_store(&place.config(engine, Opening::Existing)).is_err());
        assert!(marker_file(&place).is_file(), "{engine:?}");
        place.custody.reopen();
        let store = place.reopen(engine);
        assert!(store.stopped_cleanly(), "{engine:?}");
        store.close().unwrap();
    }
}

/// A store that stopped reports why, and its close fails and leaves no marker.
#[test]
fn a_fatal_store_reports_its_fault_and_its_close_leaves_no_marker() {
    for engine in ENGINES {
        let place = Place::new();
        let store = place.create(engine);
        create_organization(&store, 1).unwrap();
        store.sync().unwrap();
        assert_eq!(store.fatal(), None, "{engine:?}");
        place.faults.arm(
            CrashPoint::FlushError(org_store::testkit::FlushSite::Segment),
            1,
        );
        assert!(create_team(&store, 2, 0x21).is_err(), "{engine:?}");
        let fatal = store.fatal();
        assert!(
            matches!(fatal, Some(StoreError::Stopped { path: Some(_), .. })),
            "{engine:?}: {fatal:?}"
        );
        assert!(store.close().is_err(), "{engine:?}");
        assert!(!marker_file(&place).exists(), "{engine:?}");
        place.faults.revive();
        let store = place.reopen(engine);
        assert!(!store.stopped_cleanly(), "{engine:?}");
        assert_eq!(store.fatal(), None, "{engine:?}");
        store.close().unwrap();
    }
}

/// `org.snapshot` on the golden creation: generation 1, the creator as the owner, and the
/// HR lead an active agent placed on the HQ office with its profile populated.
#[test]
fn the_snapshot_of_the_golden_creation() {
    for engine in ENGINES {
        let place = Place::new();
        let store = place.create(engine);
        create_organization(&store, 1).unwrap();
        let read = DirectorySnapshot { owner: id(CREATOR) };
        let (snapshot, _) = store.read(read).unwrap();
        assert_eq!(snapshot.position, position(1), "{engine:?}");
        assert_eq!((snapshot.position.epoch, snapshot.position.seq), (1, 1));
        assert_eq!(snapshot.teams.len(), 1, "{engine:?}");
        let team = &snapshot.teams[0];
        assert_eq!(team.id, id(TOP_TEAM).as_str());
        assert_eq!(team.name, "Example");
        assert_eq!(team.owner_team_id, None);
        assert_eq!(team.lead_member_id.as_deref(), Some(id(CREATOR).as_str()));
        assert_eq!(team.version, 1);

        assert_eq!(snapshot.members.len(), 2, "{engine:?}");
        let member = |n: u64| {
            let found = snapshot.members.iter().find(|m| m.id == id(n).as_str());
            found.expect("the member").clone()
        };
        let creator = member(CREATOR);
        assert_eq!(
            (creator.kind, creator.name.as_str()),
            (MemberKind::Person, "Ana")
        );
        assert!(creator.owner);
        assert_eq!(
            creator.created_at.as_deref(),
            Some("2023-11-14T22:13:20.000Z")
        );
        let hr = member(HR);
        assert_eq!((hr.kind, hr.name.as_str()), (MemberKind::Agent, "HR"));
        assert!(!hr.owner);
        assert_eq!(hr.office_id.as_deref(), Some(id(0xa3).as_str()));
        let hr_agent = hr.agent.as_ref().expect("agent profile populated");
        assert_eq!(hr_agent.messaging_name, "hr");
        assert_eq!(hr_agent.status, org_proto::org::AgentStatus::Active);
        assert_eq!(hr.reports_to_member_id, None);

        // Each employee belongs to the top team as its home team.
        let home = id(TOP_TEAM).to_string();
        assert_eq!(creator.home_team_id.as_deref(), Some(home.as_str()));
        assert_eq!(hr.home_team_id.as_deref(), Some(home.as_str()));
        assert!(snapshot.projects.is_empty() && snapshot.project_teams.is_empty());

        // Another owner is told apart by the ID alone.
        let (other, _) = store.read(DirectorySnapshot { owner: id(HR) }).unwrap();
        let owners: Vec<_> = other.members.iter().filter(|m| m.owner).collect();
        assert_eq!(owners.len(), 1);
        assert_eq!(owners[0].id, id(HR).as_str());
        store.close().unwrap();
    }
}

// ---- team.create through the native protocol's mapping ----

/// A canonical lowercase UUIDv7 command ID numbered `n`.
fn command_id(n: u64) -> String {
    id(0x1000 + n).to_string()
}

fn params(command: u64, team: u64, parent: Option<u64>) -> TeamCreateParams {
    TeamCreateParams {
        command_id: command_id(command),
        team_id: id(team).to_string(),
        name: format!("Team {team:x}"),
        owner_team_id: parent.map(|parent| id(parent).to_string()),
    }
}

/// `team.create` from `member`, as the office will answer it.
fn wire_create(
    store: &OfficeStore,
    member: &str,
    params: &TeamCreateParams,
) -> Result<CommandAccepted, ApiError> {
    let run = |request: wire::TeamCreateRequest| {
        store.run(CreateTeamOp {
            operation_id: request.operation_id,
            here: identity(),
            owner: id(CREATOR),
            by: request.by,
            command: request.command,
        })
    };
    wire::team_create(member, params, run).expect("the store answers")
}

fn code(answer: Result<CommandAccepted, ApiError>) -> ErrorCode {
    answer.expect_err("a refusal").code
}

/// Any files, a generation taken: what B2's operations will write (an archived team, a
/// person in a team, a lead), written directly for the authorization cases.
struct Write {
    op: u64,
    change: Change,
}

impl StoreOp for Write {
    type Out = Result<Reply, StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let operation = Operation {
            id: id(self.op),
            kind: "test.write",
            actor: Actor::System,
            fingerprint: Sha256::of(&self.op.to_be_bytes()),
        };
        let change = self.change;
        store.submit(Some(operation), false, move |uow| {
            uow.next(Stream::Organization)?;
            Ok(change.clone())
        })
    }
}

fn write(store: &OfficeStore, op: u64, change: Change) {
    let reply = store.run(Write { op, change }).expect("written");
    assert_eq!(reply.outcome, Outcome::Completed);
}

/// Rewrites team `team` (under `chain`) with `edit`, one revision on.
fn edit_team(store: &OfficeStore, op: u64, chain: &[u64], edit: impl FnOnce(&mut records::Team)) {
    let chain: Vec<Id> = chain.iter().map(|n| id(*n)).collect();
    let team = chain.last().unwrap().clone();
    let read = directory(store);
    let mut record = read.teams.into_iter().find(|t| t.id == team).unwrap();
    edit(&mut record);
    record.meta.revision += 1;
    let path = records::team_path(&chain).unwrap();
    write(
        store,
        op,
        Change::new(Op::CreateTeam.ack()).put(path, records::encode_team(&record)),
    );
}

/// Puts a person `person` whose home team is the last of `chain`.
fn put_person(store: &OfficeStore, op: u64, chain: &[u64], person: u64, status: EmployeeStatus) {
    let chain: Vec<Id> = chain.iter().map(|n| id(*n)).collect();
    let meta = Meta::created(
        &By::Employee(id(CREATOR)),
        crate::organization::testkit::CREATION_AT,
    );
    let record = records::Employee {
        id: id(person),
        handle: format!("person{person:x}"),
        kind: EmployeeKind::Person,
        name: format!("Person {person:x}"),
        home_team_id: chain.last().unwrap().clone(),
        role_id: None,
        reports_to_employee_id: None,
        status,
        contact: None,
        title: None,
        focus: None,
        role_text: None,
        title_in_team: None,
        offboarded_at: None,
        may_hire: true,
        builtin_key: None,
        meta,
        body: String::new(),
    };
    let path = records::employee_path(&chain, EmployeeKind::Person, &id(person)).unwrap();
    let change = Change::new(Op::CreateTeam.ack()).put(path, records::encode_employee(&record));
    write(store, op, change);
}

#[test]
fn team_create_answers_committed_and_a_retry_gets_the_same_answer() {
    for engine in ENGINES {
        let place = Place::new();
        let store = place.create(engine);
        create_organization(&store, 1).unwrap();
        let creator = id(CREATOR).to_string();
        let request = params(1, 0x21, Some(TOP_TEAM));
        let answer = wire_create(&store, &creator, &request);
        let committed = CommandAccepted {
            status: CommandStatus::Committed,
            position: Some(position(2)),
        };
        assert_eq!(answer, Ok(committed), "{engine:?}");
        // The same command again: the same answer, and nothing new.
        assert_eq!(wire_create(&store, &creator, &request), Ok(committed));
        assert_eq!(store.read(DirectoryGeneration).unwrap().0, 2, "{engine:?}");
        // Another payload with that command ID: operation_reused only.
        let mut other = params(1, 0x22, Some(TOP_TEAM));
        assert_eq!(
            code(wire_create(&store, &creator, &other)),
            ErrorCode::OperationReused
        );
        other = params(1, 0x21, Some(TOP_TEAM));
        other.name = "Renamed".into();
        assert_eq!(
            code(wire_create(&store, &creator, &other)),
            ErrorCode::OperationReused
        );
        // The receipt uses the native UUIDv7 command ID directly, and the snapshot shows the team.
        let mapped = command_id(1);
        store.sync().unwrap();
        let receipts = tree(&place.durable().join("recovery/commands"));
        let names: Vec<&String> = receipts.keys().collect();
        assert!(
            receipts.contains_key(&format!("{mapped}.json")),
            "{names:?}"
        );
        let (snapshot, _) = store
            .read(DirectorySnapshot { owner: id(CREATOR) })
            .unwrap();
        let team = snapshot.teams.iter().find(|t| t.id == id(0x21).as_str());
        let team = team.expect("the new team");
        assert_eq!(team.owner_team_id.as_deref(), Some(id(TOP_TEAM).as_str()));
        assert_eq!(snapshot.position, position(2));

        // After a restart the retry still answers the original.
        store.close().unwrap();
        let store = place.reopen(engine);
        assert_eq!(wire_create(&store, &creator, &request), Ok(committed));
        store.close().unwrap();
    }
}

#[test]
fn team_create_refusals_carry_the_protocols_codes() {
    for engine in ENGINES {
        let place = Place::new();
        let store = place.create(engine);
        create_organization(&store, 1).unwrap();
        let creator = id(CREATOR).to_string();
        let ask = |member: &str, request: TeamCreateParams| wire_create(&store, member, &request);

        assert_eq!(
            code(ask(&creator, params(2, 0x21, Some(0x99)))),
            ErrorCode::NotFound
        );
        let mut bad_id = params(3, 0x21, Some(TOP_TEAM));
        bad_id.command_id = "01arz3ndektsv4rrffq69g5fav".into();
        assert_eq!(code(ask(&creator, bad_id)), ErrorCode::InvalidRequest);
        let mut ulid_team = params(4, 0x21, Some(TOP_TEAM));
        ulid_team.team_id = "01ARZ3NDEKTSV4RRFFQ69G5FAV".into();
        assert_eq!(code(ask(&creator, ulid_team)), ErrorCode::InvalidRequest);
        let mut unnamed = params(5, 0x21, Some(TOP_TEAM));
        unnamed.name = " padded ".into();
        assert_eq!(code(ask(&creator, unnamed)), ErrorCode::InvalidName);
        // The ID of another record.
        let taken = params(6, HR, Some(TOP_TEAM));
        assert_eq!(code(ask(&creator, taken)), ErrorCode::AlreadyExists);
        // Nobody on record; an agent.
        let hr = id(HR).to_string();
        let stranger = id(0x77).to_string();
        for member in [hr.as_str(), stranger.as_str(), "01ARZ3NDEKTSV4RRFFQ69G5FAV"] {
            let answer = ask(member, params(7, 0x21, Some(TOP_TEAM)));
            assert_eq!(code(answer), ErrorCode::NotAuthorized, "{member}");
        }
        // None of these recorded anything or took a generation.
        assert_eq!(store.read(DirectoryGeneration).unwrap().0, 1, "{engine:?}");
        assert_eq!(directory(&store).teams.len(), 1);

        // A top-level team commits cleanly with no parent.
        let top = ask(&creator, params(1, 0x20, None)).expect("top-level team");
        assert_eq!(top.status, org_proto::org::CommandStatus::Committed);
        assert_eq!(directory(&store).teams.len(), 2);

        // An archived parent is the wrong state: a recorded rejection, invalid_request
        // (the wire contract's code for it), and the same on a retry.
        assert!(ask(&creator, params(8, 0x21, Some(TOP_TEAM))).is_ok());
        edit_team(&store, 0x900, &[TOP_TEAM, 0x21], |team| {
            team.status = TeamStatus::Archived;
        });
        let generation = store.read(DirectoryGeneration).unwrap().0;
        let archived = params(9, 0x22, Some(0x21));
        let first = ask(&creator, archived.clone()).expect_err("refused");
        assert_eq!(first.code, ErrorCode::InvalidRequest);
        assert_eq!(ask(&creator, archived), Err(first));
        assert_eq!(store.read(DirectoryGeneration).unwrap().0, generation);
        store.close().unwrap();
    }
}

/// Only the organization's owner creates a team (document 04, "Who may change what"): a
/// person who leads the parent team, or a team above it, does not; nor does anyone else.
/// Each refusal is unrecorded and takes no generation.
#[test]
fn only_the_owner_creates_a_team_even_under_a_team_another_person_leads() {
    for engine in ENGINES {
        let place = Place::new();
        let store = place.create(engine);
        create_organization(&store, 1).unwrap();
        let creator = id(CREATOR).to_string();
        let ask = |member: &str, request: TeamCreateParams| wire_create(&store, member, &request);
        assert!(ask(&creator, params(1, 0x21, Some(TOP_TEAM))).is_ok());
        // A person in team 0x21, then made its lead.
        put_person(
            &store,
            0x901,
            &[TOP_TEAM, 0x21],
            0x31,
            EmployeeStatus::Active,
        );
        let person = id(0x31).to_string();
        let under_own = params(2, 0x22, Some(0x21));
        assert_eq!(
            code(ask(&person, under_own.clone())),
            ErrorCode::NotAuthorized
        );
        edit_team(&store, 0x902, &[TOP_TEAM, 0x21], |team| {
            team.lead_employee_id = Some(id(0x31));
        });
        let generation = store.read(DirectoryGeneration).unwrap().0;
        // Leading the parent grants nothing: not under it, not above it.
        assert_eq!(
            code(ask(&person, under_own.clone())),
            ErrorCode::NotAuthorized,
            "{engine:?}"
        );
        let above = params(3, 0x23, Some(TOP_TEAM));
        assert_eq!(code(ask(&person, above)), ErrorCode::NotAuthorized);
        // The built-in HR lead is an agent: refused too.
        let hr = id(HR).to_string();
        assert_eq!(
            code(ask(&hr, params(4, 0x24, Some(0x21)))),
            ErrorCode::NotAuthorized
        );
        assert_eq!(store.read(DirectoryGeneration).unwrap().0, generation);
        assert_eq!(directory(&store).teams.len(), 2, "{engine:?}");
        // The owner creates under the team the person leads, with the refused command's ID:
        // nothing was recorded for it.
        assert!(ask(&creator, under_own).is_ok(), "{engine:?}");
        assert_eq!(store.read(DirectoryGeneration).unwrap().0, generation + 1);
        store.close().unwrap();
    }
}

/// Something at the marker path that cannot be read as a marker means "not clean", and
/// the opening removes it without following it.
#[test]
fn an_unreadable_marker_is_not_clean_and_is_removed_without_being_followed() {
    // An empty folder: on both engines, the memory one rebuilding past it.
    for engine in ENGINES {
        let place = Place::new();
        let store = place.create(engine);
        create_organization(&store, 1).unwrap();
        store.close().unwrap();
        std::fs::remove_file(marker_file(&place)).unwrap();
        std::fs::create_dir(marker_file(&place)).unwrap();
        let store = place.reopen(engine);
        assert!(!store.stopped_cleanly(), "{engine:?}");
        assert!(!marker_file(&place).exists(), "{engine:?}");
        store.close().unwrap();
    }

    // A link and a file over the size limit, on SQLite, whose projection needs no rebuild
    // (a rebuild refuses both as it refuses any such object under `durable/`).
    let place = Place::new();
    let store = place.create(EngineChoice::Sqlite);
    create_organization(&store, 1).unwrap();
    store.close().unwrap();
    let target = place.root.join("elsewhere");
    let real = "{\"journal_incarnation\":\"x\",\"head_ordinal\":1}\n";
    std::fs::write(&target, real).unwrap();
    std::fs::remove_file(marker_file(&place)).unwrap();
    std::os::unix::fs::symlink(&target, marker_file(&place)).unwrap();
    let store = place.reopen(EngineChoice::Sqlite);
    assert!(!store.stopped_cleanly());
    assert!(std::fs::symlink_metadata(marker_file(&place)).is_err());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), real);
    store.close().unwrap();

    let oversize = std::fs::File::create(marker_file(&place)).unwrap();
    oversize.set_len(org_store::MAX_FILE_BYTES + 1).unwrap();
    drop(oversize);
    let store = place.reopen(EngineChoice::Sqlite);
    assert!(!store.stopped_cleanly());
    assert!(!marker_file(&place).exists());
    store.close().unwrap();

    // A folder with something in it is never emptied: the opening refuses and says why.
    std::fs::remove_file(marker_file(&place)).unwrap();
    std::fs::create_dir(marker_file(&place)).unwrap();
    std::fs::write(marker_file(&place).join("kept"), b"x").unwrap();
    let refused = open_store(&place.config(EngineChoice::Sqlite, Opening::Existing));
    let error = refused.err().expect("refused").to_string();
    assert!(error.contains("clean-stop marker"), "{error}");
    assert!(marker_file(&place).join("kept").is_file());
}
