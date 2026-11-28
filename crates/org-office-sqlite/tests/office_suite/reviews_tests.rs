//! Office-level dual-engine (`Memory` + `Sqlite`) tests for universal reviews (`TASK-92.1`).

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_proto::attachments::{
    AttachmentAttachParams, AttachmentTargetKind, AttachmentUploadBeginParams,
    AttachmentUploadChunkParams, AttachmentUploadRef,
};
use org_proto::chat::{ChatSendParams, DmOpenParams};
use org_proto::git::RepositoryKindWire;
use org_proto::org::{
    MemberAddPersonParams, MemberRemoveParams, ProjectCreateParams, TeamCreateParams,
    TeamSetLeadParams,
};
use org_proto::reviews::{
    AnchorKind, FindingAnchor, FindingSeverity, FindingStatus, ReviewCloseParams,
    ReviewFindingDraft, ReviewFindingOp, ReviewFindingParams, ReviewInspectParams,
    ReviewListParams, ReviewRequestParams, ReviewResubmitParams, ReviewRouteParams,
    ReviewRoutingSpec, ReviewSessionMode, ReviewStatus, ReviewTargetKind, ReviewVerdict,
    ReviewVerdictParams, RoutingMode,
};
use org_proto::tasks::TasksCreateParams;
use org_proto::ErrorCode;
use org_store::testkit::{identity, FakeCustody, FixedClock};
use org_store::Id;

use org_office::organization_family::testkit::CREATION_AT;
use org_office::reviews::features::ReviewInspectSections;
use org_office::reviews::files::{counter_path, event_path, review_path, round_path};
use org_office::reviews::records::{encode_counter, OfficeReviewCounterRecord, ResolvedCodePin};
use org_office::rules::OrgCommand;
use org_office::testkit_support::office_harness::{places, record, request, CA_SEED};
use org_office::testkit_support::store_open::EngineChoice;
use org_office::testkit_support::Options;
use org_office::OfficeError;
use org_office::{Custody, DataRoots, Office};

static NEXT_ID: AtomicU64 = AtomicU64::new(10_000);

fn next_uuid() -> String {
    let n = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let ms = CREATION_AT.0 + n;
    let mut b = [0u8; 16];
    b[0..6].copy_from_slice(&ms.to_be_bytes()[2..8]);
    b[6] = 0x70 | ((n >> 8) as u8 & 0x0f);
    b[7] = (n & 0xff) as u8;
    b[8] = 0x80;
    b[14] = (n >> 8) as u8;
    b[15] = (n & 0xff) as u8;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

fn engines() -> [EngineChoice; 2] {
    [
        EngineChoice::Memory,
        EngineChoice::Sqlite(org_office_sqlite::sqlite_backend()),
    ]
}
const OID_BASE: &str = "1111111111111111111111111111111111111111";
const OID_HEAD1: &str = "2222222222222222222222222222222222222222";
const OID_HEAD2: &str = "3333333333333333333333333333333333333333";
const SNIPPET_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct Fx {
    _tmp: tempfile::TempDir,
    folder: std::path::PathBuf,
    roots: DataRoots,
    custody: Custody,
    engine: EngineChoice,
    office: Office,
    owner: String,
    person: String,
    owning_team: String,
    project: String,
    _other_project: String,
    repo_id: String,
    task_id: String,
    owning_agent: String,
    contributing_agent: String,
    outsider_agent: String,
}

fn options(engine: EngineChoice) -> Options {
    Options {
        engine: engine.clone(),
        clock: Arc::new(FixedClock(CREATION_AT)),
        testing: Default::default(),
        before_final_checks: None,
    }
}

fn team(office: &Office, owner: &str, name: &str, parent: &str) -> String {
    let id = next_uuid();
    office
        .submit_org(
            owner,
            OrgCommand::TeamCreate(TeamCreateParams {
                command_id: next_uuid(),
                team_id: id.clone(),
                name: name.into(),
                owner_team_id: Some(parent.into()),
            }),
        )
        .unwrap()
        .unwrap();
    id
}

fn project(
    office: &Office,
    owner: &str,
    name: &str,
    slug: &str,
    team: &str,
    with: &[&str],
) -> String {
    let id = next_uuid();
    office
        .submit_org(
            owner,
            OrgCommand::ProjectCreate(ProjectCreateParams {
                command_id: next_uuid(),
                project_id: id.clone(),
                name: name.into(),
                key: None,
                description: None,
                teams: vec![],
                general_area_id: String::new(),
                general_area_name: None,
                repo: None,
                integration_branch: None,
                slug: Some(slug.into()),
                owner_team_id: Some(team.into()),
                create_team_id: None,
                parent_team_id: None,
                contributing_team_ids: with.iter().map(|t| (*t).to_owned()).collect(),
            }),
        )
        .unwrap()
        .unwrap();
    id
}

fn hire_agent(office: &Office, owner: &str, team_id: &str, name: &str, handle: &str) -> String {
    use org_proto::hiring::{HireProposalWire, HrHire, HrHireParams};
    use org_proto::{Outcome, Request};
    let employee_id = next_uuid();
    let req = Request::new::<HrHire>(
        1,
        &HrHireParams {
            command_id: next_uuid(),
            request_id: next_uuid(),
            employee_id: employee_id.clone(),
            home_team_id: team_id.into(),
            role_id: None,
            proposal: HireProposalWire {
                name: name.into(),
                handle: handle.into(),
                title_in_team: Some("Reviewer".into()),
                ..Default::default()
            },
            reason: "seed review test agent".into(),
        },
    )
    .unwrap();
    let resp = office.dispatch_hiring_rpc(Some(owner), &req).unwrap();
    assert!(matches!(resp.outcome, Outcome::Result(_)), "{resp:?}");
    employee_id
}

fn fixture(engine: EngineChoice) -> Fx {
    let tmp = tempfile::tempdir().unwrap();
    let (folder, roots) = places(tmp.path());
    std::fs::create_dir_all(&roots.durable).unwrap();
    std::fs::create_dir_all(&roots.runtime).unwrap();
    let custody = Custody {
        identity: identity(),
        authority: Arc::new(FakeCustody::new()),
    };
    let (new, binding) = request();
    let office = Office::provision_with(
        folder.clone(),
        &new,
        None,
        &binding,
        roots.clone(),
        custody.clone(),
        |_| record(CA_SEED),
        options(engine.clone()),
    )
    .unwrap();
    let owner = office.owner_member_id().to_owned();
    let root = office.org_snapshot().unwrap().teams[0].id.clone();
    let owning_team = team(&office, &owner, "Platform", &root);
    let contributing = team(&office, &owner, "Security", &root);
    let outsider = team(&office, &owner, "Finance", &root);
    let proj = project(
        &office,
        &owner,
        "Checkout",
        "checkout",
        &owning_team,
        &[&contributing],
    );
    let other_project = project(&office, &owner, "Billing", "billing", &outsider, &[]);
    let repo_id = next_uuid();
    office
        .register_repository(
            &owner,
            &next_uuid(),
            &repo_id,
            Some("backend".into()),
            RepositoryKindWire::Remote,
            Some("https://github.com/acme/backend.git".into()),
        )
        .unwrap()
        .unwrap();
    office
        .attach_project_repository(&owner, &next_uuid(), &proj, &repo_id, "main".into())
        .unwrap()
        .unwrap();
    let task_id = next_uuid();
    office
        .task_create(
            &owner,
            TasksCreateParams {
                command_id: next_uuid(),
                task_id: task_id.clone(),
                team_id: Some(owning_team.clone()),
                assigned_team_id: None,
                project_id: Some(proj.clone()),
                task_type: Some("task".into()),
                work_type: None,
                title: "Implement authz checks".into(),
                body_markdown: String::new(),
                assignee_id: Some(owner.clone()),
                priority: None,
                effort: None,
                severity: None,
                rank_order: None,
                labels: vec![],
                reference_files: vec![],
                modified_files: vec![],
                depends_on: vec![],
                details: None,
                dod: None,
                attachment_ids: vec![],
            },
        )
        .unwrap()
        .unwrap();
    let person = next_uuid();
    office
        .submit_org(
            &owner,
            OrgCommand::MemberAddPerson(MemberAddPersonParams {
                command_id: next_uuid(),
                member_id: person.clone(),
                name: "Ada".into(),
                handle: Some("ada".into()),
                home_team_id: Some(outsider.clone()),
                contact: None,
            }),
        )
        .unwrap()
        .unwrap();
    let owning_agent = hire_agent(&office, &owner, &owning_team, "Owner Bot", "owner-bot");
    let contributing_agent = hire_agent(&office, &owner, &contributing, "Sec Bot", "sec-bot");
    let outsider_agent = hire_agent(&office, &owner, &outsider, "Fin Bot", "fin-bot");
    Fx {
        _tmp: tmp,
        folder,
        roots,
        custody,
        engine: engine.clone(),
        office,
        owner,
        person,
        owning_team,
        project: proj,
        _other_project: other_project,
        repo_id,
        task_id,
        owning_agent,
        contributing_agent,
        outsider_agent,
    }
}

fn reopen(fx: Fx) -> Result<Fx, OfficeError> {
    let Fx {
        _tmp,
        folder,
        roots,
        custody,
        engine,
        office,
        owner,
        person,
        owning_team,
        project,
        _other_project,
        repo_id,
        task_id,
        owning_agent,
        contributing_agent,
        outsider_agent,
    } = fx;
    office.close().unwrap();
    let db = roots
        .runtime
        .join(identity().office_id.as_str())
        .join("office.db");
    if db.exists() {
        std::fs::remove_file(&db).unwrap();
    }
    let office = Office::open_with(
        folder.clone(),
        None,
        &identity().instance_id.to_string(),
        roots.clone(),
        custody.clone(),
        options(engine.clone()),
    )?;
    Ok(Fx {
        _tmp,
        folder,
        roots,
        custody,
        engine: engine.clone(),
        office,
        owner,
        person,
        owning_team,
        project,
        _other_project,
        repo_id,
        task_id,
        owning_agent,
        contributing_agent,
        outsider_agent,
    })
}

fn code_request_params(
    fx: &Fx,
    review_id: String,
    reviewer: &str,
    independent: bool,
) -> ReviewRequestParams {
    ReviewRequestParams {
        command_id: next_uuid(),
        review_id,
        title: "Audit checkout authz".into(),
        body_markdown: "Please verify project scoping.".into(),
        target_kind: ReviewTargetKind::Code,
        project_id: Some(fx.project.clone()),
        task_id: Some(fx.task_id.clone()),
        repository_id: Some(fx.repo_id.clone()),
        branch: Some("task/T-A1-authz".into()),
        base_oid: Some(OID_BASE.into()),
        expected_head_oid: Some(OID_HEAD1.into()),
        target_ref: None,
        kind_slug: "security-audit".into(),
        category_slug: Some("authz".into()),
        independent,
        session_mode: None,
        routing: Some(ReviewRoutingSpec {
            mode: RoutingMode::Explicit,
            reviewer_ids: vec![reviewer.into()],
            team_id: None,
        }),
        guidance_refs: vec!["docs/security.md#authz".into()],
        criteria: vec!["no cross-project leaks".into()],
    }
}

fn add_four_anchor_findings_and_replies(fx: &Fx) -> Vec<String> {
    let anchors = [
        FindingAnchor::general(),
        FindingAnchor {
            kind: AnchorKind::FileLine,
            path: Some("src/auth.rs".into()),
            start_line: Some(12),
            end_line: Some(18),
            anchor_snippet_sha256: Some(SNIPPET_SHA.into()),
            doc_section: None,
            checklist_item_id: None,
        },
        FindingAnchor {
            kind: AnchorKind::DocSection,
            path: Some("docs/security.md".into()),
            doc_section: Some("Scope Checks".into()),
            ..Default::default()
        },
        FindingAnchor {
            kind: AnchorKind::ChecklistItem,
            checklist_item_id: Some(next_uuid()),
            ..Default::default()
        },
    ];
    let mut finding_ids = Vec::new();
    for (i, anchor) in anchors.into_iter().enumerate() {
        let fid = next_uuid();
        let res = fx
            .office
            .review_finding(
                &fx.contributing_agent,
                ReviewFindingParams {
                    command_id: next_uuid(),
                    review: "rv-a1".into(),
                    op: ReviewFindingOp::Add(ReviewFindingDraft {
                        finding_id: fid.clone(),
                        severity: FindingSeverity::Minor,
                        anchor: anchor.clone(),
                        body_markdown: format!("Finding #{i}"),
                        attachment_ids: vec![],
                    }),
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(res.finding.number, (i as u64) + 1);
        assert_eq!(res.finding.anchor, anchor);
        finding_ids.push(fid);
    }
    let r1 = fx
        .office
        .review_finding(
            &fx.owning_agent,
            ReviewFindingParams {
                command_id: next_uuid(),
                review: "RV-A1".into(),
                op: ReviewFindingOp::Reply {
                    finding_id: finding_ids[0].clone(),
                    reply_id: next_uuid(),
                    body_markdown: "Looking into this.".into(),
                    set_status: None,
                    attachment_ids: vec![],
                },
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(r1.finding.status, FindingStatus::Open);
    assert_eq!(r1.finding.replies.len(), 1);
    let r2 = fx
        .office
        .review_finding(
            &fx.owning_agent,
            ReviewFindingParams {
                command_id: next_uuid(),
                review: "RV-A1".into(),
                op: ReviewFindingOp::Reply {
                    finding_id: finding_ids[0].clone(),
                    reply_id: next_uuid(),
                    body_markdown: String::new(),
                    set_status: Some(FindingStatus::WontFix),
                    attachment_ids: vec![],
                },
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(r2.finding.status, FindingStatus::WontFix);
    assert_eq!(r2.finding.resolved_in_round, Some(1));
    assert_eq!(r2.finding.replies.len(), 2);
    finding_ids
}

fn run_round2_blocker_approval_and_close(fx: &Fx, blocker_id: String, rev: u64) {
    let round2 = fx
        .office
        .review_resubmit(
            &fx.owning_agent,
            ReviewResubmitParams {
                command_id: next_uuid(),
                review: "RV-A1".into(),
                expected_revision: rev,
                expected_head_oid: Some(OID_HEAD2.into()),
                note_markdown: "Fixed active-caller check.".into(),
                session_mode: None,
                fresh: true,
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(round2.review.current_round, 2);
    assert_eq!(round2.review.status, ReviewStatus::Open);
    assert_eq!(round2.round.head_oid.as_deref(), Some(OID_HEAD2));

    let blocked = fx
        .office
        .review_verdict(
            &fx.contributing_agent,
            ReviewVerdictParams {
                command_id: next_uuid(),
                review: "RV-A1".into(),
                round_number: 2,
                head_oid: Some(OID_HEAD2.into()),
                verdict: ReviewVerdict::Approve,
                summary_markdown: "Trying to approve early.".into(),
                findings: vec![],
                attachment_ids: vec![],
            },
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(blocked.code, ErrorCode::Conflict);

    let resolved = fx
        .office
        .review_finding(
            &fx.contributing_agent,
            ReviewFindingParams {
                command_id: next_uuid(),
                review: "RV-A1".into(),
                op: ReviewFindingOp::Reply {
                    finding_id: blocker_id,
                    reply_id: next_uuid(),
                    body_markdown: "Verified fix in round 2.".into(),
                    set_status: Some(FindingStatus::Resolved),
                    attachment_ids: vec![],
                },
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(resolved.finding.resolved_in_round, Some(2));

    let a1 = fx
        .office
        .review_verdict(
            &fx.contributing_agent,
            ReviewVerdictParams {
                command_id: next_uuid(),
                review: "RV-A1".into(),
                round_number: 2,
                head_oid: Some(OID_HEAD2.into()),
                verdict: ReviewVerdict::Approve,
                summary_markdown: "LGTM".into(),
                findings: vec![],
                attachment_ids: vec![],
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(a1.review.status, ReviewStatus::Open);

    let a2 = fx
        .office
        .review_verdict(
            &fx.owner,
            ReviewVerdictParams {
                command_id: next_uuid(),
                review: "RV-A1".into(),
                round_number: 2,
                head_oid: Some(OID_HEAD2.into()),
                verdict: ReviewVerdict::Approve,
                summary_markdown: "Ship it".into(),
                findings: vec![],
                attachment_ids: vec![],
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(a2.review.status, ReviewStatus::Approved);

    let closed = fx
        .office
        .review_close(
            &fx.owning_agent,
            ReviewCloseParams {
                command_id: next_uuid(),
                review: "RV-A1".into(),
                expected_revision: a2.review.revision,
                reason: Some("Merged to v1".into()),
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(closed.review.status, ReviewStatus::Closed);
    assert_eq!(closed.review.closed_reason.as_deref(), Some("Merged to v1"));
    let double_close = fx
        .office
        .review_close(
            &fx.owning_agent,
            ReviewCloseParams {
                command_id: next_uuid(),
                review: "RV-A1".into(),
                expected_revision: closed.review.revision,
                reason: None,
            },
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(double_close.code, ErrorCode::Conflict);
}

fn assert_rebuild_parity(fx: Fx, review_id: &str, engine: EngineChoice) {
    let rid = Id::parse(review_id).unwrap();
    let rounds_only = fx
        .office
        .review_inspect_sections(
            &fx.owning_agent,
            ReviewInspectParams {
                review: review_id.to_owned(),
                fields: None,
            },
            ReviewInspectSections {
                rounds: true,
                reviewers: false,
                findings: false,
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(rounds_only.rounds.len(), 2);
    assert!(rounds_only.reviewers.is_empty());
    assert!(rounds_only.findings.is_empty());

    let before_inspect = fx
        .office
        .review_inspect(
            &fx.owner,
            ReviewInspectParams {
                review: "RV-A1".into(),
                fields: None,
            },
        )
        .unwrap()
        .unwrap();
    let before_events = fx
        .office
        .review_events(&fx.owner, "RV-A1")
        .unwrap()
        .unwrap();
    assert_eq!(before_events.len() as u64, before_inspect.review.revision);
    assert_eq!(
        before_inspect.findings[1]
            .anchor
            .anchor_snippet_sha256
            .as_deref(),
        Some(SNIPPET_SHA)
    );
    let before_dump = fx.office.dump();

    let fx = reopen(fx).unwrap();
    let after_inspect = fx
        .office
        .review_inspect(
            &fx.owner,
            ReviewInspectParams {
                review: "RV-A1".into(),
                fields: None,
            },
        )
        .unwrap()
        .unwrap();
    let after_events = fx
        .office
        .review_events(&fx.owner, "RV-A1")
        .unwrap()
        .unwrap();
    assert_eq!(
        before_inspect, after_inspect,
        "inspect parity on {engine:?}"
    );
    assert_eq!(before_events, after_events, "events parity on {engine:?}");
    assert_eq!(before_dump, fx.office.dump(), "dump parity on {engine:?}");
    assert!(fx
        .roots
        .durable
        .join(round_path(&rid, 2).as_str())
        .is_file());
}

#[test]
fn full_lifecycle_anchors_and_rebuild_parity() {
    for engine in engines() {
        let fx = fixture(engine.clone());
        let review_id = next_uuid();
        let params = code_request_params(&fx, review_id.clone(), &fx.contributing_agent, true);
        let created = fx
            .office
            .review_request_with_pin(
                &fx.owning_agent,
                params,
                Some(ResolvedCodePin {
                    base_oid: OID_BASE.into(),
                    head_oid: OID_HEAD1.into(),
                }),
            )
            .unwrap()
            .unwrap();
        assert_eq!(created.review.key, "RV-A1");
        assert_eq!(created.review.status, ReviewStatus::Open);
        assert_eq!(created.round.session_mode, ReviewSessionMode::Fresh);
        fx.office.sync_store().unwrap();
        let rid = Id::parse(&review_id).unwrap();
        for p in [
            review_path(&rid),
            round_path(&rid, 1),
            event_path(&rid, 1),
            counter_path(&identity().office_id),
        ] {
            assert!(fx.roots.durable.join(p.as_str()).is_file());
        }
        let routed = fx
            .office
            .review_route(
                &fx.owning_agent,
                ReviewRouteParams {
                    command_id: next_uuid(),
                    review: "RV-A1".into(),
                    expected_revision: 1,
                    routing: ReviewRoutingSpec {
                        mode: RoutingMode::Explicit,
                        reviewer_ids: vec![fx.contributing_agent.clone(), fx.owner.clone()],
                        team_id: None,
                    },
                    session_mode: Some(ReviewSessionMode::Fresh),
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(routed.reviewers.len(), 2);
        let _ = add_four_anchor_findings_and_replies(&fx);
        let blocker_id = next_uuid();
        let v1 = fx
            .office
            .review_verdict(
                &fx.contributing_agent,
                ReviewVerdictParams {
                    command_id: next_uuid(),
                    review: "RV-A1".into(),
                    round_number: 1,
                    head_oid: Some(OID_HEAD1.into()),
                    verdict: ReviewVerdict::ChangesRequested,
                    summary_markdown: "Must fix missing active-caller check.".into(),
                    findings: vec![ReviewFindingDraft {
                        finding_id: blocker_id.clone(),
                        severity: FindingSeverity::Blocker,
                        anchor: FindingAnchor::file_lines("src/auth.rs", 40, 42),
                        body_markdown: "Missing active employee check.".into(),
                        attachment_ids: vec![],
                    }],
                    attachment_ids: vec![],
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(v1.review.status, ReviewStatus::ChangesRequested);
        run_round2_blocker_approval_and_close(&fx, blocker_id, v1.review.revision);
        assert_rebuild_parity(fx, &review_id, engine.clone());
    }
}

#[test]
fn target_kinds_routing_modes_and_list_filters() {
    for engine in engines() {
        let fx = fixture(engine.clone());
        let doc_rev = fx
            .office
            .review_request(
                &fx.owner,
                ReviewRequestParams {
                    command_id: next_uuid(),
                    review_id: next_uuid(),
                    title: "Design doc review".into(),
                    body_markdown: String::new(),
                    target_kind: ReviewTargetKind::Doc,
                    project_id: Some(fx.project.clone()),
                    task_id: None,
                    repository_id: None,
                    branch: None,
                    base_oid: None,
                    expected_head_oid: None,
                    target_ref: Some("docs/design/auth.md".into()),
                    kind_slug: "design-review".into(),
                    category_slug: None,
                    independent: false,
                    session_mode: Some(ReviewSessionMode::Warm),
                    routing: Some(ReviewRoutingSpec {
                        mode: RoutingMode::Pool,
                        reviewer_ids: vec![],
                        team_id: Some(fx.owning_team.clone()),
                    }),
                    guidance_refs: vec![],
                    criteria: vec![],
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(doc_rev.review.key, "RV-A1");

        let task_rev = fx
            .office
            .review_request(
                &fx.owner,
                ReviewRequestParams {
                    command_id: next_uuid(),
                    review_id: next_uuid(),
                    title: "Task DoD review".into(),
                    body_markdown: String::new(),
                    target_kind: ReviewTargetKind::Task,
                    project_id: None,
                    task_id: Some(fx.task_id.clone()),
                    repository_id: None,
                    branch: None,
                    base_oid: None,
                    expected_head_oid: None,
                    target_ref: None,
                    kind_slug: "dod-check".into(),
                    category_slug: None,
                    independent: false,
                    session_mode: None,
                    routing: Some(ReviewRoutingSpec {
                        mode: RoutingMode::SingleAgent,
                        reviewer_ids: vec![],
                        team_id: None,
                    }),
                    guidance_refs: vec![],
                    criteria: vec![],
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(task_rev.review.key, "RV-A2");
        assert_eq!(
            task_rev.review.project_id.as_deref(),
            Some(fx.project.as_str())
        );

        let art_rev = fx
            .office
            .review_request(
                &fx.owner,
                ReviewRequestParams {
                    command_id: next_uuid(),
                    review_id: next_uuid(),
                    title: "Release artifact review".into(),
                    body_markdown: String::new(),
                    target_kind: ReviewTargetKind::Artifact,
                    project_id: None,
                    task_id: None,
                    repository_id: None,
                    branch: None,
                    base_oid: None,
                    expected_head_oid: None,
                    target_ref: Some("attachments/report-v1.pdf".into()),
                    kind_slug: "release-check".into(),
                    category_slug: None,
                    independent: false,
                    session_mode: None,
                    routing: None,
                    guidance_refs: vec![],
                    criteria: vec![],
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(art_rev.review.key, "RV-A3");

        let all = fx
            .office
            .review_list(
                &fx.owner,
                ReviewListParams {
                    limit: Some(2),
                    offset: Some(0),
                    ..Default::default()
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(all.total, 3);
        assert_eq!(all.reviews.len(), 2);

        let page2 = fx
            .office
            .review_list(
                &fx.owner,
                ReviewListParams {
                    limit: Some(2),
                    offset: Some(2),
                    ..Default::default()
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(page2.total, 3);
        assert_eq!(page2.reviews.len(), 1);

        let by_task = fx
            .office
            .review_list(
                &fx.owner,
                ReviewListParams {
                    task_id: Some(fx.task_id.clone()),
                    ..Default::default()
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(by_task.total, 1);
        assert_eq!(by_task.reviews[0].key, "RV-A2");

        let by_reviewer = fx
            .office
            .review_list(
                &fx.owner,
                ReviewListParams {
                    reviewer_id: Some(fx.owning_agent.clone()),
                    ..Default::default()
                },
            )
            .unwrap()
            .unwrap();
        assert!(by_reviewer.reviews.iter().any(|r| r.key == "RV-A1"));
    }
}

#[test]
fn independence_and_session_mode_invariants() {
    for engine in engines() {
        let fx = fixture(engine.clone());
        let self_err = fx
            .office
            .review_request(
                &fx.owning_agent,
                code_request_params(&fx, next_uuid(), &fx.owning_agent, true),
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(self_err.code, ErrorCode::InvalidRequest);

        let mut warm_params = code_request_params(&fx, next_uuid(), &fx.contributing_agent, true);
        warm_params.session_mode = Some(ReviewSessionMode::Warm);
        let warm_err = fx
            .office
            .review_request(&fx.owning_agent, warm_params)
            .unwrap()
            .unwrap_err();
        assert_eq!(warm_err.code, ErrorCode::InvalidRequest);

        let ind = fx
            .office
            .review_request(
                &fx.owning_agent,
                code_request_params(&fx, next_uuid(), &fx.contributing_agent, true),
            )
            .unwrap()
            .unwrap();
        let route_self = fx
            .office
            .review_route(
                &fx.owning_agent,
                ReviewRouteParams {
                    command_id: next_uuid(),
                    review: ind.review.key.clone(),
                    expected_revision: 1,
                    routing: ReviewRoutingSpec {
                        mode: RoutingMode::Explicit,
                        reviewer_ids: vec![fx.owning_agent.clone()],
                        team_id: None,
                    },
                    session_mode: None,
                },
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(route_self.code, ErrorCode::InvalidRequest);

        let mut non_ind = code_request_params(&fx, next_uuid(), &fx.owning_agent, false);
        non_ind.session_mode = Some(ReviewSessionMode::Warm);
        let created = fx
            .office
            .review_request(&fx.owning_agent, non_ind)
            .unwrap()
            .unwrap();
        assert_eq!(created.round.session_mode, ReviewSessionMode::Warm);
        let verdict = fx
            .office
            .review_verdict(
                &fx.owning_agent,
                ReviewVerdictParams {
                    command_id: next_uuid(),
                    review: created.review.key,
                    round_number: 1,
                    head_oid: Some(OID_HEAD1.into()),
                    verdict: ReviewVerdict::Approve,
                    summary_markdown: "Self check passed.".into(),
                    findings: vec![],
                    attachment_ids: vec![],
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(verdict.review.status, ReviewStatus::Approved);
    }
}

#[test]
fn concurrency_atomicity_and_idempotency() {
    for engine in engines() {
        let fx = fixture(engine.clone());
        let req_params = code_request_params(&fx, next_uuid(), &fx.contributing_agent, true);
        let first = fx
            .office
            .review_request(&fx.owning_agent, req_params.clone())
            .unwrap()
            .unwrap();
        let replay = fx
            .office
            .review_request(&fx.owning_agent, req_params.clone())
            .unwrap()
            .unwrap();
        assert_eq!(first, replay);

        let mut mutated = req_params;
        mutated.title = "Different title".into();
        let reused = fx
            .office
            .review_request(&fx.owning_agent, mutated)
            .unwrap()
            .unwrap_err();
        assert_eq!(reused.code, ErrorCode::OperationReused);

        let stale_route = fx
            .office
            .review_route(
                &fx.owning_agent,
                ReviewRouteParams {
                    command_id: next_uuid(),
                    review: "RV-A1".into(),
                    expected_revision: 99,
                    routing: ReviewRoutingSpec {
                        mode: RoutingMode::Explicit,
                        reviewer_ids: vec![fx.owner.clone()],
                        team_id: None,
                    },
                    session_mode: None,
                },
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(stale_route.code, ErrorCode::Conflict);
        assert_eq!(stale_route.conflict_detail().unwrap().current_revision, 1);

        let stale_round = fx
            .office
            .review_verdict(
                &fx.contributing_agent,
                ReviewVerdictParams {
                    command_id: next_uuid(),
                    review: "RV-A1".into(),
                    round_number: 2,
                    head_oid: Some(OID_HEAD1.into()),
                    verdict: ReviewVerdict::Comment,
                    summary_markdown: String::new(),
                    findings: vec![],
                    attachment_ids: vec![],
                },
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(stale_round.code, ErrorCode::Conflict);

        let wrong_head = fx
            .office
            .review_verdict(
                &fx.contributing_agent,
                ReviewVerdictParams {
                    command_id: next_uuid(),
                    review: "RV-A1".into(),
                    round_number: 1,
                    head_oid: Some(OID_HEAD2.into()),
                    verdict: ReviewVerdict::Comment,
                    summary_markdown: String::new(),
                    findings: vec![],
                    attachment_ids: vec![],
                },
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(wrong_head.code, ErrorCode::Conflict);

        let stale_verdict_rev = fx
            .office
            .review_verdict_checked(
                &fx.contributing_agent,
                ReviewVerdictParams {
                    command_id: next_uuid(),
                    review: "RV-A1".into(),
                    round_number: 1,
                    head_oid: Some(OID_HEAD1.into()),
                    verdict: ReviewVerdict::Comment,
                    summary_markdown: String::new(),
                    findings: vec![],
                    attachment_ids: vec![],
                },
                Some(42),
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(stale_verdict_rev.code, ErrorCode::Conflict);

        let dup_id = next_uuid();
        let bad_batch = fx
            .office
            .review_verdict(
                &fx.contributing_agent,
                ReviewVerdictParams {
                    command_id: next_uuid(),
                    review: "RV-A1".into(),
                    round_number: 1,
                    head_oid: Some(OID_HEAD1.into()),
                    verdict: ReviewVerdict::ChangesRequested,
                    summary_markdown: "Partial failure".into(),
                    findings: vec![
                        ReviewFindingDraft {
                            finding_id: dup_id.clone(),
                            severity: FindingSeverity::Minor,
                            anchor: FindingAnchor::general(),
                            body_markdown: "First valid finding".into(),
                            attachment_ids: vec![],
                        },
                        ReviewFindingDraft {
                            finding_id: dup_id,
                            severity: FindingSeverity::Minor,
                            anchor: FindingAnchor::general(),
                            body_markdown: "Duplicate id causes atomic rollback".into(),
                            attachment_ids: vec![],
                        },
                    ],
                    attachment_ids: vec![],
                },
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(bad_batch.code, ErrorCode::InvalidRequest);
        let inspected = fx
            .office
            .review_inspect(
                &fx.owner,
                ReviewInspectParams {
                    review: "RV-A1".into(),
                    fields: None,
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(inspected.review.revision, 1);
        assert!(inspected.findings.is_empty());
        assert!(inspected.rounds[0].verdicts.is_empty());
    }
}

#[test]
fn counter_durability_across_reopen_and_fail_closed_when_behind() {
    for engine in engines() {
        let fx = fixture(engine.clone());
        let r1 = fx
            .office
            .review_request(
                &fx.owning_agent,
                code_request_params(&fx, next_uuid(), &fx.contributing_agent, true),
            )
            .unwrap()
            .unwrap();
        let r2 = fx
            .office
            .review_request(
                &fx.owning_agent,
                code_request_params(&fx, next_uuid(), &fx.contributing_agent, true),
            )
            .unwrap()
            .unwrap();
        assert_eq!(r1.review.key, "RV-A1");
        assert_eq!(r2.review.key, "RV-A2");

        let fx = reopen(fx).unwrap();
        let r3 = fx
            .office
            .review_request(
                &fx.owning_agent,
                code_request_params(&fx, next_uuid(), &fx.contributing_agent, true),
            )
            .unwrap()
            .unwrap();
        assert_eq!(r3.review.key, "RV-A3");

        let cpath = fx
            .roots
            .durable
            .join(counter_path(&identity().office_id).as_str());
        let behind = encode_counter(&OfficeReviewCounterRecord {
            office_id: identity().office_id,
            office_code: "A".into(),
            next_number: 2,
        });
        let fx = {
            let Fx {
                _tmp,
                folder,
                roots,
                custody,
                engine,
                office,
                owner,
                person,
                owning_team,
                project,
                _other_project,
                repo_id,
                task_id,
                owning_agent,
                contributing_agent,
                outsider_agent,
            } = fx;
            office.close().unwrap();
            std::fs::write(&cpath, behind).unwrap();
            let db = roots
                .runtime
                .join(identity().office_id.as_str())
                .join("office.db");
            if db.exists() {
                std::fs::remove_file(&db).unwrap();
            }
            let office = Office::open_with(
                folder.clone(),
                None,
                &identity().instance_id.to_string(),
                roots.clone(),
                custody.clone(),
                options(engine.clone()),
            )
            .unwrap();
            Fx {
                _tmp,
                folder,
                roots,
                custody,
                engine: engine.clone(),
                office,
                owner,
                person,
                owning_team,
                project,
                _other_project,
                repo_id,
                task_id,
                owning_agent,
                contributing_agent,
                outsider_agent,
            }
        };
        let err = fx
            .office
            .review_request(
                &fx.owning_agent,
                code_request_params(&fx, next_uuid(), &fx.contributing_agent, true),
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(
            err.code,
            ErrorCode::Unavailable,
            "counter behind max must fail closed"
        );
    }
}

#[test]
fn security_project_scoping_and_unrouted_verdict() {
    for engine in engines() {
        let fx = fixture(engine.clone());
        let created = fx
            .office
            .review_request(
                &fx.owning_agent,
                code_request_params(&fx, next_uuid(), &fx.contributing_agent, true),
            )
            .unwrap()
            .unwrap();
        let key = created.review.key;
        assert_eq!(
            fx.office
                .review_inspect(
                    &fx.outsider_agent,
                    ReviewInspectParams {
                        review: key.clone(),
                        fields: None,
                    }
                )
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            fx.office
                .review_events(&fx.outsider_agent, &key)
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            fx.office
                .review_list(
                    &fx.outsider_agent,
                    ReviewListParams {
                        project_id: Some(fx.project.clone()),
                        ..Default::default()
                    }
                )
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            fx.office
                .review_list(&fx.outsider_agent, ReviewListParams::default())
                .unwrap()
                .unwrap()
                .total,
            0
        );
        assert_eq!(
            fx.office
                .review_close(
                    &fx.outsider_agent,
                    ReviewCloseParams {
                        command_id: next_uuid(),
                        review: key.clone(),
                        expected_revision: 1,
                        reason: None,
                    }
                )
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        let unrouted_verdict = fx
            .office
            .review_verdict(
                &fx.owner,
                ReviewVerdictParams {
                    command_id: next_uuid(),
                    review: key,
                    round_number: 1,
                    head_oid: Some(OID_HEAD1.into()),
                    verdict: ReviewVerdict::Approve,
                    summary_markdown: String::new(),
                    findings: vec![],
                    attachment_ids: vec![],
                },
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(unrouted_verdict.code, ErrorCode::NotAuthorized);
    }
}

#[test]
fn security_inactive_and_unknown_callers() {
    for engine in engines() {
        let fx = fixture(engine.clone());
        let created = fx
            .office
            .review_request(
                &fx.owning_agent,
                code_request_params(&fx, next_uuid(), &fx.contributing_agent, true),
            )
            .unwrap()
            .unwrap();
        let key = created.review.key;
        let rev = fx
            .office
            .load_employee(&fx.person)
            .unwrap()
            .unwrap()
            .meta
            .revision;
        fx.office
            .submit_org(
                &fx.owner,
                OrgCommand::MemberRemove(MemberRemoveParams {
                    command_id: next_uuid(),
                    member_id: fx.person.clone(),
                    expected_version: rev,
                }),
            )
            .unwrap()
            .unwrap();
        for bad_caller in [fx.person.as_str(), "not-a-uuid", &next_uuid()] {
            assert_eq!(
                fx.office
                    .review_inspect(
                        bad_caller,
                        ReviewInspectParams {
                            review: key.clone(),
                            fields: None,
                        }
                    )
                    .unwrap()
                    .unwrap_err()
                    .code,
                ErrorCode::NotAuthorized
            );
            assert_eq!(
                fx.office
                    .review_list(bad_caller, ReviewListParams::default())
                    .unwrap()
                    .unwrap_err()
                    .code,
                ErrorCode::NotAuthorized
            );
            assert_eq!(
                fx.office
                    .review_request(
                        bad_caller,
                        code_request_params(&fx, next_uuid(), &fx.contributing_agent, true)
                    )
                    .unwrap()
                    .unwrap_err()
                    .code,
                ErrorCode::NotAuthorized
            );
            assert_eq!(
                fx.office
                    .review_close(
                        bad_caller,
                        ReviewCloseParams {
                            command_id: next_uuid(),
                            review: key.clone(),
                            expected_revision: 1,
                            reason: None,
                        }
                    )
                    .unwrap()
                    .unwrap_err()
                    .code,
                ErrorCode::NotAuthorized
            );
        }
    }
}

#[test]
fn route_cannot_bypass_open_blocker_and_resets_readded_verdict() {
    for engine in engines() {
        let fx = fixture(engine.clone());
        let created = fx
            .office
            .review_request(
                &fx.owning_agent,
                code_request_params(&fx, next_uuid(), &fx.contributing_agent, true),
            )
            .unwrap()
            .unwrap();
        let key = created.review.key;
        // Route both owner and contributing_agent on round 1.
        let r2 = fx
            .office
            .review_route(
                &fx.owning_agent,
                ReviewRouteParams {
                    command_id: next_uuid(),
                    review: key.clone(),
                    expected_revision: created.review.revision,
                    routing: ReviewRoutingSpec {
                        mode: RoutingMode::Explicit,
                        reviewer_ids: vec![fx.owner.clone(), fx.contributing_agent.clone()],
                        team_id: None,
                    },
                    session_mode: Some(ReviewSessionMode::Fresh),
                },
            )
            .unwrap()
            .unwrap();
        // Owner approves cleanly while no blockers exist.
        let v_ok = fx
            .office
            .review_verdict(
                &fx.owner,
                ReviewVerdictParams {
                    command_id: next_uuid(),
                    review: key.clone(),
                    round_number: 1,
                    head_oid: Some(OID_HEAD1.into()),
                    verdict: ReviewVerdict::Approve,
                    summary_markdown: "Looks fine".into(),
                    findings: vec![],
                    attachment_ids: vec![],
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(v_ok.review.status, ReviewStatus::Open);
        // Contributing agent blocks with a Blocker finding.
        let v_blk = fx
            .office
            .review_verdict(
                &fx.contributing_agent,
                ReviewVerdictParams {
                    command_id: next_uuid(),
                    review: key.clone(),
                    round_number: 1,
                    head_oid: Some(OID_HEAD1.into()),
                    verdict: ReviewVerdict::ChangesRequested,
                    summary_markdown: "Blocker".into(),
                    findings: vec![ReviewFindingDraft {
                        finding_id: next_uuid(),
                        severity: FindingSeverity::Blocker,
                        anchor: FindingAnchor::general(),
                        body_markdown: "Open blocker must prevent route-to-approved".into(),
                        attachment_ids: vec![],
                    }],
                    attachment_ids: vec![],
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(v_blk.review.status, ReviewStatus::ChangesRequested);
        // Independent guard: author cannot remove dissenting ChangesRequested reviewer.
        let blocked_route = fx
            .office
            .review_route(
                &fx.owning_agent,
                ReviewRouteParams {
                    command_id: next_uuid(),
                    review: key.clone(),
                    expected_revision: v_blk.review.revision,
                    routing: ReviewRoutingSpec {
                        mode: RoutingMode::Explicit,
                        reviewer_ids: vec![fx.owner.clone()],
                        team_id: None,
                    },
                    session_mode: Some(ReviewSessionMode::Fresh),
                },
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(blocked_route.code, ErrorCode::NotAuthorized);
        // Non-author (owner) removing contributing_agent leaves only owner (Approve), yet open Blocker prevents Approved (P0-2).
        let routed = fx
            .office
            .review_route(
                &fx.owner,
                ReviewRouteParams {
                    command_id: next_uuid(),
                    review: key.clone(),
                    expected_revision: v_blk.review.revision,
                    routing: ReviewRoutingSpec {
                        mode: RoutingMode::Explicit,
                        reviewer_ids: vec![fx.owner.clone()],
                        team_id: None,
                    },
                    session_mode: Some(ReviewSessionMode::Fresh),
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(routed.review.status, ReviewStatus::Open);
        // Re-adding contributing_agent clears their previous removed verdict.
        let readded = fx
            .office
            .review_route(
                &fx.owner,
                ReviewRouteParams {
                    command_id: next_uuid(),
                    review: key,
                    expected_revision: routed.review.revision,
                    routing: ReviewRoutingSpec {
                        mode: RoutingMode::Explicit,
                        reviewer_ids: vec![fx.owner.clone(), fx.contributing_agent.clone()],
                        team_id: None,
                    },
                    session_mode: Some(ReviewSessionMode::Fresh),
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(readded.review.status, ReviewStatus::Open);
        let contrib = readded
            .reviewers
            .iter()
            .find(|r| r.reviewer_id == fx.contributing_agent)
            .unwrap();
        assert_eq!(contrib.last_verdict, None);
        assert_eq!(r2.reviewers.len(), 2);
    }
}

#[test]
fn pass2_security_and_resubmit_invariants() {
    for engine in engines() {
        let fx = fixture(engine.clone());
        // P1-1: outsider cannot probe another project's task_id via review_request.
        let mut probe = code_request_params(&fx, next_uuid(), &fx.outsider_agent, false);
        probe.project_id = None;
        probe.task_id = Some(fx.task_id.clone());
        let err = fx
            .office
            .review_request(&fx.outsider_agent, probe)
            .unwrap()
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);

        // P2-2: mismatched base_oid with ResolvedCodePin fails closed.
        let mut bad_base = code_request_params(&fx, next_uuid(), &fx.contributing_agent, true);
        bad_base.base_oid = Some("f".repeat(40));
        let err = fx
            .office
            .review_request_with_pin(
                &fx.owning_agent,
                bad_base,
                Some(ResolvedCodePin {
                    base_oid: OID_BASE.into(),
                    head_oid: OID_HEAD1.into(),
                }),
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Conflict);

        // P2-3 & P2-1: independent reviewer cannot resubmit, and round 3 keeps round 2's rebased base_oid.
        let created = fx
            .office
            .review_request(
                &fx.owning_agent,
                code_request_params(&fx, next_uuid(), &fx.contributing_agent, true),
            )
            .unwrap()
            .unwrap();
        let key = created.review.key;
        let reviewer_resub = fx
            .office
            .review_resubmit(
                &fx.contributing_agent,
                ReviewResubmitParams {
                    command_id: next_uuid(),
                    review: key.clone(),
                    expected_revision: created.review.revision,
                    expected_head_oid: Some(OID_HEAD2.into()),
                    note_markdown: "Reviewer trying to hijack round".into(),
                    session_mode: None,
                    fresh: true,
                },
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(reviewer_resub.code, ErrorCode::NotAuthorized);

        let rebased_base = "d".repeat(40);
        let round2 = fx
            .office
            .review_resubmit_with_pin(
                &fx.owning_agent,
                ReviewResubmitParams {
                    command_id: next_uuid(),
                    review: key.clone(),
                    expected_revision: created.review.revision,
                    expected_head_oid: Some(OID_HEAD2.into()),
                    note_markdown: "Rebased".into(),
                    session_mode: None,
                    fresh: true,
                },
                Some(ResolvedCodePin {
                    base_oid: rebased_base.clone(),
                    head_oid: OID_HEAD2.into(),
                }),
            )
            .unwrap()
            .unwrap();
        let round3 = fx
            .office
            .review_resubmit(
                &fx.owning_agent,
                ReviewResubmitParams {
                    command_id: next_uuid(),
                    review: key,
                    expected_revision: round2.review.revision,
                    expected_head_oid: Some("e".repeat(40)),
                    note_markdown: "Follow-up".into(),
                    session_mode: None,
                    fresh: true,
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            round3.round.base_oid.as_deref(),
            Some(rebased_base.as_str())
        );
    }
}

#[test]
fn standing_audit_independent_guards_blocker_downgrade_and_session_reset() {
    for engine in engines() {
        let fx = fixture(engine.clone());
        let peer_agent = hire_agent(&fx.office, &fx.owner, &fx.owning_team, "Peer", "peer");
        let bystander = hire_agent(
            &fx.office,
            &fx.owner,
            &fx.owning_team,
            "Bystander",
            "bystander",
        );

        // --- Finding #1 & #2: Independent review guards + round.submitted_by parity ---
        let created = fx
            .office
            .review_request(
                &fx.owning_agent,
                code_request_params(&fx, next_uuid(), &fx.contributing_agent, true),
            )
            .unwrap()
            .unwrap();
        let key = created.review.key.clone();

        // Bystander cannot close (NotAuthorized).
        assert_eq!(
            fx.office
                .review_close(
                    &bystander,
                    ReviewCloseParams {
                        command_id: next_uuid(),
                        review: key.clone(),
                        expected_revision: created.review.revision,
                        reason: Some("unauthorized close".into()),
                    }
                )
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotAuthorized
        );
        // Author agent cannot self-close an unapproved independent review.
        assert_eq!(
            fx.office
                .review_close(
                    &fx.owning_agent,
                    ReviewCloseParams {
                        command_id: next_uuid(),
                        review: key.clone(),
                        expected_revision: created.review.revision,
                        reason: Some("self-close bypass".into()),
                    }
                )
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotAuthorized
        );

        // Reviewer requests changes with a Blocker finding.
        let blocker_id = next_uuid();
        let _v_blk = fx
            .office
            .review_verdict(
                &fx.contributing_agent,
                ReviewVerdictParams {
                    command_id: next_uuid(),
                    review: key.clone(),
                    round_number: 1,
                    head_oid: Some(OID_HEAD1.into()),
                    verdict: ReviewVerdict::ChangesRequested,
                    summary_markdown: "Needs fix".into(),
                    findings: vec![ReviewFindingDraft {
                        finding_id: blocker_id.clone(),
                        severity: FindingSeverity::Blocker,
                        anchor: FindingAnchor::general(),
                        body_markdown: "Must fix race".into(),
                        attachment_ids: vec![],
                    }],
                    attachment_ids: vec![],
                },
            )
            .unwrap()
            .unwrap();

        // Author cannot resolve or wont_fix a Blocker finding on an independent review.
        for status in [FindingStatus::Resolved, FindingStatus::WontFix] {
            let err = fx
                .office
                .review_finding(
                    &fx.owning_agent,
                    ReviewFindingParams {
                        command_id: next_uuid(),
                        review: key.clone(),
                        op: ReviewFindingOp::Reply {
                            finding_id: blocker_id.clone(),
                            reply_id: next_uuid(),
                            body_markdown: "Author trying to dismiss blocker".into(),
                            set_status: Some(status),
                            attachment_ids: vec![],
                        },
                    },
                )
                .unwrap()
                .unwrap_err();
            assert_eq!(err.code, ErrorCode::NotAuthorized);
        }
        // Comment-only reply by author on Blocker IS allowed.
        let comment_reply = fx
            .office
            .review_finding(
                &fx.owning_agent,
                ReviewFindingParams {
                    command_id: next_uuid(),
                    review: key.clone(),
                    op: ReviewFindingOp::Reply {
                        finding_id: blocker_id.clone(),
                        reply_id: next_uuid(),
                        body_markdown: "Fixed in next round".into(),
                        set_status: None,
                        attachment_ids: vec![],
                    },
                },
            )
            .unwrap()
            .unwrap();

        // peer_agent submits round 2 (round.submitted_by != created_by).
        let r2 = fx
            .office
            .review_resubmit(
                &peer_agent,
                ReviewResubmitParams {
                    command_id: next_uuid(),
                    review: key.clone(),
                    expected_revision: comment_reply.review.revision,
                    expected_head_oid: Some(OID_HEAD2.into()),
                    note_markdown: "Peer resubmitted fix".into(),
                    session_mode: None,
                    fresh: true,
                },
            )
            .unwrap()
            .unwrap();

        // Round 2 submitter (peer_agent) cannot resolve Blocker either!
        assert_eq!(
            fx.office
                .review_finding(
                    &peer_agent,
                    ReviewFindingParams {
                        command_id: next_uuid(),
                        review: key.clone(),
                        op: ReviewFindingOp::Reply {
                            finding_id: blocker_id.clone(),
                            reply_id: next_uuid(),
                            body_markdown: "Submitter trying to resolve blocker".into(),
                            set_status: Some(FindingStatus::Resolved),
                            attachment_ids: vec![],
                        },
                    },
                )
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotAuthorized
        );
        // Round 2 submitter (peer_agent) cannot be routed as reviewer on independent review (Finding #2).
        assert_eq!(
            fx.office
                .review_route(
                    &fx.owner,
                    ReviewRouteParams {
                        command_id: next_uuid(),
                        review: key.clone(),
                        expected_revision: r2.review.revision,
                        routing: ReviewRoutingSpec {
                            mode: RoutingMode::Explicit,
                            reviewer_ids: vec![peer_agent.clone()],
                            team_id: None,
                        },
                        session_mode: Some(ReviewSessionMode::Fresh),
                    },
                )
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        // Round 2 submitter cannot self-close unapproved independent review.
        assert_eq!(
            fx.office
                .review_close(
                    &peer_agent,
                    ReviewCloseParams {
                        command_id: next_uuid(),
                        review: key.clone(),
                        expected_revision: r2.review.revision,
                        reason: Some("peer self-close".into()),
                    }
                )
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotAuthorized
        );

        // Reviewer resolves the Blocker and approves -> status Approved.
        let _ = fx
            .office
            .review_finding(
                &fx.contributing_agent,
                ReviewFindingParams {
                    command_id: next_uuid(),
                    review: key.clone(),
                    op: ReviewFindingOp::Reply {
                        finding_id: blocker_id.clone(),
                        reply_id: next_uuid(),
                        body_markdown: "Verified fix".into(),
                        set_status: Some(FindingStatus::Resolved),
                        attachment_ids: vec![],
                    },
                },
            )
            .unwrap()
            .unwrap();
        let approved = fx
            .office
            .review_verdict(
                &fx.contributing_agent,
                ReviewVerdictParams {
                    command_id: next_uuid(),
                    review: key.clone(),
                    round_number: 2,
                    head_oid: Some(OID_HEAD2.into()),
                    verdict: ReviewVerdict::Approve,
                    summary_markdown: "LGTM".into(),
                    findings: vec![],
                    attachment_ids: vec![],
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(approved.review.status, ReviewStatus::Approved);

        // --- Finding #3a: Reopening a Blocker on Approved downgrades Approved -> Open ---
        let reopened = fx
            .office
            .review_finding(
                &fx.contributing_agent,
                ReviewFindingParams {
                    command_id: next_uuid(),
                    review: key.clone(),
                    op: ReviewFindingOp::Reply {
                        finding_id: blocker_id.clone(),
                        reply_id: next_uuid(),
                        body_markdown: "Regression spotted, reopening".into(),
                        set_status: Some(FindingStatus::Open),
                        attachment_ids: vec![],
                    },
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(reopened.review.status, ReviewStatus::Open);

        // Resolve again & re-approve, then test Finding #3b: adding a NEW Blocker downgrades Approved -> Open.
        let _ = fx
            .office
            .review_finding(
                &fx.contributing_agent,
                ReviewFindingParams {
                    command_id: next_uuid(),
                    review: key.clone(),
                    op: ReviewFindingOp::Reply {
                        finding_id: blocker_id,
                        reply_id: next_uuid(),
                        body_markdown: "Actually fine".into(),
                        set_status: Some(FindingStatus::Resolved),
                        attachment_ids: vec![],
                    },
                },
            )
            .unwrap()
            .unwrap();
        let reapproved = fx
            .office
            .review_verdict(
                &fx.contributing_agent,
                ReviewVerdictParams {
                    command_id: next_uuid(),
                    review: key.clone(),
                    round_number: 2,
                    head_oid: Some(OID_HEAD2.into()),
                    verdict: ReviewVerdict::Approve,
                    summary_markdown: "Re-approved".into(),
                    findings: vec![],
                    attachment_ids: vec![],
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(reapproved.review.status, ReviewStatus::Approved);
        let new_blk = fx
            .office
            .review_finding(
                &fx.contributing_agent,
                ReviewFindingParams {
                    command_id: next_uuid(),
                    review: key.clone(),
                    op: ReviewFindingOp::Add(ReviewFindingDraft {
                        finding_id: next_uuid(),
                        severity: FindingSeverity::Blocker,
                        anchor: FindingAnchor::general(),
                        body_markdown: "Late blocker".into(),
                        attachment_ids: vec![],
                    }),
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(new_blk.review.status, ReviewStatus::Open);

        // Person (owner) can still close an open independent review.
        let closed_by_owner = fx
            .office
            .review_close(
                &fx.owner,
                ReviewCloseParams {
                    command_id: next_uuid(),
                    review: key,
                    expected_revision: new_blk.review.revision,
                    reason: Some("Owner superseded".into()),
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(closed_by_owner.review.status, ReviewStatus::Closed);

        // --- Finding #5: Session reset on re-route (was_removed || session_mode == Fresh) ---
        let warm_req = fx
            .office
            .review_request(
                &fx.owning_agent,
                code_request_params(&fx, next_uuid(), &fx.contributing_agent, false),
            )
            .unwrap()
            .unwrap();
        let wkey = warm_req.review.key;
        let wid = warm_req.review.id;
        let reviewer_sid = |office: &Office, review_id: &str, reviewer: &str| -> Option<String> {
            office
                .review_reviewer_records(review_id)
                .unwrap()
                .unwrap()
                .into_iter()
                .find(|r| r.round_number == 1 && r.reviewer_id.as_str() == reviewer)
                .and_then(|r| r.runtime_session_id)
        };
        let v_sid = fx
            .office
            .review_verdict_with_session(
                &fx.contributing_agent,
                ReviewVerdictParams {
                    command_id: next_uuid(),
                    review: wkey.clone(),
                    round_number: 1,
                    head_oid: Some(OID_HEAD1.into()),
                    verdict: ReviewVerdict::ChangesRequested,
                    summary_markdown: "Check X".into(),
                    findings: vec![],
                    attachment_ids: vec![],
                },
                None,
                Some("runtime-sess-123".into()),
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            reviewer_sid(&fx.office, &wid, &fx.contributing_agent).as_deref(),
            Some("runtime-sess-123")
        );
        // Keeping reviewer active in Warm mode preserves runtime_session_id.
        let kept_warm = fx
            .office
            .review_route(
                &fx.owning_agent,
                ReviewRouteParams {
                    command_id: next_uuid(),
                    review: wkey.clone(),
                    expected_revision: v_sid.review.revision,
                    routing: ReviewRoutingSpec {
                        mode: RoutingMode::Explicit,
                        reviewer_ids: vec![fx.contributing_agent.clone(), peer_agent.clone()],
                        team_id: None,
                    },
                    session_mode: Some(ReviewSessionMode::Warm),
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            reviewer_sid(&fx.office, &wid, &fx.contributing_agent).as_deref(),
            Some("runtime-sess-123")
        );
        // Switching to Fresh mode clears runtime_session_id even if not removed.
        let _switched_fresh = fx
            .office
            .review_route(
                &fx.owning_agent,
                ReviewRouteParams {
                    command_id: next_uuid(),
                    review: wkey.clone(),
                    expected_revision: kept_warm.review.revision,
                    routing: ReviewRoutingSpec {
                        mode: RoutingMode::Explicit,
                        reviewer_ids: vec![fx.contributing_agent.clone()],
                        team_id: None,
                    },
                    session_mode: Some(ReviewSessionMode::Fresh),
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(reviewer_sid(&fx.office, &wid, &fx.contributing_agent), None);
        // Re-stamp session ID, remove reviewer, re-add in Warm mode -> was_removed clears runtime_session_id.
        let v_sid2 = fx
            .office
            .review_verdict_with_session(
                &fx.contributing_agent,
                ReviewVerdictParams {
                    command_id: next_uuid(),
                    review: wkey.clone(),
                    round_number: 1,
                    head_oid: Some(OID_HEAD1.into()),
                    verdict: ReviewVerdict::Comment,
                    summary_markdown: "Note".into(),
                    findings: vec![],
                    attachment_ids: vec![],
                },
                None,
                Some("runtime-sess-456".into()),
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            reviewer_sid(&fx.office, &wid, &fx.contributing_agent).as_deref(),
            Some("runtime-sess-456")
        );
        let removed = fx
            .office
            .review_route(
                &fx.owning_agent,
                ReviewRouteParams {
                    command_id: next_uuid(),
                    review: wkey.clone(),
                    expected_revision: v_sid2.review.revision,
                    routing: ReviewRoutingSpec {
                        mode: RoutingMode::Explicit,
                        reviewer_ids: vec![peer_agent.clone()],
                        team_id: None,
                    },
                    session_mode: Some(ReviewSessionMode::Warm),
                },
            )
            .unwrap()
            .unwrap();
        let readded = fx
            .office
            .review_route(
                &fx.owning_agent,
                ReviewRouteParams {
                    command_id: next_uuid(),
                    review: wkey,
                    expected_revision: removed.review.revision,
                    routing: ReviewRoutingSpec {
                        mode: RoutingMode::Explicit,
                        reviewer_ids: vec![fx.contributing_agent.clone()],
                        team_id: None,
                    },
                    session_mode: Some(ReviewSessionMode::Warm),
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(readded.reviewers.iter().filter(|r| !r.removed).count(), 1);
        assert_eq!(reviewer_sid(&fx.office, &wid, &fx.contributing_agent), None);
    }
}

fn upload_and_commit(office: &Office, actor: &str, name: &str, bytes: &[u8]) -> String {
    let begun = office
        .attachment_upload_begin(
            actor,
            AttachmentUploadBeginParams {
                filename: name.into(),
                mime_type: "text/plain".into(),
                byte_count: bytes.len() as u64,
            },
        )
        .unwrap()
        .unwrap();
    let uid = begun.upload.upload_id;
    let _ = office
        .attachment_upload_chunk(
            actor,
            AttachmentUploadChunkParams {
                upload_id: uid.clone(),
                offset: 0,
                data_base64: STANDARD.encode(bytes),
            },
        )
        .unwrap()
        .unwrap();
    let _ = office
        .attachment_upload_commit(
            actor,
            AttachmentUploadRef {
                upload_id: uid.clone(),
            },
        )
        .unwrap()
        .unwrap();
    uid
}

fn bind_shared_attachment(
    office: &Office,
    actor: &str,
    name: &str,
    bytes: &[u8],
    kind: AttachmentTargetKind,
    target_id: Option<&str>,
) -> String {
    let uid = upload_and_commit(office, actor, name, bytes);
    office
        .attachment_attach(
            actor,
            AttachmentAttachParams {
                command_id: next_uuid(),
                upload_id: Some(uid),
                source_attachment_id: None,
                target_kind: kind,
                target_id: target_id.map(str::to_owned),
                title: None,
                description: None,
                artifact_status: None,
            },
        )
        .unwrap()
        .unwrap()
        .attachment
        .id
}

#[test]
fn pass1_p0_routing_project_acl_and_repository_binding() {
    for engine in engines() {
        let fx = fixture(engine.clone());
        let root = fx
            .office
            .org_snapshot()
            .unwrap()
            .teams
            .into_iter()
            .find(|t| t.owner_team_id.is_none())
            .unwrap()
            .id;
        // Dedicated outsider-only team (no persons) for Pool routing test.
        let outsider_only_team = team(&fx.office, &fx.owner, "HiddenPool", &root);
        let hidden_agent = hire_agent(
            &fx.office,
            &fx.owner,
            &outsider_only_team,
            "Hidden Bot",
            "hidden-bot",
        );

        // --- P0-1: Explicit, SingleAgent, and Pool MUST refuse outsiders on private projects ---
        for (label, routing, expected_code) in [
            (
                "explicit outsider",
                ReviewRoutingSpec {
                    mode: RoutingMode::Explicit,
                    reviewer_ids: vec![fx.outsider_agent.clone()],
                    team_id: None,
                },
                ErrorCode::NotAuthorized,
            ),
            (
                "single_agent outsider",
                ReviewRoutingSpec {
                    mode: RoutingMode::SingleAgent,
                    reviewer_ids: vec![fx.outsider_agent.clone()],
                    team_id: None,
                },
                ErrorCode::NotAuthorized,
            ),
            (
                "pool outsider-only team",
                ReviewRoutingSpec {
                    mode: RoutingMode::Pool,
                    reviewer_ids: vec![],
                    team_id: Some(outsider_only_team.clone()),
                },
                ErrorCode::InvalidRequest,
            ),
        ] {
            let mut req = code_request_params(&fx, next_uuid(), &fx.contributing_agent, false);
            req.routing = Some(routing);
            let err = fx
                .office
                .review_request(&fx.owning_agent, req)
                .unwrap()
                .unwrap_err();
            assert_eq!(
                err.code, expected_code,
                "review_request {label} must fail closed on {engine:?}"
            );
        }

        // Create a valid review on fx.project and verify review_route also blocks all three!
        let created = fx
            .office
            .review_request(
                &fx.owning_agent,
                code_request_params(&fx, next_uuid(), &fx.contributing_agent, false),
            )
            .unwrap()
            .unwrap();
        for (label, routing, expected_code) in [
            (
                "route explicit outsider",
                ReviewRoutingSpec {
                    mode: RoutingMode::Explicit,
                    reviewer_ids: vec![hidden_agent.clone()],
                    team_id: None,
                },
                ErrorCode::NotAuthorized,
            ),
            (
                "route single_agent outsider",
                ReviewRoutingSpec {
                    mode: RoutingMode::SingleAgent,
                    reviewer_ids: vec![hidden_agent.clone()],
                    team_id: None,
                },
                ErrorCode::NotAuthorized,
            ),
            (
                "route pool outsider-only team",
                ReviewRoutingSpec {
                    mode: RoutingMode::Pool,
                    reviewer_ids: vec![],
                    team_id: Some(outsider_only_team.clone()),
                },
                ErrorCode::InvalidRequest,
            ),
        ] {
            let err = fx
                .office
                .review_route(
                    &fx.owning_agent,
                    ReviewRouteParams {
                        command_id: next_uuid(),
                        review: created.review.key.clone(),
                        expected_revision: created.review.revision,
                        routing,
                        session_mode: None,
                    },
                )
                .unwrap()
                .unwrap_err();
            assert_eq!(
                err.code, expected_code,
                "{label} must fail closed on {engine:?}"
            );
        }
        // Outsider still has zero read access to the private project review.
        assert_eq!(
            fx.office
                .review_inspect(
                    &hidden_agent,
                    ReviewInspectParams {
                        review: created.review.key.clone(),
                        fields: None,
                    }
                )
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );

        // --- P0-2: repository_belongs_to_project + org-level private repository visibility ---
        let private_repo_b = next_uuid();
        fx.office
            .register_repository(
                &fx.owner,
                &next_uuid(),
                &private_repo_b,
                Some("billing-repo".into()),
                RepositoryKindWire::Remote,
                Some("https://github.com/acme/billing.git".into()),
            )
            .unwrap()
            .unwrap();
        fx.office
            .attach_project_repository(
                &fx.owner,
                &next_uuid(),
                &fx._other_project,
                &private_repo_b,
                "main".into(),
            )
            .unwrap()
            .unwrap();

        // Case A: Project A member requests code review with project_id=Some(proj_a) naming Private Project B's repo.
        let mut cross_proj = code_request_params(&fx, next_uuid(), &fx.contributing_agent, false);
        cross_proj.repository_id = Some(private_repo_b.clone());
        let err_a = fx
            .office
            .review_request(&fx.owning_agent, cross_proj)
            .unwrap()
            .unwrap_err();
        assert_eq!(err_a.code, ErrorCode::InvalidRequest);

        // Case B: Project A member requests code review with project_id=None naming Private Project B's repo.
        let mut org_level_cross =
            code_request_params(&fx, next_uuid(), &fx.contributing_agent, false);
        org_level_cross.project_id = None;
        org_level_cross.task_id = None;
        org_level_cross.repository_id = Some(private_repo_b.clone());
        let err_b = fx
            .office
            .review_request(&fx.owning_agent, org_level_cross)
            .unwrap()
            .unwrap_err();
        assert_eq!(err_b.code, ErrorCode::NotFound);

        // Case C: Unattached repo refused on project-scoped review, allowed on org-level review.
        let shared_repo = next_uuid();
        fx.office
            .register_repository(
                &fx.owner,
                &next_uuid(),
                &shared_repo,
                Some("shared-repo".into()),
                RepositoryKindWire::Remote,
                Some("https://github.com/acme/shared.git".into()),
            )
            .unwrap()
            .unwrap();
        let mut unattached_proj =
            code_request_params(&fx, next_uuid(), &fx.contributing_agent, false);
        unattached_proj.repository_id = Some(shared_repo.clone());
        assert_eq!(
            fx.office
                .review_request(&fx.owning_agent, unattached_proj)
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        let mut unattached_org =
            code_request_params(&fx, next_uuid(), &fx.contributing_agent, false);
        unattached_org.project_id = None;
        unattached_org.task_id = None;
        unattached_org.repository_id = Some(shared_repo);
        assert!(fx
            .office
            .review_request(&fx.owning_agent, unattached_org)
            .unwrap()
            .is_ok());
    }
}

#[test]
fn pass1_p1_attachment_acl_target_ref_and_p2_independent_dissent() {
    for engine in engines() {
        let fx = fixture(engine.clone());

        // --- P1-2: doc & artifact target_ref refuse leading `-` and `..` ---
        for kind in [ReviewTargetKind::Doc, ReviewTargetKind::Artifact] {
            for bad_ref in ["-evil.md", "--flag", "../secret.md", "docs/../passwd"] {
                let err = fx
                    .office
                    .review_request(
                        &fx.owning_agent,
                        ReviewRequestParams {
                            command_id: next_uuid(),
                            review_id: next_uuid(),
                            title: "Bad ref".into(),
                            body_markdown: String::new(),
                            target_kind: kind,
                            project_id: Some(fx.project.clone()),
                            task_id: None,
                            repository_id: None,
                            branch: None,
                            base_oid: None,
                            expected_head_oid: None,
                            target_ref: Some(bad_ref.into()),
                            kind_slug: "doc-review".into(),
                            category_slug: None,
                            independent: false,
                            session_mode: None,
                            routing: Some(ReviewRoutingSpec {
                                mode: RoutingMode::Explicit,
                                reviewer_ids: vec![fx.contributing_agent.clone()],
                                team_id: None,
                            }),
                            guidance_refs: vec![],
                            criteria: vec![],
                        },
                    )
                    .unwrap()
                    .unwrap_err();
                assert_eq!(
                    err.code,
                    ErrorCode::InvalidRequest,
                    "{kind:?} `{bad_ref}` must be rejected on {engine:?}"
                );
            }
        }

        // --- P1-1: Domain attachment UUID + ACL + project-scope validation ---
        let proj_review = fx
            .office
            .review_request(
                &fx.owning_agent,
                code_request_params(&fx, next_uuid(), &fx.contributing_agent, false),
            )
            .unwrap()
            .unwrap();
        let org_review = fx
            .office
            .review_request(
                &fx.owner,
                ReviewRequestParams {
                    command_id: next_uuid(),
                    review_id: next_uuid(),
                    title: "Org artifact review".into(),
                    body_markdown: String::new(),
                    target_kind: ReviewTargetKind::Artifact,
                    project_id: None,
                    task_id: None,
                    repository_id: None,
                    branch: None,
                    base_oid: None,
                    expected_head_oid: None,
                    target_ref: Some("reports/q4.pdf".into()),
                    kind_slug: "artifact-review".into(),
                    category_slug: None,
                    independent: false,
                    session_mode: None,
                    routing: Some(ReviewRoutingSpec {
                        mode: RoutingMode::Explicit,
                        reviewer_ids: vec![fx.contributing_agent.clone()],
                        team_id: None,
                    }),
                    guidance_refs: vec![],
                    criteria: vec![],
                },
            )
            .unwrap()
            .unwrap();

        let org_att = bind_shared_attachment(
            &fx.office,
            &fx.owner,
            "org.txt",
            b"org wide",
            AttachmentTargetKind::Organization,
            None,
        );
        let proj_a_att = bind_shared_attachment(
            &fx.office,
            &fx.owning_agent,
            "proj-a.txt",
            b"project a",
            AttachmentTargetKind::Project,
            Some(&fx.project),
        );
        let task_a_att = bind_shared_attachment(
            &fx.office,
            &fx.owning_agent,
            "task-a.txt",
            b"task a",
            AttachmentTargetKind::Task,
            Some(&fx.task_id),
        );
        let proj_b_att = bind_shared_attachment(
            &fx.office,
            &fx.owner,
            "proj-b.txt",
            b"project b secret",
            AttachmentTargetKind::Project,
            Some(&fx._other_project),
        );
        let private_att = bind_shared_attachment(
            &fx.office,
            &fx.owner,
            "private.txt",
            b"owner private",
            AttachmentTargetKind::UserPrivate,
            Some(&fx.owner),
        );
        // DM attachment between owner and contributing_agent.
        let _dm_conv = fx
            .office
            .dm_open(
                &fx.owner,
                DmOpenParams {
                    participants: vec![fx.contributing_agent.clone()],
                    name: None,
                    conversation_id: None,
                },
            )
            .unwrap()
            .unwrap();
        let dm_upload = upload_and_commit(&fx.office, &fx.owner, "dm.txt", b"secret dm");
        let dm_msg = fx
            .office
            .chat_send(
                &fx.owner,
                ChatSendParams {
                    id: next_uuid(),
                    to: fx.contributing_agent.clone(),
                    kind: org_proto::msg::KIND_MSG.into(),
                    priority: None,
                    subject: None,
                    body: "dm file".into(),
                    attachment_ids: vec![dm_upload],
                    meta: Default::default(),
                    reply_to: None,
                    also_in_channel: false,
                    reason: None,
                },
            )
            .unwrap()
            .unwrap();
        let dm_att = dm_msg.message.attachments[0].id.clone();

        // Valid attachments (org, same-project, same-project task) succeed on Project A review.
        let good_finding = fx
            .office
            .review_finding(
                &fx.contributing_agent,
                ReviewFindingParams {
                    command_id: next_uuid(),
                    review: proj_review.review.key.clone(),
                    op: ReviewFindingOp::Add(ReviewFindingDraft {
                        finding_id: next_uuid(),
                        severity: FindingSeverity::Minor,
                        anchor: FindingAnchor::general(),
                        body_markdown: "Valid shared attachments".into(),
                        attachment_ids: vec![org_att, proj_a_att.clone(), task_a_att],
                    }),
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(good_finding.finding.attachment_ids.len(), 3);

        // Non-UUID, nonexistent, private, DM, and cross-project attachments ALL rejected.
        for (actor, review_key, att, expected) in [
            (
                fx.contributing_agent.as_str(),
                proj_review.review.key.as_str(),
                "not-a-uuid",
                ErrorCode::InvalidRequest,
            ),
            (
                fx.contributing_agent.as_str(),
                proj_review.review.key.as_str(),
                &next_uuid(),
                ErrorCode::NotFound,
            ),
            (
                fx.owner.as_str(),
                proj_review.review.key.as_str(),
                private_att.as_str(),
                ErrorCode::NotFound,
            ),
            (
                fx.owner.as_str(),
                proj_review.review.key.as_str(),
                dm_att.as_str(),
                ErrorCode::NotFound,
            ),
            // Agent on Project A cannot even confirm Project B's attachment exists (NotFound).
            (
                fx.contributing_agent.as_str(),
                proj_review.review.key.as_str(),
                proj_b_att.as_str(),
                ErrorCode::NotFound,
            ),
            // Person who can see both projects still cannot cross-link Project B onto Project A review!
            (
                fx.owner.as_str(),
                proj_review.review.key.as_str(),
                proj_b_att.as_str(),
                ErrorCode::NotAuthorized,
            ),
            // Project A attachment cannot be linked onto an org-wide review (would leak to outsiders)!
            (
                fx.owner.as_str(),
                org_review.review.key.as_str(),
                proj_a_att.as_str(),
                ErrorCode::NotAuthorized,
            ),
        ] {
            let err = fx
                .office
                .review_finding(
                    actor,
                    ReviewFindingParams {
                        command_id: next_uuid(),
                        review: review_key.into(),
                        op: ReviewFindingOp::Add(ReviewFindingDraft {
                            finding_id: next_uuid(),
                            severity: FindingSeverity::Minor,
                            anchor: FindingAnchor::general(),
                            body_markdown: "Bad attachment".into(),
                            attachment_ids: vec![att.into()],
                        }),
                    },
                )
                .unwrap()
                .unwrap_err();
            assert_eq!(
                err.code, expected,
                "attachment `{att}` on {review_key} by {actor} must fail on {engine:?}"
            );
        }

        // --- P2-4: Peer agent cannot remove dissenting ChangesRequested reviewer on independent review ---
        let peer_agent = hire_agent(
            &fx.office,
            &fx.owner,
            &fx.owning_team,
            "Peer Bot",
            "peer-bot-p2",
        );
        let lead_agent = hire_agent(
            &fx.office,
            &fx.owner,
            &fx.owning_team,
            "Lead Bot",
            "lead-bot-p2",
        );
        let team_ver = fx
            .office
            .org_snapshot()
            .unwrap()
            .teams
            .into_iter()
            .find(|t| t.id == fx.owning_team)
            .unwrap()
            .version;
        fx.office
            .submit_org(
                &fx.owner,
                OrgCommand::TeamSetLead(TeamSetLeadParams {
                    command_id: next_uuid(),
                    team_id: fx.owning_team.clone(),
                    expected_version: team_ver,
                    lead_member_id: Some(lead_agent.clone()),
                }),
            )
            .unwrap()
            .unwrap();

        let ind = fx
            .office
            .review_request(
                &fx.owning_agent,
                code_request_params(&fx, next_uuid(), &fx.contributing_agent, true),
            )
            .unwrap()
            .unwrap();
        let dissent = fx
            .office
            .review_verdict(
                &fx.contributing_agent,
                ReviewVerdictParams {
                    command_id: next_uuid(),
                    review: ind.review.key.clone(),
                    round_number: 1,
                    head_oid: Some(OID_HEAD1.into()),
                    verdict: ReviewVerdict::ChangesRequested,
                    summary_markdown: "Needs security fix.".into(),
                    findings: vec![],
                    attachment_ids: vec![],
                },
            )
            .unwrap()
            .unwrap();

        // Peer agent (neither person nor accountable team lead) MUST be refused!
        let peer_drop = fx
            .office
            .review_route(
                &peer_agent,
                ReviewRouteParams {
                    command_id: next_uuid(),
                    review: ind.review.key.clone(),
                    expected_revision: dissent.review.revision,
                    routing: ReviewRoutingSpec {
                        mode: RoutingMode::Explicit,
                        reviewer_ids: vec![peer_agent.clone()],
                        team_id: None,
                    },
                    session_mode: Some(ReviewSessionMode::Fresh),
                },
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(peer_drop.code, ErrorCode::NotAuthorized);

        // Non-submitter accountable team lead IS allowed to override and re-route.
        let lead_drop = fx
            .office
            .review_route(
                &lead_agent,
                ReviewRouteParams {
                    command_id: next_uuid(),
                    review: ind.review.key,
                    expected_revision: dissent.review.revision,
                    routing: ReviewRoutingSpec {
                        mode: RoutingMode::Explicit,
                        reviewer_ids: vec![peer_agent],
                        team_id: None,
                    },
                    session_mode: Some(ReviewSessionMode::Fresh),
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(lead_drop.reviewers.iter().filter(|r| !r.removed).count(), 1);
    }
}
