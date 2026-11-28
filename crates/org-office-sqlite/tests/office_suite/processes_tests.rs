//! Engine-independent conformance tests for `TASK-44` (`C5`: Organization Processes & Standards,
//! `doc-34`, `doc-75`, `FC-110`, `FC-111`, `FC-112`, `FC-184`).
//!
//! Verifies single-chain inheritance (`organization -> owning team chain -> project`),
//! contributing-team exclusion, non-overridable standard sealing, agent write boundaries,
//! same-scope uniqueness vs cross-scope override, optimistic concurrency, idempotency,
//! UTF-8 pagination, built-in skills, `MoveTeam` document relocation, archived-scope refusal,
//! accountable projectless task resolution, and SQLite `office.db` delete+rebuild parity.

use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use org_proto::org::{ProjectCreateParams, TeamCreateParams, TeamDeleteParams, TeamMoveParams};
use org_proto::processes::{
    DocumentKindWire, DocumentStatusWire, ProcessEffectiveParams, ProcessForWire, ProcessScopeWire,
    ProcessShowParams, ProcessViewParams, ProcessWhyParams, ProcessWriteParams,
};
use org_proto::tasks::TasksCreateParams;
use org_proto::ErrorCode;
use org_store::testkit::{identity, FakeCustody, FixedClock};

use org_office::organization_family::testkit::CREATION_AT;
use org_office::rules::OrgCommand;
use org_office::testkit_support::office_harness::{places, record, request, CA_SEED};
use org_office::testkit_support::store_open::EngineChoice;
use org_office::testkit_support::Options;
use org_office::{Custody, DataRoots, Office};

static NEXT_ID: AtomicU64 = AtomicU64::new(90_000);

fn next_uuid() -> String {
    let n = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let ms = CREATION_AT.0 + n;
    let mut b = [0u8; 16];
    b[0..6].copy_from_slice(&ms.to_be_bytes()[2..8]);
    b[6] = 0x70 | ((n >> 8) as u8 & 0x0f);
    b[7] = (n & 0xff) as u8;
    b[8] = 0x80;
    b[15] = (n & 0xff) as u8;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

struct ProcessFixture {
    _tmp: tempfile::TempDir,
    folder: std::path::PathBuf,
    roots: DataRoots,
    custody: Custody,
    office: Office,
    owner: String,
    root_team: String,
    owning_team: String,
    contributing_team: String,
    project_id: String,
    owning_agent: String,
    child_agent: String,
    contributing_agent: String,
}

struct WriteDocArgs<'a> {
    caller: &'a str,
    command_id: String,
    document_id: Option<String>,
    kind: DocumentKindWire,
    scope: ProcessScopeWire,
    key: Option<&'a str>,
    title: &'a str,
    overridable: bool,
    body: &'a str,
    owner_employee_id: Option<String>,
    expected_revision: Option<u64>,
}

fn write_doc(
    office: &Office,
    args: WriteDocArgs<'_>,
) -> Result<org_proto::processes::ProcessWriteResult, org_proto::ApiError> {
    office
        .process_write(
            args.caller,
            ProcessWriteParams {
                command_id: args.command_id,
                document_id: args.document_id,
                kind: args.kind,
                scope: args.scope,
                key: args.key.map(str::to_owned),
                title: args.title.to_owned(),
                status: DocumentStatusWire::Current,
                owner_employee_id: args.owner_employee_id,
                overridable: args.overridable,
                body_markdown: args.body.to_owned(),
                expected_revision: args.expected_revision,
            },
        )
        .expect("storage ok")
}

fn seed_agent(office: &Office, owner: &str, team_id: &str, name: &str, handle: &str) -> String {
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
                title_in_team: Some("Engineer".into()),
                ..Default::default()
            },
            reason: "seed C5 test agent".into(),
        },
    )
    .expect("hr.hire request");
    let resp = office
        .dispatch_hiring_rpc(Some(owner), &req)
        .expect("hr.hire handled");
    assert!(matches!(resp.outcome, Outcome::Result(_)));
    employee_id
}

fn seed_fixture(engine: EngineChoice) -> ProcessFixture {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (folder, roots) = places(tmp.path());
    std::fs::create_dir_all(&roots.durable).unwrap();
    std::fs::create_dir_all(&roots.runtime).unwrap();
    let custody = Custody {
        identity: identity(),
        authority: Arc::new(FakeCustody::new()),
    };
    let (new, binding) = request();
    let options = Options {
        engine: engine.clone(),
        clock: Arc::new(FixedClock(CREATION_AT)),
        testing: Default::default(),
        before_final_checks: None,
    };
    let office = Office::provision_with(
        folder.clone(),
        &new,
        None,
        &binding,
        roots.clone(),
        custody.clone(),
        |_| record(CA_SEED),
        options,
    )
    .expect("provision test office");
    let owner = office.owner_member_id().to_owned();
    let root_team = office.org_snapshot().unwrap().teams[0].id.clone();

    let owning_team = next_uuid();
    office
        .submit_org(
            &owner,
            OrgCommand::TeamCreate(TeamCreateParams {
                command_id: next_uuid(),
                team_id: owning_team.clone(),
                name: "Platform".into(),
                owner_team_id: Some(root_team.clone()),
            }),
        )
        .unwrap()
        .unwrap();

    let child_team = next_uuid();
    office
        .submit_org(
            &owner,
            OrgCommand::TeamCreate(TeamCreateParams {
                command_id: next_uuid(),
                team_id: child_team.clone(),
                name: "Platform Core".into(),
                owner_team_id: Some(owning_team.clone()),
            }),
        )
        .unwrap()
        .unwrap();

    let contributing_team = next_uuid();
    office
        .submit_org(
            &owner,
            OrgCommand::TeamCreate(TeamCreateParams {
                command_id: next_uuid(),
                team_id: contributing_team.clone(),
                name: "Design Ops".into(),
                owner_team_id: Some(root_team.clone()),
            }),
        )
        .unwrap()
        .unwrap();

    let project_id = next_uuid();
    office
        .submit_org(
            &owner,
            OrgCommand::ProjectCreate(ProjectCreateParams {
                command_id: next_uuid(),
                project_id: project_id.clone(),
                name: "Checkout".into(),
                key: None,
                description: None,
                teams: vec![],
                general_area_id: String::new(),
                general_area_name: None,
                repo: None,
                integration_branch: None,
                slug: Some("checkout".into()),
                owner_team_id: Some(owning_team.clone()),
                create_team_id: None,
                parent_team_id: None,
                contributing_team_ids: vec![contributing_team.clone()],
            }),
        )
        .unwrap()
        .unwrap();

    let owning_agent = seed_agent(&office, &owner, &owning_team, "Owner Bot", "owner-bot");
    let child_agent = seed_agent(&office, &owner, &child_team, "Child Bot", "child-bot");
    let contributing_agent = seed_agent(
        &office,
        &owner,
        &contributing_team,
        "Design Bot",
        "design-bot",
    );

    ProcessFixture {
        _tmp: tmp,
        folder,
        roots,
        custody,
        office,
        owner,
        root_team,
        owning_team,
        contributing_team,
        project_id,
        owning_agent,
        child_agent,
        contributing_agent,
    }
}

fn reopen_rebuilt_sqlite(fx: ProcessFixture) -> (tempfile::TempDir, Office) {
    let db_path = fx
        .roots
        .runtime
        .join(identity().office_id.as_str())
        .join("office.db");
    fx.office.close().expect("close office");
    if db_path.exists() {
        std::fs::remove_file(&db_path).expect("remove office.db");
    }
    let reopened = Office::open_with(
        fx.folder,
        None,
        &identity().instance_id.to_string(),
        fx.roots,
        fx.custody,
        Options {
            engine: EngineChoice::Sqlite(org_office_sqlite::sqlite_backend()),
            clock: Arc::new(FixedClock(CREATION_AT)),
            testing: Default::default(),
            before_final_checks: None,
        },
    )
    .expect("reopen rebuilt office");
    (fx._tmp, reopened)
}

fn seed_chain_docs(fx: &ProcessFixture) -> (String, String, String) {
    let sec = write_doc(
        &fx.office,
        WriteDocArgs {
            caller: &fx.owner,
            command_id: next_uuid(),
            document_id: None,
            kind: DocumentKindWire::Standard,
            scope: ProcessScopeWire::Organization,
            key: Some("security-baseline"),
            title: "Security Baseline",
            overridable: false,
            body: "# Security\nNever commit secrets.",
            owner_employee_id: None, // Defaults to caller on Current create!
            expected_revision: None,
        },
    )
    .expect("org security standard");
    assert_eq!(sec.revision, 1);
    assert_eq!(
        sec.document.owner_employee_id.as_deref(),
        Some(fx.owner.as_str())
    );
    assert!(sec.document.path.starts_with("state/standards/"));

    let org_review = write_doc(
        &fx.office,
        WriteDocArgs {
            caller: &fx.owner,
            command_id: next_uuid(),
            document_id: None,
            kind: DocumentKindWire::Process,
            scope: ProcessScopeWire::Organization,
            key: Some("code-review"),
            title: "Org Code Review",
            overridable: true,
            body: "One reviewer required.",
            owner_employee_id: Some(fx.owner.clone()),
            expected_revision: None,
        },
    )
    .expect("org review process");

    let team_review = write_doc(
        &fx.office,
        WriteDocArgs {
            caller: &fx.owner,
            command_id: next_uuid(),
            document_id: None,
            kind: DocumentKindWire::Process,
            scope: ProcessScopeWire::Team {
                team_id: fx.owning_team.clone(),
            },
            key: Some("code-review"),
            title: "Platform Code Review",
            overridable: true,
            body: "Two Platform reviewers required.",
            owner_employee_id: Some(fx.owner.clone()),
            expected_revision: None,
        },
    )
    .expect("owning team override");
    assert!(team_review.document.path.contains(&fx.root_team));
    assert!(team_review.document.path.contains(&fx.owning_team));

    write_doc(
        &fx.office,
        WriteDocArgs {
            caller: &fx.owner,
            command_id: next_uuid(),
            document_id: None,
            kind: DocumentKindWire::Standard,
            scope: ProcessScopeWire::Team {
                team_id: fx.contributing_team.clone(),
            },
            key: Some("design-tokens"),
            title: "Design Ops Tokens",
            overridable: false,
            body: "Only applies to Design Ops, NOT Checkout project.",
            owner_employee_id: Some(fx.owner.clone()),
            expected_revision: None,
        },
    )
    .expect("contributing team doc");

    let proj_review = write_doc(
        &fx.office,
        WriteDocArgs {
            caller: &fx.owner,
            command_id: next_uuid(),
            document_id: None,
            kind: DocumentKindWire::Process,
            scope: ProcessScopeWire::Project {
                project_id: fx.project_id.clone(),
            },
            key: Some("code-review"),
            title: "Checkout Code Review",
            overridable: true,
            body: "Checkout requires PCI review + bench check.",
            owner_employee_id: Some(fx.owner.clone()),
            expected_revision: None,
        },
    )
    .expect("project override");
    assert!(proj_review.document.path.starts_with("state/projects/"));

    (
        org_review.document.id,
        team_review.document.id,
        proj_review.document.id,
    )
}

fn verify_single_chain_engine(engine: EngineChoice) {
    let fx = seed_fixture(engine.clone());
    let (org_id, team_id, proj_id) = seed_chain_docs(&fx);

    // Same-scope duplicate key -> AlreadyExists.
    let dup = write_doc(
        &fx.office,
        WriteDocArgs {
            caller: &fx.owner,
            command_id: next_uuid(),
            document_id: None,
            kind: DocumentKindWire::Process,
            scope: ProcessScopeWire::Organization,
            key: Some("code-review"),
            title: "Dup",
            overridable: true,
            body: "Dup",
            owner_employee_id: Some(fx.owner.clone()),
            expected_revision: None,
        },
    )
    .expect_err("same-scope dup");
    assert_eq!(dup.code, ErrorCode::AlreadyExists);

    // Overriding non-overridable org standard at team scope -> sealed refusal.
    let sealed = write_doc(
        &fx.office,
        WriteDocArgs {
            caller: &fx.owner,
            command_id: next_uuid(),
            document_id: None,
            kind: DocumentKindWire::Standard,
            scope: ProcessScopeWire::Team {
                team_id: fx.owning_team.clone(),
            },
            key: Some("security-baseline"),
            title: "Relaxed",
            overridable: true,
            body: "Bypass",
            owner_employee_id: Some(fx.owner.clone()),
            expected_revision: None,
        },
    )
    .expect_err("sealed");
    assert_eq!(sealed.code, ErrorCode::InvalidRequest);
    assert!(sealed.message.contains("sealed"));

    let eff = fx
        .office
        .process_effective(
            &fx.owner,
            ProcessEffectiveParams {
                employee_id: None,
                r#for: Some(ProcessForWire::Project {
                    project_id: fx.project_id.clone(),
                }),
                kind: None,
            },
        )
        .unwrap()
        .unwrap();
    assert!(eff
        .entries
        .iter()
        .any(|e| e.document.key.as_deref() == Some("security-baseline") && e.mandatory));
    let review_entry = eff
        .entries
        .iter()
        .find(|e| e.document.key.as_deref() == Some("code-review"))
        .expect("code-review effective");
    assert_eq!(review_entry.document.id, proj_id);
    assert!(!eff
        .entries
        .iter()
        .any(|e| e.document.key.as_deref() == Some("design-tokens")));

    let why = fx
        .office
        .process_why(
            &fx.owner,
            ProcessWhyParams {
                key: "code-review".into(),
                employee_id: None,
                r#for: Some(ProcessForWire::Project {
                    project_id: fx.project_id.clone(),
                }),
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(why.chain.len(), 3);
    assert_eq!(why.chain[0].document.id, org_id);
    assert_eq!(why.chain[0].outcome, "replaced");
    assert_eq!(why.chain[1].document.id, team_id);
    assert_eq!(why.chain[1].outcome, "replaced");
    assert_eq!(why.chain[2].document.id, proj_id);
    assert_eq!(why.chain[2].outcome, "applies");

    let show = fx
        .office
        .process_show(
            &fx.owner,
            ProcessShowParams {
                key: None,
                document_id: Some(proj_id.clone()),
                employee_id: None,
                r#for: None,
                offset: Some(0),
                revision: None,
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        show.parent.as_ref().map(|p| p.id.as_str()),
        Some(team_id.as_str())
    );

    let view = fx
        .office
        .process_view(
            &fx.owner,
            ProcessViewParams {
                scope: ProcessScopeWire::Project {
                    project_id: fx.project_id.clone(),
                },
                kind: None,
            },
        )
        .unwrap()
        .unwrap();
    assert!(view.problems.is_empty());
    assert!(view.rows.iter().any(|r| r.document.id == proj_id));

    if engine.is_sqlite() {
        let owner = fx.owner.clone();
        let pid = fx.project_id.clone();
        let (_tmp, reopened) = reopen_rebuilt_sqlite(fx);
        let after = reopened
            .process_effective(
                &owner,
                ProcessEffectiveParams {
                    employee_id: None,
                    r#for: Some(ProcessForWire::Project { project_id: pid }),
                    kind: None,
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(eff.entries.len(), after.entries.len());
        reopened.close().expect("close reopened");
    }
}

#[test]
fn single_chain_inheritance_seals_contributing_exclusion_and_sqlite_rebuild() {
    verify_single_chain_engine(EngineChoice::Memory);
    verify_single_chain_engine(EngineChoice::Sqlite(org_office_sqlite::sqlite_backend()));
}

#[test]
fn move_team_relocates_structure_documents_and_survives_rebuild() {
    let fx = seed_fixture(EngineChoice::Sqlite(org_office_sqlite::sqlite_backend()));
    let doc = write_doc(
        &fx.office,
        WriteDocArgs {
            caller: &fx.owner,
            command_id: next_uuid(),
            document_id: None,
            kind: DocumentKindWire::Standard,
            scope: ProcessScopeWire::Team {
                team_id: fx.owning_team.clone(),
            },
            key: Some("platform-slo"),
            title: "Platform SLO v1",
            overridable: true,
            body: "99.9% uptime.",
            owner_employee_id: Some(fx.owner.clone()),
            expected_revision: None,
        },
    )
    .expect("create team standard");
    let old_path = fx.office.durable_root().join(&doc.document.path);

    // Move `owning_team` under `contributing_team`.
    fx.office
        .submit_org(
            &fx.owner,
            OrgCommand::TeamMove(TeamMoveParams {
                command_id: next_uuid(),
                team_id: fx.owning_team.clone(),
                owner_team_id: Some(fx.contributing_team.clone()),
                expected_version: 1,
            }),
        )
        .unwrap()
        .unwrap();

    // Update the relocated document to revision 2.
    let updated = write_doc(
        &fx.office,
        WriteDocArgs {
            caller: &fx.owner,
            command_id: next_uuid(),
            document_id: Some(doc.document.id.clone()),
            kind: DocumentKindWire::Standard,
            scope: ProcessScopeWire::Team {
                team_id: fx.owning_team.clone(),
            },
            key: Some("platform-slo"),
            title: "Platform SLO v2",
            overridable: true,
            body: "99.99% uptime.",
            owner_employee_id: Some(fx.owner.clone()),
            expected_revision: Some(1),
        },
    )
    .expect("update relocated standard");
    assert_eq!(updated.revision, 2);
    assert!(updated.document.path.contains(&fx.contributing_team));
    let new_path = fx.office.durable_root().join(&updated.document.path);

    let owner = fx.owner.clone();
    let doc_id = doc.document.id.clone();
    let (_tmp, reopened) = reopen_rebuilt_sqlite(fx);
    assert!(
        !old_path.exists(),
        "old durable file must be removed by MoveTeam: {}",
        old_path.display()
    );
    assert!(
        new_path.exists(),
        "new relocated durable file must exist: {}",
        new_path.display()
    );
    let shown = reopened
        .process_show(
            &owner,
            ProcessShowParams {
                key: None,
                document_id: Some(doc_id),
                employee_id: None,
                r#for: None,
                offset: None,
                revision: None,
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(shown.document.revision, 2);
    assert_eq!(shown.document.title, "Platform SLO v2");
    reopened.close().unwrap();
}

#[test]
fn projectless_task_resolves_accountable_team_and_archived_team_refuses_writes() {
    let fx = seed_fixture(EngineChoice::Sqlite(org_office_sqlite::sqlite_backend()));

    // Seed distinct standards on owning_team vs contributing_team.
    write_doc(
        &fx.office,
        WriteDocArgs {
            caller: &fx.owner,
            command_id: next_uuid(),
            document_id: None,
            kind: DocumentKindWire::Standard,
            scope: ProcessScopeWire::Team {
                team_id: fx.owning_team.clone(),
            },
            key: Some("platform-only"),
            title: "Platform Accountable Rule",
            overridable: false,
            body: "Follow Platform rules.",
            owner_employee_id: Some(fx.owner.clone()),
            expected_revision: None,
        },
    )
    .expect("owning team doc");

    write_doc(
        &fx.office,
        WriteDocArgs {
            caller: &fx.owner,
            command_id: next_uuid(),
            document_id: None,
            kind: DocumentKindWire::Standard,
            scope: ProcessScopeWire::Team {
                team_id: fx.contributing_team.clone(),
            },
            key: Some("design-only"),
            title: "Design Ops Rule",
            overridable: false,
            body: "Follow Design rules.",
            owner_employee_id: Some(fx.owner.clone()),
            expected_revision: None,
        },
    )
    .expect("contributing team doc");

    // Projectless task owned by `owning_team` but assigned to `contributing_team`.
    let task = fx
        .office
        .task_create(
            &fx.owner,
            TasksCreateParams {
                command_id: next_uuid(),
                task_id: next_uuid(),
                team_id: Some(fx.owning_team.clone()),
                project_id: None,
                assigned_team_id: Some(fx.contributing_team.clone()),
                assignee_id: Some(fx.contributing_agent.clone()),
                task_type: Some("task".into()),
                work_type: None,
                title: "Cross-team projectless task".into(),
                body_markdown: "Verify accountable team chain.".into(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: Some(1),
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
        .unwrap()
        .task;

    let rendered = fx
        .office
        .render_agent_process_instructions(
            &fx.contributing_agent,
            None,
            Some(&task.id),
            BTreeMap::new(),
            128 * 1024,
        )
        .unwrap()
        .unwrap();
    assert!(
        rendered.text.contains("Platform Accountable Rule"),
        "projectless task must resolve accountable team_id: {}",
        rendered.text
    );
    assert!(
        !rendered.text.contains("Design Ops Rule"),
        "assigned_team_id must not replace accountable team_id: {}",
        rendered.text
    );

    // The turn naming the task carries only what the task's chain changes about the agent's
    // standing (home team) set: the accountable team's rule applies, the home team's does not.
    let delta = fx
        .office
        .render_agent_task_process_delta(&fx.contributing_agent, &[task.id.as_str()], 128 * 1024)
        .unwrap()
        .unwrap()
        .expect("the task's chain differs from the home team chain");
    assert!(
        delta.contains("Platform Accountable Rule") && delta.contains("Follow Platform rules."),
        "{delta}"
    );
    assert!(
        delta.contains("do not apply to this task:\n- Design Ops Rule"),
        "{delta}"
    );
    assert!(!delta.contains("Follow Design rules."), "{delta}");
    // A task that no longer exists has no chain to apply (its wake is stale); it never hides
    // the delta of a task named beside it.
    let unknown = next_uuid();
    assert_eq!(
        fx.office
            .render_agent_task_process_delta(
                &fx.contributing_agent,
                &[unknown.as_str()],
                128 * 1024
            )
            .unwrap()
            .unwrap(),
        None
    );
    let beside = fx
        .office
        .render_agent_task_process_delta(
            &fx.contributing_agent,
            &[unknown.as_str(), task.id.as_str()],
            128 * 1024,
        )
        .unwrap()
        .unwrap()
        .expect("the existing task's delta survives an unknown one");
    assert_eq!(beside, delta);
    let gone = fx
        .office
        .render_agent_task_process_delta(&next_uuid(), &[task.id.as_str()], 128 * 1024)
        .unwrap()
        .expect_err("a missing employee fails closed");
    assert_eq!(gone.code, ErrorCode::NotFound);

    // Missing employee fails closed with NotFound rather than silently returning empty instructions.
    let missing = fx
        .office
        .render_agent_process_instructions(&next_uuid(), None, None, BTreeMap::new(), 128 * 1024)
        .unwrap()
        .expect_err("missing employee must fail closed");
    assert_eq!(missing.code, ErrorCode::NotFound);

    // Archived team refuses writes.
    let empty_team = next_uuid();
    fx.office
        .submit_org(
            &fx.owner,
            OrgCommand::TeamCreate(TeamCreateParams {
                command_id: next_uuid(),
                team_id: empty_team.clone(),
                name: "Temp Team".into(),
                owner_team_id: Some(fx.root_team.clone()),
            }),
        )
        .unwrap()
        .unwrap();
    fx.office
        .submit_org(
            &fx.owner,
            OrgCommand::TeamDelete(TeamDeleteParams {
                command_id: next_uuid(),
                team_id: empty_team.clone(),
                expected_version: 1,
            }),
        )
        .unwrap()
        .unwrap();
    let archived_err = write_doc(
        &fx.office,
        WriteDocArgs {
            caller: &fx.owner,
            command_id: next_uuid(),
            document_id: None,
            kind: DocumentKindWire::Process,
            scope: ProcessScopeWire::Team {
                team_id: empty_team,
            },
            key: Some("archived-proc"),
            title: "Should Fail",
            overridable: true,
            body: "No writes to archived teams.",
            owner_employee_id: Some(fx.owner.clone()),
            expected_revision: None,
        },
    )
    .expect_err("archived team must refuse writes");
    assert_eq!(archived_err.code, ErrorCode::InvalidRequest);
}

#[test]
fn agent_write_boundaries_idempotency_concurrency_and_skills() {
    let fx = seed_fixture(EngineChoice::Sqlite(org_office_sqlite::sqlite_backend()));

    write_doc(
        &fx.office,
        WriteDocArgs {
            caller: &fx.owner,
            command_id: next_uuid(),
            document_id: None,
            kind: DocumentKindWire::Process,
            scope: ProcessScopeWire::Organization,
            key: Some("release-checklist"),
            title: "Org Release Checklist",
            overridable: true,
            body: "Verify changelog.",
            owner_employee_id: Some(fx.owner.clone()),
            expected_revision: None,
        },
    )
    .expect("org checklist");

    // Person creates a non-overridable project standard; agent must NOT be able to unseal it.
    let locked_proj = write_doc(
        &fx.office,
        WriteDocArgs {
            caller: &fx.owner,
            command_id: next_uuid(),
            document_id: None,
            kind: DocumentKindWire::Standard,
            scope: ProcessScopeWire::Project {
                project_id: fx.project_id.clone(),
            },
            key: Some("pci-compliance"),
            title: "PCI Compliance",
            overridable: false,
            body: "Never log card numbers.",
            owner_employee_id: Some(fx.owner.clone()),
            expected_revision: None,
        },
    )
    .expect("locked project standard");

    let unseal_denied = write_doc(
        &fx.office,
        WriteDocArgs {
            caller: &fx.owning_agent,
            command_id: next_uuid(),
            document_id: Some(locked_proj.document.id),
            kind: DocumentKindWire::Standard,
            scope: ProcessScopeWire::Project {
                project_id: fx.project_id.clone(),
            },
            key: Some("pci-compliance"),
            title: "PCI Unlocked",
            overridable: true,
            body: "Trying to unseal.",
            owner_employee_id: Some(fx.owning_agent.clone()),
            expected_revision: Some(1),
        },
    )
    .expect_err("agent cannot unseal non-overridable project standard");
    assert_eq!(unseal_denied.code, ErrorCode::NotAuthorized);

    // Org scope, team scope, non-overridable, inherited key, and contributing-team agent refused.
    for (caller, scope, key, ovr) in [
        (&fx.owning_agent, ProcessScopeWire::Organization, "a1", true),
        (
            &fx.owning_agent,
            ProcessScopeWire::Team {
                team_id: fx.owning_team.clone(),
            },
            "a2",
            true,
        ),
        (
            &fx.owning_agent,
            ProcessScopeWire::Project {
                project_id: fx.project_id.clone(),
            },
            "a3",
            false,
        ),
        (
            &fx.owning_agent,
            ProcessScopeWire::Project {
                project_id: fx.project_id.clone(),
            },
            "release-checklist",
            true,
        ),
        (
            &fx.contributing_agent,
            ProcessScopeWire::Project {
                project_id: fx.project_id.clone(),
            },
            "a5",
            true,
        ),
    ] {
        let denied = write_doc(
            &fx.office,
            WriteDocArgs {
                caller,
                command_id: next_uuid(),
                document_id: None,
                kind: DocumentKindWire::Process,
                scope,
                key: Some(key),
                title: "Denied",
                overridable: ovr,
                body: "Denied",
                owner_employee_id: Some(caller.clone()),
                expected_revision: None,
            },
        )
        .expect_err("agent boundary must refuse");
        assert_eq!(denied.code, ErrorCode::NotAuthorized);
    }

    // Owning-team agent creates overridable project doc; idempotent replay & OperationReused.
    let cmd_id = next_uuid();
    let created = write_doc(
        &fx.office,
        WriteDocArgs {
            caller: &fx.owning_agent,
            command_id: cmd_id.clone(),
            document_id: None,
            kind: DocumentKindWire::Process,
            scope: ProcessScopeWire::Project {
                project_id: fx.project_id.clone(),
            },
            key: Some("local-dev-setup"),
            title: "Local Dev Setup",
            overridable: true,
            body: "Run `cargo test`.",
            owner_employee_id: None,
            expected_revision: None,
        },
    )
    .expect("owning agent project write allowed");
    assert_eq!(created.revision, 1);

    let replayed = write_doc(
        &fx.office,
        WriteDocArgs {
            caller: &fx.owning_agent,
            command_id: cmd_id.clone(),
            document_id: None,
            kind: DocumentKindWire::Process,
            scope: ProcessScopeWire::Project {
                project_id: fx.project_id.clone(),
            },
            key: Some("local-dev-setup"),
            title: "Local Dev Setup",
            overridable: true,
            body: "Run `cargo test`.",
            owner_employee_id: None,
            expected_revision: None,
        },
    )
    .expect("idempotent replay");
    assert_eq!(replayed.document.id, created.document.id);
    assert_eq!(replayed.revision, 1);

    let reused = write_doc(
        &fx.office,
        WriteDocArgs {
            caller: &fx.owning_agent,
            command_id: cmd_id,
            document_id: None,
            kind: DocumentKindWire::Process,
            scope: ProcessScopeWire::Project {
                project_id: fx.project_id.clone(),
            },
            key: Some("local-dev-setup"),
            title: "Different Title",
            overridable: true,
            body: "Changed.",
            owner_employee_id: None,
            expected_revision: None,
        },
    )
    .expect_err("reused command_id");
    assert_eq!(reused.code, ErrorCode::OperationReused);

    // Stale expected_revision -> Conflict; matching revision from child-team agent succeeds.
    let conflict = write_doc(
        &fx.office,
        WriteDocArgs {
            caller: &fx.child_agent,
            command_id: next_uuid(),
            document_id: Some(created.document.id.clone()),
            kind: DocumentKindWire::Process,
            scope: ProcessScopeWire::Project {
                project_id: fx.project_id.clone(),
            },
            key: Some("local-dev-setup"),
            title: "v2",
            overridable: true,
            body: "v2",
            owner_employee_id: Some(fx.child_agent.clone()),
            expected_revision: Some(99),
        },
    )
    .expect_err("stale revision");
    assert_eq!(conflict.code, ErrorCode::Conflict);

    let updated = write_doc(
        &fx.office,
        WriteDocArgs {
            caller: &fx.child_agent,
            command_id: next_uuid(),
            document_id: Some(created.document.id),
            kind: DocumentKindWire::Process,
            scope: ProcessScopeWire::Project {
                project_id: fx.project_id.clone(),
            },
            key: Some("local-dev-setup"),
            title: "Local Dev Setup v2",
            overridable: true,
            body: "Updated by child-team agent with UTF-8 café.",
            owner_employee_id: Some(fx.child_agent.clone()),
            expected_revision: Some(1),
        },
    )
    .expect("child-team agent update");
    assert_eq!(updated.revision, 2);

    // Built-in skills.
    let skills = fx.office.skill_list();
    assert_eq!(
        skills.skills.len(),
        org_process::list_builtin_skills().len()
    );
    assert!(fx
        .office
        .skill_read("agent-os-basics")
        .unwrap()
        .body_markdown
        .contains("agent-os"));
    assert_eq!(
        fx.office.skill_read("nonexistent-skill").unwrap_err().code,
        ErrorCode::NotFound
    );
}

#[test]
fn standing_and_task_must_follow_text_share_one_instruction_budget() {
    let fx = seed_fixture(EngineChoice::Sqlite(org_office_sqlite::sqlite_backend()));
    let home_body = "h".repeat(70 * 1024);
    let task_body = "t".repeat(70 * 1024);
    for (team, key, title, body) in [
        (
            &fx.contributing_team,
            "home-heavy",
            "Home Heavy Rule",
            &home_body,
        ),
        (&fx.owning_team, "task-heavy", "Task Heavy Rule", &task_body),
    ] {
        write_doc(
            &fx.office,
            WriteDocArgs {
                caller: &fx.owner,
                command_id: next_uuid(),
                document_id: None,
                kind: DocumentKindWire::Standard,
                scope: ProcessScopeWire::Team {
                    team_id: team.clone(),
                },
                key: Some(key),
                title,
                overridable: false,
                body,
                owner_employee_id: Some(fx.owner.clone()),
                expected_revision: None,
            },
        )
        .expect("heavy must-follow standard");
    }
    let task = fx
        .office
        .task_create(
            &fx.owner,
            TasksCreateParams {
                command_id: next_uuid(),
                task_id: next_uuid(),
                team_id: Some(fx.owning_team.clone()),
                project_id: None,
                assigned_team_id: Some(fx.contributing_team.clone()),
                assignee_id: Some(fx.contributing_agent.clone()),
                task_type: Some("task".into()),
                work_type: None,
                title: "Cross-team heavy task".into(),
                body_markdown: "Both chains carry 70 KiB of must-follow text.".into(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: Some(1),
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
        .unwrap()
        .task;

    // Each chain alone fits 128 KiB, so the launch and a launch for the task both render.
    fx.office
        .render_agent_process_instructions(
            &fx.contributing_agent,
            None,
            None,
            BTreeMap::new(),
            128 * 1024,
        )
        .unwrap()
        .expect("the standing set alone fits");
    fx.office
        .render_agent_process_instructions(
            &fx.contributing_agent,
            None,
            Some(&task.id),
            BTreeMap::new(),
            128 * 1024,
        )
        .unwrap()
        .expect("the task's set alone fits");

    // The turn would hold the standing 70 KiB and the task's 70 KiB: refused, naming both.
    let refused = fx
        .office
        .render_agent_task_process_delta(&fx.contributing_agent, &[task.id.as_str()], 128 * 1024)
        .unwrap()
        .expect_err("standing + delta must-follow text over budget fails closed");
    assert_eq!(refused.code, ErrorCode::InvalidRequest);
    assert!(
        refused.message.contains("exceed instruction budget"),
        "{}",
        refused.message
    );
    assert!(
        refused.message.contains("Home Heavy Rule") && refused.message.contains("Task Heavy Rule"),
        "{}",
        refused.message
    );
    fx.office.close().unwrap();
}

#[test]
fn a_live_task_whose_project_cannot_be_loaded_refuses_the_turn() {
    let fx = seed_fixture(EngineChoice::Sqlite(org_office_sqlite::sqlite_backend()));
    write_doc(
        &fx.office,
        WriteDocArgs {
            caller: &fx.owner,
            command_id: next_uuid(),
            document_id: None,
            kind: DocumentKindWire::Standard,
            scope: ProcessScopeWire::Project {
                project_id: fx.project_id.clone(),
            },
            key: Some("checkout-only"),
            title: "Checkout Release Rule",
            overridable: false,
            body: "Tag every release.",
            owner_employee_id: Some(fx.owner.clone()),
            expected_revision: None,
        },
    )
    .expect("project must-follow standard");
    let task = fx
        .office
        .task_create(
            &fx.owner,
            TasksCreateParams {
                command_id: next_uuid(),
                task_id: next_uuid(),
                team_id: Some(fx.owning_team.clone()),
                project_id: Some(fx.project_id.clone()),
                assigned_team_id: Some(fx.contributing_team.clone()),
                assignee_id: Some(fx.contributing_agent.clone()),
                task_type: Some("task".into()),
                work_type: None,
                title: "Checkout task".into(),
                body_markdown: "Resolves against the project's chain.".into(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: Some(1),
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
        .unwrap()
        .task;
    let delta = fx
        .office
        .render_agent_task_process_delta(&fx.contributing_agent, &[task.id.as_str()], 128 * 1024)
        .unwrap()
        .unwrap()
        .expect("the project's chain differs from the home team chain");
    assert!(delta.contains("Tag every release."), "{delta}");

    // The task still resolves but its project no longer loads (the projection lost its row;
    // a rebuild refuses that state, so only drift reaches it): the turn is refused rather than
    // sent without the project's must-follow rules. A task that did not resolve at all would
    // be skipped as stale instead.
    let db = fx
        .roots
        .runtime
        .join(identity().office_id.as_str())
        .join("office.db");
    let removed = org_office_sqlite::delete_project_row_for_drift_test(&db, fx.project_id.as_str());
    assert_eq!(removed, 1);
    let refused = fx
        .office
        .render_agent_task_process_delta(&fx.contributing_agent, &[task.id.as_str()], 128 * 1024)
        .unwrap()
        .expect_err("a live task's missing project fails closed");
    assert_eq!(refused.code, ErrorCode::NotFound);
    assert!(
        refused
            .message
            .contains(&format!("task project `{}` not found", fx.project_id)),
        "{}",
        refused.message
    );
    // A task that no longer exists is still only stale.
    assert_eq!(
        fx.office
            .render_agent_task_process_delta(
                &fx.contributing_agent,
                &[next_uuid().as_str()],
                128 * 1024
            )
            .unwrap()
            .unwrap(),
        None
    );
    fx.office.close().unwrap();
}
