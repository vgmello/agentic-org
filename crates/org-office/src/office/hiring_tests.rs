//! Dual-engine (`Sqlite` + `Memory`) office-level tests for `TASK-41` (`C2`: HR hiring,
//! role catalog, `may_hire` / `max_members` enforcement, HR message guard, agent lifecycle,
//! private home provisioning, and projection rebuild equivalence).

use std::os::unix::fs::PermissionsExt;

use org_proto::hiring::{
    AgentExecutionGet, AgentExecutionGetParams, AgentExecutionRecord, AgentExecutionSet,
    AgentExecutionSetParams, AgentMove, AgentMoveParams, AgentOffboard, AgentOffboardParams,
    AgentSetStatus, AgentSetStatusParams, AgentUpdate, AgentUpdateParams, EntityOpResult,
    HireOpResult, HireProposalWire, HireRequestRecord, HrHire, HrHireConfirm, HrHireConfirmParams,
    HrHireDraft, HrHireDraftParams, HrHireInspect, HrHireInspectParams, HrHireList,
    HrHireListParams, HrHireListResult, HrHireParams, HrHireWithdraw, HrHireWithdrawParams,
    MemberSetMayHire, MemberSetMayHireParams, ModelProfileList, ModelProfileListParams,
    ModelProfileListResult, ProviderConnectionCreate, ProviderConnectionCreateParams, RoleCreate,
    RoleCreateParams, RoleInspect, RoleInspectParams, RoleList, RoleListParams, RoleListResult,
    RoleRecord, RoleUpdate, RoleUpdateParams, TeamSetMaxMembers, TeamSetMaxMembersParams,
};
use org_proto::org::{
    MemberAddPersonParams, OrgSnapshot, TeamCreateParams, TeamMoveParams, TeamSetLeadParams,
};
use org_proto::{ApiError, Call, CallError, ErrorCode, Request};

use super::{Site, CREATOR, ENGINES, HR, TEAM_A, TEAM_B, TOP_TEAM};
use crate::office::Office;
use crate::organization::features::HIRE_DENIED;
use crate::rules::OrgCommand;

const ROLE_BE: &str = "01900000-0000-7000-8000-000000000101";
const CONN_1: &str = "01900000-0000-7000-8000-000000000111";
const PROF_1: &str = "01900000-0000-7000-8000-000000000112";
const REQ_1: &str = "01900000-0000-7000-8000-000000000121";
const REQ_2: &str = "01900000-0000-7000-8000-000000000122";
const REQ_3: &str = "01900000-0000-7000-8000-000000000123";
const AGENT_1: &str = "01900000-0000-7000-8000-000000000131";
const AGENT_2: &str = "01900000-0000-7000-8000-000000000132";
const AGENT_3: &str = "01900000-0000-7000-8000-000000000133";
const PERSON_ADA: &str = "01900000-0000-7000-8000-000000000141";

fn cmd_id(n: u32) -> String {
    let ms = crate::organization::testkit::CREATION_AT.0;
    format!(
        "{:08x}-{:04x}-7000-8000-00000001{:04x}",
        (ms >> 16) as u32,
        (ms & 0xffff) as u16,
        n
    )
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

fn snapshot(office: &Office) -> OrgSnapshot {
    office.org_snapshot().expect("snapshot")
}

fn org_write(office: &Office, actor: &str, cmd: OrgCommand) {
    let reply = office.submit_org(actor, cmd).expect("store ok");
    assert!(reply.is_ok(), "{reply:?}");
}

fn base_proposal(name: &str, handle: &str) -> HireProposalWire {
    HireProposalWire {
        name: name.into(),
        handle: handle.into(),
        title: Some("Engineer".into()),
        focus: Some("Backend".into()),
        model_profile_id: Some(PROF_1.into()),
        model_id: Some("claude-sonnet-4-6".into()),
        permission_preset: Some("workspace_edits".into()),
        tools_config: vec!["read".into(), "edit".into()],
        system_prompt_markdown: Some("Be helpful and concise.".into()),
        ..HireProposalWire::default()
    }
}

#[test]
fn c2_roles_provider_two_step_hire_fail_retry_and_private_home() {
    for engine in ENGINES {
        let site = Site::new();
        let office = site.provision(engine).unwrap();

        // 1. Role catalog create, update, list, inspect.
        let created: EntityOpResult = call_c2::<RoleCreate>(
            &office,
            CREATOR,
            &RoleCreateParams {
                command_id: cmd_id(1),
                role_id: ROLE_BE.into(),
                key: Some("backend-engineer".into()),
                name: "Backend Engineer".into(),
                body: "Builds Rust services.".into(),
            },
        )
        .unwrap();
        assert_eq!(created.revision, 1);

        let updated: EntityOpResult = call_c2::<RoleUpdate>(
            &office,
            CREATOR,
            &RoleUpdateParams {
                command_id: cmd_id(2),
                role_id: ROLE_BE.into(),
                expected_version: 1,
                key: None,
                clear_key: false,
                name: Some("Senior Backend Engineer".into()),
                body: Some("Builds resilient Rust services.".into()),
            },
        )
        .unwrap();
        assert_eq!(updated.revision, 2);

        let roles: RoleListResult =
            call_c2::<RoleList>(&office, CREATOR, &RoleListParams {}).unwrap();
        assert_eq!(roles.roles.len(), 1);
        let inspected: RoleRecord = call_c2::<RoleInspect>(
            &office,
            CREATOR,
            &RoleInspectParams {
                role: "backend-engineer".into(),
            },
        )
        .unwrap();
        assert_eq!(inspected.name, "Senior Backend Engineer");
        assert_eq!(inspected.created_by_kind.as_deref(), Some("employee"));
        assert_eq!(inspected.created_by_id.as_deref(), Some(CREATOR));
        assert_eq!(inspected.updated_by_kind.as_deref(), Some("employee"));
        assert_eq!(inspected.updated_by_id.as_deref(), Some(CREATOR));

        // 2. Provider connection atomically creates an auto-generated default model profile.
        call_c2::<ProviderConnectionCreate>(
            &office,
            CREATOR,
            &ProviderConnectionCreateParams {
                command_id: cmd_id(3),
                connection_id: CONN_1.into(),
                default_profile_id: PROF_1.into(),
                key: "claude-prod".into(),
                name: "Claude Prod".into(),
                provider_type: "anthropic".into(),
                auth_mode: "api_key".into(),
                subscription_identity: None,
                runtime_driver: "claude-code".into(),
                endpoint_url: None,
                max_concurrent_turns: 2,
            },
        )
        .unwrap();
        let profiles: ModelProfileListResult =
            call_c2::<ModelProfileList>(&office, CREATOR, &ModelProfileListParams {}).unwrap();
        assert_eq!(profiles.profiles.len(), 1);
        assert_eq!(profiles.profiles[0].id, PROF_1);
        assert!(profiles.profiles[0].is_auto_generated);

        // 3. DraftHire creates ONLY hires/<id>.yaml (zero employee, zero private home).
        let drafted: HireOpResult = call_c2::<HrHireDraft>(
            &office,
            CREATOR,
            &HrHireDraftParams {
                command_id: cmd_id(4),
                request_id: REQ_1.into(),
                home_team_id: TOP_TEAM.into(),
                role_id: Some(ROLE_BE.into()),
                proposal: base_proposal("Turing", "turing"),
                reason: "Need backend engineer".into(),
            },
        )
        .unwrap();
        assert_eq!(drafted.state, "drafting");
        assert!(drafted.employee_id.is_none());
        assert!(!office.agent_private_home_path(AGENT_1).exists());
        assert!(snapshot(&office).members.iter().all(|m| m.id != AGENT_1));

        // 4. ConfirmHire with simulated failure keeps state = hiring + failed_step; zero agent & zero home.
        let failed: HireOpResult = call_c2::<HrHireConfirm>(
            &office,
            CREATOR,
            &HrHireConfirmParams {
                command_id: cmd_id(5),
                request_id: REQ_1.into(),
                employee_id: AGENT_1.into(),
                expected_version: 1,
                fail_at_step: Some("provision_home".into()),
            },
        )
        .unwrap();
        assert_eq!(failed.state, "hiring");
        assert_eq!(failed.failed_step.as_deref(), Some("provision_home"));
        assert!(!office.agent_private_home_path(AGENT_1).exists());
        assert!(snapshot(&office).members.iter().all(|m| m.id != AGENT_1));

        // 5. Retry ConfirmHire ("Try again") transitions to Hired, creates execution & 0700 home.
        let hired: HireOpResult = call_c2::<HrHireConfirm>(
            &office,
            CREATOR,
            &HrHireConfirmParams {
                command_id: cmd_id(6),
                request_id: REQ_1.into(),
                employee_id: AGENT_1.into(),
                expected_version: 2,
                fail_at_step: None,
            },
        )
        .unwrap();
        assert_eq!(hired.state, "hired");
        assert_eq!(hired.employee_id.as_deref(), Some(AGENT_1));
        let hired_member = snapshot(&office)
            .members
            .into_iter()
            .find(|m| m.id == AGENT_1)
            .expect("hired agent in snapshot");
        assert_eq!(hired_member.role_id.as_deref(), Some(ROLE_BE));
        let home = office.agent_private_home_path(AGENT_1);
        assert!(home.is_dir(), "private home must exist after confirm");
        assert_eq!(
            std::fs::metadata(&home).unwrap().permissions().mode() & 0o777,
            0o700
        );

        let exec: AgentExecutionRecord = call_c2::<AgentExecutionGet>(
            &office,
            CREATOR,
            &AgentExecutionGetParams {
                employee_id: AGENT_1.into(),
            },
        )
        .unwrap();
        assert_eq!(exec.model_profile_id, PROF_1);
        assert_eq!(exec.model_id, "claude-sonnet-4-6");
        assert_eq!(exec.permission_preset, "workspace_edits");

        // 6. WithdrawHire works on drafting request and refuses already-hired request.
        call_c2::<HrHireDraft>(
            &office,
            CREATOR,
            &HrHireDraftParams {
                command_id: cmd_id(7),
                request_id: REQ_2.into(),
                home_team_id: TOP_TEAM.into(),
                role_id: None,
                proposal: base_proposal("Grace", "grace"),
                reason: "Extra capacity".into(),
            },
        )
        .unwrap();
        let withdrawn: HireOpResult = call_c2::<HrHireWithdraw>(
            &office,
            CREATOR,
            &HrHireWithdrawParams {
                command_id: cmd_id(8),
                request_id: REQ_2.into(),
                expected_version: 1,
                closed_reason: "No longer needed".into(),
            },
        )
        .unwrap();
        assert_eq!(withdrawn.state, "withdrawn");

        let bad_withdraw = call_c2::<HrHireWithdraw>(
            &office,
            CREATOR,
            &HrHireWithdrawParams {
                command_id: cmd_id(9),
                request_id: REQ_1.into(),
                expected_version: 3,
                closed_reason: "Too late".into(),
            },
        )
        .unwrap_err();
        assert_eq!(bad_withdraw.code, ErrorCode::InvalidRequest);

        // 7. Delete runtime/ and reopen: every role, hire, agent, and execution rebuilds.
        drop(office);
        let _ = std::fs::remove_dir_all(site.runtime_folder());
        let reopened = site.open(engine).unwrap();
        let listed: HrHireListResult =
            call_c2::<HrHireList>(&reopened, CREATOR, &HrHireListParams::default()).unwrap();
        assert_eq!(listed.hires.len(), 2);
        let req1: HireRequestRecord = call_c2::<HrHireInspect>(
            &reopened,
            CREATOR,
            &HrHireInspectParams {
                request_id: REQ_1.into(),
            },
        )
        .unwrap();
        assert_eq!(req1.state, "hired");
        assert_eq!(
            call_c2::<RoleList>(&reopened, CREATOR, &RoleListParams {})
                .unwrap()
                .roles
                .len(),
            1
        );
        assert_eq!(
            call_c2::<AgentExecutionGet>(
                &reopened,
                CREATOR,
                &AgentExecutionGetParams {
                    employee_id: AGENT_1.into(),
                },
            )
            .unwrap()
            .model_id,
            "claude-sonnet-4-6"
        );
    }
}

#[test]
fn c2_authorization_hire_1_through_6_and_hr_message_guard() {
    for engine in ENGINES {
        let site = Site::new();
        let office = site.provision(engine).unwrap();

        // Add person Ada (default may_hire = true) and hire Agent 1 (default may_hire = false).
        org_write(
            &office,
            CREATOR,
            OrgCommand::MemberAddPerson(MemberAddPersonParams {
                command_id: cmd_id(10),
                member_id: PERSON_ADA.into(),
                name: "Ada".into(),
                handle: Some("ada".into()),
                home_team_id: Some(TOP_TEAM.into()),
                contact: None,
            }),
        );
        let hired = call_c2::<HrHire>(
            &office,
            CREATOR,
            &HrHireParams {
                command_id: cmd_id(11),
                request_id: REQ_1.into(),
                employee_id: AGENT_1.into(),
                home_team_id: TOP_TEAM.into(),
                role_id: None,
                proposal: HireProposalWire {
                    name: "Bot One".into(),
                    handle: "bot1".into(),
                    ..HireProposalWire::default()
                },
                reason: "Initial agent".into(),
            },
        )
        .unwrap();
        assert_eq!(hired.state, "hired");

        // HIRE-1 & HIRE-2: Agent 1 has may_hire = false -> refused on hr.hire & @hr messaging.
        let denied = call_c2::<HrHire>(
            &office,
            AGENT_1,
            &HrHireParams {
                command_id: cmd_id(12),
                request_id: REQ_2.into(),
                employee_id: AGENT_2.into(),
                home_team_id: TOP_TEAM.into(),
                role_id: None,
                proposal: HireProposalWire {
                    name: "Bot Two".into(),
                    handle: "bot2".into(),
                    ..HireProposalWire::default()
                },
                reason: "Unauthorized attempt".into(),
            },
        )
        .unwrap_err();
        assert_eq!(denied.code, ErrorCode::NotAuthorized);
        assert_eq!(denied.message, HIRE_DENIED);

        let hr_msg_err = office
            .check_hr_message_allowed(AGENT_1, "hr")
            .unwrap()
            .unwrap_err();
        assert_eq!(hr_msg_err.code, ErrorCode::NotAuthorized);
        assert_eq!(hr_msg_err.message, HIRE_DENIED);
        assert!(office
            .check_hr_message_allowed(AGENT_1, HR)
            .unwrap()
            .is_err());
        assert!(office
            .check_hr_message_allowed(AGENT_1, PERSON_ADA)
            .unwrap()
            .is_ok());

        // HIRE-5: Agents cannot toggle may_hire; HR lead cannot be toggled; self-toggle refused.
        let agent_toggle = call_c2::<MemberSetMayHire>(
            &office,
            AGENT_1,
            &MemberSetMayHireParams {
                command_id: cmd_id(13),
                member_id: AGENT_1.into(),
                expected_version: 1,
                may_hire: true,
            },
        )
        .unwrap_err();
        assert_eq!(agent_toggle.code, ErrorCode::NotAuthorized);

        let hr_toggle = call_c2::<MemberSetMayHire>(
            &office,
            CREATOR,
            &MemberSetMayHireParams {
                command_id: cmd_id(14),
                member_id: HR.into(),
                expected_version: 1,
                may_hire: false,
            },
        )
        .unwrap_err();
        assert_eq!(hr_toggle.code, ErrorCode::InvalidRequest);

        // Owner grants may_hire = true to Agent 1; until Agent 1 leads a team, still refused!
        call_c2::<MemberSetMayHire>(
            &office,
            CREATOR,
            &MemberSetMayHireParams {
                command_id: cmd_id(15),
                member_id: AGENT_1.into(),
                expected_version: 1,
                may_hire: true,
            },
        )
        .unwrap();
        assert!(office
            .check_hr_message_allowed(AGENT_1, "hr")
            .unwrap()
            .is_err());

        // Appoint Agent 1 as lead of TOP_TEAM -> now Agent 1 can message @hr AND hire!
        org_write(
            &office,
            CREATOR,
            OrgCommand::TeamSetLead(TeamSetLeadParams {
                command_id: cmd_id(150),
                team_id: TOP_TEAM.into(),
                expected_version: 1,
                lead_member_id: Some(AGENT_1.into()),
            }),
        );
        assert!(office
            .check_hr_message_allowed(AGENT_1, "hr")
            .unwrap()
            .is_ok());
        let hired_by_agent = call_c2::<HrHire>(
            &office,
            AGENT_1,
            &HrHireParams {
                command_id: cmd_id(16),
                request_id: REQ_2.into(),
                employee_id: AGENT_2.into(),
                home_team_id: TOP_TEAM.into(),
                role_id: None,
                proposal: HireProposalWire {
                    name: "Bot Two".into(),
                    handle: "bot2".into(),
                    ..HireProposalWire::default()
                },
                reason: "Authorized agent hire".into(),
            },
        )
        .unwrap();
        assert_eq!(hired_by_agent.state, "hired");

        // HIRE-3 & HIRE-4: Owner revokes Ada's may_hire -> Ada is refused on draft and @hr msg.
        call_c2::<MemberSetMayHire>(
            &office,
            CREATOR,
            &MemberSetMayHireParams {
                command_id: cmd_id(17),
                member_id: PERSON_ADA.into(),
                expected_version: 1,
                may_hire: false,
            },
        )
        .unwrap();
        assert!(office
            .check_hr_message_allowed(PERSON_ADA, "hr")
            .unwrap()
            .is_err());
        let ada_denied = call_c2::<HrHireDraft>(
            &office,
            PERSON_ADA,
            &HrHireDraftParams {
                command_id: cmd_id(18),
                request_id: REQ_3.into(),
                home_team_id: TOP_TEAM.into(),
                role_id: None,
                proposal: HireProposalWire {
                    name: "Bot Three".into(),
                    handle: "bot3".into(),
                    ..HireProposalWire::default()
                },
                reason: "Revoked hirer".into(),
            },
        )
        .unwrap_err();
        assert_eq!(ada_denied.code, ErrorCode::NotAuthorized);
        assert_eq!(ada_denied.message, HIRE_DENIED);

        // HIRE-6: TeamSetMaxMembers refuses agent lead (even though Agent 1 leads TOP_TEAM!), accepts owner.
        let agent_limit = call_c2::<TeamSetMaxMembers>(
            &office,
            AGENT_1,
            &TeamSetMaxMembersParams {
                command_id: cmd_id(19),
                team_id: TOP_TEAM.into(),
                expected_version: 2,
                max_members: 20,
            },
        )
        .unwrap_err();
        assert_eq!(agent_limit.code, ErrorCode::NotAuthorized);

        // TOP_TEAM currently has 5 active members (Owner, HR, Ada, Agent1, Agent2). Set max = 5.
        call_c2::<TeamSetMaxMembers>(
            &office,
            CREATOR,
            &TeamSetMaxMembersParams {
                command_id: cmd_id(20),
                team_id: TOP_TEAM.into(),
                expected_version: 2,
                max_members: 5,
            },
        )
        .unwrap();
        let over_limit = call_c2::<HrHire>(
            &office,
            CREATOR,
            &HrHireParams {
                command_id: cmd_id(21),
                request_id: REQ_3.into(),
                employee_id: AGENT_3.into(),
                home_team_id: TOP_TEAM.into(),
                role_id: None,
                proposal: HireProposalWire {
                    name: "Bot Three".into(),
                    handle: "bot3".into(),
                    ..HireProposalWire::default()
                },
                reason: "Over limit".into(),
            },
        )
        .unwrap_err();
        assert_eq!(over_limit.code, ErrorCode::InvalidRequest);
        assert!(
            over_limit.message.contains("max_members"),
            "{}",
            over_limit.message
        );
    }
}

#[test]
fn c2_agent_lifecycle_move_team_relocation_and_hr_protection() {
    for engine in ENGINES {
        let site = Site::new();
        let office = site.provision(engine).unwrap();

        org_write(
            &office,
            CREATOR,
            OrgCommand::TeamCreate(TeamCreateParams {
                command_id: cmd_id(30),
                team_id: TEAM_A.into(),
                name: "Platform".into(),
                owner_team_id: Some(TOP_TEAM.into()),
            }),
        );
        org_write(
            &office,
            CREATOR,
            OrgCommand::TeamCreate(TeamCreateParams {
                command_id: cmd_id(31),
                team_id: TEAM_B.into(),
                name: "Data".into(),
                owner_team_id: Some(TOP_TEAM.into()),
            }),
        );
        call_c2::<ProviderConnectionCreate>(
            &office,
            CREATOR,
            &ProviderConnectionCreateParams {
                command_id: cmd_id(32),
                connection_id: CONN_1.into(),
                default_profile_id: PROF_1.into(),
                key: "claude-main".into(),
                name: "Claude Main".into(),
                provider_type: "anthropic".into(),
                auth_mode: "api_key".into(),
                subscription_identity: None,
                runtime_driver: "claude-code".into(),
                endpoint_url: None,
                max_concurrent_turns: 1,
            },
        )
        .unwrap();
        call_c2::<HrHire>(
            &office,
            CREATOR,
            &HrHireParams {
                command_id: cmd_id(33),
                request_id: REQ_1.into(),
                employee_id: AGENT_1.into(),
                home_team_id: TEAM_A.into(),
                role_id: None,
                proposal: base_proposal("Atlas", "atlas"),
                reason: "Platform lead".into(),
            },
        )
        .unwrap();

        // Update agent profile + execution config.
        let upd = call_c2::<AgentUpdate>(
            &office,
            CREATOR,
            &AgentUpdateParams {
                command_id: cmd_id(34),
                member_id: AGENT_1.into(),
                expected_version: 1,
                name: Some("Atlas Prime".into()),
                role_id: None,
                clear_role_id: false,
                reports_to_member_id: None,
                clear_reports_to: false,
                title: Some("Principal Agent".into()),
                clear_title: false,
                focus: None,
                clear_focus: true,
                role_text: None,
                clear_role_text: false,
                title_in_team: None,
                clear_title_in_team: false,
                body: Some("Updated profile body.".into()),
            },
        )
        .unwrap();
        assert_eq!(upd.revision, 2);

        let exec_upd = call_c2::<AgentExecutionSet>(
            &office,
            CREATOR,
            &AgentExecutionSetParams {
                command_id: cmd_id(35),
                employee_id: AGENT_1.into(),
                expected_version: Some(1),
                model_profile_id: PROF_1.into(),
                model_id: "claude-opus-4-6".into(),
                tools_config: vec!["read".into()],
                permission_preset: Some("read_only".into()),
                enabled: true,
                system_prompt_markdown: "Strict mode.".into(),
                effort: None,
            },
        )
        .unwrap();
        assert_eq!(exec_upd.revision, 2);

        // MoveTeam relocates both execution.md and hires/*.yaml cleanly and survives rebuild.
        org_write(
            &office,
            CREATOR,
            OrgCommand::TeamMove(TeamMoveParams {
                command_id: cmd_id(36),
                team_id: TEAM_A.into(),
                expected_version: 1,
                owner_team_id: Some(TEAM_B.into()),
            }),
        );
        // MoveAgent to TEAM_B moves profile.md and execution.md.
        call_c2::<AgentMove>(
            &office,
            CREATOR,
            &AgentMoveParams {
                command_id: cmd_id(37),
                member_id: AGENT_1.into(),
                expected_version: 2,
                to_team_id: TEAM_B.into(),
            },
        )
        .unwrap();

        // Appoint agent as TEAM_B lead, then pause, resume, and offboard (clears lead!).
        org_write(
            &office,
            CREATOR,
            OrgCommand::TeamSetLead(TeamSetLeadParams {
                command_id: cmd_id(38),
                team_id: TEAM_B.into(),
                expected_version: 1,
                lead_member_id: Some(AGENT_1.into()),
            }),
        );
        call_c2::<AgentSetStatus>(
            &office,
            CREATOR,
            &AgentSetStatusParams {
                command_id: cmd_id(39),
                member_id: AGENT_1.into(),
                expected_version: 3,
                paused: true,
            },
        )
        .unwrap();
        call_c2::<AgentSetStatus>(
            &office,
            CREATOR,
            &AgentSetStatusParams {
                command_id: cmd_id(40),
                member_id: AGENT_1.into(),
                expected_version: 4,
                paused: false,
            },
        )
        .unwrap();
        // HR lead cannot be paused or offboarded.
        assert_eq!(
            call_c2::<AgentSetStatus>(
                &office,
                CREATOR,
                &AgentSetStatusParams {
                    command_id: cmd_id(41),
                    member_id: HR.into(),
                    expected_version: 1,
                    paused: true,
                },
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            call_c2::<AgentOffboard>(
                &office,
                CREATOR,
                &AgentOffboardParams {
                    command_id: cmd_id(42),
                    member_id: HR.into(),
                    expected_version: 1,
                },
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidRequest
        );

        call_c2::<AgentOffboard>(
            &office,
            CREATOR,
            &AgentOffboardParams {
                command_id: cmd_id(43),
                member_id: AGENT_1.into(),
                expected_version: 5,
            },
        )
        .unwrap();
        let snap = snapshot(&office);
        let team_b = snap.teams.iter().find(|t| t.id == TEAM_B).unwrap();
        assert!(team_b.lead_member_id.is_none(), "offboard clears team lead");

        // Delete runtime/ and rebuild after MoveTeam + MoveAgent + OffboardAgent!
        drop(office);
        let _ = std::fs::remove_dir_all(site.runtime_folder());
        let reopened = site.open(engine).unwrap();
        assert_eq!(
            call_c2::<AgentExecutionGet>(
                &reopened,
                CREATOR,
                &AgentExecutionGetParams {
                    employee_id: AGENT_1.into(),
                },
            )
            .unwrap()
            .model_id,
            "claude-opus-4-6"
        );
    }
}

const TEAM_C: &str = "01900000-0000-7000-8000-000000000023";
const AGENT_4: &str = "01900000-0000-7000-8000-000000000134";
const REQ_4: &str = "01900000-0000-7000-8000-000000000124";

fn setup_three_teams(office: &Office) {
    for (i, (tid, name, parent)) in [
        (TEAM_A, "Engineering", TOP_TEAM),
        (TEAM_B, "Marketing", TOP_TEAM),
        (TEAM_C, "Backend", TEAM_A),
    ]
    .into_iter()
    .enumerate()
    {
        org_write(
            office,
            CREATOR,
            OrgCommand::TeamCreate(TeamCreateParams {
                command_id: cmd_id(200 + i as u32),
                team_id: tid.into(),
                name: name.into(),
                owner_team_id: Some(parent.into()),
            }),
        );
    }
}

fn quick_hire(
    office: &Office,
    actor: &str,
    cmd: u32,
    req: &str,
    emp: &str,
    team: &str,
    handle: &str,
) -> Result<HireOpResult, ApiError> {
    call_c2::<HrHire>(
        office,
        actor,
        &HrHireParams {
            command_id: cmd_id(cmd),
            request_id: req.into(),
            employee_id: emp.into(),
            home_team_id: team.into(),
            role_id: None,
            proposal: HireProposalWire {
                name: handle.to_uppercase(),
                handle: handle.into(),
                ..HireProposalWire::default()
            },
            reason: "test hire".into(),
        },
    )
}

#[test]
fn c2_hire_2_whitespace_hire_4_hire_7_and_cross_subtree_move() {
    for engine in ENGINES {
        let site = Site::new();
        let office = site.provision(engine).unwrap();
        setup_three_teams(&office);
        org_write(
            &office,
            CREATOR,
            OrgCommand::MemberAddPerson(MemberAddPersonParams {
                command_id: cmd_id(210),
                member_id: PERSON_ADA.into(),
                name: "Ada".into(),
                handle: Some("ada".into()),
                home_team_id: Some(TEAM_A.into()),
                contact: None,
            }),
        );
        org_write(
            &office,
            CREATOR,
            OrgCommand::TeamSetLead(TeamSetLeadParams {
                command_id: cmd_id(211),
                team_id: TEAM_A.into(),
                expected_version: 1,
                lead_member_id: Some(PERSON_ADA.into()),
            }),
        );
        // HIRE-4: Ada leads TEAM_A so cannot hire into unrelated sibling TEAM_B.
        assert_eq!(
            quick_hire(&office, PERSON_ADA, 212, REQ_1, AGENT_1, TEAM_B, "mkt")
                .unwrap_err()
                .code,
            ErrorCode::NotAuthorized
        );
        quick_hire(&office, PERSON_ADA, 213, REQ_1, AGENT_1, TEAM_A, "director").unwrap();
        // Cross-subtree MoveAgent by Team A lead to Team B is refused.
        assert_eq!(
            call_c2::<AgentMove>(
                &office,
                PERSON_ADA,
                &AgentMoveParams {
                    command_id: cmd_id(214),
                    member_id: AGENT_1.into(),
                    expected_version: 1,
                    to_team_id: TEAM_B.into(),
                },
            )
            .unwrap_err()
            .code,
            ErrorCode::NotAuthorized
        );
        // HIRE-2 whitespace and @hr variants must all be refused for Agent 1 (may_hire=false).
        for variant in [" hr ", "@hr", " @HR ", &format!(" {HR} ")] {
            assert_eq!(
                office
                    .check_hr_message_allowed(AGENT_1, variant)
                    .unwrap()
                    .unwrap_err()
                    .code,
                ErrorCode::NotAuthorized
            );
        }
        verify_hire_7_ancestor_director(&office);
    }
}

fn verify_hire_7_ancestor_director(office: &Office) {
    org_write(
        office,
        CREATOR,
        OrgCommand::TeamSetLead(TeamSetLeadParams {
            command_id: cmd_id(215),
            team_id: TEAM_A.into(),
            expected_version: 2,
            lead_member_id: Some(AGENT_1.into()),
        }),
    );
    call_c2::<MemberSetMayHire>(
        office,
        CREATOR,
        &MemberSetMayHireParams {
            command_id: cmd_id(216),
            member_id: AGENT_1.into(),
            expected_version: 1,
            may_hire: true,
        },
    )
    .unwrap();
    quick_hire(office, AGENT_1, 217, REQ_2, AGENT_2, TEAM_C, "sublead").unwrap();
    org_write(
        office,
        CREATOR,
        OrgCommand::TeamSetLead(TeamSetLeadParams {
            command_id: cmd_id(218),
            team_id: TEAM_C.into(),
            expected_version: 1,
            lead_member_id: Some(AGENT_2.into()),
        }),
    );
    assert_eq!(
        quick_hire(office, AGENT_2, 219, REQ_3, AGENT_3, TEAM_C, "worker")
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
    assert_eq!(
        quick_hire(office, AGENT_1, 220, REQ_3, AGENT_3, TEAM_C, "worker")
            .unwrap()
            .state,
        "hired"
    );
}

#[test]
fn c2_bounds_stalled_giveup_revoked_confirm_and_role_name_lookup() {
    for engine in ENGINES {
        let site = Site::new();
        let office = site.provision(engine).unwrap();
        verify_bounds_role_lookup_and_giveup(&office);
        verify_confirm_after_hirer_revoked(&office);
    }
}

fn verify_bounds_role_lookup_and_giveup(office: &Office) {
    for bad in [0u64, 101] {
        assert_eq!(
            call_c2::<TeamSetMaxMembers>(
                office,
                CREATOR,
                &TeamSetMaxMembersParams {
                    command_id: cmd_id(300 + bad as u32),
                    team_id: TOP_TEAM.into(),
                    expected_version: 1,
                    max_members: bad,
                },
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidRequest
        );
    }
    call_c2::<RoleCreate>(
        office,
        CREATOR,
        &RoleCreateParams {
            command_id: cmd_id(310),
            role_id: ROLE_BE.into(),
            key: None,
            name: "Security Reviewer".into(),
            body: "Reviews code.".into(),
        },
    )
    .unwrap();
    assert_eq!(
        call_c2::<RoleInspect>(
            office,
            CREATOR,
            &RoleInspectParams {
                role: "security reviewer".into(),
            },
        )
        .unwrap()
        .id,
        ROLE_BE
    );
    call_c2::<HrHireDraft>(
        office,
        CREATOR,
        &HrHireDraftParams {
            command_id: cmd_id(311),
            request_id: REQ_4.into(),
            home_team_id: TOP_TEAM.into(),
            role_id: None,
            proposal: HireProposalWire {
                name: "Stalled".into(),
                handle: "stalled".into(),
                ..HireProposalWire::default()
            },
            reason: "Will stall".into(),
        },
    )
    .unwrap();
    assert_eq!(
        call_c2::<HrHireConfirm>(
            office,
            CREATOR,
            &HrHireConfirmParams {
                command_id: cmd_id(312),
                request_id: REQ_4.into(),
                employee_id: AGENT_4.into(),
                expected_version: 1,
                fail_at_step: Some("provision_home".into()),
            },
        )
        .unwrap()
        .state,
        "hiring"
    );
    assert_eq!(
        call_c2::<HrHireWithdraw>(
            office,
            CREATOR,
            &HrHireWithdrawParams {
                command_id: cmd_id(313),
                request_id: REQ_4.into(),
                expected_version: 2,
                closed_reason: "Given up".into(),
            },
        )
        .unwrap()
        .state,
        "failed"
    );
    assert_eq!(
        call_c2::<HrHireConfirm>(
            office,
            CREATOR,
            &HrHireConfirmParams {
                command_id: cmd_id(314),
                request_id: REQ_4.into(),
                employee_id: AGENT_4.into(),
                expected_version: 3,
                fail_at_step: None,
            },
        )
        .unwrap_err()
        .code,
        ErrorCode::InvalidRequest
    );
}

fn verify_confirm_after_hirer_revoked(office: &Office) {
    org_write(
        office,
        CREATOR,
        OrgCommand::MemberAddPerson(MemberAddPersonParams {
            command_id: cmd_id(320),
            member_id: PERSON_ADA.into(),
            name: "Ada".into(),
            handle: Some("ada".into()),
            home_team_id: Some(TOP_TEAM.into()),
            contact: None,
        }),
    );
    org_write(
        office,
        CREATOR,
        OrgCommand::TeamSetLead(TeamSetLeadParams {
            command_id: cmd_id(321),
            team_id: TOP_TEAM.into(),
            expected_version: 1,
            lead_member_id: Some(PERSON_ADA.into()),
        }),
    );
    call_c2::<HrHireDraft>(
        office,
        PERSON_ADA,
        &HrHireDraftParams {
            command_id: cmd_id(322),
            request_id: REQ_1.into(),
            home_team_id: TOP_TEAM.into(),
            role_id: None,
            proposal: HireProposalWire {
                name: "Turing".into(),
                handle: "turing".into(),
                ..HireProposalWire::default()
            },
            reason: "Draft before revoke".into(),
        },
    )
    .unwrap();
    call_c2::<MemberSetMayHire>(
        office,
        CREATOR,
        &MemberSetMayHireParams {
            command_id: cmd_id(323),
            member_id: PERSON_ADA.into(),
            expected_version: 1,
            may_hire: false,
        },
    )
    .unwrap();
    assert_eq!(
        call_c2::<HrHireConfirm>(
            office,
            PERSON_ADA,
            &HrHireConfirmParams {
                command_id: cmd_id(324),
                request_id: REQ_1.into(),
                employee_id: AGENT_1.into(),
                expected_version: 1,
                fail_at_step: None,
            },
        )
        .unwrap_err()
        .code,
        ErrorCode::NotAuthorized
    );
    assert_eq!(
        call_c2::<HrHireConfirm>(
            office,
            CREATOR,
            &HrHireConfirmParams {
                command_id: cmd_id(325),
                request_id: REQ_1.into(),
                employee_id: AGENT_1.into(),
                expected_version: 1,
                fail_at_step: None,
            },
        )
        .unwrap()
        .state,
        "hired"
    );
}

#[test]
fn role_inspect_prefers_exact_id_over_colliding_key() {
    const ROLE_A: &str = "01900000-0000-7000-8000-0000000000a1";
    const ROLE_B: &str = "01900000-0000-7000-8000-0000000000b2";
    for engine in ENGINES {
        let site = Site::new();
        let office = site.provision(engine).unwrap();
        call_c2::<RoleCreate>(
            &office,
            CREATOR,
            &RoleCreateParams {
                command_id: cmd_id(90),
                role_id: ROLE_A.into(),
                key: Some(ROLE_B.into()),
                name: "Shadowing Role".into(),
                body: "First role with colliding key.".into(),
            },
        )
        .unwrap();
        call_c2::<RoleCreate>(
            &office,
            CREATOR,
            &RoleCreateParams {
                command_id: cmd_id(91),
                role_id: ROLE_B.into(),
                key: Some("real-role-b".into()),
                name: "Target Role".into(),
                body: "Second role targeted by exact ID.".into(),
            },
        )
        .unwrap();
        let inspected: RoleRecord = call_c2::<RoleInspect>(
            &office,
            CREATOR,
            &RoleInspectParams {
                role: ROLE_B.into(),
            },
        )
        .unwrap();
        assert_eq!(inspected.id, ROLE_B);
        assert_eq!(inspected.name, "Target Role");
    }
}

fn effort_set(n: u32, expected: Option<u64>, effort: Option<&str>) -> AgentExecutionSetParams {
    AgentExecutionSetParams {
        command_id: cmd_id(n),
        employee_id: AGENT_1.into(),
        expected_version: expected,
        model_profile_id: PROF_1.into(),
        model_id: "claude-opus-4-6".into(),
        tools_config: vec!["read".into()],
        permission_preset: Some("workspace_edits".into()),
        effort: effort.map(str::to_owned),
        enabled: true,
        system_prompt_markdown: "Think hard.".into(),
    }
}

fn effort_of(office: &Office) -> (Option<String>, u64) {
    let exec: AgentExecutionRecord = call_c2::<AgentExecutionGet>(
        office,
        CREATOR,
        &AgentExecutionGetParams {
            employee_id: AGENT_1.into(),
        },
    )
    .unwrap();
    (exec.effort, exec.revision)
}

/// The agent's `execution.md` as text, wherever its home team's chain puts it. Only that
/// file is read: the office is open, so other durable files may come and go meanwhile.
fn execution_file(site: &Site) -> String {
    let suffix = format!("agents/{AGENT_1}/execution.md");
    let mut folders = vec![site.roots.durable.clone()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for path in entries.flatten().map(|entry| entry.path()) {
            if path.ends_with(&suffix) {
                return std::fs::read_to_string(&path).unwrap();
            }
            if path.is_dir() {
                folders.push(path);
            }
        }
    }
    panic!("no {suffix} under the durable root");
}

/// TASK-181: an agent's reasoning effort is set at hire and by `agent.execution_set`, read
/// back by `agent.execution_get`, written to `execution.md` only when set, refused when it is
/// not one of the five levels, cleared by a set without it, and rebuilt from the files the
/// same way on SQLite and on Memory.
#[test]
fn agent_execution_effort_round_trips_on_every_engine_and_unknown_levels_are_refused() {
    let mut seen = Vec::new();
    for engine in ENGINES {
        let site = Site::new();
        let office = site.provision(engine).unwrap();
        call_c2::<ProviderConnectionCreate>(
            &office,
            CREATOR,
            &ProviderConnectionCreateParams {
                command_id: cmd_id(400),
                connection_id: CONN_1.into(),
                default_profile_id: PROF_1.into(),
                key: "claude-main".into(),
                name: "Claude Main".into(),
                provider_type: "anthropic".into(),
                auth_mode: "api_key".into(),
                subscription_identity: None,
                runtime_driver: "claude-code".into(),
                endpoint_url: None,
                max_concurrent_turns: 1,
            },
        )
        .unwrap();

        // A hire proposal names an unknown effort: refused before anything is written.
        let mut bad = base_proposal("Atlas", "atlas");
        bad.effort = Some("turbo".into());
        let hire = |n: u32, proposal: HireProposalWire| {
            call_c2::<HrHire>(
                &office,
                CREATOR,
                &HrHireParams {
                    command_id: cmd_id(n),
                    request_id: REQ_1.into(),
                    employee_id: AGENT_1.into(),
                    home_team_id: TOP_TEAM.into(),
                    role_id: None,
                    proposal,
                    reason: "Deep reviews".into(),
                },
            )
        };
        assert_eq!(hire(401, bad).unwrap_err().code, ErrorCode::InvalidRequest);
        let mut proposal = base_proposal("Atlas", "atlas");
        proposal.effort = Some("high".into());
        hire(402, proposal).unwrap();
        assert_eq!(effort_of(&office), (Some("high".into()), 1));
        let inspected: HireRequestRecord = call_c2::<HrHireInspect>(
            &office,
            CREATOR,
            &HrHireInspectParams {
                request_id: REQ_1.into(),
            },
        )
        .unwrap();
        assert_eq!(inspected.proposal.effort.as_deref(), Some("high"));

        call_c2::<AgentExecutionSet>(&office, CREATOR, &effort_set(403, Some(1), Some("xhigh")))
            .unwrap();
        assert_eq!(effort_of(&office), (Some("xhigh".into()), 2));
        office.sync_store().unwrap();
        assert!(
            execution_file(&site).contains("\neffort: \"xhigh\"\n"),
            "{}",
            execution_file(&site)
        );

        // Unknown levels, an empty one and `none` are refused and change nothing.
        for (n, wrong) in [(404, "turbo"), (405, ""), (406, "none"), (407, "XHIGH")] {
            let err = call_c2::<AgentExecutionSet>(
                &office,
                CREATOR,
                &effort_set(n, Some(2), Some(wrong)),
            )
            .unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidRequest, "{wrong:?}");
        }
        assert_eq!(effort_of(&office), (Some("xhigh".into()), 2));

        // Every level is accepted; the last one stays across a rebuild from the files.
        for (n, level) in [(408, "low"), (409, "medium"), (410, "high"), (411, "max")] {
            call_c2::<AgentExecutionSet>(&office, CREATOR, &effort_set(n, None, Some(level)))
                .unwrap();
            assert_eq!(effort_of(&office).0.as_deref(), Some(level));
        }
        drop(office);
        let _ = std::fs::remove_dir_all(site.runtime_folder());
        let office = site.open(engine).unwrap();
        assert_eq!(effort_of(&office), (Some("max".into()), 6));

        // A full replacement without an effort clears it: no key in the file any more.
        call_c2::<AgentExecutionSet>(&office, CREATOR, &effort_set(412, Some(6), None)).unwrap();
        assert_eq!(effort_of(&office), (None, 7));
        office.sync_store().unwrap();
        assert!(!execution_file(&site).contains("effort"));
        drop(office);
        let _ = std::fs::remove_dir_all(site.runtime_folder());
        let office = site.open(engine).unwrap();
        assert_eq!(effort_of(&office), (None, 7));
        seen.push(execution_file(&site));
    }
    // Both engines leave the same `execution.md` behind.
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0], seen[1]);
}

/// TASK-155: an agent's folder and home are made owner-only even when they already existed
/// more open (an earlier build, a `0002` mask), and a link in the home's place is refused,
/// never followed.
#[test]
fn an_existing_open_agent_home_is_made_owner_only_and_a_linked_one_is_refused() {
    let site = Site::new();
    let office = site.provision(ENGINES[1]).unwrap();
    let mode = |path: &std::path::Path| {
        std::fs::symlink_metadata(path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777
    };
    let home = office.agent_private_home_path(AGENT_1);
    let agent = home.parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&home).unwrap();
    for dir in [&agent, &home] {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o775)).unwrap();
    }
    std::fs::write(home.join("notes.md"), b"x").unwrap();
    std::fs::set_permissions(
        home.join("notes.md"),
        std::fs::Permissions::from_mode(0o664),
    )
    .unwrap();
    assert_eq!(office.ensure_private_home(AGENT_1).unwrap(), home);
    assert_eq!(mode(&agent), 0o700);
    assert_eq!(mode(&home), 0o700);
    // What the agent wrote inside keeps its own mode: the folder is the boundary.
    assert_eq!(mode(&home.join("notes.md")), 0o664);

    let other_home = office.agent_private_home_path(AGENT_2);
    let outside = site.root.join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::create_dir_all(other_home.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&outside, &other_home).unwrap();
    assert!(office.ensure_private_home(AGENT_2).is_err());
    assert_eq!(mode(&outside), 0o755, "a linked home is never followed");
}
