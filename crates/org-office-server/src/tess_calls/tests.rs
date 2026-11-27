use std::sync::{Arc, Mutex};

use org_office::rules::OrgCommand;
use org_office::{Office, TestOffice};
use org_proto::budgets::{BudgetPeriodKind, BudgetScopeKind, BudgetSetParams, BudgetUnit};
use org_proto::chat::{ChatChannelCreateParams, ChatSendParams, DmOpenParams};
use org_proto::hiring::{
    HireProposalWire, HrHire, HrHireDraft, HrHireDraftParams, HrHireParams, MemberSetMayHire,
    MemberSetMayHireParams,
};
use org_proto::org::{MemberAddPersonParams, TeamCreateParams, TeamSetLeadParams};
use org_proto::tasks::TasksCreateParams;
use org_proto::tess::{
    SuggestionCreate, SuggestionCreateParams, SuggestionCreateResult, SuggestionDismiss,
    SuggestionDismissParams, SuggestionInspect, SuggestionInspectParams, SuggestionInspectResult,
    SuggestionList, SuggestionListParams, SuggestionListResult, SuggestionStatus,
    SuggestionWithdraw, SuggestionWithdrawParams, UiEntityKind, UiEntityLinkInput, UiFollowUp,
    UiMetric, UiNavigation, UiPresent, UiPresentParams, UiPresentResult,
};
use org_proto::{ApiError, Call, ErrorCode, Outcome, Request};
use serde_json::{json, Map, Value};

use super::TessService;

fn next_id() -> String {
    let ms = org_office::organization_family::testkit::CREATION_AT.0;
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(9100);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut bytes = [0u8; 16];
    bytes[0..6].copy_from_slice(&ms.to_be_bytes()[2..8]);
    bytes[6] = 0x70 | ((seq >> 8) as u8 & 0x0f);
    bytes[7] = (seq & 0xff) as u8;
    bytes[8] = 0x80;
    bytes[15] = (seq & 0xff) as u8;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

struct Harness {
    office: Arc<Office>,
    svc: TessService,
    acting_for: Arc<Mutex<Option<String>>>,
    owner: String,
    tess: String,
    bob: String,
    team_id: String,
    task_key: String,
    task_revision: u64,
    channel_id: String,
    bob_dm_id: String,
    tess_budget_id: String,
}

fn harness() -> Harness {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner = office.owner_member_id().to_owned();
    let tess = office
        .chief_of_staff()
        .unwrap()
        .expect("Tess")
        .id
        .to_string();
    let team_id = office.org_snapshot().unwrap().teams[0].id.clone();

    let bob = next_id();
    office
        .submit_org(
            &owner,
            OrgCommand::MemberAddPerson(MemberAddPersonParams {
                command_id: next_id(),
                member_id: bob.clone(),
                name: "Bob".into(),
                handle: Some("bob".into()),
                home_team_id: Some(team_id.clone()),
                contact: None,
            }),
        )
        .unwrap()
        .unwrap();

    let task = office
        .task_create(
            &owner,
            TasksCreateParams {
                command_id: next_id(),
                task_id: next_id(),
                team_id: Some(team_id.clone()),
                project_id: None,
                assigned_team_id: None,
                assignee_id: None,
                task_type: Some("task".into()),
                work_type: None,
                title: "Ship Tess foundations".into(),
                body_markdown: String::new(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: None,
                labels: Vec::new(),
                reference_files: Vec::new(),
                modified_files: Vec::new(),
                depends_on: Vec::new(),
                details: None,
                dod: None,
                attachment_ids: Vec::new(),
            },
        )
        .unwrap()
        .unwrap();

    let channel_id = office
        .create_channel(
            &owner,
            ChatChannelCreateParams {
                id: None,
                name: "ops-room".into(),
                members: vec![owner.clone(), bob.clone(), tess.clone()],
            },
        )
        .unwrap()
        .unwrap()
        .channel
        .id;

    let bob_dm_id = office
        .dm_open(
            &bob,
            DmOpenParams {
                participants: vec![tess.clone()],
                name: None,
                conversation_id: None,
            },
        )
        .unwrap()
        .unwrap()
        .conversation
        .id;

    let tess_budget_id = office
        .budget_set(
            &owner,
            BudgetSetParams {
                operation_id: next_id(),
                scope_kind: BudgetScopeKind::Agent,
                task_id: None,
                agent_id: Some(tess.clone()),
                release_id: None,
                period_kind: BudgetPeriodKind::Day,
                unit: BudgetUnit::MicroUsd,
                limit_amount: 10_000_000,
                time_zone: Some("UTC".into()),
                time_zone_source: None,
                reason: Some("test daily cap".into()),
            },
        )
        .unwrap()
        .unwrap()
        .budget
        .id;

    let acting_for = Arc::new(Mutex::new(Some(owner.clone())));
    let slot = Arc::clone(&acting_for);
    let tess_clone = tess.clone();
    let svc = TessService::new(
        Arc::clone(&office),
        Arc::new(move |member| {
            (member == tess_clone)
                .then(|| slot.lock().unwrap().clone())
                .flatten()
        }),
    );

    Harness {
        office,
        svc,
        acting_for,
        owner,
        tess,
        bob,
        team_id,
        task_key: task.task.key,
        task_revision: task.task.revision,
        channel_id,
        bob_dm_id,
        tess_budget_id,
    }
}

impl Harness {
    fn set_acting(&self, caller: Option<&str>) {
        *self.acting_for.lock().unwrap() = caller.map(str::to_owned);
    }

    fn call<C: Call>(&self, member: &str, params: &C::Params) -> Result<C::Result, ErrorCode> {
        self.call_err::<C>(member, params).map_err(|err| err.code)
    }

    fn call_err<C: Call>(&self, member: &str, params: &C::Params) -> Result<C::Result, ApiError> {
        let req = Request::new::<C>(1, params).unwrap();
        let resp = self
            .svc
            .dispatch(Some(member), &req)
            .expect("tess_calls handles method");
        match resp.outcome {
            Outcome::Result(val) => Ok(serde_json::from_value(val).unwrap()),
            Outcome::Error(err) => Err(err),
        }
    }
}

fn map(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap()
}

#[test]
fn suggestion_create_preflights_commands_revisions_and_caller_authority() {
    let h = harness();

    // Unknown command fails closed.
    let err = h
        .call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "bogus launch".into(),
                input: Map::new(),
                summary: "Invalid command".into(),
                rationale: None,
                expected_revision: None,
                ttl_seconds: None,
                dry_run: false,
            },
        )
        .unwrap_err();
    assert_eq!(err, ErrorCode::InvalidRequest);

    // ForbiddenForAgent command cannot be proposed.
    let err = h
        .call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "repo push".into(),
                input: map(json!({"task": h.task_key})),
                summary: "Push branch".into(),
                rationale: None,
                expected_revision: None,
                ttl_seconds: None,
                dry_run: false,
            },
        )
        .unwrap_err();
    assert_eq!(err, ErrorCode::NotAuthorized);

    // ReadOnly command does not require confirmation.
    let err = h
        .call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "task list".into(),
                input: Map::new(),
                summary: "List tasks".into(),
                rationale: None,
                expected_revision: None,
                ttl_seconds: None,
                dry_run: false,
            },
        )
        .unwrap_err();
    assert_eq!(err, ErrorCode::InvalidRequest);

    // Invalid input flags rejected by target schema validator.
    let err = h
        .call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "task create".into(),
                input: map(json!({"bogus_flag": 123})),
                summary: "Bad input".into(),
                rationale: None,
                expected_revision: None,
                ttl_seconds: None,
                dry_run: false,
            },
        )
        .unwrap_err();
    assert_eq!(err, ErrorCode::InvalidRequest);

    // Stale expected_revision fails real task_inspect preflight with Conflict.
    let err = h
        .call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "task update".into(),
                input: map(json!({"task": h.task_key, "title": "Updated"})),
                summary: "Update task title".into(),
                rationale: None,
                expected_revision: Some(h.task_revision + 99),
                ttl_seconds: None,
                dry_run: false,
            },
        )
        .unwrap_err();
    assert_eq!(err, ErrorCode::Conflict);

    // Valid task update with exact revision pin passes preflight and stores cleanly.
    let created: SuggestionCreateResult = h
        .call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "task update".into(),
                input: map(json!({"task": h.task_key, "title": "Updated"})),
                summary: "Update task title".into(),
                rationale: Some("Clarify scope".into()),
                expected_revision: Some(h.task_revision),
                ttl_seconds: Some(300),
                dry_run: false,
            },
        )
        .unwrap();
    assert_eq!(created.suggestion.status, SuggestionStatus::Pending);
    assert_eq!(created.suggestion.command, "task update");
    assert_eq!(created.suggestion.call, "tasks.update");
    assert_eq!(created.suggestion.for_caller_id, h.owner);
    assert!(created.suggestion.preflight_ok);
    assert_eq!(
        created.ui_block.suggestion_id.as_deref(),
        Some(created.suggestion.id.as_str())
    );

    // Org command runs real `check_org` dry-run preflight for Owner, and is refused for Bob.
    let dry_org = h
        .call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "team create".into(),
                input: map(json!({"name": "Reliability", "parent_team_id": h.team_id})),
                summary: "Create Reliability team".into(),
                rationale: None,
                expected_revision: None,
                ttl_seconds: None,
                dry_run: true,
            },
        )
        .unwrap();
    assert!(dry_org.dry_run);
    // Dry-run suggestion must NOT be stored.
    let listed = h
        .call::<SuggestionList>(&h.tess, &SuggestionListParams::default())
        .unwrap();
    assert_eq!(listed.suggestions.len(), 1);

    // Acting for non-owner Bob: owner-only `team create` proposal fails closed.
    h.set_acting(Some(&h.bob));
    let err = h
        .call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "team create".into(),
                input: map(json!({"name": "Unauthorized Team", "parent_team_id": h.team_id})),
                summary: "Create team".into(),
                rationale: None,
                expected_revision: None,
                ttl_seconds: None,
                dry_run: false,
            },
        )
        .unwrap_err();
    assert_eq!(err, ErrorCode::NotAuthorized);

    // Wake run (no human caller) is refused.
    h.set_acting(None);
    let err = h
        .call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "task create".into(),
                input: map(json!({"title": "Wake task"})),
                summary: "Wake task".into(),
                rationale: None,
                expected_revision: None,
                ttl_seconds: None,
                dry_run: false,
            },
        )
        .unwrap_err();
    assert_eq!(err, ErrorCode::NotAuthorized);
}

#[test]
fn suggestions_are_strictly_caller_scoped_and_enforce_withdraw_and_dismiss_lifecycle() {
    let h = harness();

    // Owner gets one proposal.
    let owner_prop = h
        .call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "task create".into(),
                input: map(json!({"title": "Owner proposal"})),
                summary: "Owner proposal".into(),
                rationale: None,
                expected_revision: None,
                ttl_seconds: None,
                dry_run: false,
            },
        )
        .unwrap()
        .suggestion;

    // Bob NEVER sees, inspects, or dismisses Owner's proposal — neither directly nor via Tess!
    h.set_acting(Some(&h.bob));
    let bob_via_tess: SuggestionListResult = h
        .call::<SuggestionList>(&h.tess, &SuggestionListParams::default())
        .unwrap();
    assert!(bob_via_tess.suggestions.is_empty());

    let bob_direct: SuggestionListResult = h
        .call::<SuggestionList>(&h.bob, &SuggestionListParams::default())
        .unwrap();
    assert!(bob_direct.suggestions.is_empty());

    assert_eq!(
        h.call::<SuggestionInspect>(
            &h.bob,
            &SuggestionInspectParams {
                suggestion_id: owner_prop.id.clone(),
            },
        )
        .unwrap_err(),
        ErrorCode::NotFound
    );
    assert_eq!(
        h.call::<SuggestionInspect>(
            &h.tess,
            &SuggestionInspectParams {
                suggestion_id: owner_prop.id.clone(),
            },
        )
        .unwrap_err(),
        ErrorCode::NotFound
    );
    assert_eq!(
        h.call::<SuggestionWithdraw>(
            &h.tess,
            &SuggestionWithdrawParams {
                suggestion_id: owner_prop.id.clone(),
                reason: None,
                dry_run: false,
            },
        )
        .unwrap_err(),
        ErrorCode::NotFound
    );
    assert_eq!(
        h.call::<SuggestionDismiss>(
            &h.bob,
            &SuggestionDismissParams {
                suggestion_id: owner_prop.id.clone(),
                reason: None,
            },
        )
        .unwrap_err(),
        ErrorCode::NotFound
    );

    // Owner sees and inspects their own proposal; Tess cannot call `suggestion.dismiss`.
    h.set_acting(Some(&h.owner));
    let inspected: SuggestionInspectResult = h
        .call::<SuggestionInspect>(
            &h.owner,
            &SuggestionInspectParams {
                suggestion_id: owner_prop.id.clone(),
            },
        )
        .unwrap();
    assert_eq!(inspected.suggestion.id, owner_prop.id);

    assert_eq!(
        h.call::<SuggestionDismiss>(
            &h.tess,
            &SuggestionDismissParams {
                suggestion_id: owner_prop.id.clone(),
                reason: None,
            },
        )
        .unwrap_err(),
        ErrorCode::NotAuthorized
    );

    // Human Owner dismisses their proposal; subsequent withdraw returns Conflict.
    let dismissed = h
        .call::<SuggestionDismiss>(
            &h.owner,
            &SuggestionDismissParams {
                suggestion_id: owner_prop.id.clone(),
                reason: Some("Not needed".into()),
            },
        )
        .unwrap();
    assert_eq!(dismissed.suggestion.status, SuggestionStatus::Dismissed);

    assert_eq!(
        h.call::<SuggestionWithdraw>(
            &h.tess,
            &SuggestionWithdrawParams {
                suggestion_id: owner_prop.id.clone(),
                reason: None,
                dry_run: false,
            },
        )
        .unwrap_err(),
        ErrorCode::Conflict
    );

    // Create a second proposal for Bob and let Tess withdraw it cleanly.
    h.set_acting(Some(&h.bob));
    let bob_prop = h
        .call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "task create".into(),
                input: map(json!({"title": "Bob proposal"})),
                summary: "Bob proposal".into(),
                rationale: None,
                expected_revision: None,
                ttl_seconds: None,
                dry_run: false,
            },
        )
        .unwrap()
        .suggestion;

    let withdrawn = h
        .call::<SuggestionWithdraw>(
            &h.tess,
            &SuggestionWithdrawParams {
                suggestion_id: bob_prop.id.clone(),
                reason: Some("Superseded".into()),
                dry_run: false,
            },
        )
        .unwrap();
    assert_eq!(withdrawn.suggestion.status, SuggestionStatus::Withdrawn);
    assert_eq!(
        withdrawn.suggestion.close_reason.as_deref(),
        Some("Superseded")
    );
}

#[test]
fn ui_present_verifies_entities_and_fails_closed_on_unknown_unauthorized_or_excess_followups() {
    let h = harness();
    let office_id = h.office.info().office_id.clone();

    // Owner sees task, team, member, visible channel, office, and agent budget.
    let presented: UiPresentResult = h
        .call::<UiPresent>(
            &h.tess,
            &UiPresentParams {
                title: Some("Executive Summary".into()),
                entity_links: vec![
                    UiEntityLinkInput {
                        kind: UiEntityKind::Task,
                        id: h.task_key.clone(),
                        label: None,
                    },
                    UiEntityLinkInput {
                        kind: UiEntityKind::Team,
                        id: h.team_id.clone(),
                        label: None,
                    },
                    UiEntityLinkInput {
                        kind: UiEntityKind::Member,
                        id: "bob".into(),
                        label: None,
                    },
                    UiEntityLinkInput {
                        kind: UiEntityKind::Channel,
                        id: "#ops-room".into(),
                        label: None,
                    },
                    UiEntityLinkInput {
                        kind: UiEntityKind::Office,
                        id: office_id,
                        label: None,
                    },
                    UiEntityLinkInput {
                        kind: UiEntityKind::Budget,
                        id: h.tess_budget_id.clone(),
                        label: None,
                    },
                ],
                metrics: vec![UiMetric {
                    label: "Open tasks".into(),
                    value: "1".into(),
                    tone: Some("neutral".into()),
                    unit: None,
                }],
                navigation: vec![UiNavigation {
                    label: "Open ops room".into(),
                    route: format!("/chat/channels/{}", h.channel_id),
                    entity_kind: Some(UiEntityKind::Channel),
                    entity_id: Some(h.channel_id.clone()),
                }],
                follow_ups: vec![
                    UiFollowUp {
                        label: "Check blockers".into(),
                        prompt: "Show blocked tasks".into(),
                    },
                    UiFollowUp {
                        label: "Review spend".into(),
                        prompt: "Summarize this month's cost".into(),
                    },
                    UiFollowUp {
                        label: "Check offices".into(),
                        prompt: "Check office sync health".into(),
                    },
                ],
            },
        )
        .unwrap();
    assert_eq!(presented.ui_block.entity_links.len(), 6);
    assert_eq!(presented.ui_block.follow_ups.len(), 3);
    assert_eq!(
        presented.ui_block.entity_links[0].route,
        format!("/tasks/{}", h.task_key)
    );

    // Non-existent entity fails closed with NotFound.
    assert_eq!(
        h.call::<UiPresent>(
            &h.tess,
            &UiPresentParams {
                entity_links: vec![UiEntityLinkInput {
                    kind: UiEntityKind::Task,
                    id: "T-NOPE999".into(),
                    label: None,
                }],
                ..UiPresentParams::default()
            },
        )
        .unwrap_err(),
        ErrorCode::NotFound
    );

    // DMs are NEVER linkable via `UiEntityKind::Channel`.
    assert_eq!(
        h.call::<UiPresent>(
            &h.tess,
            &UiPresentParams {
                entity_links: vec![UiEntityLinkInput {
                    kind: UiEntityKind::Channel,
                    id: h.bob_dm_id.clone(),
                    label: None,
                }],
                ..UiPresentParams::default()
            },
        )
        .unwrap_err(),
        ErrorCode::NotFound
    );

    // Non-owner Bob cannot link an owner-only agent budget (fails closed!).
    h.set_acting(Some(&h.bob));
    let budget_err = h
        .call::<UiPresent>(
            &h.tess,
            &UiPresentParams {
                entity_links: vec![UiEntityLinkInput {
                    kind: UiEntityKind::Budget,
                    id: h.tess_budget_id.clone(),
                    label: None,
                }],
                ..UiPresentParams::default()
            },
        )
        .unwrap_err();
    assert!(
        matches!(budget_err, ErrorCode::NotFound | ErrorCode::NotAuthorized),
        "expected NotFound/NotAuthorized for Bob linking agent budget, got {budget_err:?}"
    );

    // More than 3 follow-ups fails closed with InvalidRequest.
    h.set_acting(Some(&h.owner));
    assert_eq!(
        h.call::<UiPresent>(
            &h.tess,
            &UiPresentParams {
                follow_ups: (0..4)
                    .map(|i| UiFollowUp {
                        label: format!("Follow-up {i}"),
                        prompt: format!("Prompt {i}"),
                    })
                    .collect(),
                ..UiPresentParams::default()
            },
        )
        .unwrap_err(),
        ErrorCode::InvalidRequest
    );

    // Unsafe external or traversal navigation route fails closed.
    for bad_route in [
        "https://evil.example/x",
        "//evil.example/x",
        "/tasks/../secret",
    ] {
        assert_eq!(
            h.call::<UiPresent>(
                &h.tess,
                &UiPresentParams {
                    navigation: vec![UiNavigation {
                        label: "Bad".into(),
                        route: bad_route.into(),
                        entity_kind: None,
                        entity_id: None,
                    }],
                    ..UiPresentParams::default()
                },
            )
            .unwrap_err(),
            ErrorCode::InvalidRequest,
            "route `{bad_route}` must fail closed"
        );
    }
}

fn dispatch_hiring<C: Call>(office: &Office, actor: &str, params: &C::Params) -> C::Result {
    let req = Request::new::<C>(1, params).unwrap();
    let resp = office
        .dispatch_hiring_rpc(Some(actor), &req)
        .expect("hiring handles call");
    match resp.outcome {
        Outcome::Result(val) => serde_json::from_value(val).unwrap(),
        Outcome::Error(err) => panic!("hiring `{}` failed: {err:?}", C::METHOD),
    }
}

fn send_dm_msg(office: &Office, from: &str, to: &str, body: &str) -> String {
    office
        .chat_send(
            from,
            ChatSendParams {
                id: next_id(),
                to: to.to_owned(),
                kind: "msg".into(),
                priority: None,
                subject: None,
                body: body.to_owned(),
                attachment_ids: Vec::new(),
                meta: Default::default(),
                reply_to: None,
                also_in_channel: false,
                reason: None,
            },
        )
        .unwrap()
        .unwrap()
        .message
        .id
}

#[test]
fn p1_2_p2_5_p2_7_preflight_blocks_hire_bypass_owner_drift_cross_dm_builtins_and_resolves_team_lead(
) {
    let h = harness();

    // Create a disjoint sibling team (`team_b`), make Bob lead of `h.team_id` only.
    let team_b = next_id();
    h.office
        .submit_org(
            &h.owner,
            OrgCommand::TeamCreate(TeamCreateParams {
                command_id: next_id(),
                team_id: team_b.clone(),
                name: "Beta".into(),
                owner_team_id: None,
            }),
        )
        .unwrap()
        .unwrap();
    let team_a_ver = h
        .office
        .org_snapshot()
        .unwrap()
        .teams
        .iter()
        .find(|t| t.id == h.team_id)
        .unwrap()
        .version;
    h.office
        .submit_org(
            &h.owner,
            OrgCommand::TeamSetLead(TeamSetLeadParams {
                command_id: next_id(),
                team_id: h.team_id.clone(),
                expected_version: team_a_ver,
                lead_member_id: Some(h.bob.clone()),
            }),
        )
        .unwrap()
        .unwrap();
    let bob_ver = h
        .office
        .load_employee(&h.bob)
        .unwrap()
        .unwrap()
        .meta
        .revision;
    dispatch_hiring::<MemberSetMayHire>(
        &h.office,
        &h.owner,
        &MemberSetMayHireParams {
            command_id: next_id(),
            member_id: h.bob.clone(),
            expected_version: bob_ver,
            may_hire: true,
        },
    );
    assert!(h.office.caller_may_hire_into(&h.bob, &h.team_id).unwrap());
    assert!(!h.office.caller_may_hire_into(&h.bob, &team_b).unwrap());

    // Draft three hire requests: Owner on team_b, Owner on team_a, Bob on team_a.
    let proposal = |name: &str, handle: &str| HireProposalWire {
        name: name.into(),
        handle: handle.into(),
        title: None,
        focus: None,
        role_text: None,
        title_in_team: None,
        reports_to_employee_id: None,
        office_id: None,
        model_profile_id: None,
        model_id: None,
        permission_preset: None,
        effort: None,
        tools_config: Vec::new(),
        system_prompt_markdown: None,
        body: None,
    };
    let req_b = next_id();
    dispatch_hiring::<HrHireDraft>(
        &h.office,
        &h.owner,
        &HrHireDraftParams {
            command_id: next_id(),
            request_id: req_b.clone(),
            home_team_id: team_b.clone(),
            role_id: None,
            proposal: proposal("BetaBot", "betabot"),
            reason: "beta capacity".into(),
        },
    );
    let req_a_owner = next_id();
    dispatch_hiring::<HrHireDraft>(
        &h.office,
        &h.owner,
        &HrHireDraftParams {
            command_id: next_id(),
            request_id: req_a_owner.clone(),
            home_team_id: h.team_id.clone(),
            role_id: None,
            proposal: proposal("OwnerBot", "ownerbot"),
            reason: "owner draft".into(),
        },
    );
    let req_a_bob = next_id();
    dispatch_hiring::<HrHireDraft>(
        &h.office,
        &h.bob,
        &HrHireDraftParams {
            command_id: next_id(),
            request_id: req_a_bob.clone(),
            home_team_id: h.team_id.clone(),
            role_id: None,
            proposal: proposal("BobBot", "bobbot"),
            reason: "bob draft".into(),
        },
    );

    // P1-2 + P2-5: Bob trying `hr hire` with `--id <teamB>` + `--team <teamA>` MUST NOT bypass
    // existing-hire scope check, and MUST return the exact same uniform NotFound as a nonexistent
    // hire request or another caller's draft on team_a (zero cross-team existence oracle!).
    h.set_acting(Some(&h.bob));
    let nonexistent_req = next_id();
    let err_bypass = h
        .call_err::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "hr hire".into(),
                input: map(json!({"id": req_b, "team": h.team_id})),
                summary: "Confirm cross-team hire".into(),
                rationale: None,
                expected_revision: Some(1),
                ttl_seconds: None,
                dry_run: false,
            },
        )
        .unwrap_err();
    let err_other_owner = h
        .call_err::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "hr hire".into(),
                input: map(json!({"id": req_a_owner})),
                summary: "Confirm owner draft".into(),
                rationale: None,
                expected_revision: Some(1),
                ttl_seconds: None,
                dry_run: false,
            },
        )
        .unwrap_err();
    let err_missing = h
        .call_err::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "hr hire".into(),
                input: map(json!({"id": nonexistent_req})),
                summary: "Confirm missing draft".into(),
                rationale: None,
                expected_revision: Some(1),
                ttl_seconds: None,
                dry_run: false,
            },
        )
        .unwrap_err();
    assert_eq!(err_bypass.code, ErrorCode::NotFound);
    assert_eq!(err_other_owner.code, ErrorCode::NotFound);
    assert_eq!(err_missing.code, ErrorCode::NotFound);
    assert_eq!(err_bypass.message, err_missing.message);
    assert_eq!(err_other_owner.message, err_missing.message);
    assert_eq!(
        err_bypass.message,
        "hire request not found or not visible to caller"
    );

    // Same uniform NotFound in `ui.present` (P2-5)!
    let ui_cross = h
        .call_err::<UiPresent>(
            &h.tess,
            &UiPresentParams {
                entity_links: vec![UiEntityLinkInput {
                    kind: UiEntityKind::HireRequest,
                    id: req_b.clone(),
                    label: None,
                }],
                ..UiPresentParams::default()
            },
        )
        .unwrap_err();
    let ui_missing = h
        .call_err::<UiPresent>(
            &h.tess,
            &UiPresentParams {
                entity_links: vec![UiEntityLinkInput {
                    kind: UiEntityKind::HireRequest,
                    id: nonexistent_req,
                    label: None,
                }],
                ..UiPresentParams::default()
            },
        )
        .unwrap_err();
    assert_eq!(ui_cross.code, ErrorCode::NotFound);
    assert_eq!(ui_missing.code, ErrorCode::NotFound);
    assert_eq!(ui_cross.message, ui_missing.message);

    // Invalid flag combos and revision verification on Bob's own draft.
    assert_eq!(
        h.call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "hr hire".into(),
                input: map(json!({"id": req_a_bob, "draft": true})),
                summary: "Invalid combo".into(),
                rationale: None,
                expected_revision: Some(1),
                ttl_seconds: None,
                dry_run: false,
            },
        )
        .unwrap_err(),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        h.call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "hr hire".into(),
                input: map(json!({"withdraw": true, "team": h.team_id})),
                summary: "Withdraw without id".into(),
                rationale: None,
                expected_revision: None,
                ttl_seconds: None,
                dry_run: false,
            },
        )
        .unwrap_err(),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        h.call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "hr hire".into(),
                input: map(json!({"id": req_a_bob})),
                summary: "Missing revision".into(),
                rationale: None,
                expected_revision: None,
                ttl_seconds: None,
                dry_run: false,
            },
        )
        .unwrap_err(),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        h.call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "hr hire".into(),
                input: map(json!({"id": req_a_bob})),
                summary: "Stale revision".into(),
                rationale: None,
                expected_revision: Some(99),
                ttl_seconds: None,
                dry_run: false,
            },
        )
        .unwrap_err(),
        ErrorCode::Conflict
    );
    let confirm_ok = h
        .call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "hr hire".into(),
                input: map(json!({"id": req_a_bob})),
                summary: "Confirm Bob draft".into(),
                rationale: None,
                expected_revision: Some(1),
                ttl_seconds: None,
                dry_run: true,
            },
        )
        .unwrap();
    assert_eq!(confirm_ok.suggestion.call, "hr.hire.confirm");

    // Nonexistent team rejected EVEN for Owner.
    h.set_acting(Some(&h.owner));
    assert_eq!(
        h.call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "hr hire".into(),
                input: map(json!({
                    "team": next_id(),
                    "name": "Ghost",
                    "handle": "ghost",
                    "reason": "test"
                })),
                summary: "Hire into missing team".into(),
                rationale: None,
                expected_revision: None,
                ttl_seconds: None,
                dry_run: false,
            },
        )
        .unwrap_err(),
        ErrorCode::NotFound
    );

    // CallerOwner parity (derived directly from `serve::delegation::access`): Bob cannot propose
    // `repo fetch` or `worktree prepare`.
    h.set_acting(Some(&h.bob));
    assert_eq!(
        h.call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "repo fetch".into(),
                input: map(json!({"repo": "api"})),
                summary: "Fetch repo".into(),
                rationale: None,
                expected_revision: None,
                ttl_seconds: None,
                dry_run: false,
            },
        )
        .unwrap_err(),
        ErrorCode::NotAuthorized
    );
    assert_eq!(
        h.call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "worktree prepare".into(),
                input: map(json!({"task": h.task_key, "repo": "api"})),
                summary: "Prepare worktree".into(),
                rationale: None,
                expected_revision: None,
                ttl_seconds: None,
                dry_run: false,
            },
        )
        .unwrap_err(),
        ErrorCode::NotAuthorized
    );

    // Messaging target preflight: Bob CANNOT propose `msg send` to Owner / Owner's DM or
    // `msg accept` / `msg done` / `msg defer` / `msg unsnooze` / `msg withdraw` on Owner's message.
    let owner_dm_id = h
        .office
        .dm_open(
            &h.owner,
            DmOpenParams {
                participants: vec![h.tess.clone()],
                name: None,
                conversation_id: None,
            },
        )
        .unwrap()
        .unwrap()
        .conversation
        .id;
    let owner_msg_id = send_dm_msg(&h.office, &h.owner, &h.tess, "Owner secret plan");
    let bob_msg_id = send_dm_msg(&h.office, &h.bob, &h.tess, "Bob note");

    for bad_to in ["owner", h.owner.as_str(), owner_dm_id.as_str()] {
        assert_eq!(
            h.call::<SuggestionCreate>(
                &h.tess,
                &SuggestionCreateParams {
                    command: "msg send".into(),
                    input: map(json!({"to": bad_to, "body": "snooping"})),
                    summary: "Send to owner".into(),
                    rationale: None,
                    expected_revision: None,
                    ttl_seconds: None,
                    dry_run: false,
                },
            )
            .unwrap_err(),
            ErrorCode::NotAuthorized,
            "Bob must not pass preflight for msg send to `{bad_to}`"
        );
    }
    assert_eq!(
        h.call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "msg accept".into(),
                input: map(json!({"id": owner_msg_id})),
                summary: "Accept owner message".into(),
                rationale: None,
                expected_revision: None,
                ttl_seconds: None,
                dry_run: false,
            },
        )
        .unwrap_err(),
        ErrorCode::NotFound
    );
    assert_eq!(
        h.call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "channel mark-read".into(),
                input: map(json!({"channel": owner_dm_id})),
                summary: "Mark owner DM read".into(),
                rationale: None,
                expected_revision: None,
                ttl_seconds: None,
                dry_run: false,
            },
        )
        .unwrap_err(),
        ErrorCode::NotAuthorized
    );
    // Bob CAN pass preflight for his own DM, visible channel, and his own message.
    assert!(h
        .call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "msg send".into(),
                input: map(json!({"to": "#ops-room", "body": "Status update"})),
                summary: "Post in ops-room".into(),
                rationale: None,
                expected_revision: None,
                ttl_seconds: None,
                dry_run: true,
            },
        )
        .is_ok());
    assert!(h
        .call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "msg done".into(),
                input: map(json!({"id": bob_msg_id})),
                summary: "Mark bob msg done".into(),
                rationale: None,
                expected_revision: None,
                ttl_seconds: None,
                dry_run: true,
            },
        )
        .is_ok());

    // Built-in agent offboard & person target & revision parity.
    h.set_acting(Some(&h.owner));
    let hr_id = h
        .office
        .list_employees()
        .unwrap()
        .into_iter()
        .find(|e| e.builtin_key == Some(org_office::organization_family::records::BuiltinKey::Hr))
        .expect("built-in HR")
        .id
        .to_string();
    for builtin in [h.tess.as_str(), "tess", hr_id.as_str(), "hr"] {
        assert_eq!(
            h.call::<SuggestionCreate>(
                &h.tess,
                &SuggestionCreateParams {
                    command: "agent offboard".into(),
                    input: map(json!({"member": builtin})),
                    summary: "Offboard built-in".into(),
                    rationale: None,
                    expected_revision: Some(1),
                    ttl_seconds: None,
                    dry_run: false,
                },
            )
            .unwrap_err(),
            ErrorCode::NotAuthorized,
            "built-in `{builtin}` must never pass agent offboard preflight"
        );
    }
    for person in [h.owner.as_str(), h.bob.as_str(), "owner", "bob"] {
        assert_eq!(
            h.call::<SuggestionCreate>(
                &h.tess,
                &SuggestionCreateParams {
                    command: "agent offboard".into(),
                    input: map(json!({"member": person})),
                    summary: "Offboard person".into(),
                    rationale: None,
                    expected_revision: Some(1),
                    ttl_seconds: None,
                    dry_run: false,
                },
            )
            .unwrap_err(),
            ErrorCode::InvalidRequest,
            "person `{person}` must be rejected on agent offboard"
        );
    }

    // Hire a regular non-builtin agent and verify revision check on `agent offboard` & `team set-limit`.
    let ada_id = next_id();
    dispatch_hiring::<HrHire>(
        &h.office,
        &h.owner,
        &HrHireParams {
            command_id: next_id(),
            request_id: next_id(),
            employee_id: ada_id.clone(),
            home_team_id: h.team_id.clone(),
            role_id: None,
            proposal: proposal("Ada", "ada"),
            reason: "regular agent".into(),
        },
    );
    let ada_rev = h
        .office
        .load_employee(&ada_id)
        .unwrap()
        .unwrap()
        .meta
        .revision;
    assert_eq!(
        h.call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "agent offboard".into(),
                input: map(json!({"member": ada_id})),
                summary: "Offboard Ada wrong rev".into(),
                rationale: None,
                expected_revision: Some(ada_rev + 5),
                ttl_seconds: None,
                dry_run: false,
            },
        )
        .unwrap_err(),
        ErrorCode::Conflict
    );
    assert!(h
        .call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "agent offboard".into(),
                input: map(json!({"member": ada_id})),
                summary: "Offboard Ada".into(),
                rationale: None,
                expected_revision: Some(ada_rev),
                ttl_seconds: None,
                dry_run: true,
            },
        )
        .is_ok());

    let latest_team_ver = h
        .office
        .org_snapshot()
        .unwrap()
        .teams
        .iter()
        .find(|t| t.id == h.team_id)
        .unwrap()
        .version;
    assert_eq!(
        h.call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "team set-limit".into(),
                input: map(json!({"team": h.team_id, "max_members": 15})),
                summary: "Team limit wrong rev".into(),
                rationale: None,
                expected_revision: Some(latest_team_ver + 7),
                ttl_seconds: None,
                dry_run: false,
            },
        )
        .unwrap_err(),
        ErrorCode::Conflict
    );

    // P2-7: `team update --lead-member-id` persists resolved wire call `team.set_lead`.
    let set_lead = h
        .call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "team update".into(),
                input: map(json!({"team": h.team_id, "lead_member_id": h.owner})),
                summary: "Set team lead".into(),
                rationale: None,
                expected_revision: Some(latest_team_ver),
                ttl_seconds: None,
                dry_run: false,
            },
        )
        .unwrap();
    assert_eq!(set_lead.suggestion.command, "team update");
    assert_eq!(set_lead.suggestion.call, "team.set_lead");
}

#[test]
fn p1_3_ui_present_rejects_entity_route_mismatch_deep_unverified_routes_and_unsafe_route_chars() {
    let h = harness();

    // Entity/route mismatch fails closed with InvalidRequest.
    assert_eq!(
        h.call::<UiPresent>(
            &h.tess,
            &UiPresentParams {
                navigation: vec![UiNavigation {
                    label: "Spoofed route".into(),
                    route: format!("/chat/dm/{}", h.bob_dm_id),
                    entity_kind: Some(UiEntityKind::Channel),
                    entity_id: Some(h.channel_id.clone()),
                }],
                ..UiPresentParams::default()
            },
        )
        .unwrap_err(),
        ErrorCode::InvalidRequest
    );

    // Unverified deep routes and unsafe query/fragment/percent/whitespace routes fail closed.
    for bad_route in [
        "/chat/dm/foo",
        "/budgets/0193a5c0-0000-7000-8000-000000000001",
        "/hr/hires/0193a5c0-0000-7000-8000-000000000001",
        "/tasks/FAKE",
        "/tasks?next=//evil",
        "/tasks#javascript:alert(1)",
        "/%2e%2e/x",
        "/tasks foo",
        "/tasks//evil",
    ] {
        assert_eq!(
            h.call::<UiPresent>(
                &h.tess,
                &UiPresentParams {
                    navigation: vec![UiNavigation {
                        label: "Bad nav".into(),
                        route: bad_route.into(),
                        entity_kind: None,
                        entity_id: None,
                    }],
                    ..UiPresentParams::default()
                },
            )
            .unwrap_err(),
            ErrorCode::InvalidRequest,
            "route `{bad_route}` must fail closed"
        );
    }

    // Explicit static top-level route AND matching verified entity route both succeed.
    let ok = h
        .call::<UiPresent>(
            &h.tess,
            &UiPresentParams {
                navigation: vec![
                    UiNavigation {
                        label: "All tasks".into(),
                        route: "/tasks".into(),
                        entity_kind: None,
                        entity_id: None,
                    },
                    UiNavigation {
                        label: "Target task".into(),
                        route: format!("/tasks/{}", h.task_key),
                        entity_kind: Some(UiEntityKind::Task),
                        entity_id: Some(h.task_key.clone()),
                    },
                ],
                ..UiPresentParams::default()
            },
        )
        .unwrap();
    assert_eq!(ok.ui_block.navigation.len(), 2);
    assert_eq!(ok.ui_block.navigation[0].route, "/tasks");
    assert_eq!(
        ok.ui_block.navigation[1].route,
        format!("/tasks/{}", h.task_key)
    );
}

#[test]
fn p1_4_p2_6_suggestion_store_enforces_size_control_chars_list_cap_and_caller_isolation() {
    let h = harness();

    // P1-4: > 16 KiB input fails closed BEFORE preflight with ErrorCode::TooLarge.
    let huge = "x".repeat(17 * 1024);
    assert_eq!(
        h.call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "task create".into(),
                input: map(json!({"title": "Big", "body": huge})),
                summary: "Oversized proposal".into(),
                rationale: None,
                expected_revision: None,
                ttl_seconds: None,
                dry_run: false,
            },
        )
        .unwrap_err(),
        ErrorCode::TooLarge
    );

    // P2-6: Control chars rejected in summary, rationale (except `\n`), withdraw reason, dismiss reason.
    for bad_summary in [
        "Bad\0summary",
        "Bad\x1b[31mANSI",
        "Bad\rsummary",
        "Bad\nsummary",
    ] {
        assert_eq!(
            h.call::<SuggestionCreate>(
                &h.tess,
                &SuggestionCreateParams {
                    command: "task create".into(),
                    input: map(json!({"title": "Ok"})),
                    summary: bad_summary.into(),
                    rationale: None,
                    expected_revision: None,
                    ttl_seconds: None,
                    dry_run: false,
                },
            )
            .unwrap_err(),
            ErrorCode::InvalidRequest,
            "summary `{bad_summary:?}` must be rejected"
        );
    }
    for bad_rationale in ["Line\0null", "Line\x1besc", "Line\rcr"] {
        assert_eq!(
            h.call::<SuggestionCreate>(
                &h.tess,
                &SuggestionCreateParams {
                    command: "task create".into(),
                    input: map(json!({"title": "Ok"})),
                    summary: "Valid summary".into(),
                    rationale: Some(bad_rationale.into()),
                    expected_revision: None,
                    ttl_seconds: None,
                    dry_run: false,
                },
            )
            .unwrap_err(),
            ErrorCode::InvalidRequest
        );
    }

    // Multi-line rationale with `\n` is allowed; Owner's pending proposal created cleanly.
    let owner_prop = h
        .call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "task create".into(),
                input: map(json!({"title": "Owner critical task"})),
                summary: "Owner critical proposal".into(),
                rationale: Some("Line 1\nLine 2".into()),
                expected_revision: None,
                ttl_seconds: None,
                dry_run: false,
            },
        )
        .unwrap()
        .suggestion;

    // Control characters in withdraw/dismiss `reason` fail closed.
    assert_eq!(
        h.call::<SuggestionWithdraw>(
            &h.tess,
            &SuggestionWithdrawParams {
                suggestion_id: owner_prop.id.clone(),
                reason: Some("Bad\x1breason".into()),
                dry_run: true,
            },
        )
        .unwrap_err(),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        h.call::<SuggestionDismiss>(
            &h.owner,
            &SuggestionDismissParams {
                suggestion_id: owner_prop.id.clone(),
                reason: Some("Bad\rreason".into()),
            },
        )
        .unwrap_err(),
        ErrorCode::InvalidRequest
    );

    // P1-4: Bob spamming 75 proposals (past per-caller cap 64) NEVER evicts Owner's pending
    // proposal, and `suggestion.list` is clamped to at most 50 entries.
    h.set_acting(Some(&h.bob));
    for i in 0..75 {
        h.call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "task create".into(),
                input: map(json!({"title": format!("Bob spam {i}")})),
                summary: format!("Bob spam {i}"),
                rationale: None,
                expected_revision: None,
                ttl_seconds: None,
                dry_run: false,
            },
        )
        .unwrap();
    }
    let bob_listed = h
        .call::<SuggestionList>(
            &h.bob,
            &SuggestionListParams {
                status: None,
                limit: Some(500),
            },
        )
        .unwrap();
    assert_eq!(bob_listed.suggestions.len(), 50);

    // Owner's pending proposal is completely intact!
    h.set_acting(Some(&h.owner));
    let still_there = h
        .call::<SuggestionInspect>(
            &h.owner,
            &SuggestionInspectParams {
                suggestion_id: owner_prop.id.clone(),
            },
        )
        .unwrap();
    assert_eq!(still_there.suggestion.id, owner_prop.id);
    assert_eq!(still_there.suggestion.status, SuggestionStatus::Pending);

    // Global cap (512): when 8 callers each hold 64 pending proposals, a 9th caller gets
    // RateLimited and NO existing caller's pending proposal is evicted.
    let h_cap = harness();
    let mut callers = vec![h_cap.owner.clone(), h_cap.bob.clone()];
    for i in 0..7 {
        let member = next_id();
        h_cap
            .office
            .submit_org(
                &h_cap.owner,
                OrgCommand::MemberAddPerson(MemberAddPersonParams {
                    command_id: next_id(),
                    member_id: member.clone(),
                    name: format!("Caller {i}"),
                    handle: Some(format!("caller{i}")),
                    home_team_id: Some(h_cap.team_id.clone()),
                    contact: None,
                }),
            )
            .unwrap()
            .unwrap();
        callers.push(member);
    }
    let mut first_ids = Vec::new();
    for caller in &callers[0..8] {
        h_cap.set_acting(Some(caller));
        for j in 0..64 {
            let created = h_cap
                .call::<SuggestionCreate>(
                    &h_cap.tess,
                    &SuggestionCreateParams {
                        command: "task create".into(),
                        input: map(json!({"title": format!("t-{j}")})),
                        summary: format!("Proposal {j}"),
                        rationale: None,
                        expected_revision: None,
                        ttl_seconds: None,
                        dry_run: false,
                    },
                )
                .unwrap();
            if j == 0 {
                first_ids.push((caller.clone(), created.suggestion.id));
            }
        }
    }
    h_cap.set_acting(Some(&callers[8]));
    assert_eq!(
        h_cap
            .call::<SuggestionCreate>(
                &h_cap.tess,
                &SuggestionCreateParams {
                    command: "task create".into(),
                    input: map(json!({"title": "overflow"})),
                    summary: "Overflow proposal".into(),
                    rationale: None,
                    expected_revision: None,
                    ttl_seconds: None,
                    dry_run: false,
                },
            )
            .unwrap_err(),
        ErrorCode::RateLimited
    );
    for (caller, first_id) in first_ids {
        assert!(
            h_cap
                .call::<SuggestionInspect>(
                    &caller,
                    &SuggestionInspectParams {
                        suggestion_id: first_id,
                    },
                )
                .is_ok(),
            "caller `{caller}` pending proposal must not be evicted"
        );
    }
}

#[test]
fn pass3_non_tess_dm_fails_closed_and_builtin_move_status_rejected() {
    let h = harness();

    // Create a private 1:1 DM between Owner and Bob (excluding Tess) and post a message inside it.
    let owner_bob_dm_id = h
        .office
        .dm_open(
            &h.owner,
            DmOpenParams {
                participants: vec![h.bob.clone()],
                name: None,
                conversation_id: None,
            },
        )
        .unwrap()
        .unwrap()
        .conversation
        .id;
    let owner_bob_msg_id = send_dm_msg(&h.office, &h.owner, &owner_bob_dm_id, "Private 1:1 note");
    let nonexistent_msg_id = next_id();

    // P1: Enforce true `caller ∩ tess` intersection — private non-Tess DM `{owner, bob}` MUST fail
    // closed for `msg send`, `channel mark-read`, and `msg done` when Tess acts for Owner OR Bob,
    // with zero message-existence oracle.
    for caller in [h.owner.as_str(), h.bob.as_str()] {
        h.set_acting(Some(caller));

        assert_eq!(
            h.call::<SuggestionCreate>(
                &h.tess,
                &SuggestionCreateParams {
                    command: "msg send".into(),
                    input: map(json!({"to": owner_bob_dm_id, "body": "probe"})),
                    summary: "Send to non-Tess DM".into(),
                    rationale: None,
                    expected_revision: None,
                    ttl_seconds: None,
                    dry_run: false,
                },
            )
            .unwrap_err(),
            ErrorCode::NotAuthorized,
            "msg send to non-Tess DM must fail closed for caller `{caller}`"
        );

        assert_eq!(
            h.call::<SuggestionCreate>(
                &h.tess,
                &SuggestionCreateParams {
                    command: "channel mark-read".into(),
                    input: map(json!({"channel": owner_bob_dm_id})),
                    summary: "Mark non-Tess DM read".into(),
                    rationale: None,
                    expected_revision: None,
                    ttl_seconds: None,
                    dry_run: false,
                },
            )
            .unwrap_err(),
            ErrorCode::NotAuthorized,
            "channel mark-read on non-Tess DM must fail closed for caller `{caller}`"
        );

        let done_dm_err = h
            .call_err::<SuggestionCreate>(
                &h.tess,
                &SuggestionCreateParams {
                    command: "msg done".into(),
                    input: map(json!({"id": owner_bob_msg_id})),
                    summary: "Mark non-Tess DM msg done".into(),
                    rationale: None,
                    expected_revision: None,
                    ttl_seconds: None,
                    dry_run: false,
                },
            )
            .unwrap_err();
        let done_missing_err = h
            .call_err::<SuggestionCreate>(
                &h.tess,
                &SuggestionCreateParams {
                    command: "msg done".into(),
                    input: map(json!({"id": nonexistent_msg_id})),
                    summary: "Mark nonexistent msg done".into(),
                    rationale: None,
                    expected_revision: None,
                    ttl_seconds: None,
                    dry_run: false,
                },
            )
            .unwrap_err();
        assert_eq!(done_dm_err.code, ErrorCode::NotFound);
        assert_eq!(done_dm_err.code, done_missing_err.code);
        assert_eq!(done_dm_err.message, done_missing_err.message);
    }

    // P2-A: Built-in agents (Tess, HR) must also be rejected for `agent move`, `agent pause`,
    // and `agent resume` during preflight (matching `agent_lifecycle.rs`).
    h.set_acting(Some(&h.owner));
    let dest_team = next_id();
    h.office
        .submit_org(
            &h.owner,
            OrgCommand::TeamCreate(TeamCreateParams {
                command_id: next_id(),
                team_id: dest_team.clone(),
                name: "TargetTeam".into(),
                owner_team_id: None,
            }),
        )
        .unwrap()
        .unwrap();
    let hr_id = h
        .office
        .list_employees()
        .unwrap()
        .into_iter()
        .find(|e| e.builtin_key == Some(org_office::organization_family::records::BuiltinKey::Hr))
        .expect("built-in HR")
        .id
        .to_string();
    for builtin in [h.tess.as_str(), "tess", hr_id.as_str(), "hr"] {
        assert_eq!(
            h.call::<SuggestionCreate>(
                &h.tess,
                &SuggestionCreateParams {
                    command: "agent move".into(),
                    input: map(json!({"member": builtin, "to_team_id": dest_team})),
                    summary: "Move built-in".into(),
                    rationale: None,
                    expected_revision: Some(1),
                    ttl_seconds: None,
                    dry_run: false,
                },
            )
            .unwrap_err(),
            ErrorCode::NotAuthorized,
            "built-in `{builtin}` must never pass agent move preflight"
        );
        for cmd in ["agent pause", "agent resume"] {
            assert_eq!(
                h.call::<SuggestionCreate>(
                    &h.tess,
                    &SuggestionCreateParams {
                        command: cmd.into(),
                        input: map(json!({"member": builtin})),
                        summary: format!("{cmd} built-in"),
                        rationale: None,
                        expected_revision: Some(1),
                        ttl_seconds: None,
                        dry_run: false,
                    },
                )
                .unwrap_err(),
                ErrorCode::NotAuthorized,
                "built-in `{builtin}` must never pass `{cmd}` preflight"
            );
        }
    }
}
