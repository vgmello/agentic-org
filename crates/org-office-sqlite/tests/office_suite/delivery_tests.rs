//! Dual-engine (`Memory` + `Sqlite`) and `Delete+Rebuild` tests for `TASK-55` AC #2
//! (`FC-058..070`): canonical features, Why links, knowledge-library search with project-scoped
//! visibility, releases, milestones, progress rollups, changelogs, deployments, rollback guards,
//! feature flags, release/deployment flag snapshots, and `used_flag_identity_immutable`.

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use org_office::organization_family::testkit::CREATION_AT;
use org_office::rules::OrgCommand;
use org_office::testkit_support::office_harness::{places, record, request, CA_SEED};
use org_office::testkit_support::store_open::EngineChoice;
use org_office::testkit_support::Options;
use org_office::{Custody, DataRoots, Office, OfficeError, TestSatellite};
use org_proto::delivery::{
    DeploymentFlagSettingInput, DeploymentFlagSettingsSetParams, DeploymentOperationWire,
    DeploymentStatusWire, DeploymentsCreateParams, DeploymentsInspectParams, DeploymentsListParams,
    DeploymentsSetChangelogParams, DeploymentsUpdateParams, FeatureStatusWire, FeatureWorkTypeWire,
    FeaturesCreateParams, FeaturesInspectParams, FeaturesListParams, FeaturesRetireParams,
    FeaturesUpdateParams, FlagStatusWire, FlagValueTypeWire, FlagsCreateParams, FlagsInspectParams,
    FlagsListParams, FlagsUpdateParams, LinkEntityKindWire, LinkKindWire, LinksAddParams,
    LinksListParams, LinksRemoveParams, MilestonesCreateParams, MilestonesUpdateParams,
    ReleaseFlagSettingInput, ReleaseFlagSettingsSetParams, ReleasesCreateParams,
    ReleasesInspectParams, ReleasesListParams, ReleasesSetChangelogParams, ReleasesUpdateParams,
    RepoCommitWire,
};
use org_proto::docs::{DocStatusWire, DocsCreateParams};
use org_proto::git::RepositoryKindWire;
use org_proto::hiring::{HireProposalWire, HrHire, HrHireParams};
use org_proto::org::{MemberAddPersonParams, ProjectCreateParams, TeamCreateParams};
use org_proto::search::{SearchEntityType, SearchQueryParams};
use org_proto::tasks::{
    PatchField, TaskDetailsWire, TaskStatus, TasksCreateParams, TasksTransitionParams,
    TasksUpdateParams,
};
use org_proto::{ApiError, ErrorCode, Outcome, Request};
use org_store::testkit::{identity, FakeCustody, FixedClock};
use serde_json::{json, Value};

static NEXT_ID: AtomicU64 = AtomicU64::new(220_000);

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

fn ok<T>(res: Result<Result<T, ApiError>, OfficeError>) -> T {
    res.expect("storage").expect("accepted")
}

fn err<T: std::fmt::Debug>(res: Result<Result<T, ApiError>, OfficeError>) -> ApiError {
    res.expect("storage").expect_err("expected refusal")
}

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
    other_project: String,
    repo: String,
    owning_agent: String,
    outsider_agent: String,
}

fn options(engine: EngineChoice) -> Options {
    Options {
        engine,
        clock: Arc::new(FixedClock(CREATION_AT)),
        testing: Default::default(),
        before_final_checks: None,
    }
}

fn seed(engine: EngineChoice) -> Fx {
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

    let mk_team = |name: &str| {
        let id = next_uuid();
        office
            .submit_org(
                &owner,
                OrgCommand::TeamCreate(TeamCreateParams {
                    command_id: next_uuid(),
                    team_id: id.clone(),
                    name: name.into(),
                    owner_team_id: Some(root.clone()),
                }),
            )
            .unwrap()
            .unwrap();
        id
    };
    let owning_team = mk_team("Platform");
    let outsider_team = mk_team("Finance");

    let mk_project = |name: &str, slug: &str, team_id: &str| {
        let id = next_uuid();
        office
            .submit_org(
                &owner,
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
                    owner_team_id: Some(team_id.into()),
                    create_team_id: None,
                    parent_team_id: None,
                    contributing_team_ids: vec![],
                }),
            )
            .unwrap()
            .unwrap();
        id
    };
    let project = mk_project("Checkout", "checkout", &owning_team);
    let other_project = mk_project("Billing", "billing", &outsider_team);

    let repo = next_uuid();
    ok(office.register_repository(
        &owner,
        &next_uuid(),
        &repo,
        Some("checkout-api".into()),
        RepositoryKindWire::Remote,
        Some("https://github.com/acme/checkout-api.git".into()),
    ));
    ok(office.attach_project_repository(
        &owner,
        &next_uuid(),
        "checkout",
        "checkout-api",
        "main".into(),
    ));

    let person = next_uuid();
    office
        .submit_org(
            &owner,
            OrgCommand::MemberAddPerson(MemberAddPersonParams {
                command_id: next_uuid(),
                member_id: person.clone(),
                name: "Ada".into(),
                handle: Some("ada".into()),
                home_team_id: Some(outsider_team.clone()),
                contact: None,
            }),
        )
        .unwrap()
        .unwrap();

    let hire = |team_id: &str, handle: &str| {
        let id = next_uuid();
        let req = Request::new::<HrHire>(
            1,
            &HrHireParams {
                command_id: next_uuid(),
                request_id: next_uuid(),
                employee_id: id.clone(),
                home_team_id: team_id.into(),
                role_id: None,
                proposal: HireProposalWire {
                    name: handle.into(),
                    handle: handle.into(),
                    ..Default::default()
                },
                reason: "delivery test".into(),
            },
        )
        .unwrap();
        let ans = office.dispatch_hiring_rpc(Some(&owner), &req).unwrap();
        assert!(matches!(ans.outcome, Outcome::Result(_)), "{ans:?}");
        id
    };

    let owning_agent = hire(&owning_team, "platform-bot");
    let outsider_agent = hire(&outsider_team, "finance-bot");
    Fx {
        _tmp: tmp,
        folder,
        roots,
        custody,
        engine,
        office,
        owner,
        person,
        owning_team,
        project,
        other_project,
        repo,
        owning_agent,
        outsider_agent,
    }
}

fn reopen(fx: Fx) -> Fx {
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
        other_project,
        repo,
        owning_agent,
        outsider_agent,
    } = fx;
    office.close().unwrap();
    let runtime = roots.runtime.join(identity().office_id.as_str());
    if runtime.exists() {
        std::fs::remove_dir_all(&runtime).unwrap();
    }
    let office = Office::open_with(
        folder.clone(),
        None,
        identity().instance_id.as_str(),
        roots.clone(),
        custody.clone(),
        options(engine.clone()),
    )
    .expect("reopen after projection delete");
    Fx {
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
        other_project,
        repo,
        owning_agent,
        outsider_agent,
    }
}

fn task_create(
    fx: &Fx,
    title: &str,
    task_type: &str,
    project_id: &str,
    affects_feature_id: Option<String>,
) -> String {
    let id = next_uuid();
    ok(fx.office.task_create(
        &fx.owner,
        TasksCreateParams {
            command_id: next_uuid(),
            task_id: id.clone(),
            team_id: Some(fx.owning_team.clone()),
            assigned_team_id: None,
            project_id: Some(project_id.to_owned()),
            task_type: Some(task_type.to_owned()),
            work_type: None,
            title: title.to_owned(),
            body_markdown: String::new(),
            assignee_id: Some(fx.owning_agent.clone()),
            priority: None,
            effort: None,
            severity: None,
            rank_order: None,
            labels: vec![],
            reference_files: vec![],
            modified_files: vec![],
            depends_on: vec![],
            details: affects_feature_id.map(|fid| TaskDetailsWire {
                affects_feature_id: Some(fid),
                ..Default::default()
            }),
            dod: None,
            attachment_ids: vec![],
        },
    ));
    id
}

fn delivery_snapshot(fx: &Fx, f1: &str, r1: &str, d1: &str, fl1: &str) -> Value {
    json!({
        "features": ok(fx.office.features_list(&fx.owner, FeaturesListParams { project: fx.project.clone(), status: None })),
        "feature1": ok(fx.office.features_inspect(&fx.owner, FeaturesInspectParams { project: fx.project.clone(), feature: f1.into() })),
        "links": ok(fx.office.links_list(&fx.owner, LinksListParams { project: fx.project.clone(), ..Default::default() })),
        "releases": ok(fx.office.releases_list(&fx.owner, ReleasesListParams { project: fx.project.clone() })),
        "release1": ok(fx.office.releases_inspect(&fx.owner, ReleasesInspectParams { project: fx.project.clone(), release: r1.into() })),
        "deployments": ok(fx.office.deployments_list(&fx.owner, DeploymentsListParams { project: fx.project.clone(), ..Default::default() })),
        "deployment1": ok(fx.office.deployments_inspect(&fx.owner, DeploymentsInspectParams { project: fx.project.clone(), deployment_id: d1.into() })),
        "flags": ok(fx.office.flags_list(&fx.owner, FlagsListParams { project: fx.project.clone(), status: None, used_only: false })),
        "flag1": ok(fx.office.flags_inspect(&fx.owner, FlagsInspectParams { project: fx.project.clone(), flag: fl1.into() })),
    })
}

fn phase_features(fx: &Fx) -> String {
    let p = &fx.project;
    let f1 = ok(fx.office.features_create(
        &fx.owning_agent,
        FeaturesCreateParams {
            command_id: next_uuid(),
            project: p.clone(),
            title: "Apple Pay".into(),
            local_slug: Some("apple-pay".into()),
            status: Some(FeatureStatusWire::InProgress),
            work_type: Some(FeatureWorkTypeWire::Build),
            owner_employee_id: Some(fx.owning_agent.clone()),
            body_markdown: "One-tap Apple Pay checkout.".into(),
            delivered_at: None,
        },
    ))
    .feature;
    assert_eq!(
        (f1.key.as_str(), f1.slug.as_str()),
        ("F-1", "checkout/apple-pay")
    );
    let clear_owner = err(fx.office.features_update(
        &fx.owning_agent,
        FeaturesUpdateParams {
            command_id: next_uuid(),
            project: p.clone(),
            feature_id: f1.id.clone(),
            expected_revision: 1,
            owner_employee_id: Some(String::new()),
            ..Default::default()
        },
    ));
    assert_eq!(clear_owner.code, ErrorCode::InvalidRequest);

    let f2 = ok(fx.office.features_create(
        &fx.owning_agent,
        FeaturesCreateParams {
            command_id: next_uuid(),
            project: p.clone(),
            title: "Express Pay".into(),
            local_slug: Some("express-pay".into()),
            status: Some(FeatureStatusWire::Live),
            owner_employee_id: Some(fx.owning_agent.clone()),
            ..Default::default()
        },
    ))
    .feature;
    assert_eq!(f2.key, "F-2");

    let self_rep = err(fx.office.features_retire(
        &fx.owner,
        FeaturesRetireParams {
            command_id: next_uuid(),
            project: p.clone(),
            feature_id: f1.id.clone(),
            expected_revision: 1,
            retired_reason: Some("self".into()),
            replaced_by_feature_id: Some(f1.id.clone()),
        },
    ));
    assert_eq!(self_rep.code, ErrorCode::InvalidRequest);

    let f1_ret = ok(fx.office.features_retire(
        &fx.owner,
        FeaturesRetireParams {
            command_id: next_uuid(),
            project: p.clone(),
            feature_id: f1.id.clone(),
            expected_revision: 1,
            retired_reason: Some("superseded".into()),
            replaced_by_feature_id: Some(f2.id.clone()),
        },
    ))
    .feature;
    assert_eq!(f1_ret.status, FeatureStatusWire::Retired);
    let cycle = err(fx.office.features_retire(
        &fx.owner,
        FeaturesRetireParams {
            command_id: next_uuid(),
            project: p.clone(),
            feature_id: f2.id.clone(),
            expected_revision: 1,
            retired_reason: Some("cycle".into()),
            replaced_by_feature_id: Some(f1.id.clone()),
        },
    ));
    assert_eq!(cycle.code, ErrorCode::InvalidRequest);

    for r in ["F-2", "express-pay", "checkout/express-pay"] {
        let got = ok(fx.office.features_inspect(
            &fx.owning_agent,
            FeaturesInspectParams {
                project: "checkout".into(),
                feature: r.into(),
            },
        ));
        assert_eq!(got.feature.id, f2.id, "{r}");
    }
    f2.id
}

fn phase_doc_and_tasks(fx: &Fx, f2_id: &str) -> (String, String, String) {
    let p = &fx.project;
    let doc = ok(fx.office.docs_create(
        &fx.owning_agent,
        DocsCreateParams {
            command_id: next_uuid(),
            project: p.clone(),
            kind: "adr".into(),
            title: "Payment Gateway ADR".into(),
            section: Some("decisions".into()),
            body_markdown: "Why we chose Express Pay.".into(),
            ..Default::default()
        },
    ))
    .document;
    let impl_task = task_create(fx, "Build Express Pay", "task", p, None);
    let bug_task = task_create(
        fx,
        "Express Pay Safari crash",
        "bug",
        p,
        Some(f2_id.to_owned()),
    );
    let other_f = ok(fx.office.features_create(
        &fx.owner,
        FeaturesCreateParams {
            command_id: next_uuid(),
            project: fx.other_project.clone(),
            title: "Invoice PDF".into(),
            ..Default::default()
        },
    ))
    .feature;
    let cross_bug = err(fx.office.task_create(
        &fx.owner,
        TasksCreateParams {
            command_id: next_uuid(),
            task_id: next_uuid(),
            team_id: Some(fx.owning_team.clone()),
            assigned_team_id: None,
            project_id: Some(p.clone()),
            task_type: Some("bug".into()),
            work_type: None,
            title: "Bad cross link".into(),
            body_markdown: String::new(),
            assignee_id: None,
            priority: None,
            effort: None,
            severity: None,
            rank_order: None,
            labels: vec![],
            reference_files: vec![],
            modified_files: vec![],
            depends_on: vec![],
            details: Some(TaskDetailsWire {
                affects_feature_id: Some(other_f.id),
                ..Default::default()
            }),
            dod: None,
            attachment_ids: vec![],
        },
    ));
    assert_eq!(cross_bug.code, ErrorCode::NotFound);
    (doc.id, impl_task, bug_task)
}

fn blank_task_update(task: &str, expected_revision: u64) -> TasksUpdateParams {
    TasksUpdateParams {
        command_id: next_uuid(),
        task: task.to_owned(),
        expected_revision,
        title: None,
        body_markdown: None,
        append_body: None,
        task_type: None,
        work_type: PatchField::Unchanged,
        priority: None,
        effort: PatchField::Unchanged,
        severity: PatchField::Unchanged,
        rank_order: PatchField::Unchanged,
        labels: None,
        add_labels: vec![],
        remove_labels: vec![],
        reference_files: None,
        add_reference_files: vec![],
        remove_reference_files: vec![],
        modified_files: None,
        add_modified_files: vec![],
        remove_modified_files: vec![],
        details: None,
        feature_id: PatchField::Unchanged,
        milestone_id: PatchField::Unchanged,
        add_attachments: vec![],
        remove_attachments: vec![],
    }
}

fn phase_release_and_milestone(fx: &Fx, f2_id: &str, impl_task: &str) -> String {
    let p = &fx.project;
    let r1 = ok(fx.office.releases_create(
        &fx.owning_agent,
        ReleasesCreateParams {
            command_id: next_uuid(),
            project: p.clone(),
            title: "Release 1.0".into(),
            version_label: Some("v1.0.0".into()),
            body_markdown: "Initial checkout release.".into(),
            repository_commits: vec![RepoCommitWire {
                repository_id: fx.repo.clone(),
                commit_sha: "abcdef0123456789abcdef0123456789abcdef01".into(),
            }],
            feature_ids: vec![f2_id.to_owned()],
            task_ids: vec![impl_task.to_owned()],
        },
    ))
    .release;
    assert_eq!(r1.key, "R-1");
    assert!(!r1.has_changelog);

    let m1 = ok(fx.office.milestones_create(
        &fx.owning_agent,
        MilestonesCreateParams {
            command_id: next_uuid(),
            project: p.clone(),
            release_id: r1.id.clone(),
            title: "GA Cut".into(),
            body_markdown: "All checkout tasks complete.".into(),
        },
    ))
    .milestone;
    assert_eq!(m1.key, "M-1");
    ok(fx.office.milestones_update(
        &fx.owning_agent,
        MilestonesUpdateParams {
            command_id: next_uuid(),
            project: p.clone(),
            release_id: Some(r1.id.clone()),
            milestone_id: m1.id.clone(),
            expected_revision: 1,
            title: Some("GA Cut Final".into()),
            body_markdown: None,
        },
    ));

    let mut attach = blank_task_update(impl_task, 1);
    attach.feature_id = PatchField::Set(Some(f2_id.to_owned()));
    attach.milestone_id = PatchField::Set(Some(m1.id));
    ok(fx.office.task_update(&fx.owner, attach));

    let r_before = ok(fx.office.releases_inspect(
        &fx.owning_agent,
        ReleasesInspectParams {
            project: p.clone(),
            release: "R-1".into(),
        },
    ));
    assert_eq!(r_before.release.progress.milestone_count, 1);
    assert_eq!(r_before.release.progress.task_count, 1);
    assert_eq!(r_before.release.progress.done_task_count, 0);
    assert!(!r_before.release.progress.is_done);
    assert!(!r_before.milestones[0].progress.is_done);

    ok(fx.office.task_transition(
        &fx.owning_agent,
        TasksTransitionParams {
            command_id: next_uuid(),
            task: impl_task.to_owned(),
            expected_revision: 2,
            status: TaskStatus::Done,
            blocked_reason: None,
            resolution: Some("completed".into()),
            note: None,
            done_anyway_reason: None,
            final_summary: None,
        },
    ));
    let r_after = ok(fx.office.releases_inspect(
        &fx.owning_agent,
        ReleasesInspectParams {
            project: p.clone(),
            release: r1.id.clone(),
        },
    ));
    assert_eq!(r_after.release.progress.done_task_count, 1);
    assert!(r_after.release.progress.is_done);
    assert!(r_after.milestones[0].progress.is_done);
    phase_release_changelog(fx, &r1.id);
    r1.id
}

fn phase_release_changelog(fx: &Fx, release_id: &str) {
    let p = &fx.project;
    let cl1 = ok(fx.office.releases_set_changelog(
        &fx.owning_agent,
        ReleasesSetChangelogParams {
            command_id: next_uuid(),
            project: p.clone(),
            release_id: release_id.to_owned(),
            title: "v1.0.0 Changelog".into(),
            body_markdown: "- Added Express Pay".into(),
            status: Some(DocStatusWire::Draft),
            owner_employee_id: None,
            expected_revision: None,
        },
    ))
    .changelog;
    assert_eq!(cl1.revision, 1);
    let cl2 = ok(fx.office.releases_set_changelog(
        &fx.owning_agent,
        ReleasesSetChangelogParams {
            command_id: next_uuid(),
            project: p.clone(),
            release_id: release_id.to_owned(),
            title: "v1.0.0 Final Changelog".into(),
            body_markdown: "- Added Express Pay\n- Fixed Safari bug".into(),
            status: Some(DocStatusWire::Current),
            owner_employee_id: Some(fx.owning_agent.clone()),
            expected_revision: Some(1),
        },
    ))
    .changelog;
    assert_eq!((cl2.id.as_str(), cl2.revision), (cl1.id.as_str(), 2));
}

fn phase_links(fx: &Fx, f2_id: &str, doc_id: &str, impl_task: &str, bug_task: &str) {
    let p = &fx.project;
    let why_f_doc = ok(fx.office.links_add(
        &fx.owning_agent,
        LinksAddParams {
            command_id: next_uuid(),
            project: p.clone(),
            source_kind: LinkEntityKindWire::Feature,
            source_id: f2_id.to_owned(),
            kind: LinkKindWire::RelatesTo,
            target_kind: LinkEntityKindWire::Document,
            target_id: doc_id.to_owned(),
        },
    ))
    .link;
    let why_t_f = ok(fx.office.links_add(
        &fx.owning_agent,
        LinksAddParams {
            command_id: next_uuid(),
            project: p.clone(),
            source_kind: LinkEntityKindWire::Task,
            source_id: impl_task.to_owned(),
            kind: LinkKindWire::RelatesTo,
            target_kind: LinkEntityKindWire::Feature,
            target_id: f2_id.to_owned(),
        },
    ))
    .link;
    let why_fix = ok(fx.office.links_add(
        &fx.owning_agent,
        LinksAddParams {
            command_id: next_uuid(),
            project: p.clone(),
            source_kind: LinkEntityKindWire::Task,
            source_id: bug_task.to_owned(),
            kind: LinkKindWire::Fixes,
            target_kind: LinkEntityKindWire::Task,
            target_id: impl_task.to_owned(),
        },
    ))
    .link;
    assert_eq!(
        err(fx.office.links_add(
            &fx.owning_agent,
            LinksAddParams {
                command_id: next_uuid(),
                project: p.clone(),
                source_kind: LinkEntityKindWire::Feature,
                source_id: f2_id.to_owned(),
                kind: LinkKindWire::RelatesTo,
                target_kind: LinkEntityKindWire::Feature,
                target_id: f2_id.to_owned(),
            },
        ))
        .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        err(fx.office.links_add(
            &fx.owning_agent,
            LinksAddParams {
                command_id: next_uuid(),
                project: p.clone(),
                source_kind: LinkEntityKindWire::Feature,
                source_id: f2_id.to_owned(),
                kind: LinkKindWire::Fixes,
                target_kind: LinkEntityKindWire::Task,
                target_id: impl_task.to_owned(),
            },
        ))
        .code,
        ErrorCode::InvalidRequest
    );

    let mut label_edit = blank_task_update(impl_task, 3);
    label_edit.labels = Some(vec!["checkout".into()]);
    ok(fx.office.task_update(&fx.owner, label_edit));
    let all_links = ok(fx.office.links_list(
        &fx.owning_agent,
        LinksListParams {
            project: p.clone(),
            ..Default::default()
        },
    ))
    .links;
    assert_eq!(all_links.len(), 3, "task update must preserve Why links");
    let removed = ok(fx.office.links_remove(
        &fx.owning_agent,
        LinksRemoveParams {
            command_id: next_uuid(),
            project: p.clone(),
            source_kind: LinkEntityKindWire::Task,
            source_id: bug_task.to_owned(),
            link_id: why_fix.id.clone(),
        },
    ));
    assert_eq!(removed.removed_id, why_fix.id);
    assert!(all_links.iter().any(|l| l.id == why_f_doc.id));
    assert!(all_links.iter().any(|l| l.id == why_t_f.id));
}

fn phase_deployments(fx: &Fx, r1_id: &str) -> String {
    let p = &fx.project;
    let bad_ts = err(fx.office.deployments_create(
        &fx.owning_agent,
        DeploymentsCreateParams {
            command_id: next_uuid(),
            project: p.clone(),
            release_id: r1_id.to_owned(),
            environment: "prod".into(),
            status: Some(DeploymentStatusWire::Succeeded),
            started_at: Some("2026-11-27T10:00:00Z".into()),
            finished_at: None,
            ..Default::default()
        },
    ));
    assert_eq!(bad_ts.code, ErrorCode::InvalidRequest);

    let d1 = ok(fx.office.deployments_create(
        &fx.owning_agent,
        DeploymentsCreateParams {
            command_id: next_uuid(),
            project: p.clone(),
            release_id: r1_id.to_owned(),
            environment: "prod".into(),
            target: Some("us-east".into()),
            operation: Some(DeploymentOperationWire::Deploy),
            status: Some(DeploymentStatusWire::Running),
            started_at: Some("2026-11-27T10:00:00Z".into()),
            repository_commits: vec![RepoCommitWire {
                repository_id: fx.repo.clone(),
                commit_sha: "abcdef0123456789abcdef0123456789abcdef01".into(),
            }],
            ..Default::default()
        },
    ))
    .deployment;
    ok(fx.office.deployments_update(
        &fx.owning_agent,
        DeploymentsUpdateParams {
            command_id: next_uuid(),
            project: p.clone(),
            deployment_id: d1.id.clone(),
            expected_revision: 1,
            status: Some(DeploymentStatusWire::Succeeded),
            finished_at: Some("2026-11-27T10:05:00Z".into()),
            ..Default::default()
        },
    ));
    let cross_env = err(fx.office.deployments_create(
        &fx.owning_agent,
        DeploymentsCreateParams {
            command_id: next_uuid(),
            project: p.clone(),
            release_id: r1_id.to_owned(),
            environment: "staging".into(),
            target: Some("us-east".into()),
            operation: Some(DeploymentOperationWire::Rollback),
            rollback_of_id: Some(d1.id.clone()),
            ..Default::default()
        },
    ));
    assert_eq!(cross_env.code, ErrorCode::InvalidRequest);

    let d2 = ok(fx.office.deployments_create(
        &fx.owning_agent,
        DeploymentsCreateParams {
            command_id: next_uuid(),
            project: p.clone(),
            release_id: r1_id.to_owned(),
            environment: "prod".into(),
            target: Some("us-east".into()),
            operation: Some(DeploymentOperationWire::Rollback),
            rollback_of_id: Some(d1.id.clone()),
            ..Default::default()
        },
    ))
    .deployment;
    let rb_cycle = err(fx.office.deployments_update(
        &fx.owning_agent,
        DeploymentsUpdateParams {
            command_id: next_uuid(),
            project: p.clone(),
            deployment_id: d2.id.clone(),
            expected_revision: 1,
            rollback_of_id: Some(d2.id),
            ..Default::default()
        },
    ));
    assert_eq!(rb_cycle.code, ErrorCode::InvalidRequest);

    let dcl = ok(fx.office.deployments_set_changelog(
        &fx.owning_agent,
        DeploymentsSetChangelogParams {
            command_id: next_uuid(),
            project: p.clone(),
            deployment_id: d1.id.clone(),
            title: "Prod Deploy Notes".into(),
            body_markdown: "Zero-downtime rollout.".into(),
            status: Some(DocStatusWire::Draft),
            owner_employee_id: None,
            expected_revision: None,
        },
    ))
    .changelog;
    assert_eq!(dcl.deployment_id.as_deref(), Some(d1.id.as_str()));
    d1.id
}

fn phase_flag_create_and_snapshots(fx: &Fx, f2_id: &str, r1_id: &str, d1_id: &str) -> String {
    let p = &fx.project;
    let bad_default = err(fx.office.flags_create(
        &fx.owning_agent,
        FlagsCreateParams {
            command_id: next_uuid(),
            project: p.clone(),
            key: "express-checkout".into(),
            value_type: FlagValueTypeWire::Boolean,
            default_value: json!("not-a-bool"),
            feature_id: Some(f2_id.to_owned()),
            owner_employee_id: Some(fx.owning_agent.clone()),
            status: Some(FlagStatusWire::Active),
            body_markdown: String::new(),
        },
    ));
    assert_eq!(bad_default.code, ErrorCode::InvalidRequest);

    let fl1 = ok(fx.office.flags_create(
        &fx.owning_agent,
        FlagsCreateParams {
            command_id: next_uuid(),
            project: p.clone(),
            key: "express-pay-beta".into(),
            value_type: FlagValueTypeWire::Boolean,
            default_value: json!(false),
            feature_id: Some(f2_id.to_owned()),
            owner_employee_id: Some(fx.owning_agent.clone()),
            status: Some(FlagStatusWire::Active),
            body_markdown: "Gates Express Pay rollout.".into(),
        },
    ))
    .flag;
    assert!(!fl1.used);
    let fl1 = ok(fx.office.flags_update(
        &fx.owning_agent,
        FlagsUpdateParams {
            command_id: next_uuid(),
            project: p.clone(),
            flag_id: fl1.id,
            expected_revision: 1,
            key: Some("express-pay-enabled".into()),
            ..Default::default()
        },
    ))
    .flag;
    assert_eq!((fl1.key.as_str(), fl1.revision), ("express-pay-enabled", 2));

    for (val, rules) in [(json!(123), None), (json!(true), Some(json!([1, 2, 3])))] {
        assert_eq!(
            err(fx.office.releases_set_flag_settings(
                &fx.owning_agent,
                ReleaseFlagSettingsSetParams {
                    command_id: next_uuid(),
                    project: p.clone(),
                    release_id: r1_id.to_owned(),
                    expected_revision: 1,
                    settings: vec![ReleaseFlagSettingInput {
                        flag_id: fl1.id.clone(),
                        environment: "prod".into(),
                        target: Some("us-east".into()),
                        value: val,
                        rollout_rules: rules,
                    }],
                },
            ))
            .code,
            ErrorCode::InvalidRequest
        );
    }

    ok(fx.office.releases_set_flag_settings(
        &fx.owning_agent,
        ReleaseFlagSettingsSetParams {
            command_id: next_uuid(),
            project: p.clone(),
            release_id: r1_id.to_owned(),
            expected_revision: 1,
            settings: vec![ReleaseFlagSettingInput {
                flag_id: fl1.id.clone(),
                environment: "prod".into(),
                target: Some("us-east".into()),
                value: json!(true),
                rollout_rules: Some(json!({"percent": 25})),
            }],
        },
    ));
    ok(fx.office.deployments_set_flag_settings(
        &fx.owning_agent,
        DeploymentFlagSettingsSetParams {
            command_id: next_uuid(),
            project: p.clone(),
            deployment_id: d1_id.to_owned(),
            expected_revision: 2,
            settings: vec![DeploymentFlagSettingInput {
                flag_id: fl1.id.clone(),
                value: json!(true),
                rollout_rules: Some(json!({"percent": 25})),
                applied_at: Some("2026-11-27T10:02:00Z".into()),
            }],
        },
    ));
    fl1.id
}

fn phase_flag_immutability(fx: &Fx, fl1_id: &str) {
    let p = &fx.project;
    let inspected_flag = ok(fx.office.flags_inspect(
        &fx.owning_agent,
        FlagsInspectParams {
            project: p.clone(),
            flag: "express-pay-enabled".into(),
        },
    ));
    assert!(inspected_flag.flag.used);
    assert_eq!(inspected_flag.usages.len(), 2);

    let key_mut = err(fx.office.flags_update(
        &fx.owning_agent,
        FlagsUpdateParams {
            command_id: next_uuid(),
            project: p.clone(),
            flag_id: fl1_id.to_owned(),
            expected_revision: 2,
            key: Some("cannot-rename".into()),
            ..Default::default()
        },
    ));
    assert_eq!(key_mut.code, ErrorCode::Conflict);

    let type_mut = err(fx.office.flags_update(
        &fx.owning_agent,
        FlagsUpdateParams {
            command_id: next_uuid(),
            project: p.clone(),
            flag_id: fl1_id.to_owned(),
            expected_revision: 2,
            value_type: Some(FlagValueTypeWire::String),
            default_value: Some(json!("off")),
            ..Default::default()
        },
    ));
    assert_eq!(type_mut.code, ErrorCode::Conflict);

    ok(fx.office.flags_update(
        &fx.owning_agent,
        FlagsUpdateParams {
            command_id: next_uuid(),
            project: p.clone(),
            flag_id: fl1_id.to_owned(),
            expected_revision: 2,
            body_markdown: Some("Updated rollout doc.".into()),
            ..Default::default()
        },
    ));
}

#[test]
fn full_delivery_lifecycle_parity_and_delete_rebuild_on_both_engines() {
    let mut engine_views: Vec<(String, Value)> = Vec::new();
    for engine in engines() {
        let fx = seed(engine.clone());
        let f2_id = phase_features(&fx);
        let (doc_id, impl_task, bug_task) = phase_doc_and_tasks(&fx, &f2_id);
        let r1_id = phase_release_and_milestone(&fx, &f2_id, &impl_task);
        phase_links(&fx, &f2_id, &doc_id, &impl_task, &bug_task);
        let d1_id = phase_deployments(&fx, &r1_id);
        let fl1_id = phase_flag_create_and_snapshots(&fx, &f2_id, &r1_id, &d1_id);
        phase_flag_immutability(&fx, &fl1_id);

        let f2_detail = ok(fx.office.features_inspect(
            &fx.owning_agent,
            FeaturesInspectParams {
                project: fx.project.clone(),
                feature: f2_id.clone(),
            },
        ));
        assert_eq!(f2_detail.implementing_task_ids, vec![impl_task]);
        assert_eq!(f2_detail.affecting_bug_ids, vec![bug_task]);
        assert_eq!(f2_detail.release_ids, vec![r1_id.clone()]);
        assert_eq!(f2_detail.flag_ids, vec![fl1_id.clone()]);
        assert_eq!(f2_detail.links.len(), 2);

        let before_rebuild = delivery_snapshot(&fx, &f2_id, &r1_id, &d1_id, &fl1_id);
        let fx = reopen(fx);
        let after_rebuild = delivery_snapshot(&fx, &f2_id, &r1_id, &d1_id, &fl1_id);
        assert_eq!(after_rebuild, before_rebuild, "{engine:?} rebuild parity");
        engine_views.push((format!("{engine:?}"), after_rebuild));
        fx.office.close().unwrap();
    }
    let (mem, sql) = (&engine_views[0].1, &engine_views[1].1);
    assert_eq!(
        mem["features"]["features"].as_array().unwrap().len(),
        sql["features"]["features"].as_array().unwrap().len()
    );
    assert_eq!(
        mem["feature1"]["feature"]["slug"],
        sql["feature1"]["feature"]["slug"]
    );
    assert_eq!(
        mem["release1"]["release"]["progress"],
        sql["release1"]["release"]["progress"]
    );
    assert_eq!(mem["flag1"]["flag"]["used"], sql["flag1"]["flag"]["used"]);
    assert_eq!(
        mem["flag1"]["usages"].as_array().unwrap().len(),
        sql["flag1"]["usages"].as_array().unwrap().len()
    );
}

fn assert_outsider_hidden(fx: &Fx, f1: &str, r1: &str, d1: &str, fl1: &str) {
    let p = &fx.project;
    let out = &fx.outsider_agent;
    let checks: Vec<(&str, ApiError)> = vec![
        (
            "features.list",
            err(fx.office.features_list(
                out,
                FeaturesListParams {
                    project: p.clone(),
                    status: None,
                },
            )),
        ),
        (
            "features.inspect",
            err(fx.office.features_inspect(
                out,
                FeaturesInspectParams {
                    project: p.clone(),
                    feature: f1.into(),
                },
            )),
        ),
        (
            "features.create",
            err(fx.office.features_create(
                out,
                FeaturesCreateParams {
                    command_id: next_uuid(),
                    project: p.clone(),
                    title: "Sneak".into(),
                    ..Default::default()
                },
            )),
        ),
        (
            "features.update",
            err(fx.office.features_update(
                out,
                FeaturesUpdateParams {
                    command_id: next_uuid(),
                    project: p.clone(),
                    feature_id: f1.into(),
                    expected_revision: 1,
                    title: Some("X".into()),
                    ..Default::default()
                },
            )),
        ),
        (
            "links.list",
            err(fx.office.links_list(
                out,
                LinksListParams {
                    project: p.clone(),
                    ..Default::default()
                },
            )),
        ),
        (
            "releases.list",
            err(fx
                .office
                .releases_list(out, ReleasesListParams { project: p.clone() })),
        ),
        (
            "releases.update",
            err(fx.office.releases_update(
                out,
                ReleasesUpdateParams {
                    command_id: next_uuid(),
                    project: p.clone(),
                    release_id: r1.into(),
                    expected_revision: 1,
                    title: Some("X".into()),
                    ..Default::default()
                },
            )),
        ),
        (
            "deployments.list",
            err(fx.office.deployments_list(
                out,
                DeploymentsListParams {
                    project: p.clone(),
                    ..Default::default()
                },
            )),
        ),
        (
            "deployments.inspect",
            err(fx.office.deployments_inspect(
                out,
                DeploymentsInspectParams {
                    project: p.clone(),
                    deployment_id: d1.into(),
                },
            )),
        ),
        (
            "flags.list",
            err(fx.office.flags_list(
                out,
                FlagsListParams {
                    project: p.clone(),
                    status: None,
                    used_only: false,
                },
            )),
        ),
        (
            "flags.inspect",
            err(fx.office.flags_inspect(
                out,
                FlagsInspectParams {
                    project: p.clone(),
                    flag: fl1.into(),
                },
            )),
        ),
    ];
    for (name, e) in checks {
        assert_eq!(
            (e.code, e.message.as_str()),
            (ErrorCode::NotFound, "project not found"),
            "{name}"
        );
    }
}

fn assert_satellite_read_only(fx: &Fx) {
    let files = fx.office.collect_authoritative_state_snapshot().unwrap();
    let info = fx.office.info();
    let sat = TestSatellite::joined_to(&info.org_id, &info.office_id);
    let batch = files.into_iter().map(|(p, b)| (p, Some(b))).collect();
    sat.office
        .apply_replicated_state_files(&next_uuid(), batch)
        .unwrap();
    assert_eq!(
        ok(sat.office.features_list(
            &fx.owning_agent,
            FeaturesListParams {
                project: fx.project.clone(),
                status: None,
            },
        ))
        .features
        .len(),
        1
    );
    assert_eq!(
        err(sat.office.features_create(
            &fx.owner,
            FeaturesCreateParams {
                command_id: next_uuid(),
                project: fx.project.clone(),
                title: "Local".into(),
                ..Default::default()
            },
        ))
        .code,
        ErrorCode::Unavailable
    );
    sat.office.close().unwrap();
}

#[test]
fn delivery_project_isolation_and_satellite_read_only_boundary() {
    for engine in engines() {
        let fx = seed(engine.clone());
        let p = &fx.project;
        let f1 = ok(fx.office.features_create(
            &fx.owning_agent,
            FeaturesCreateParams {
                command_id: next_uuid(),
                project: p.clone(),
                title: "Private Capability".into(),
                ..Default::default()
            },
        ))
        .feature;
        let r1 = ok(fx.office.releases_create(
            &fx.owning_agent,
            ReleasesCreateParams {
                command_id: next_uuid(),
                project: p.clone(),
                title: "Private Release".into(),
                ..Default::default()
            },
        ))
        .release;
        let d1 = ok(fx.office.deployments_create(
            &fx.owning_agent,
            DeploymentsCreateParams {
                command_id: next_uuid(),
                project: p.clone(),
                release_id: r1.id.clone(),
                environment: "prod".into(),
                ..Default::default()
            },
        ))
        .deployment;
        let fl1 = ok(fx.office.flags_create(
            &fx.owning_agent,
            FlagsCreateParams {
                command_id: next_uuid(),
                project: p.clone(),
                key: "private-flag".into(),
                value_type: FlagValueTypeWire::Boolean,
                default_value: json!(false),
                feature_id: None,
                owner_employee_id: None,
                status: None,
                body_markdown: String::new(),
            },
        ))
        .flag;

        assert_outsider_hidden(&fx, &f1.id, &r1.id, &d1.id, &fl1.id);
        assert_eq!(
            ok(fx.office.features_list(
                &fx.person,
                FeaturesListParams {
                    project: p.clone(),
                    status: None,
                },
            ))
            .features
            .len(),
            1
        );
        if matches!(engine, EngineChoice::Memory) {
            assert_satellite_read_only(&fx);
        }
        fx.office.close().unwrap();
    }
}

#[test]
fn knowledge_library_search_enforces_project_visibility_on_both_engines_and_rebuild() {
    for engine in engines() {
        let fx = seed(engine.clone());

        // Seed matching doc + feature in `project` (checkout) AND `other_project` (billing).
        let doc_a = ok(fx.office.docs_create(
            &fx.owner,
            DocsCreateParams {
                command_id: next_uuid(),
                project: fx.project.clone(),
                kind: "guide".into(),
                title: "Quasar Checkout Guide".into(),
                body_markdown: "quasartoken checkout architecture notes".into(),
                ..Default::default()
            },
        ))
        .document;
        let feat_a = ok(fx.office.features_create(
            &fx.owner,
            FeaturesCreateParams {
                command_id: next_uuid(),
                project: fx.project.clone(),
                title: "Quasar Checkout Engine".into(),
                body_markdown: "quasartoken checkout capability".into(),
                ..Default::default()
            },
        ))
        .feature;
        let doc_b = ok(fx.office.docs_create(
            &fx.owner,
            DocsCreateParams {
                command_id: next_uuid(),
                project: fx.other_project.clone(),
                kind: "guide".into(),
                title: "Quasar Billing Guide".into(),
                body_markdown: "quasartoken secret billing ledger".into(),
                ..Default::default()
            },
        ))
        .document;
        let feat_b = ok(fx.office.features_create(
            &fx.owner,
            FeaturesCreateParams {
                command_id: next_uuid(),
                project: fx.other_project.clone(),
                title: "Quasar Billing Engine".into(),
                body_markdown: "quasartoken secret billing capability".into(),
                ..Default::default()
            },
        ))
        .feature;

        let check = |fx: &Fx, phase: &str| {
            let query = |caller: &str, types: Vec<SearchEntityType>| {
                ok(fx.office.search(
                    caller,
                    SearchQueryParams {
                        q: "quasartoken".into(),
                        types,
                        ..Default::default()
                    },
                ))
            };
            let both = vec![SearchEntityType::Document, SearchEntityType::Feature];

            // Person (owner & Ada) sees docs & features across ALL projects.
            for person in [&fx.owner, &fx.person] {
                let res = query(person, both.clone());
                assert_eq!(res.total, 4, "{engine:?} {phase} person sees all 4");
                assert_eq!(res.hits.len(), 4);
            }

            // Owning agent (checkout) sees ONLY checkout's doc & feature (never billing!).
            let res_a = query(&fx.owning_agent, both.clone());
            assert_eq!(
                res_a.total, 2,
                "{engine:?} {phase} owning_agent total must not leak billing"
            );
            let ids_a: Vec<&str> = res_a.hits.iter().map(|h| h.entity_id.as_str()).collect();
            assert!(ids_a.contains(&doc_a.id.as_str()));
            assert!(ids_a.contains(&feat_a.id.as_str()));
            assert!(!ids_a.contains(&doc_b.id.as_str()));
            assert!(!ids_a.contains(&feat_b.id.as_str()));

            // Outsider agent (billing) sees ONLY billing's doc & feature (never checkout!).
            let res_b = query(&fx.outsider_agent, both.clone());
            assert_eq!(
                res_b.total, 2,
                "{engine:?} {phase} outsider_agent total must not leak checkout"
            );
            let ids_b: Vec<&str> = res_b.hits.iter().map(|h| h.entity_id.as_str()).collect();
            assert!(ids_b.contains(&doc_b.id.as_str()));
            assert!(ids_b.contains(&feat_b.id.as_str()));
            assert!(!ids_b.contains(&doc_a.id.as_str()));
            assert!(!ids_b.contains(&feat_a.id.as_str()));

            // Explicit cross-project query by outsider agent for checkout is refused uniformly
            // as NotFound("project not found"), matching docs.* and delivery.* isolation.
            let cross = err(fx.office.search(
                &fx.outsider_agent,
                SearchQueryParams {
                    q: "quasartoken".into(),
                    types: both,
                    project: Some(fx.project.clone()),
                    ..Default::default()
                },
            ));
            assert_eq!(
                (cross.code, cross.message.as_str()),
                (ErrorCode::NotFound, "project not found"),
                "{engine:?} {phase} explicit private project filter must fail closed"
            );
        };

        check(&fx, "live");
        let fx = reopen(fx);
        check(&fx, "rebuilt");
        fx.office.close().unwrap();
    }
}

#[path = "delivery_hardening_tests.rs"]
mod hardening;
