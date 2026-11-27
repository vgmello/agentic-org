//! The built-in Chief of Staff at the office (`TASK-80.1`): provisioned with the organization,
//! bootstrapped once into an older organization (collision-safe, idempotent across a deleted
//! and rebuilt projection), never offboarded, and hiring into any team only while the owner
//! leaves her `may_hire` on, through the normal hire flow.

use org_proto::hiring::{
    AgentMove, AgentMoveParams, AgentOffboard, AgentOffboardParams, HireProposalWire, HrHire,
    HrHireParams, MemberSetMayHire, MemberSetMayHireParams, TeamSetMaxMembers,
    TeamSetMaxMembersParams,
};
use org_proto::org::{MemberAddPersonParams, TeamCreateParams};
use org_proto::{ApiError, Call, CallError, ErrorCode, Request};
use org_store::{testkit::identity, Ack, Change, Id, Stream};

use super::{Site, CREATOR, ENGINES, HR, TEAM_A, TOP_TEAM};
use crate::office::Office;
use crate::organization::{
    chief_of_staff::{chief_of_staff_id, CHIEF_OF_STAFF_CHARTER},
    features::HIRE_DENIED,
    records::{self, BuiltinKey, EmployeeKind, EmployeeStatus},
};
use crate::rules::OrgCommand;
use crate::store::open::{OfficeEngine, StoreOp};

const AGENT_1: &str = "01900000-0000-7000-8000-000000000231";
const AGENT_2: &str = "01900000-0000-7000-8000-000000000232";
const AGENT_3: &str = "01900000-0000-7000-8000-000000000233";
const REQ_1: &str = "01900000-0000-7000-8000-000000000221";
const REQ_2: &str = "01900000-0000-7000-8000-000000000222";
const REQ_3: &str = "01900000-0000-7000-8000-000000000223";
const PERSON_TESS: &str = "01900000-0000-7000-8000-000000000241";

fn cmd_id(n: u32) -> String {
    let ms = crate::organization::testkit::CREATION_AT.0;
    format!(
        "{:08x}-{:04x}-7000-8000-00000002{:04x}",
        (ms >> 16) as u32,
        (ms & 0xffff) as u16,
        n
    )
}

fn tess_id() -> String {
    chief_of_staff_id(&identity().organization_id, 0).to_string()
}

fn call_c2<C: Call>(
    office: &Office,
    actor: &str,
    params: &C::Params,
) -> Result<C::Result, ApiError> {
    let req = Request::new::<C>(1, params).expect("encode request");
    let resp = office
        .dispatch_hiring_rpc(Some(actor), &req)
        .expect("method handled by dispatch_hiring_rpc");
    match resp.into_result::<C>() {
        Ok(v) => Ok(v),
        Err(CallError::Api(err)) => Err(err),
        Err(CallError::Decode(e)) => panic!("decode result: {e}"),
    }
}

fn org_write(office: &Office, actor: &str, cmd: OrgCommand) {
    let reply = office.submit_org(actor, cmd).expect("store ok");
    assert!(reply.is_ok(), "{reply:?}");
}

fn hire(office: &Office, actor: &str, n: u32, ids: (&str, &str), team: &str) -> ApiError {
    let (request_id, employee_id) = ids;
    let params = HrHireParams {
        command_id: cmd_id(n),
        request_id: request_id.into(),
        employee_id: employee_id.into(),
        home_team_id: team.into(),
        role_id: None,
        proposal: HireProposalWire {
            name: format!("Bot {n}"),
            handle: format!("bot-{n}"),
            ..HireProposalWire::default()
        },
        reason: "Tess staffs the team".into(),
    };
    match call_c2::<HrHire>(office, actor, &params) {
        Ok(_) => ApiError::new(ErrorCode::Unavailable, "hired"),
        Err(err) => err,
    }
}

fn hired(err: &ApiError) -> bool {
    err.code == ErrorCode::Unavailable && err.message == "hired"
}

/// Removes Tess's profile as an older build's creation never wrote it: the organization as it
/// was before `TASK-80.1`.
struct ForgetTess;

impl StoreOp for ForgetTess {
    type Out = ();

    fn run<E: OfficeEngine>(self, store: &org_store::Store<E>) {
        let tess = Id::parse(&tess_id()).expect("tess id");
        let chain = [Id::parse(TOP_TEAM).expect("team")];
        let path =
            records::employee_path(&chain, EmployeeKind::Agent, &tess).expect("profile path");
        store
            .submit(None, true, move |uow| {
                uow.next(Stream::Organization)?;
                Ok(Change::new(Ack::Durable)
                    .remove(path.clone())
                    .reply(b"{}".to_vec()))
            })
            .expect("tess forgotten");
    }
}

#[test]
fn a_new_organization_has_tess_beside_hr_reporting_to_the_owner() {
    for engine in ENGINES {
        let site = Site::new();
        let office = site.provision(engine).unwrap();
        let tess = office.chief_of_staff().unwrap().expect("Tess");
        assert_eq!(tess.id.as_str(), tess_id(), "{engine:?}");
        assert_eq!((tess.name.as_str(), tess.handle.as_str()), ("Tess", "tess"));
        assert_eq!(tess.title.as_deref(), Some("Chief of Staff"));
        assert_eq!(tess.builtin_key, Some(BuiltinKey::ChiefOfStaff));
        assert_eq!(
            (tess.kind, tess.status),
            (EmployeeKind::Agent, EmployeeStatus::Active)
        );
        assert!(tess.may_hire);
        assert_eq!(tess.home_team_id.as_str(), TOP_TEAM);
        // Reporting to the owner is written as reporting to no one: she follows a handover.
        assert_eq!(tess.reports_to_employee_id, None);
        assert_eq!(tess.body.trim(), CHIEF_OF_STAFF_CHARTER);
        assert!(office.is_chief_of_staff(&tess_id()));
        assert!(!office.is_chief_of_staff(HR));
        // No hire request is behind her, and the bootstrap has nothing to do.
        assert_eq!(office.ensure_chief_of_staff().unwrap(), None);
        let hr = office.load_employee(HR).unwrap().expect("HR");
        assert_eq!(hr.builtin_key, Some(BuiltinKey::Hr));
        assert!(!hr.may_hire);
        // She takes no slot of the team: the creator and the HR lead are its two members.
        let top = org_store::Id::parse(TOP_TEAM).unwrap();
        let count = office
            .store
            .read(TeamCount(top))
            .map(|(count, _)| count)
            .unwrap();
        assert_eq!(count, 2, "{engine:?}");
    }
}

/// The active member count `max_members` is held to.
struct TeamCount(org_store::Id);

impl crate::store::open::ReadOp for TeamCount {
    type Out = u64;

    fn read<S: crate::tasks::ports::TasksRead>(
        self,
        read: &S,
    ) -> Result<u64, org_store::StoreError> {
        crate::organization::features::authority::team_active_member_count(read, &self.0)
    }
}

#[test]
fn an_older_organization_gets_tess_once_and_keeps_her_across_a_rebuild() {
    for engine in ENGINES {
        let site = Site::new();
        let office = site.provision(engine).unwrap();
        office.store.run(ForgetTess);
        assert_eq!(office.chief_of_staff().unwrap(), None, "{engine:?}");

        let ensured = office
            .ensure_chief_of_staff()
            .unwrap()
            .expect("provisioned");
        assert_eq!(ensured.employee_id.as_str(), tess_id());
        assert_eq!(ensured.handle, "tess");
        let tess = office.chief_of_staff().unwrap().expect("Tess");
        assert_eq!(tess.meta.created_by, records::By::System);
        // Reporting to the owner is written as reporting to no one: she follows a handover.
        assert_eq!(tess.reports_to_employee_id, None);
        assert!(tess.may_hire);
        // Idempotent: the second run writes nothing.
        assert_eq!(office.ensure_chief_of_staff().unwrap(), None);
        let count = |office: &Office| {
            office
                .list_employees()
                .unwrap()
                .iter()
                .filter(|e| e.builtin_key == Some(BuiltinKey::ChiefOfStaff))
                .count()
        };
        assert_eq!(count(&office), 1);

        // Delete the projection and rebuild it from the durable files: still one Tess, and the
        // open-time bootstrap adds nobody.
        office.close().unwrap();
        // The memory engine keeps no projection on disk, and always rebuilds.
        let _ = std::fs::remove_dir_all(site.runtime_folder());
        let office = site.open(engine).unwrap();
        assert_eq!(count(&office), 1, "{engine:?}");
        assert_eq!(office.chief_of_staff().unwrap(), Some(tess));
        office.close().unwrap();
    }
}

#[test]
fn the_bootstrap_takes_another_handle_when_a_person_holds_tess() {
    for engine in ENGINES {
        let site = Site::new();
        let office = site.provision(engine).unwrap();
        office.store.run(ForgetTess);
        org_write(
            &office,
            CREATOR,
            OrgCommand::MemberAddPerson(MemberAddPersonParams {
                command_id: cmd_id(1),
                member_id: PERSON_TESS.into(),
                name: "Tess Human".into(),
                handle: Some("tess".into()),
                home_team_id: Some(TOP_TEAM.into()),
                contact: None,
            }),
        );
        let ensured = office
            .ensure_chief_of_staff()
            .unwrap()
            .expect("provisioned");
        assert_eq!(ensured.handle, "chief-of-staff", "{engine:?}");
        let person = office.load_employee(PERSON_TESS).unwrap().expect("person");
        assert_eq!((person.handle.as_str(), person.builtin_key), ("tess", None));
        // The open-time bootstrap of a reopened office is a no-op too.
        office.close().unwrap();
        let office = site.open(engine).unwrap();
        let tess = office.chief_of_staff().unwrap().expect("Tess");
        assert_eq!(tess.handle, "chief-of-staff");
        office.close().unwrap();
    }
}

#[test]
fn tess_is_never_offboarded_or_moved_and_never_sets_her_own_hiring() {
    for engine in ENGINES {
        let site = Site::new();
        let office = site.provision(engine).unwrap();
        let tess = tess_id();
        let offboard = call_c2::<AgentOffboard>(
            &office,
            CREATOR,
            &AgentOffboardParams {
                command_id: cmd_id(10),
                member_id: tess.clone(),
                expected_version: 1,
            },
        )
        .unwrap_err();
        assert_eq!(offboard.code, ErrorCode::InvalidRequest, "{engine:?}");
        assert!(offboard
            .message
            .contains("the built-in Chief of Staff cannot be offboarded"));

        org_write(
            &office,
            CREATOR,
            OrgCommand::TeamCreate(TeamCreateParams {
                command_id: cmd_id(11),
                team_id: TEAM_A.into(),
                name: "Platform".into(),
                owner_team_id: Some(TOP_TEAM.into()),
            }),
        );
        let moved = call_c2::<AgentMove>(
            &office,
            CREATOR,
            &AgentMoveParams {
                command_id: cmd_id(12),
                member_id: tess.clone(),
                expected_version: 1,
                to_team_id: TEAM_A.into(),
            },
        )
        .unwrap_err();
        assert!(moved
            .message
            .contains("Chief of Staff stays in the top-level team"));

        // She never sets her own may_hire (an agent sets nobody's).
        let own = call_c2::<MemberSetMayHire>(
            &office,
            &tess,
            &MemberSetMayHireParams {
                command_id: cmd_id(13),
                member_id: tess.clone(),
                expected_version: 1,
                may_hire: true,
            },
        )
        .unwrap_err();
        assert_eq!(own.code, ErrorCode::NotAuthorized);
        // The HR lead's stays fixed.
        let hr = call_c2::<MemberSetMayHire>(
            &office,
            CREATOR,
            &MemberSetMayHireParams {
                command_id: cmd_id(14),
                member_id: HR.into(),
                expected_version: 1,
                may_hire: true,
            },
        )
        .unwrap_err();
        assert!(hr
            .message
            .contains("the built-in HR lead's hiring permission"));
        assert_eq!(
            office.chief_of_staff().unwrap().unwrap().status,
            EmployeeStatus::Active
        );
    }
}

#[test]
fn tess_hires_into_any_team_only_while_the_owner_leaves_may_hire_on() {
    for engine in ENGINES {
        let site = Site::new();
        let office = site.provision(engine).unwrap();
        let tess = tess_id();
        org_write(
            &office,
            CREATOR,
            OrgCommand::TeamCreate(TeamCreateParams {
                command_id: cmd_id(20),
                team_id: TEAM_A.into(),
                name: "Platform".into(),
                owner_team_id: Some(TOP_TEAM.into()),
            }),
        );
        // A sub-team she does not lead: org-wide authority, through the normal hire flow.
        assert!(
            hired(&hire(&office, &tess, 21, (REQ_1, AGENT_1), TEAM_A)),
            "{engine:?}"
        );
        let agent = office.load_employee(AGENT_1).unwrap().expect("hired");
        assert_eq!(agent.home_team_id.as_str(), TEAM_A);
        assert!(!agent.may_hire, "a new agent never starts with may_hire");

        // An ordinary agent with may_hire but no team to lead stays bounded.
        call_c2::<MemberSetMayHire>(
            &office,
            CREATOR,
            &MemberSetMayHireParams {
                command_id: cmd_id(22),
                member_id: AGENT_1.into(),
                expected_version: 1,
                may_hire: true,
            },
        )
        .unwrap();
        let bounded = hire(&office, AGENT_1, 23, (REQ_2, AGENT_2), TEAM_A);
        assert_eq!(
            (bounded.code, bounded.message.as_str()),
            (ErrorCode::NotAuthorized, HIRE_DENIED)
        );

        // `max_members` still holds for her.
        call_c2::<TeamSetMaxMembers>(
            &office,
            CREATOR,
            &TeamSetMaxMembersParams {
                command_id: cmd_id(24),
                team_id: TEAM_A.into(),
                expected_version: 1,
                max_members: 1,
            },
        )
        .unwrap();
        let full = hire(&office, &tess, 25, (REQ_2, AGENT_2), TEAM_A);
        assert_eq!(full.code, ErrorCode::InvalidRequest, "{full:?}");
        assert!(full.message.contains("max_members"));

        // The owner revokes her hiring power: she hires nowhere.
        call_c2::<MemberSetMayHire>(
            &office,
            CREATOR,
            &MemberSetMayHireParams {
                command_id: cmd_id(26),
                member_id: tess.clone(),
                expected_version: 1,
                may_hire: false,
            },
        )
        .unwrap();
        let revoked = hire(&office, &tess, 27, (REQ_3, AGENT_3), TOP_TEAM);
        assert_eq!(
            (revoked.code, revoked.message.as_str()),
            (ErrorCode::NotAuthorized, HIRE_DENIED)
        );
        // And restores it.
        call_c2::<MemberSetMayHire>(
            &office,
            CREATOR,
            &MemberSetMayHireParams {
                command_id: cmd_id(28),
                member_id: tess.clone(),
                expected_version: 2,
                may_hire: true,
            },
        )
        .unwrap();
        assert!(hired(&hire(&office, &tess, 29, (REQ_3, AGENT_3), TOP_TEAM)));
    }
}

#[test]
fn caller_authority_is_read_live() {
    let site = Site::new();
    let office = site.provision(org_store_engine()).unwrap();
    let owner = office.caller_authority(CREATOR).unwrap();
    assert!(owner.active && owner.is_person && owner.is_owner && owner.may_hire);
    let tess = office.caller_authority(&tess_id()).unwrap();
    assert!(tess.active && !tess.is_person && !tess.is_owner);
    let unknown = office.caller_authority("not-an-id").unwrap();
    assert_eq!(unknown, crate::office::CallerAuthority::default());
    let hr = office.caller_authority(HR).unwrap();
    assert!(!hr.may_hire && !hr.is_owner);
}

fn org_store_engine() -> crate::store::open::EngineChoice {
    crate::store::open::EngineChoice::Memory
}
