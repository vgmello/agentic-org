//! Unit tests for Ask Tess prompt/viewport sanitization, `TurnHub` replay/bounds/isolation,
//! and `suggestion.apply` revision reconciliation (`TASK-80.4` Part B).

use std::collections::BTreeMap;

use org_proto::tess::{TessViewportContext, UiEntityKind, MAX_VIEWPORT_CONTEXT_BYTES};
use org_proto::ErrorCode;

use crate::turns::ask_partition;
use crate::viewport::{compose_ask_instruction, validate_and_render_viewport};

#[test]
fn viewport_context_is_appended_strictly_last_behind_untrusted_fence() {
    let prompt = "What tasks are blocked right now?";
    let vp = r#"{"route":"/tasks/T-A1","entity_id":"T-A1"}"#;
    let combined = compose_ask_instruction(prompt, Some(vp), false);
    let prompt_idx = combined.find(prompt).unwrap();
    let fence_open = combined.find("<<<UNTRUSTED_VIEWPORT_CONTEXT").unwrap();
    let fence_close = combined
        .find("<<<END_UNTRUSTED_VIEWPORT_CONTEXT>>>")
        .unwrap();
    assert!(prompt_idx < fence_open);
    assert!(fence_open < fence_close);
    assert!(combined.ends_with("<<<END_UNTRUSTED_VIEWPORT_CONTEXT>>>"));

    let reset = compose_ask_instruction(prompt, Some(vp), true);
    assert!(reset.starts_with("[Session reset by caller:"));
    assert!(reset.ends_with("<<<END_UNTRUSTED_VIEWPORT_CONTEXT>>>"));
    assert_eq!(ask_partition("owner-1"), "tess:ask:owner-1");
}

#[test]
fn oversized_or_malformed_viewport_context_is_rejected_and_fence_markers_scrubbed() {
    let valid = TessViewportContext {
        route: Some("/tasks/T-A1".into()),
        route_label: Some("Task <<<breakout>>>".into()),
        entity_kind: Some(UiEntityKind::Task),
        entity_id: Some("T-A1".into()),
        entity_title: Some("Ship Tess".into()),
        decision_id: None,
        filters: BTreeMap::from([("status".into(), "blocked".into())]),
    };
    let rendered = validate_and_render_viewport(&valid).unwrap();
    assert!(rendered.len() <= MAX_VIEWPORT_CONTEXT_BYTES);
    assert!(!rendered.contains("<<<"));
    assert!(!rendered.contains(">>>"));

    // Oversized UTF-8 multibyte viewport context (> 800 B) fails cleanly with InvalidRequest.
    let big = TessViewportContext {
        route: Some("/tasks".into()),
        entity_title: Some("é".repeat(450)), // 900 UTF-8 bytes
        ..TessViewportContext::default()
    };
    let err = validate_and_render_viewport(&big).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);

    // Unsafe external or traversal route fails closed.
    for bad in [
        "https://evil.example/a",
        "//evil.example/a",
        "/tasks/../secret",
    ] {
        let vp = TessViewportContext {
            route: Some(bad.into()),
            ..TessViewportContext::default()
        };
        assert_eq!(
            validate_and_render_viewport(&vp).unwrap_err().code,
            ErrorCode::InvalidRequest,
            "route `{bad}` must be rejected"
        );
    }
}

#[test]
fn apply_revision_parsing_and_reconciliation_enforces_exact_match() {
    assert_eq!(
        crate::apply::parse_caller_revision(Some(3), None).unwrap(),
        Some(3)
    );
    assert_eq!(
        crate::apply::parse_caller_revision(None, Some("rev-3")).unwrap(),
        Some(3)
    );
    assert_eq!(
        crate::apply::parse_caller_revision(Some(3), Some("3")).unwrap(),
        Some(3)
    );
    assert_eq!(
        crate::apply::parse_caller_revision(Some(3), Some("4"))
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        crate::apply::parse_caller_revision(None, Some("not-a-rev"))
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        crate::apply::reconcile_revision(Some(2), Some(3))
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert_eq!(
        crate::apply::reconcile_revision(Some(2), Some(2)).unwrap(),
        Some(2)
    );
}

#[test]
fn apply_transport_forbids_privileged_and_recursive_methods() {
    for forbidden in [
        "suggestion.apply",
        "suggestion.create",
        "tess.ask",
        "tess.turn_events",
        "auth.login_request",
        "provider.credential_status",
        "member.session_start",
        "member.external_approve",
        "git.push",
        "office.remove",
        "office.restart",
        "tasks.delete",
    ] {
        assert!(
            !crate::apply::is_permitted_apply_call(forbidden, "tasks.create", "tasks.create"),
            "`{forbidden}` must be blocked by ServerApplyTransport"
        );
    }
    assert!(crate::apply::is_permitted_apply_call(
        "tasks.create",
        "tasks.create",
        "tasks.create"
    ));
    assert!(crate::apply::is_permitted_apply_call(
        "org.snapshot",
        "tasks.create",
        "tasks.create"
    ));
}

#[test]
fn apply_translator_covers_every_eligible_confirm_required_command() {
    let table = org_command_catalogue::table();
    let mut covered = 0usize;
    for row in table.rows() {
        if table.mutation_policy(row) != org_command_catalogue::MutationPolicy::ConfirmRequired {
            continue;
        }
        if !crate::preflight::ALLOWED_SUGGESTION_CALLS.contains(&row.call) {
            continue;
        }
        let input: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(row.example.input).unwrap_or_else(|e| {
                panic!(
                    "example input for `{}` is not a JSON object: {e}",
                    row.name()
                )
            });
        let rev = (row.if_match() != org_command_catalogue::IfMatch::No).then_some(1);
        let mut seq = 0u64;
        let mut next_id = || {
            seq += 1;
            format!("0193a5c0-0000-7000-8000-{seq:012x}")
        };
        let prepared =
            crate::apply::prepare_apply_rpc(&mut next_id, row.resource, row.verb, &input, rev)
                .unwrap_or_else(|| {
                    panic!(
                        "eligible ConfirmRequired command `{}` (call `{}`) missing from org-tess apply translator",
                        row.name(),
                        row.call
                    )
                })
                .unwrap_or_else(|e| {
                    panic!(
                        "example input for `{}` failed apply translation: {e:?}",
                        row.name()
                    )
                });
        assert!(
            crate::apply::is_permitted_apply_call(prepared.method, row.call, prepared.method),
            "`{}` translated to forbidden RPC `{}`",
            row.name(),
            prepared.method
        );
        covered += 1;
    }
    assert!(
        covered >= 45,
        "expected >= 45 eligible ConfirmRequired commands covered, got {covered}"
    );
}

fn test_uuid(seq: u64) -> String {
    let ms = org_office::organization_family::testkit::CREATION_AT.0;
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

struct UiRouteFixture {
    svc: crate::TessService,
    tess: String,
    owner: String,
    team_id: String,
    task_key: String,
    project_id: String,
    project_key: String,
    role_id: String,
    process_doc_id: String,
    channel_id: String,
    dm_id: String,
    office_id: String,
    budget_id: String,
    hire_request_id: String,
}

fn seed_role_and_hire(office: &org_office::Office, owner: &str, team_id: &str) -> (String, String) {
    use org_proto::hiring::{
        HireProposalWire, HrHireDraft, HrHireDraftParams, RoleCreate, RoleCreateParams,
    };
    use org_proto::{Outcome, Request};

    let role_id = test_uuid(110);
    let role_req = Request::new::<RoleCreate>(
        1,
        &RoleCreateParams {
            command_id: test_uuid(111),
            role_id: role_id.clone(),
            key: Some("staff-eng".into()),
            name: "Staff Engineer".into(),
            body: "Own architecture.".into(),
        },
    )
    .unwrap();
    let role_resp = office.dispatch_hiring_rpc(Some(owner), &role_req).unwrap();
    assert!(matches!(role_resp.outcome, Outcome::Result(_)));

    let hire_request_id = test_uuid(112);
    let hire_req = Request::new::<HrHireDraft>(
        2,
        &HrHireDraftParams {
            command_id: test_uuid(113),
            request_id: hire_request_id.clone(),
            home_team_id: team_id.to_owned(),
            role_id: Some(role_id.clone()),
            proposal: HireProposalWire {
                name: "Nova".into(),
                handle: "nova".into(),
                ..HireProposalWire::default()
            },
            reason: "Expand backend capacity".into(),
        },
    )
    .unwrap();
    let hire_resp = office.dispatch_hiring_rpc(Some(owner), &hire_req).unwrap();
    assert!(matches!(hire_resp.outcome, Outcome::Result(_)));

    (role_id, hire_request_id)
}

fn build_ui_route_fixture() -> UiRouteFixture {
    use std::sync::Arc;

    use org_office::rules::OrgCommand;
    use org_office::{Office, TestOffice};
    use org_proto::budgets::{BudgetPeriodKind, BudgetScopeKind, BudgetSetParams, BudgetUnit};
    use org_proto::chat::{ChatChannelCreateParams, DmOpenParams};
    use org_proto::org::ProjectCreateParams;
    use org_proto::processes::{
        DocumentKindWire, DocumentStatusWire, ProcessScopeWire, ProcessWriteParams,
    };
    use org_proto::tasks::TasksCreateParams;

    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner = office.owner_member_id().to_owned();
    let tess = office
        .chief_of_staff()
        .unwrap()
        .expect("Tess")
        .id
        .to_string();
    let team_id = office.org_snapshot().unwrap().teams[0].id.clone();
    let office_id = office.info().office_id.clone();

    let project_id = test_uuid(101);
    let proj_params: ProjectCreateParams = serde_json::from_value(serde_json::json!({
        "command_id": test_uuid(102),
        "project_id": project_id,
        "name": "Checkout",
        "slug": "checkout",
        "owner_team_id": team_id,
        "contributing_team_ids": [],
    }))
    .unwrap();
    office
        .submit_org(&owner, OrgCommand::ProjectCreate(proj_params))
        .unwrap()
        .unwrap();
    let project_key = office.inspect_project(&project_id).unwrap().unwrap().key;

    let task_key = office
        .task_create(
            &owner,
            TasksCreateParams {
                command_id: test_uuid(103),
                task_id: test_uuid(104),
                team_id: Some(team_id.clone()),
                project_id: Some(project_id.clone()),
                assigned_team_id: None,
                assignee_id: None,
                task_type: Some("task".into()),
                work_type: None,
                title: "Verify canonical routes".into(),
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
        .unwrap()
        .task
        .key;

    let process_doc_id = office
        .process_write(
            &owner,
            ProcessWriteParams {
                command_id: test_uuid(105),
                document_id: None,
                kind: DocumentKindWire::Process,
                scope: ProcessScopeWire::Organization,
                key: Some("release-process".into()),
                title: "Release Process".into(),
                status: DocumentStatusWire::Current,
                owner_employee_id: Some(owner.clone()),
                overridable: true,
                body_markdown: "## Steps\n1. Verify checks.\n".into(),
                expected_revision: None,
            },
        )
        .unwrap()
        .unwrap()
        .document_id;

    let channel_id = office
        .create_channel(
            &owner,
            ChatChannelCreateParams {
                id: None,
                name: "ops-room".into(),
                members: vec![owner.clone(), tess.clone()],
            },
        )
        .unwrap()
        .unwrap()
        .channel
        .id;

    let dm_id = office
        .dm_open(
            &owner,
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

    let budget_id = office
        .budget_set(
            &owner,
            BudgetSetParams {
                operation_id: test_uuid(106),
                scope_kind: BudgetScopeKind::Agent,
                task_id: None,
                agent_id: Some(tess.clone()),
                release_id: None,
                period_kind: BudgetPeriodKind::Day,
                unit: BudgetUnit::MicroUsd,
                limit_amount: 5_000_000,
                time_zone: Some("UTC".into()),
                time_zone_source: None,
                reason: Some("unit test cap".into()),
            },
        )
        .unwrap()
        .unwrap()
        .budget
        .id;

    let (role_id, hire_request_id) = seed_role_and_hire(&office, &owner, &team_id);

    let acting_owner = owner.clone();
    let tess_clone = tess.clone();
    let svc = crate::TessService::new(
        office,
        Arc::new(move |member| (member == tess_clone).then(|| acting_owner.clone())),
        |_| crate::DelegationAccess::Open,
    );

    UiRouteFixture {
        svc,
        tess,
        owner,
        team_id,
        task_key,
        project_id,
        project_key,
        role_id,
        process_doc_id,
        channel_id,
        dm_id,
        office_id,
        budget_id,
        hire_request_id,
    }
}

fn call_ui_present(
    svc: &crate::TessService,
    member: &str,
    params: &org_proto::tess::UiPresentParams,
) -> Result<org_proto::tess::UiPresentResult, org_proto::ApiError> {
    use org_proto::tess::UiPresent;
    use org_proto::{Outcome, Request};

    let req = Request::new::<UiPresent>(1, params).unwrap();
    let resp = svc.dispatch(Some(member), &req).unwrap();
    match resp.outcome {
        Outcome::Result(val) => Ok(serde_json::from_value(val).unwrap()),
        Outcome::Error(err) => Err(err),
    }
}

#[test]
fn ui_present_returns_canonical_routes_for_all_ten_entity_kinds() {
    use org_proto::tess::{UiEntityLinkInput, UiPresentParams};

    let f = build_ui_route_fixture();
    let presented = call_ui_present(
        &f.svc,
        &f.tess,
        &UiPresentParams {
            title: Some("All canonical entities".into()),
            entity_links: vec![
                UiEntityLinkInput {
                    kind: UiEntityKind::Task,
                    id: f.task_key.clone(),
                    label: None,
                },
                UiEntityLinkInput {
                    kind: UiEntityKind::Project,
                    id: f.project_key.clone(),
                    label: None,
                },
                UiEntityLinkInput {
                    kind: UiEntityKind::Team,
                    id: f.team_id.clone(),
                    label: None,
                },
                UiEntityLinkInput {
                    kind: UiEntityKind::Member,
                    id: "owner".into(),
                    label: None,
                },
                UiEntityLinkInput {
                    kind: UiEntityKind::Role,
                    id: "staff-eng".into(),
                    label: None,
                },
                UiEntityLinkInput {
                    kind: UiEntityKind::Process,
                    id: "release-process".into(),
                    label: None,
                },
                UiEntityLinkInput {
                    kind: UiEntityKind::Channel,
                    id: "#ops-room".into(),
                    label: None,
                },
                UiEntityLinkInput {
                    kind: UiEntityKind::Office,
                    id: f.office_id.clone(),
                    label: None,
                },
                UiEntityLinkInput {
                    kind: UiEntityKind::Budget,
                    id: f.budget_id.clone(),
                    label: None,
                },
                UiEntityLinkInput {
                    kind: UiEntityKind::HireRequest,
                    id: f.hire_request_id.clone(),
                    label: None,
                },
            ],
            ..UiPresentParams::default()
        },
    )
    .unwrap();

    let routes: Vec<(UiEntityKind, &str)> = presented
        .ui_block
        .entity_links
        .iter()
        .map(|l| (l.kind, l.route.as_str()))
        .collect();
    assert_eq!(
        routes,
        vec![
            (UiEntityKind::Task, format!("/work/{}", f.task_key).as_str()),
            (
                UiEntityKind::Project,
                format!("/projects/{}", f.project_id).as_str()
            ),
            (UiEntityKind::Team, format!("/teams/{}", f.team_id).as_str()),
            (
                UiEntityKind::Member,
                format!("/members/{}", f.owner).as_str()
            ),
            (
                UiEntityKind::Role,
                format!("/settings/roles/{}", f.role_id).as_str()
            ),
            (
                UiEntityKind::Process,
                format!("/settings/processes/documents/{}", f.process_doc_id).as_str()
            ),
            (
                UiEntityKind::Channel,
                format!("/chat?c={}", f.channel_id).as_str()
            ),
            (UiEntityKind::Office, "/settings/offices"),
            (UiEntityKind::Budget, "/settings/costs"),
            (UiEntityKind::HireRequest, "/teams/hires"),
        ]
    );
    // Project link preserves its human key `P-1` while routing via UUIDv7 `id`.
    assert_eq!(
        presented.ui_block.entity_links[1].key.as_deref(),
        Some(f.project_key.as_str())
    );
    assert_eq!(presented.ui_block.entity_links[1].id, f.project_id);

    // DMs are never linkable via `UiEntityKind::Channel`.
    let dm_err = call_ui_present(
        &f.svc,
        &f.tess,
        &UiPresentParams {
            entity_links: vec![UiEntityLinkInput {
                kind: UiEntityKind::Channel,
                id: f.dm_id,
                label: None,
            }],
            ..UiPresentParams::default()
        },
    )
    .unwrap_err();
    assert_eq!(dm_err.code, ErrorCode::NotFound);
}

fn present_nav(
    f: &UiRouteFixture,
    route: &str,
    entity_kind: Option<UiEntityKind>,
    entity_id: Option<&str>,
) -> Result<org_proto::tess::UiPresentResult, org_proto::ApiError> {
    use org_proto::tess::{UiNavigation, UiPresentParams};

    call_ui_present(
        &f.svc,
        &f.tess,
        &UiPresentParams {
            navigation: vec![UiNavigation {
                label: "Navigate".into(),
                route: route.to_owned(),
                entity_kind,
                entity_id: entity_id.map(str::to_owned),
            }],
            ..UiPresentParams::default()
        },
    )
}

#[test]
fn ui_present_navigation_accepts_static_routes_and_verified_channel() {
    let f = build_ui_route_fixture();

    for route in crate::ui::STATIC_TOP_LEVEL_ROUTES {
        let res = present_nav(&f, route, None, None)
            .unwrap_or_else(|e| panic!("static route `{route}` should be accepted: {e:?}"));
        assert_eq!(res.ui_block.navigation[0].route, *route);
    }

    // Verified channel navigation accepts `/chat?c={channel_id}` (both by ID and by `#name`).
    let channel_route = format!("/chat?c={}", f.channel_id);
    for entity_ref in [f.channel_id.as_str(), "#ops-room"] {
        let ok = present_nav(
            &f,
            &channel_route,
            Some(UiEntityKind::Channel),
            Some(entity_ref),
        )
        .unwrap();
        assert_eq!(ok.ui_block.navigation[0].route, channel_route);
        assert_eq!(
            ok.ui_block.navigation[0].entity_id.as_deref(),
            Some(f.channel_id.as_str())
        );
    }
}

#[test]
fn ui_present_navigation_rejects_dead_routes_and_unsafe_query_strings() {
    let f = build_ui_route_fixture();
    let channel_route = format!("/chat?c={}", f.channel_id);

    // Dead top-level paths and unverified/arbitrary query strings fail closed.
    let unverified_rejects = [
        "/tasks",
        "/org",
        "/offices",
        "/budgets",
        "/processes",
        "/hr",
        "/work?next=//evil",
        "/chat?c=fake",
        channel_route.as_str(),
        "/chat?c=fake&extra=1",
        "/settings?tab=security",
        "/projects?new=1",
        "/work#frag",
        "/%2e%2e/secret",
    ];
    for bad_route in unverified_rejects {
        let err = present_nav(&f, bad_route, None, None).unwrap_err();
        assert_eq!(
            err.code,
            ErrorCode::InvalidRequest,
            "unverified route `{bad_route}` must fail closed"
        );
    }

    // Query injection even WITH a verified entity_kind/entity_id fails closed.
    let verified_query_injections = [
        (
            format!("/chat?c={}&extra=1", f.channel_id),
            UiEntityKind::Channel,
            f.channel_id.as_str(),
        ),
        (
            format!("/chat?c={}?extra=1", f.channel_id),
            UiEntityKind::Channel,
            f.channel_id.as_str(),
        ),
        (
            format!("/chat?c={}", f.dm_id),
            UiEntityKind::Channel,
            f.channel_id.as_str(),
        ),
        (
            format!("/work/{}?next=//evil", f.task_key),
            UiEntityKind::Task,
            f.task_key.as_str(),
        ),
        (
            format!("/projects/{}", f.project_key),
            UiEntityKind::Project,
            f.project_id.as_str(),
        ),
    ];
    for (route, kind, id) in verified_query_injections {
        let err = present_nav(&f, &route, Some(kind), Some(id)).unwrap_err();
        assert_eq!(
            err.code,
            ErrorCode::InvalidRequest,
            "tampered verified route `{route}` must fail closed"
        );
    }

    assert!(crate::ui::clean_route("/chat?c=chan-1", false).is_err());
    assert!(crate::ui::clean_route("/chat?c=chan-1", true).is_ok());
    assert!(crate::ui::clean_route("/chat?c=chan-1&x=1", true).is_err());
    assert!(crate::ui::clean_route("/chat?c=", true).is_err());
}

#[test]
fn apply_extract_entity_link_returns_canonical_routes_for_all_receipt_variants() {
    use serde_json::json;

    let cases: &[(&str, serde_json::Value, Option<&str>, Option<&str>)] = &[
        (
            "tasks.create",
            json!({"id": "t1", "key": "T-A1", "title": "Fix routing"}),
            Some("/work/T-A1"),
            Some("Fix routing"),
        ),
        (
            "tasks.comment",
            json!({"task_id": "t1", "key": "T-A2"}),
            Some("/work/T-A2"),
            Some("T-A2"),
        ),
        (
            "tasks.checklist.add",
            json!({"task_id": "t1", "key": "T-A3"}),
            Some("/work/T-A3"),
            Some("T-A3"),
        ),
        (
            "project.create",
            json!({"id": "0193a5c0-0000-7000-8000-000000000010", "status": "committed"}),
            Some("/projects/0193a5c0-0000-7000-8000-000000000010"),
            Some("0193a5c0-0000-7000-8000-000000000010"),
        ),
        (
            "project.update",
            json!({"id": "0193a5c0-0000-7000-8000-000000000010", "key": "P-1", "name": "Checkout"}),
            Some("/projects/0193a5c0-0000-7000-8000-000000000010"),
            Some("Checkout"),
        ),
        (
            "project.add_team",
            json!({"id": "0193a5c0-0000-7000-8000-000000000099", "status": "committed"}),
            None,
            None,
        ),
        (
            "project.add_team",
            json!({"id": "0193a5c0-0000-7000-8000-000000000099", "project_id": "0193a5c0-0000-7000-8000-000000000010", "name": "Checkout"}),
            Some("/projects/0193a5c0-0000-7000-8000-000000000010"),
            Some("Checkout"),
        ),
        (
            "team.create",
            json!({"id": "team-1", "name": "Platform"}),
            Some("/teams/team-1"),
            Some("Platform"),
        ),
        (
            "team.set_max_members",
            json!({"id": "team-2", "revision": 2, "generation": 1}),
            Some("/teams/team-2"),
            Some("team-2"),
        ),
        (
            "team.set_title",
            json!({"id": "mem-1", "status": "committed"}),
            Some("/members/mem-1"),
            Some("mem-1"),
        ),
        (
            "member.add_person",
            json!({"id": "mem-2", "name": "Bob"}),
            Some("/members/mem-2"),
            Some("Bob"),
        ),
        (
            "member.agent_update",
            json!({"id": "agent-1", "revision": 2, "generation": 1}),
            Some("/members/agent-1"),
            Some("agent-1"),
        ),
        (
            "role.create",
            json!({"id": "role-1", "revision": 1, "generation": 1}),
            Some("/settings/roles/role-1"),
            Some("role-1"),
        ),
        (
            "process.write",
            json!({"document_id": "doc-1", "revision": 1, "document": {"id": "doc-1", "title": "Release Process"}}),
            Some("/settings/processes/documents/doc-1"),
            Some("Release Process"),
        ),
        (
            "chat.channel_create",
            json!({"id": "chan-1", "name": "ops-room"}),
            Some("/chat?c=chan-1"),
            Some("ops-room"),
        ),
        (
            "chat.channel_join",
            json!({"conversation_id": "chan-2", "name": "alerts", "is_member": true}),
            Some("/chat?c=chan-2"),
            Some("alerts"),
        ),
        (
            "hr.hire",
            json!({"request_id": "hire-1", "state": "hired"}),
            Some("/teams/hires"),
            Some("Hire hire-1"),
        ),
        (
            "hr.hire.draft",
            json!({"request_id": "hire-2", "state": "drafting"}),
            Some("/teams/hires"),
            Some("Hire hire-2"),
        ),
        (
            "msg.done",
            json!({"id": "msg-1", "state": "done"}),
            None,
            None,
        ),
        (
            "team.delete",
            json!({"id": "team-9", "name": "Retired"}),
            None,
            None,
        ),
        (
            "member.remove",
            json!({"id": "mem-9", "name": "Departed"}),
            None,
            None,
        ),
        (
            "member.agent_offboard",
            json!({"member_id": "agent-9", "name": "FormerBot"}),
            None,
            None,
        ),
        (
            "member.offboard",
            json!({"member_id": "mem-10", "name": "FormerPerson"}),
            None,
            None,
        ),
        (
            "team.update",
            json!({"id": "   ", "name": "Blank"}),
            None,
            None,
        ),
    ];

    for (call, payload, expected_href, expected_label) in cases {
        let (href, label) = crate::apply::extract_entity_link(call, payload);
        assert_eq!(
            href.as_deref(),
            *expected_href,
            "unexpected href for `{call}` with payload {payload}"
        );
        assert_eq!(
            label.as_deref(),
            *expected_label,
            "unexpected label for `{call}` with payload {payload}"
        );
    }
}

#[test]
fn clean_route_enforces_strict_channel_id_allowlist() {
    assert_eq!(
        crate::ui::clean_route("/chat?c=chan_01-AZ", true).unwrap(),
        "/chat?c=chan_01-AZ"
    );
    for bad in [
        "/chat?c=",
        "/chat?c=chan;drop",
        "/chat?c=chan:1",
        "/chat?c=chan!bad",
        "/chat?c=chan.dot",
    ] {
        assert!(
            crate::ui::clean_route(bad, true).is_err(),
            "expected `{bad}` to be rejected"
        );
    }
}

#[test]
fn task_transition_apply_translation_matches_cli_and_office_parity() {
    use serde_json::{json, Map, Value};

    let prep = |raw: Value| {
        let map: Map<String, Value> = raw.as_object().unwrap().clone();
        let mut seq = 0u64;
        let mut next_id = || {
            seq += 1;
            format!("0193a5c0-0000-7000-8000-{seq:012x}")
        };
        crate::apply::prepare_apply_rpc(&mut next_id, "task", "transition", &map, Some(3))
            .expect("task transition route")
    };

    // `--note` alone on `blocked` maps to `blocked_reason`, clearing `note`.
    let note_alias = prep(json!({
        "task": "T-A2",
        "status": "blocked",
        "note": "Waiting on DNS",
        "waiting_on": "none",
    }))
    .expect("note alias on blocked");
    assert_eq!(note_alias.params["blocked_reason"], "Waiting on DNS");
    assert!(note_alias.params.get("note").is_none());
    assert_eq!(note_alias.params["waiting_on_owner"], false);

    // `reason` + `waiting_on: "owner"` on `blocked` succeeds.
    let reason_owner = prep(json!({
        "task": "T-A2",
        "status": "blocked",
        "reason": "Need owner approval",
        "waiting_on": "owner",
    }))
    .expect("reason + owner on blocked");
    assert_eq!(reason_owner.params["blocked_reason"], "Need owner approval");
    assert_eq!(reason_owner.params["waiting_on_owner"], true);

    // Non-blocked with valid `reason` succeeds.
    let non_blocked = prep(json!({
        "task": "T-A2",
        "status": "in_progress",
        "reason": "Owner approved",
    }))
    .expect("non-blocked reason");
    assert_eq!(non_blocked.params["reason"], "Owner approved");
    assert!(non_blocked.params.get("blocked_reason").is_none());

    // Fail-closed parity cases:
    // - `waiting_on_owner: false` / `true` and `waiting_on: "none"` / `"owner"` on non-blocked
    // - whitespace-only `reason` (blocked and non-blocked) and whitespace-only `note` on blocked
    // - `blocked_reason` on non-blocked
    for bad in [
        json!({"task": "T-A2", "status": "in_progress", "waiting_on_owner": false}),
        json!({"task": "T-A2", "status": "in_progress", "waiting_on_owner": true}),
        json!({"task": "T-A2", "status": "in_progress", "waiting_on": "none"}),
        json!({"task": "T-A2", "status": "in_progress", "waiting_on": "owner"}),
        json!({"task": "T-A2", "status": "in_progress", "reason": "   \t "}),
        json!({"task": "T-A2", "status": "blocked", "reason": "   \n "}),
        json!({"task": "T-A2", "status": "blocked", "note": "   \t "}),
        json!({"task": "T-A2", "status": "blocked"}),
        json!({"task": "T-A2", "status": "in_progress", "blocked_reason": "wrong"}),
    ] {
        let err = prep(bad.clone())
            .err()
            .unwrap_or_else(|| panic!("expected invalid_request for {bad}"));
        assert_eq!(err.code, ErrorCode::InvalidRequest, "{bad}");
    }
}
