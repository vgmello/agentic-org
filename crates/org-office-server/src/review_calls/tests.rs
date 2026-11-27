//! End-to-end tests for [`ReviewService`], `org-git` bare-cache diffs/worktree pinning/blob
//! snippet hashes, outsider isolation, `--fields` pruning, and ADR-0028 review wakes (`TASK-92.1`).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use org_git::RepoLocks;
use org_office::rules::OrgCommand;
use org_office::{EngineChoice, Office, TestOffice};
use org_office_sessions::{SessionManager, SessionManagerConfig, WakeKind};
use org_proto::git::{
    RepoAttach, RepoAttachParams, RepoRegister, RepoRegisterParams, RepositoryKindWire,
    WorktreePrepare, WorktreePrepareParams, WorktreePrepareResult,
};
use org_proto::hiring::{
    HireProposalWire, HrHire, HrHireParams, ProviderConnectionCreate,
    ProviderConnectionCreateParams,
};
use org_proto::org::{ProjectCreateParams, TeamCreateParams};
use org_proto::reviews::{
    FindingAnchor, FindingSeverity, ReviewClose, ReviewCloseParams, ReviewDiff, ReviewDiffParams,
    ReviewDiffResult, ReviewFinding, ReviewFindingDraft, ReviewFindingOp, ReviewFindingParams,
    ReviewFindingResult, ReviewInspect, ReviewInspectParams, ReviewInspectResult, ReviewList,
    ReviewListParams, ReviewListResult, ReviewMutationResult, ReviewRequest, ReviewRequestParams,
    ReviewResubmit, ReviewResubmitParams, ReviewRoundResult, ReviewRoute, ReviewRouteParams,
    ReviewRouteResult, ReviewRoutingSpec, ReviewSessionMode, ReviewStatus, ReviewTargetKind,
    ReviewVerdict, ReviewVerdictCall, ReviewVerdictParams, ReviewVerdictResult, RoutingMode,
};
use org_proto::tasks::TasksCreateParams;
use org_proto::{Call, ErrorCode, Outcome, Request};

use crate::git_calls::GitService;
use crate::live::LiveEvents;

use super::ReviewService;

fn uuid(n: u64) -> String {
    format!("018bce83-9200-7000-8000-{n:012x}")
}

fn run_git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args(args)
        .output()
        .expect("spawn git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn seed_origin(dir: &Path) -> PathBuf {
    let bare = dir.join("origin.git");
    std::fs::create_dir_all(&bare).unwrap();
    run_git(&bare, &["init", "--bare", "--initial-branch=main"]);
    let seed = dir.join("seed");
    std::fs::create_dir_all(&seed).unwrap();
    run_git(&seed, &["init", "--initial-branch=main"]);
    run_git(&seed, &["config", "user.name", "Seed"]);
    run_git(&seed, &["config", "user.email", "seed@example.test"]);
    std::fs::write(seed.join("lib.rs"), "pub fn alpha() -> u32 {\n    1\n}\n").unwrap();
    run_git(&seed, &["add", "lib.rs"]);
    run_git(&seed, &["commit", "-m", "initial"]);
    run_git(&seed, &["remote", "add", "origin", bare.to_str().unwrap()]);
    run_git(&seed, &["push", "origin", "main"]);
    bare
}

fn call_ok<C: Call>(svc: &ReviewService, member: &str, params: &C::Params) -> C::Result {
    let req = Request::new::<C>(1, params).unwrap();
    let resp = svc.dispatch(Some(member), &req).expect("handled");
    match resp.outcome {
        Outcome::Result(v) => serde_json::from_value(v).unwrap(),
        Outcome::Error(e) => panic!("expected ok for {}, got error: {e:?}", C::METHOD),
    }
}

fn call_err<C: Call>(svc: &ReviewService, member: &str, params: &C::Params) -> ErrorCode {
    let req = Request::new::<C>(1, params).unwrap();
    let resp = svc.dispatch(Some(member), &req).expect("handled");
    match resp.outcome {
        Outcome::Error(e) => e.code,
        Outcome::Result(v) => panic!("expected error for {}, got result: {v}", C::METHOD),
    }
}

fn git_ok<C: Call>(svc: &GitService, member: &str, params: &C::Params) -> C::Result {
    let req = Request::new::<C>(1, params).unwrap();
    let resp = svc.dispatch(Some(member), &req).expect("handled");
    match resp.outcome {
        Outcome::Result(v) => serde_json::from_value(v).unwrap(),
        Outcome::Error(e) => panic!("expected git ok for {}, got error: {e:?}", C::METHOD),
    }
}

struct ReviewFixture {
    _office: Arc<Office>,
    _git_svc: GitService,
    review_svc: ReviewService,
    sessions: Arc<SessionManager>,
    owner: String,
    project_id: String,
    task_id: String,
    repo_id: String,
    branch: String,
    worktree_dir: PathBuf,
    author_agent: String,
    reviewer_agent: String,
    outsider_agent: String,
    _origin_tmp: tempfile::TempDir,
    _sock_tmp: tempfile::TempDir,
}

fn hire_agent(
    office: &Office,
    owner: &str,
    team_id: &str,
    prof_id: &str,
    seq: u64,
    handle: &str,
) -> String {
    let emp_id = uuid(seq);
    let req = Request::new::<HrHire>(
        2,
        &HrHireParams {
            command_id: uuid(seq + 1),
            request_id: uuid(seq + 2),
            employee_id: emp_id.clone(),
            home_team_id: team_id.to_owned(),
            role_id: None,
            proposal: HireProposalWire {
                name: format!("Agent {handle}"),
                handle: handle.to_owned(),
                title: Some("Engineer".into()),
                focus: None,
                role_text: None,
                title_in_team: Some("Engineer".into()),
                reports_to_employee_id: Some(owner.to_owned()),
                office_id: None,
                model_profile_id: Some(prof_id.to_owned()),
                model_id: Some("claude-sonnet-4-6".into()),
                permission_preset: Some("workspace_edits".into()),
                tools_config: Vec::new(),
                system_prompt_markdown: None,
                body: Some("Review code carefully.".into()),
                effort: None,
            },
            reason: "review fixture".into(),
        },
    )
    .unwrap();
    let resp = office.dispatch_hiring_rpc(Some(owner), &req).unwrap();
    assert!(matches!(resp.outcome, Outcome::Result(_)), "{resp:?}");
    emp_id
}

fn setup_fixture() -> ReviewFixture {
    org_office_sqlite::install();
    let office = Arc::new(Office::for_test(TestOffice {
        engine: EngineChoice::Sqlite,
    }));
    let owner = office.owner_member_id().to_owned();
    let root_team = office.org_snapshot().unwrap().teams[0].id.clone();

    let team_a = uuid(10);
    let team_b = uuid(11);
    for (tid, name, cid) in [(&team_a, "Core", 12), (&team_b, "Outsiders", 13)] {
        office
            .submit_org(
                &owner,
                OrgCommand::TeamCreate(TeamCreateParams {
                    command_id: uuid(cid),
                    team_id: tid.clone(),
                    name: name.into(),
                    owner_team_id: Some(root_team.clone()),
                }),
            )
            .unwrap()
            .unwrap();
    }

    let conn_id = uuid(20);
    let prof_id = uuid(21);
    let conn_req = Request::new::<ProviderConnectionCreate>(
        1,
        &ProviderConnectionCreateParams {
            command_id: uuid(22),
            connection_id: conn_id,
            default_profile_id: prof_id.clone(),
            key: "rev-conn".into(),
            name: "Review Conn".into(),
            provider_type: "anthropic".into(),
            auth_mode: "api_key".into(),
            subscription_identity: None,
            runtime_driver: "claude-code".into(),
            endpoint_url: None,
            max_concurrent_turns: 2,
        },
    )
    .unwrap();
    let _ = office.dispatch_hiring_rpc(Some(&owner), &conn_req);

    let author_agent = hire_agent(&office, &owner, &team_a, &prof_id, 30, "author-bot");
    let reviewer_agent = hire_agent(&office, &owner, &team_a, &prof_id, 40, "reviewer-bot");
    let outsider_agent = hire_agent(&office, &owner, &team_b, &prof_id, 50, "outsider-bot");

    let project_id = uuid(60);
    office
        .submit_org(
            &owner,
            OrgCommand::ProjectCreate(ProjectCreateParams {
                command_id: uuid(61),
                project_id: project_id.clone(),
                name: "Review Core".into(),
                key: None,
                description: None,
                teams: vec![],
                general_area_id: String::new(),
                general_area_name: None,
                repo: None,
                integration_branch: None,
                slug: Some("review-core".into()),
                owner_team_id: Some(team_a.clone()),
                create_team_id: None,
                parent_team_id: None,
                contributing_team_ids: vec![],
            }),
        )
        .unwrap()
        .unwrap();

    let task = office
        .task_create(
            &owner,
            TasksCreateParams {
                command_id: uuid(62),
                task_id: uuid(63),
                title: "Implement beta".into(),
                body_markdown: "Add beta helper".into(),
                project_id: Some(project_id.clone()),
                team_id: Some(team_a),
                assigned_team_id: None,
                assignee_id: Some(author_agent.clone()),
                task_type: None,
                work_type: None,
                priority: None,
                effort: None,
                severity: None,
                rank_order: None,
                labels: vec![],
                reference_files: vec![],
                modified_files: vec![],
                depends_on: vec![],
                attachment_ids: vec![],
                details: None,
                dod: None,
            },
        )
        .unwrap()
        .unwrap()
        .task;

    let origin_tmp = tempfile::tempdir().unwrap();
    let bare_origin = seed_origin(origin_tmp.path());
    let live = Arc::new(LiveEvents::for_owner(Box::new(|| {}), owner.clone()));
    let locks = Arc::new(RepoLocks::new());
    let git_svc =
        GitService::with_locks(Arc::clone(&office), Arc::clone(&live), Arc::clone(&locks));

    let sock_tmp = tempfile::tempdir().unwrap();
    let cfg = SessionManagerConfig::with_runtimes(
        sock_tmp.path().join("office.sock"),
        org_office_sessions::RuntimeRegistry::empty(),
    );
    let sessions = Arc::new(SessionManager::new(Arc::clone(&office), cfg));
    let review_svc = ReviewService::new(
        Arc::clone(&office),
        live,
        locks,
        Some(Arc::clone(&sessions)),
    );

    let repo_id = uuid(70);
    let _ = git_ok::<RepoRegister>(
        &git_svc,
        &owner,
        &RepoRegisterParams {
            command_id: uuid(71),
            repository_id: repo_id.clone(),
            key: Some("core-repo".into()),
            kind: RepositoryKindWire::Local,
            remote_url: None,
            origin_path: Some(bare_origin.to_string_lossy().into_owned()),
        },
    );
    let _ = git_ok::<RepoAttach>(
        &git_svc,
        &owner,
        &RepoAttachParams {
            command_id: uuid(72),
            project: project_id.clone(),
            repository: repo_id.clone(),
            integration_branch: "main".into(),
        },
    );
    let prep: WorktreePrepareResult = git_ok::<WorktreePrepare>(
        &git_svc,
        &author_agent,
        &WorktreePrepareParams {
            command_id: uuid(73),
            task: task.id.clone(),
            repository: repo_id.clone(),
            base_branch: Some("main".into()),
        },
    );
    let worktree_dir = PathBuf::from(&prep.worktree.worktree_path);

    ReviewFixture {
        _office: office,
        _git_svc: git_svc,
        review_svc,
        sessions,
        owner,
        project_id,
        task_id: task.id,
        repo_id,
        branch: prep.worktree.branch,
        worktree_dir,
        author_agent,
        reviewer_agent,
        outsider_agent,
        _origin_tmp: origin_tmp,
        _sock_tmp: sock_tmp,
    }
}

#[test]
fn code_review_end_to_end_pinning_diffs_anchors_pruning_outsider_isolation_and_wakes() {
    let fx = setup_fixture();

    // 1. Dirty worktree MUST be refused with Conflict on review.request.
    std::fs::write(
        fx.worktree_dir.join("lib.rs"),
        "pub fn alpha() -> u32 {\n    2\n}\npub fn beta() -> u32 {\n    42\n}\n",
    )
    .unwrap();
    let dirty_req = ReviewRequestParams {
        command_id: uuid(100),
        review_id: uuid(101),
        target_kind: ReviewTargetKind::Code,
        title: "Add beta".into(),
        body_markdown: "Please review beta().".into(),
        kind_slug: "code-review".into(),
        category_slug: None,
        project_id: Some(fx.project_id.clone()),
        task_id: Some(fx.task_id.clone()),
        repository_id: Some(fx.repo_id.clone()),
        branch: Some(fx.branch.clone()),
        target_ref: None,
        base_oid: None,
        expected_head_oid: None,
        independent: false,
        routing: Some(ReviewRoutingSpec {
            mode: RoutingMode::Explicit,
            reviewer_ids: vec![fx.reviewer_agent.clone()],
            team_id: None,
        }),
        session_mode: Some(ReviewSessionMode::Warm),
        guidance_refs: vec![],
        criteria: vec!["beta returns 42".into()],
    };
    assert_eq!(
        call_err::<ReviewRequest>(&fx.review_svc, &fx.author_agent, &dirty_req),
        ErrorCode::Conflict
    );

    // 2. Outsider from unrelated team receives NotFound BEFORE touching git worktree!
    let mut outsider_req = dirty_req.clone();
    outsider_req.command_id = uuid(102);
    outsider_req.review_id = uuid(103);
    assert_eq!(
        call_err::<ReviewRequest>(&fx.review_svc, &fx.outsider_agent, &outsider_req),
        ErrorCode::NotFound
    );

    // 3. Commit worktree changes -> review.request pins commit OID cleanly & wakes reviewer.
    run_git(&fx.worktree_dir, &["add", "lib.rs"]);
    run_git(
        &fx.worktree_dir,
        &[
            "-c",
            "user.name=Author",
            "-c",
            "user.email=author@example.test",
            "commit",
            "-m",
            "round 1",
        ],
    );
    let head1 = run_git(&fx.worktree_dir, &["rev-parse", "HEAD"]);
    let mut req1 = dirty_req.clone();
    req1.command_id = uuid(104);
    req1.review_id = uuid(105);
    req1.expected_head_oid = Some(head1.clone());
    let created: ReviewRoundResult =
        call_ok::<ReviewRequest>(&fx.review_svc, &fx.author_agent, &req1);
    assert_eq!(created.round.head_oid.as_deref(), Some(head1.as_str()));
    assert_eq!(created.review.status, ReviewStatus::Open);
    assert!(!created.review.pin_stale);

    // Verify ADR-0028 ReviewRequested wake queued for reviewer_agent.
    let wakes_r1 = fx
        .sessions
        .store()
        .read_pending_wakes(&fx.reviewer_agent)
        .unwrap()
        .expect("reviewer wake queued");
    assert_eq!(wakes_r1.items.len(), 1);
    assert_eq!(wakes_r1.items[0].kind, WakeKind::ReviewRequested);
    assert_eq!(wakes_r1.items[0].source_id, created.review.key);

    // 4. Outsider cannot inspect or diff the review (NotFound before git).
    assert_eq!(
        call_err::<ReviewDiff>(
            &fx.review_svc,
            &fx.outsider_agent,
            &ReviewDiffParams {
                review: created.review.key.clone(),
                round: None,
                since_round: None,
                path: None,
                stat_only: false,
                context_lines: None,
            }
        ),
        ErrorCode::NotFound
    );

    // 5. Bare-cache review.diff returns exact unified diff + per-file stats.
    let diff1: ReviewDiffResult = call_ok::<ReviewDiff>(
        &fx.review_svc,
        &fx.reviewer_agent,
        &ReviewDiffParams {
            review: created.review.key.clone(),
            round: Some(1),
            since_round: None,
            path: Some("lib.rs".into()),
            stat_only: false,
            context_lines: Some(2),
        },
    );
    assert_eq!(diff1.head_oid, head1);
    assert_eq!(diff1.files.len(), 1);
    assert_eq!(diff1.files[0].path, "lib.rs");
    assert!(diff1.unified_diff.contains("pub fn beta()"));

    // 6. Out-of-range FileLine anchor is refused; valid anchor gets server-computed sha256.
    let bad_finding = ReviewFindingParams {
        command_id: uuid(110),
        review: created.review.key.clone(),
        op: ReviewFindingOp::Add(ReviewFindingDraft {
            finding_id: uuid(111),
            severity: FindingSeverity::Blocker,
            anchor: FindingAnchor::file_lines("lib.rs", 99, 100),
            body_markdown: "Out of bounds".into(),
            attachment_ids: vec![],
        }),
    };
    assert_eq!(
        call_err::<ReviewFinding>(&fx.review_svc, &fx.reviewer_agent, &bad_finding),
        ErrorCode::InvalidRequest
    );

    let f_id = uuid(112);
    let added: ReviewFindingResult = call_ok::<ReviewFinding>(
        &fx.review_svc,
        &fx.reviewer_agent,
        &ReviewFindingParams {
            command_id: uuid(113),
            review: created.review.key.clone(),
            op: ReviewFindingOp::Add(ReviewFindingDraft {
                finding_id: f_id.clone(),
                severity: FindingSeverity::Blocker,
                anchor: FindingAnchor::file_lines("lib.rs", 4, 6),
                body_markdown: "Return 43 instead of 42.".into(),
                attachment_ids: vec![],
            }),
        },
    );
    let sha = added
        .finding
        .anchor
        .anchor_snippet_sha256
        .clone()
        .expect("server computes anchor_snippet_sha256");
    assert_eq!(sha.len(), 64);
    assert!(!added.finding.anchor_stale);

    // Burst replies on the same finding coalesce into ONE ReviewReply wake for author_agent.
    let _ = call_ok::<ReviewFinding>(
        &fx.review_svc,
        &fx.reviewer_agent,
        &ReviewFindingParams {
            command_id: uuid(114),
            review: created.review.key.clone(),
            op: ReviewFindingOp::Reply {
                finding_id: f_id.clone(),
                reply_id: uuid(115),
                body_markdown: "Extra note on beta.".into(),
                set_status: None,
                attachment_ids: vec![],
            },
        },
    );
    let author_wakes = fx
        .sessions
        .store()
        .read_pending_wakes(&fx.author_agent)
        .unwrap()
        .expect("author wakes queued");
    let reply_wakes: Vec<_> = author_wakes
        .items
        .iter()
        .filter(|i| i.kind == WakeKind::ReviewReply)
        .collect();
    assert_eq!(
        reply_wakes.len(),
        1,
        "finding activity coalesces per finding"
    );
    assert!(reply_wakes[0].summary.contains("Extra note on beta"));

    // 7. Reviewer submits verdict ChangesRequested (also exercises review.verdict + wake).
    let verdict_res: ReviewVerdictResult = call_ok::<ReviewVerdictCall>(
        &fx.review_svc,
        &fx.reviewer_agent,
        &ReviewVerdictParams {
            command_id: uuid(116),
            review: created.review.key.clone(),
            round_number: 1,
            head_oid: Some(head1.clone()),
            verdict: ReviewVerdict::ChangesRequested,
            summary_markdown: "Fix beta() return value.".into(),
            findings: vec![],
            attachment_ids: vec![],
        },
    );
    assert_eq!(verdict_res.review.status, ReviewStatus::ChangesRequested);

    // 8. Author commits fix -> pin_stale becomes true on inspect & list BEFORE resubmission!
    std::fs::write(
        fx.worktree_dir.join("lib.rs"),
        "pub fn alpha() -> u32 {\n    2\n}\npub fn beta() -> u32 {\n    43\n}\n",
    )
    .unwrap();
    run_git(&fx.worktree_dir, &["add", "lib.rs"]);
    run_git(
        &fx.worktree_dir,
        &[
            "-c",
            "user.name=Author",
            "-c",
            "user.email=author@example.test",
            "commit",
            "-m",
            "round 2 fix",
        ],
    );
    let head2 = run_git(&fx.worktree_dir, &["rev-parse", "HEAD"]);

    let listed: ReviewListResult = call_ok::<ReviewList>(
        &fx.review_svc,
        &fx.author_agent,
        &ReviewListParams {
            project_id: Some(fx.project_id.clone()),
            ..Default::default()
        },
    );
    assert_eq!(listed.reviews.len(), 1);
    assert!(listed.reviews[0].pin_stale, "list must detect stale pin");

    // 9. Server-side --fields pruning omits unrequested sections and body_markdown!
    let pruned_req = Request::new::<ReviewInspect>(
        2,
        &ReviewInspectParams {
            review: created.review.key.clone(),
            fields: Some(vec!["review.status".into(), "review.pin_stale".into()]),
        },
    )
    .unwrap();
    let pruned_resp = fx
        .review_svc
        .dispatch(Some(&fx.author_agent), &pruned_req)
        .unwrap();
    let Outcome::Result(pruned_val) = pruned_resp.outcome else {
        panic!("expected pruned inspect ok");
    };
    assert!(pruned_val.get("rounds").is_none());
    assert!(pruned_val.get("reviewers").is_none());
    assert!(pruned_val.get("findings").is_none());
    assert!(pruned_val["review"].get("body_markdown").is_none());
    assert_eq!(pruned_val["review"]["pin_stale"], true);

    // 10. Resubmit as Round 2 (Warm): pins head2, wakes reviewer with inter-round diff cues.
    let resub: ReviewRoundResult = call_ok::<ReviewResubmit>(
        &fx.review_svc,
        &fx.author_agent,
        &ReviewResubmitParams {
            command_id: uuid(120),
            review: created.review.key.clone(),
            expected_revision: verdict_res.review.revision,
            expected_head_oid: Some(head2.clone()),
            session_mode: Some(ReviewSessionMode::Warm),
            fresh: false,
            note_markdown: "Updated beta() to return 43.".into(),
        },
    );
    assert_eq!(resub.round.round_number, 2);
    assert_eq!(resub.round.head_oid.as_deref(), Some(head2.as_str()));
    assert_eq!(resub.round.session_mode, ReviewSessionMode::Warm);

    let wakes_r2 = fx
        .sessions
        .store()
        .read_pending_wakes(&fx.reviewer_agent)
        .unwrap()
        .unwrap();
    let r2_item = wakes_r2
        .items
        .iter()
        .find(|i| i.item_id.contains(":r2:"))
        .expect("round 2 wake present");
    assert!(r2_item.summary.contains("Inter-round diff"));
    assert!(r2_item.summary.contains("--since-round 1"));
    assert!(r2_item
        .summary
        .contains("regressions or previously unexamined areas"));
    assert_eq!(
        r2_item.session_affinity.as_ref().map(|a| a.fresh),
        Some(false)
    );

    // 11. Inter-round diff (`--since-round 1`) shows only the 42 -> 43 delta!
    let inter_diff: ReviewDiffResult = call_ok::<ReviewDiff>(
        &fx.review_svc,
        &fx.reviewer_agent,
        &ReviewDiffParams {
            review: created.review.key.clone(),
            round: Some(2),
            since_round: Some(1),
            path: None,
            stat_only: false,
            context_lines: None,
        },
    );
    assert_eq!(inter_diff.base_oid, head1);
    assert_eq!(inter_diff.head_oid, head2);
    assert!(inter_diff.unified_diff.contains("-    42"));
    assert!(inter_diff.unified_diff.contains("+    43"));

    // 12. Full inspect at Round 2 marks the Round 1 finding anchor as stale (`anchor_stale = true`).
    let insp2: ReviewInspectResult = call_ok::<ReviewInspect>(
        &fx.review_svc,
        &fx.reviewer_agent,
        &ReviewInspectParams {
            review: created.review.key.clone(),
            fields: None,
        },
    );
    assert!(!insp2.review.pin_stale);
    assert!(
        insp2.findings[0].anchor_stale,
        "changing anchored lines 4..=6 must mark anchor_stale = true"
    );

    // 13. Fresh resubmission (`--fresh`) records Fresh mode & queues `fresh: true` wake affinity.
    let resub_fresh: ReviewRoundResult = call_ok::<ReviewResubmit>(
        &fx.review_svc,
        &fx.author_agent,
        &ReviewResubmitParams {
            command_id: uuid(121),
            review: created.review.key.clone(),
            expected_revision: resub.review.revision,
            expected_head_oid: Some(head2.clone()),
            session_mode: None,
            fresh: true,
            note_markdown: "Final clean validation pass.".into(),
        },
    );
    assert_eq!(resub_fresh.round.round_number, 3);
    assert_eq!(resub_fresh.round.session_mode, ReviewSessionMode::Fresh);
    let wakes_r3 = fx
        .sessions
        .store()
        .read_pending_wakes(&fx.reviewer_agent)
        .unwrap()
        .unwrap();
    let r3_item = wakes_r3
        .items
        .iter()
        .find(|i| i.item_id.contains(":r3:"))
        .expect("round 3 fresh wake present");
    assert_eq!(
        r3_item.session_affinity.as_ref().map(|a| a.fresh),
        Some(true)
    );
    assert_eq!(
        r3_item
            .session_affinity
            .as_ref()
            .and_then(|a| a.resume_runtime_session_id.as_deref()),
        None
    );

    // 14. Route & Close work cleanly.
    let routed: ReviewRouteResult = call_ok::<ReviewRoute>(
        &fx.review_svc,
        &fx.owner,
        &ReviewRouteParams {
            command_id: uuid(122),
            review: created.review.key.clone(),
            expected_revision: resub_fresh.review.revision,
            routing: ReviewRoutingSpec {
                mode: RoutingMode::Explicit,
                reviewer_ids: vec![fx.owner.clone(), fx.reviewer_agent.clone()],
                team_id: None,
            },
            session_mode: None,
        },
    );
    assert_eq!(routed.reviewers.iter().filter(|r| !r.removed).count(), 2);

    let closed: ReviewMutationResult = call_ok::<ReviewClose>(
        &fx.review_svc,
        &fx.owner,
        &ReviewCloseParams {
            command_id: uuid(123),
            review: created.review.key,
            expected_revision: routed.review.revision,
            reason: Some("merged".into()),
        },
    );
    assert_eq!(closed.review.status, ReviewStatus::Closed);
}

#[test]
fn round_five_escalation_advisory_notice_triggers_on_resubmit_and_verdict() {
    let fx = setup_fixture();
    run_git(
        &fx.worktree_dir,
        &[
            "-c",
            "user.name=Author",
            "-c",
            "user.email=author@example.test",
            "commit",
            "--allow-empty",
            "-m",
            "r1",
        ],
    );
    let head = run_git(&fx.worktree_dir, &["rev-parse", "HEAD"]);
    let created: ReviewRoundResult = call_ok::<ReviewRequest>(
        &fx.review_svc,
        &fx.author_agent,
        &ReviewRequestParams {
            command_id: uuid(200),
            review_id: uuid(201),
            target_kind: ReviewTargetKind::Code,
            title: "Long review loop".into(),
            body_markdown: String::new(),
            kind_slug: "code-review".into(),
            category_slug: None,
            project_id: Some(fx.project_id.clone()),
            task_id: Some(fx.task_id.clone()),
            repository_id: Some(fx.repo_id.clone()),
            branch: Some(fx.branch.clone()),
            target_ref: None,
            base_oid: None,
            expected_head_oid: Some(head.clone()),
            independent: false,
            routing: Some(ReviewRoutingSpec {
                mode: RoutingMode::Explicit,
                reviewer_ids: vec![fx.reviewer_agent.clone()],
                team_id: None,
            }),
            session_mode: Some(ReviewSessionMode::Warm),
            guidance_refs: vec![],
            criteria: vec![],
        },
    );
    let mut rev = created.review.revision;
    for r in 2u32..=5 {
        let resub: ReviewRoundResult = call_ok::<ReviewResubmit>(
            &fx.review_svc,
            &fx.author_agent,
            &ReviewResubmitParams {
                command_id: uuid(200 + u64::from(r) * 10),
                review: created.review.key.clone(),
                expected_revision: rev,
                expected_head_oid: Some(head.clone()),
                session_mode: Some(ReviewSessionMode::Warm),
                fresh: false,
                note_markdown: format!("round {r}"),
            },
        );
        rev = resub.review.revision;
    }
    let wakes = fx
        .sessions
        .store()
        .read_pending_wakes(&fx.reviewer_agent)
        .unwrap()
        .unwrap();
    let r5_wake = wakes
        .items
        .iter()
        .find(|i| i.item_id.contains(":r5:"))
        .expect("round 5 wake");
    assert!(
        r5_wake.summary.contains("Advisory escalation notice")
            && r5_wake.summary.contains("round 5"),
        "expected >=5 round escalation notice, got: {}",
        r5_wake.summary
    );

    let _ = call_ok::<ReviewVerdictCall>(
        &fx.review_svc,
        &fx.reviewer_agent,
        &ReviewVerdictParams {
            command_id: uuid(299),
            review: created.review.key,
            round_number: 5,
            head_oid: Some(head),
            verdict: ReviewVerdict::ChangesRequested,
            summary_markdown: "Still needs alignment.".into(),
            findings: vec![],
            attachment_ids: vec![],
        },
    );
    let author_wakes = fx
        .sessions
        .store()
        .read_pending_wakes(&fx.author_agent)
        .unwrap()
        .unwrap();
    let v5_wake = author_wakes
        .items
        .iter()
        .find(|i| i.kind == WakeKind::ReviewVerdict)
        .expect("verdict wake");
    assert!(v5_wake.summary.contains("Advisory escalation notice"));
}
