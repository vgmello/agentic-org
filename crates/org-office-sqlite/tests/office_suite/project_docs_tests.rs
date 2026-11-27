//! Office-level tests of the project documentation tree (`TASK-55`, `L11`, `FC-180`), each on
//! both engines: the full lifecycle (`create` -> `inspect` -> `list` -> `index` -> `update` ->
//! `move` -> `put_media`) with `index.yaml` regenerated in every write and reconstructed
//! identically by a delete-and-rebuild; optimistic concurrency, exact-retry idempotency and the
//! path, kind and section refusals; and the project-membership boundary on every `docs.*` call.

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use base64::Engine as _;
use org_proto::docs::{
    DocMediaKindWire, DocStatusWire, DocsCreateParams, DocsDeleteParams, DocsIndexParams,
    DocsIndexResult, DocsInspectParams, DocsListParams, DocsMoveParams, DocsPutMediaParams,
    DocsUpdateParams, DocsWriteResult,
};
use org_proto::org::{MemberAddPersonParams, ProjectCreateParams, TeamCreateParams};
use org_proto::{ApiError, ErrorCode};
use org_store::testkit::{identity, FakeCustody, FixedClock};

use org_media::testkit::{png, png_with_text, GPS_MARKER};
use org_office::organization_family::testkit::CREATION_AT;
use org_office::rules::OrgCommand;
use org_office::testkit_support::office_harness::{places, record, request, CA_SEED};
use org_office::testkit_support::store_open::EngineChoice;
use org_office::testkit_support::Options;
use org_office::OfficeError;
use org_office::{Custody, DataRoots, Office};

static NEXT_ID: AtomicU64 = AtomicU64::new(120_000);

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

struct Fx {
    _tmp: tempfile::TempDir,
    folder: std::path::PathBuf,
    roots: DataRoots,
    custody: Custody,
    engine: EngineChoice,
    office: Office,
    owner: String,
    person: String,
    project: String,
    other_project: String,
    owning_agent: String,
    contributing_agent: String,
    outsider_agent: String,
    child_agent: String,
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

fn agent(office: &Office, owner: &str, team_id: &str, name: &str, handle: &str) -> String {
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
            reason: "seed a documentation test agent".into(),
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
    let owning = team(&office, &owner, "Platform", &root);
    let child = team(&office, &owner, "Platform Core", &owning);
    let contributing = team(&office, &owner, "Design Ops", &root);
    let outsider = team(&office, &owner, "Finance", &root);
    let project_id = project(
        &office,
        &owner,
        "Checkout",
        "checkout",
        &owning,
        &[&contributing],
    );
    let other_project = project(&office, &owner, "Billing", "billing", &outsider, &[]);
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
    Fx {
        owning_agent: agent(&office, &owner, &owning, "Owner Bot", "owner-bot"),
        contributing_agent: agent(&office, &owner, &contributing, "Design Bot", "design-bot"),
        outsider_agent: agent(&office, &owner, &outsider, "Finance Bot", "finance-bot"),
        child_agent: agent(&office, &owner, &child, "Core Bot", "core-bot"),
        _tmp: tmp,
        folder,
        roots,
        custody,
        engine: engine.clone(),
        office,
        owner,
        person,
        project: project_id,
        other_project,
    }
}

/// Closes the office, deletes its projection (SQLite) and opens it again: the memory engine
/// always starts empty, so either way the projection is rebuilt from the files.
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
        project,
        other_project,
        owning_agent,
        contributing_agent,
        outsider_agent,
        child_agent,
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
        project,
        other_project,
        owning_agent,
        contributing_agent,
        outsider_agent,
        child_agent,
    })
}

fn ok<T>(answer: Result<Result<T, ApiError>, OfficeError>) -> T {
    answer
        .expect("storage")
        .unwrap_or_else(|e| panic!("refused: {e:?}"))
}

fn err<T: std::fmt::Debug>(answer: Result<Result<T, ApiError>, OfficeError>) -> ApiError {
    answer.expect("storage").expect_err("must be refused")
}

fn create_params(project: &str, title: &str, path: Option<&str>) -> DocsCreateParams {
    DocsCreateParams {
        command_id: next_uuid(),
        project: project.into(),
        kind: "guide".into(),
        title: title.into(),
        path: path.map(str::to_owned),
        body_markdown: format!("# {title}\n"),
        ..Default::default()
    }
}

fn create(fx: &Fx, caller: &str, title: &str, path: Option<&str>) -> DocsWriteResult {
    ok(fx
        .office
        .docs_create(caller, create_params(&fx.project, title, path)))
}

fn index(fx: &Fx, caller: &str) -> DocsIndexResult {
    ok(fx.office.docs_index(
        caller,
        DocsIndexParams {
            project: fx.project.clone(),
        },
    ))
}

fn index_file(fx: &Fx) -> std::path::PathBuf {
    fx.roots
        .durable
        .join(format!("state/projects/{}/docs/index.yaml", fx.project))
}

fn docs_rows(fx: &Fx) -> Vec<String> {
    let marker = format!("{}/docs", fx.project);
    fx.office
        .dump()
        .into_iter()
        .filter(|line| {
            line.starts_with("project_docs ")
                || line.starts_with("documents ")
                || line.starts_with("documentation_media ")
                || line.contains(&marker)
        })
        .collect()
}

fn media_params(project: &str, file: &str, bytes: &[u8]) -> DocsPutMediaParams {
    DocsPutMediaParams {
        command_id: next_uuid(),
        project: project.into(),
        file_name: file.into(),
        media_type: "image/png".into(),
        kind: DocMediaKindWire::Diagram,
        title: "Architecture".into(),
        summary: Some("The checkout flow.".into()),
        content_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        expected_revision: None,
    }
}

#[test]
fn full_lifecycle_regenerates_the_index_and_rebuilds_identically_on_both_engines() {
    for engine in engines() {
        let fx = fixture(engine.clone());
        let mut setup = create_params(&fx.project, "Setup", Some("guides/setup.md"));
        setup.status = Some(DocStatusWire::Current);
        setup.summary = Some("How to set up checkout.".into());
        let setup = ok(fx.office.docs_create(&fx.owner, setup)).document;
        assert_eq!((setup.revision, setup.section.as_str()), (1, "guides"));
        assert_eq!(setup.owner_employee_id.as_deref(), Some(fx.owner.as_str()));
        let mut runbook = create_params(&fx.project, "Deploy Runbook!", None);
        runbook.kind = "runbook".into();
        runbook.section = Some("runbooks".into());
        let runbook = ok(fx.office.docs_create(&fx.contributing_agent, runbook)).document;
        assert_eq!(runbook.path, "runbooks/deploy-runbook.md");

        for reference in [setup.id.as_str(), "guides/setup.md"] {
            let inspected = ok(fx.office.docs_inspect(
                &fx.owning_agent,
                DocsInspectParams {
                    project: "P-1".into(),
                    document: reference.into(),
                },
            ));
            assert_eq!(inspected.document, setup, "{engine:?}");
            assert_eq!(inspected.body_markdown, "# Setup\n");
        }

        let list = |params: DocsListParams| ok(fx.office.docs_list(&fx.owner, params));
        let all = list(DocsListParams {
            project: "checkout".into(),
            ..Default::default()
        });
        let paths: Vec<_> = all.documents.iter().map(|d| d.path.as_str()).collect();
        assert_eq!(paths, ["guides/setup.md", "runbooks/deploy-runbook.md"]);
        let project = fx.project.clone();
        let filtered =
            |section: Option<&str>, kind: Option<&str>, status: Option<DocStatusWire>| {
                list(DocsListParams {
                    project: project.clone(),
                    section: section.map(str::to_owned),
                    kind: kind.map(str::to_owned),
                    status,
                    ..Default::default()
                })
                .documents
                .len()
            };
        assert_eq!(filtered(Some("guides"), None, None), 1);
        assert_eq!(filtered(None, Some("runbook"), None), 1);
        assert_eq!(filtered(None, None, Some(DocStatusWire::Draft)), 1);
        assert_eq!(filtered(Some("missing"), None, None), 0);
        let first = list(DocsListParams {
            project: project.clone(),
            limit: Some(1),
            ..Default::default()
        });
        assert_eq!(first.next_cursor.as_deref(), Some("guides/setup.md"));
        let second = list(DocsListParams {
            project: project.clone(),
            limit: Some(1),
            cursor: first.next_cursor,
            ..Default::default()
        });
        assert_eq!(second.documents[0].path, "runbooks/deploy-runbook.md");
        assert_eq!(second.next_cursor, None);

        let tree = index(&fx, &fx.contributing_agent);
        assert_eq!(tree.sections.len(), 2);
        assert_eq!(tree.documents.len(), 2);

        assert_update_move_media_and_rebuild(fx, &setup.id, engine.clone());
    }
}

fn assert_update_move_media_and_rebuild(fx: Fx, setup_id: &str, engine: EngineChoice) {
    let updated = ok(fx.office.docs_update(
        &fx.owning_agent,
        DocsUpdateParams {
            command_id: next_uuid(),
            project: fx.project.clone(),
            document_id: setup_id.to_owned(),
            expected_revision: 1,
            title: Some("Install".into()),
            body_markdown: Some("# Install\nRun it.\n".into()),
            ..Default::default()
        },
    ))
    .document;
    assert_eq!((updated.revision, updated.title.as_str()), (2, "Install"));
    assert_eq!(
        updated.updated_by_id.as_deref(),
        Some(fx.owning_agent.as_str())
    );

    let moved = ok(fx.office.docs_move(
        &fx.contributing_agent,
        DocsMoveParams {
            command_id: next_uuid(),
            project: fx.project.clone(),
            document_id: setup_id.to_owned(),
            expected_revision: 2,
            new_section: Some("guides/install".into()),
            ..Default::default()
        },
    ))
    .document;
    assert_eq!(
        (moved.revision, moved.path.as_str()),
        (3, "guides/install/setup.md")
    );
    let gone = err(fx.office.docs_inspect(
        &fx.owner,
        DocsInspectParams {
            project: fx.project.clone(),
            document: "guides/setup.md".into(),
        },
    ));
    assert_eq!(gone.code, ErrorCode::NotFound);

    let media = ok(fx
        .office
        .docs_put_media(&fx.owner, media_params(&fx.project, "arch.png", &png(8, 4))))
    .media;
    assert_eq!(
        (media.path.as_str(), media.revision, media.sha256.len()),
        ("media/arch.png", 1, 64)
    );

    fx.office.sync_store().unwrap();
    let docs_dir = fx
        .roots
        .durable
        .join(format!("state/projects/{}/docs", fx.project));
    assert!(
        !docs_dir.join("guides/setup.md").exists(),
        "the move removed the old file"
    );
    let moved_file = std::fs::read_to_string(docs_dir.join("guides/install/setup.md")).unwrap();
    assert!(moved_file.starts_with("---\n") && moved_file.contains("# Install"));
    assert!(
        docs_dir.join("media/arch.png").exists() && docs_dir.join("media/arch.png.yaml").exists()
    );
    let stored_index = std::fs::read(index_file(&fx)).unwrap();
    let text = String::from_utf8(stored_index.clone()).unwrap();
    assert!(
        text.contains("\"path\":\"guides/install/setup.md\"") && text.contains("\"revision\":3")
    );
    assert!(!text.contains("\"path\":\"guides/setup.md\""));
    assert!(text.contains("\"path\":\"media/arch.png\""));

    let tree_before = index(&fx, &fx.owner);
    let rows_before = docs_rows(&fx);
    assert!(rows_before.len() >= 3, "{rows_before:?}");
    let fx = reopen(fx).expect("the tree rebuilds");
    assert_eq!(
        index(&fx, &fx.owner),
        tree_before,
        "{engine:?}: the rebuilt tree"
    );
    assert_eq!(docs_rows(&fx), rows_before, "{engine:?}: the rebuilt rows");
    assert_eq!(std::fs::read(index_file(&fx)).unwrap(), stored_index);
    fx.office.close().unwrap();
}

#[test]
fn a_hand_edited_index_fails_the_rebuild_on_both_engines() {
    for engine in engines() {
        let fx = fixture(engine.clone());
        create(&fx, &fx.owner, "Setup", Some("setup.md"));
        fx.office.sync_store().unwrap();
        let tampered = std::fs::read_to_string(index_file(&fx))
            .unwrap()
            .replace("Setup", "Edited");
        std::fs::write(index_file(&fx), tampered).unwrap();
        let error = reopen(fx).err().expect("a stale index refuses the rebuild");
        let detail = format!("{error:?}");
        assert!(detail.contains("index.yaml"), "{engine:?}: {detail}");
    }
}

#[test]
fn concurrency_idempotency_and_path_kind_section_refusals_on_both_engines() {
    for engine in engines() {
        let fx = fixture(engine.clone());
        let first = create_params(&fx.project, "Setup", Some("guides/setup.md"));
        let created = ok(fx.office.docs_create(&fx.owner, first.clone()));
        // An exact retry is answered from the receipt; the same id with other params is reused.
        assert_eq!(ok(fx.office.docs_create(&fx.owner, first.clone())), created);
        let mut reused = first.clone();
        reused.title = "Other".into();
        assert_eq!(
            err(fx.office.docs_create(&fx.owner, reused)).code,
            ErrorCode::OperationReused
        );
        assert_eq!(index(&fx, &fx.owner).documents.len(), 1, "{engine:?}");

        let refused = |params: DocsCreateParams| err(fx.office.docs_create(&fx.owner, params)).code;
        for bad in [
            "../escape.md",
            "/abs.md",
            "index.yaml",
            "a\0b.md",
            "media/x.md",
            "a//b.md",
            "Up.md",
            "a\\b.md",
        ] {
            let code = refused(create_params(&fx.project, "Bad", Some(bad)));
            assert_eq!(code, ErrorCode::InvalidRequest, "path {bad:?}");
        }
        assert_eq!(
            refused(create_params(&fx.project, "Dup", Some("guides/setup.md"))),
            ErrorCode::AlreadyExists
        );
        for kind in ["Bad Kind", "brief", "changelog", ""] {
            let mut params = create_params(&fx.project, "Kinded", None);
            params.kind = kind.into();
            assert_eq!(refused(params), ErrorCode::InvalidRequest, "kind {kind:?}");
        }
        for section in ["../x", "/x", "media", "a.b", "x/"] {
            let mut params = create_params(&fx.project, "Sectioned", None);
            params.section = Some(section.into());
            assert_eq!(
                refused(params),
                ErrorCode::InvalidRequest,
                "section {section:?}"
            );
        }
        let mut mismatched = create_params(&fx.project, "Mismatch", Some("guides/x.md"));
        mismatched.section = Some("runbooks".into());
        assert_eq!(refused(mismatched), ErrorCode::InvalidRequest);
        let mut huge = create_params(&fx.project, "Huge", None);
        huge.body_markdown = "x".repeat(256 * 1024 + 1);
        assert_eq!(refused(huge), ErrorCode::TooLarge);
        let mut bad_command = create_params(&fx.project, "Bad command", None);
        bad_command.command_id = "not-a-uuid".into();
        assert_eq!(refused(bad_command), ErrorCode::InvalidRequest);

        let update = |expected: u64, title: &str| {
            fx.office.docs_update(
                &fx.owner,
                DocsUpdateParams {
                    command_id: next_uuid(),
                    project: fx.project.clone(),
                    document_id: created.document.id.clone(),
                    expected_revision: expected,
                    title: Some(title.into()),
                    ..Default::default()
                },
            )
        };
        assert_eq!(err(update(7, "Stale")).code, ErrorCode::Conflict);
        assert_eq!(ok(update(1, "Fresh")).document.revision, 2);
        assert_eq!(err(update(1, "Lost update")).code, ErrorCode::Conflict);
        assert_eq!(
            err(update(2, "Fresh")).code,
            ErrorCode::InvalidRequest,
            "nothing to change"
        );

        let other = create(&fx, &fx.owner, "Other", Some("other.md")).document;
        let mv = |path: &str, expected: u64| {
            fx.office.docs_move(
                &fx.owner,
                DocsMoveParams {
                    command_id: next_uuid(),
                    project: fx.project.clone(),
                    document_id: other.id.clone(),
                    expected_revision: expected,
                    new_path: Some(path.into()),
                    ..Default::default()
                },
            )
        };
        assert_eq!(err(mv("guides/setup.md", 1)).code, ErrorCode::AlreadyExists);
        assert_eq!(err(mv("../escape.md", 1)).code, ErrorCode::InvalidRequest);
        assert_eq!(err(mv("index.yaml", 1)).code, ErrorCode::InvalidRequest);
        assert_eq!(err(mv("other.md", 1)).code, ErrorCode::InvalidRequest);
        assert_eq!(err(mv("moved.md", 3)).code, ErrorCode::Conflict);
        assert_eq!(
            ok(mv("archive/other.md", 1)).document.path,
            "archive/other.md"
        );
        fx.office.close().unwrap();
    }
}

#[test]
fn agents_outside_the_project_are_refused_on_every_docs_call_without_a_leak() {
    for engine in engines() {
        let fx = fixture(engine.clone());
        let doc = create(&fx, &fx.owner, "Secret plan", Some("plans/secret.md")).document;
        ok(fx
            .office
            .docs_put_media(&fx.owner, media_params(&fx.project, "arch.png", &png(4, 4))));
        let missing = next_uuid();
        for project in [
            fx.project.as_str(),
            "P-1",
            "checkout",
            missing.as_str(),
            "nope",
        ] {
            let refusals = every_call(&fx, &fx.outsider_agent, project, &doc.id);
            for (call, error) in refusals {
                assert_eq!(
                    (error.code, error.message.as_str()),
                    (ErrorCode::NotFound, "project not found"),
                    "{engine:?} {call} as an outsider on {project}"
                );
            }
        }
        // A member asking for a document of another project through its own is not told.
        let foreign = ok(fx.office.docs_create(
            &fx.owner,
            create_params(&fx.other_project, "Billing", Some("billing.md")),
        ));
        let leak = err(fx.office.docs_inspect(
            &fx.owning_agent,
            DocsInspectParams {
                project: fx.project.clone(),
                document: foreign.document.id,
            },
        ));
        assert_eq!(leak.code, ErrorCode::NotFound);
        assert_eq!(leak.message, "document not found");

        // The accountable and the contributing team's agents, an agent of a team below the
        // accountable one, the owner and any person may.
        for member in [
            &fx.owning_agent,
            &fx.contributing_agent,
            &fx.child_agent,
            &fx.owner,
            &fx.person,
        ] {
            let tree = index(&fx, member);
            assert!(tree.documents.iter().any(|d| d.id == doc.id), "{engine:?}");
            let mut params = create_params(&fx.project, "By member", None);
            params.section = Some(format!("by-{}", &member[member.len() - 4..]));
            ok(fx.office.docs_create(member, params));
        }
        // An agent never reads another project's tree, even one with no documents.
        let other = err(fx.office.docs_index(
            &fx.owning_agent,
            DocsIndexParams {
                project: fx.other_project.clone(),
            },
        ));
        assert_eq!(other.code, ErrorCode::NotFound);

        offboard(&fx, &fx.contributing_agent);
        for (call, error) in every_call(&fx, &fx.contributing_agent, &fx.project, &doc.id) {
            assert_eq!(
                error.code,
                ErrorCode::NotAuthorized,
                "{engine:?} {call} when offboarded"
            );
        }
        let stranger = next_uuid();
        for (call, error) in every_call(&fx, &stranger, &fx.project, &doc.id) {
            assert_eq!(
                error.code,
                ErrorCode::NotAuthorized,
                "{call} by a non-employee"
            );
        }
        fx.office.close().unwrap();
    }
}

fn offboard(fx: &Fx, agent: &str) {
    use org_proto::hiring::{AgentOffboard, AgentOffboardParams};
    use org_proto::{Outcome, Request};
    let version = fx
        .office
        .org_snapshot()
        .unwrap()
        .members
        .into_iter()
        .find(|m| m.id == agent)
        .unwrap()
        .version;
    let req = Request::new::<AgentOffboard>(
        1,
        &AgentOffboardParams {
            command_id: next_uuid(),
            member_id: agent.into(),
            expected_version: version,
        },
    )
    .unwrap();
    let resp = fx
        .office
        .dispatch_hiring_rpc(Some(&fx.owner), &req)
        .unwrap();
    assert!(matches!(resp.outcome, Outcome::Result(_)), "{resp:?}");
}

/// Every `docs.*` call as `caller` on `project`, each expected to be refused.
fn every_call(fx: &Fx, caller: &str, project: &str, doc: &str) -> Vec<(&'static str, ApiError)> {
    let mut calls = every_read(fx, caller, project, doc);
    calls.extend(every_write(fx, caller, project, doc));
    calls
}

/// The three `docs.*` reads as `caller` on `project`, each expected to be refused.
fn every_read(fx: &Fx, caller: &str, project: &str, doc: &str) -> Vec<(&'static str, ApiError)> {
    let o = &fx.office;
    let p = project.to_owned();
    vec![
        (
            "index",
            err(o.docs_index(caller, DocsIndexParams { project: p.clone() })),
        ),
        (
            "list",
            err(o.docs_list(
                caller,
                DocsListParams {
                    project: p.clone(),
                    ..Default::default()
                },
            )),
        ),
        (
            "inspect",
            err(o.docs_inspect(
                caller,
                DocsInspectParams {
                    project: p,
                    document: doc.into(),
                },
            )),
        ),
    ]
}

/// The four `docs.*` writes as `caller` on `project`, each expected to be refused.
fn every_write(fx: &Fx, caller: &str, project: &str, doc: &str) -> Vec<(&'static str, ApiError)> {
    let o = &fx.office;
    let p = project.to_owned();
    vec![
        (
            "create",
            err(o.docs_create(caller, create_params(project, "Intrusion", None))),
        ),
        (
            "update",
            err(o.docs_update(
                caller,
                DocsUpdateParams {
                    command_id: next_uuid(),
                    project: p.clone(),
                    document_id: doc.into(),
                    expected_revision: 1,
                    title: Some("Defaced".into()),
                    ..Default::default()
                },
            )),
        ),
        (
            "move",
            err(o.docs_move(
                caller,
                DocsMoveParams {
                    command_id: next_uuid(),
                    project: p.clone(),
                    document_id: doc.into(),
                    expected_revision: 1,
                    new_path: Some("stolen.md".into()),
                    ..Default::default()
                },
            )),
        ),
        (
            "delete",
            err(o.docs_delete(
                caller,
                DocsDeleteParams {
                    command_id: next_uuid(),
                    project: p.clone(),
                    document_id: doc.into(),
                    expected_revision: 1,
                },
            )),
        ),
        (
            "put_media",
            err(o.docs_put_media(caller, media_params(project, "x.png", &png(2, 2)))),
        ),
    ]
}

#[test]
fn delete_removes_document_regenerates_index_and_rebuilds_on_both_engines() {
    for engine in engines() {
        let fx = fixture(engine.clone());
        let first = create(&fx, &fx.owning_agent, "Setup", Some("guides/setup.md")).document;
        let second = create(&fx, &fx.owning_agent, "Overview", Some("overview.md")).document;
        assert_eq!(index(&fx, &fx.owner).documents.len(), 2);

        // Stale revision is refused with Conflict; outsider sees uniform NotFound.
        let stale = err(fx.office.docs_delete(
            &fx.owning_agent,
            DocsDeleteParams {
                command_id: next_uuid(),
                project: fx.project.clone(),
                document_id: first.id.clone(),
                expected_revision: 99,
            },
        ));
        assert_eq!(stale.code, ErrorCode::Conflict, "{engine:?}");
        let hidden = err(fx.office.docs_delete(
            &fx.outsider_agent,
            DocsDeleteParams {
                command_id: next_uuid(),
                project: fx.project.clone(),
                document_id: first.id.clone(),
                expected_revision: 1,
            },
        ));
        assert_eq!(
            (hidden.code, hidden.message.as_str()),
            (ErrorCode::NotFound, "project not found"),
            "{engine:?}"
        );

        // Delete first doc + exact retry idempotency.
        let delete_cmd = next_uuid();
        let params = DocsDeleteParams {
            command_id: delete_cmd.clone(),
            project: fx.project.clone(),
            document_id: first.id.clone(),
            expected_revision: 1,
        };
        let deleted = ok(fx.office.docs_delete(&fx.owning_agent, params.clone()));
        assert_eq!(deleted.deleted_id, first.id);
        assert_eq!(deleted.deleted_path, "guides/setup.md");
        let retry = ok(fx.office.docs_delete(&fx.owning_agent, params));
        assert_eq!(retry, deleted, "{engine:?} retry idempotency");

        // Index regenerated: guides section gone, overview.md remains in root section "".
        let after_one = index(&fx, &fx.owner);
        assert_eq!(after_one.documents.len(), 1);
        assert_eq!(after_one.documents[0].path, "overview.md");
        assert_eq!(after_one.sections.len(), 1);
        assert_eq!(after_one.sections[0].path, "");
        assert_eq!(after_one.sections[0].document_count, 1);
        fx.office.sync_store().unwrap();
        let yaml_text = std::fs::read_to_string(index_file(&fx)).unwrap();
        assert!(!yaml_text.contains("guides/setup.md"));
        assert!(yaml_text.contains("overview.md"));

        // Delete last doc: index.yaml stays regenerated with empty documents & sections.
        ok(fx.office.docs_delete(
            &fx.owner,
            DocsDeleteParams {
                command_id: next_uuid(),
                project: fx.project.clone(),
                document_id: second.id.clone(),
                expected_revision: 1,
            },
        ));
        let empty = index(&fx, &fx.owner);
        assert!(empty.documents.is_empty());
        assert!(empty.sections.is_empty());

        // Delete+Rebuild reproduces identical empty index on both engines.
        let fx = reopen(fx).expect("rebuild after delete");
        assert_eq!(index(&fx, &fx.owner), empty, "{engine:?}");
        fx.office.close().unwrap();
    }
}

#[test]
fn media_is_sanitized_typed_bounded_and_versioned_on_both_engines() {
    for engine in engines() {
        let fx = fixture(engine.clone());
        let put = |params: DocsPutMediaParams| fx.office.docs_put_media(&fx.owner, params);
        let first = ok(put(media_params(
            &fx.project,
            "arch.png",
            &png_with_text(6, 6, GPS_MARKER),
        )))
        .media;
        fx.office.sync_store().unwrap();
        let blob = fx
            .roots
            .durable
            .join(format!("state/projects/{}/docs/media/arch.png", fx.project));
        let kept = std::fs::read(&blob).unwrap();
        assert!(
            !kept
                .windows(GPS_MARKER.len())
                .any(|w| w == GPS_MARKER.as_bytes()),
            "metadata dropped"
        );
        assert_eq!(first.bytes, kept.len() as u64);

        let mut svg = media_params(&fx.project, "evil.png", b"<svg onload=alert(1)>");
        svg.media_type = "image/svg+xml".into();
        assert_eq!(err(put(svg)).code, ErrorCode::Unsupported);
        let lying = media_params(&fx.project, "lying.png", b"<script>alert(1)</script>");
        assert_ne!(err(put(lying)).code, ErrorCode::NotFound);
        let mut wrong_ext = media_params(&fx.project, "arch.jpg", &png(2, 2));
        wrong_ext.command_id = next_uuid();
        assert_eq!(err(put(wrong_ext)).code, ErrorCode::Unsupported);
        for bad in ["../x.png", "a/b.png", "x.svg", "X.png"] {
            assert_eq!(
                err(put(media_params(&fx.project, bad, &png(2, 2)))).code,
                ErrorCode::InvalidRequest,
                "{bad}"
            );
        }
        let mut bad_b64 = media_params(&fx.project, "b.png", &png(2, 2));
        bad_b64.content_base64 = "***".into();
        assert_eq!(err(put(bad_b64)).code, ErrorCode::InvalidRequest);
        let mut big = media_params(&fx.project, "big.png", &[0u8; 8]);
        big.content_base64 = "A".repeat(org_proto::docs::DOC_MEDIA_MAX_BASE64_CHARS + 4);
        assert_eq!(err(put(big)).code, ErrorCode::TooLarge);
        let mut unknown = media_params(&fx.project, "u.png", &png(2, 2));
        unknown.kind = DocMediaKindWire::Unknown;
        assert_eq!(err(put(unknown)).code, ErrorCode::InvalidRequest);

        let again = media_params(&fx.project, "arch.png", &png(3, 3));
        assert_eq!(err(put(again.clone())).code, ErrorCode::AlreadyExists);
        let mut replace = again;
        replace.command_id = next_uuid();
        replace.expected_revision = Some(1);
        let second = ok(put(replace)).media;
        assert_eq!(
            (second.revision, second.id.as_str()),
            (2, first.id.as_str())
        );
        assert_ne!(second.sha256, first.sha256);
        let mut stale = media_params(&fx.project, "arch.png", &png(5, 5));
        stale.expected_revision = Some(1);
        assert_eq!(err(put(stale)).code, ErrorCode::Conflict);
        let tree = index(&fx, &fx.owning_agent);
        assert_eq!(tree.media.len(), 1);
        assert_eq!(tree.media[0].sha256, second.sha256);
        let fx = reopen(fx).expect("media rebuild");
        assert_eq!(index(&fx, &fx.owner).media, tree.media, "{engine:?}");
        fx.office.close().unwrap();
    }
}

#[test]
fn a_stray_file_under_docs_is_skipped_by_the_rebuild_and_never_projected() {
    for engine in engines() {
        let fx = fixture(engine.clone());
        create(&fx, &fx.owner, "Setup", Some("setup.md"));
        fx.office.sync_store().unwrap();
        let docs_dir = fx
            .roots
            .durable
            .join(format!("state/projects/{}/docs", fx.project));
        std::fs::write(docs_dir.join("notes.txt"), "hand-made").unwrap();
        let tree = index(&fx, &fx.owner);
        let fx = reopen(fx).expect("a stray file does not block the office");
        assert_eq!(index(&fx, &fx.owner), tree, "{engine:?}");
        fx.office.close().unwrap();
    }
}

#[path = "project_docs_guard_tests.rs"]
mod guards;
