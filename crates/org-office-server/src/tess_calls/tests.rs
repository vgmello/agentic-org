use std::sync::{Arc, Mutex};

use org_office::rules::OrgCommand;
use org_office::{Office, TestOffice};
use org_proto::budgets::{BudgetPeriodKind, BudgetScopeKind, BudgetSetParams, BudgetUnit};
use org_proto::chat::{ChatChannelCreateParams, DmOpenParams};
use org_proto::org::MemberAddPersonParams;
use org_proto::tasks::TasksCreateParams;
use org_proto::tess::{
    SuggestionCreate, SuggestionCreateParams, SuggestionCreateResult, SuggestionDismiss,
    SuggestionDismissParams, SuggestionInspect, SuggestionInspectParams, SuggestionInspectResult,
    SuggestionList, SuggestionListParams, SuggestionListResult, SuggestionStatus,
    SuggestionWithdraw, SuggestionWithdrawParams, UiEntityKind, UiEntityLinkInput, UiFollowUp,
    UiMetric, UiNavigation, UiPresent, UiPresentParams, UiPresentResult,
};
use org_proto::{Call, ErrorCode, Outcome, Request};
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
        let req = Request::new::<C>(1, params).unwrap();
        let resp = self
            .svc
            .dispatch(Some(member), &req)
            .expect("tess_calls handles method");
        match resp.outcome {
            Outcome::Result(val) => Ok(serde_json::from_value(val).unwrap()),
            Outcome::Error(err) => Err(err.code),
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
