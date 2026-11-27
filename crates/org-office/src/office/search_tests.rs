//! `TASK-91` unified search through the office facade, on BOTH `Memory` and `Sqlite`: every
//! entity kind found, the filters, pages and fields, the same answers from both engines, a
//! full `runtime/<office-id>/` delete + rebuild, and the isolation that must never leak: a DM
//! to an outsider, another author's private notes and decisions (to any agent, team lead
//! included), an inactive or unknown caller, and a satellite that holds no private notes.

use std::os::unix::fs::PermissionsExt;

use org_proto::chat::ChatSendParams;
use org_proto::hiring::{AgentSetStatus, AgentSetStatusParams};
use org_proto::org::{ProjectCreateParams, TeamSetLeadParams};
use org_proto::private_notes::{PrivateNoteAddParams, PrivateNoteKind};
use org_proto::processes::{
    DocumentKindWire, DocumentStatusWire, ProcessScopeWire, ProcessWriteParams,
};
use org_proto::search::{
    SearchCoverage, SearchEntityType, SearchHitField, SearchOmission, SearchOmissionReason,
    SearchQueryParams, SearchQueryResult, SearchSource,
};
use org_proto::tasks::{TasksChecklistAddParams, TasksCommentParams, TasksCreateParams};
use org_proto::{ApiError, ErrorCode, Outcome, Request};

use crate::office::private_notes_tests::{
    next_uuid, rebuilt_sqlite, seed_fixture, PrivateNoteFixture,
};
use crate::office::{
    CreateInvitationParams, Custody, DataRoots, JoinRequestAccepted, JoinRequestParams, Office,
    Options, SatelliteProvisionSpec, TestOffice,
};
use crate::rules::OrgCommand;
use crate::store::open::EngineChoice;

const ENGINES: [EngineChoice; 2] = [EngineChoice::Memory, EngineChoice::Sqlite];
const CHANGED_FILE: &str = "crates/org-office/src/search.rs";

/// The fixture's owner (a person), agents A and B and A's task, plus: a project with a second
/// task (B's, a label, a modified file, a comment, a checklist step), an organization
/// standard, a channel message, a DM between the owner and A, and private notes of both.
struct World {
    f: PrivateNoteFixture,
    project: String,
    zebra_task: String,
    dm: String,
}

fn ok<T>(answer: Result<Result<T, ApiError>, crate::OfficeError>) -> T {
    answer.expect("storage").expect("accepted")
}

fn send(office: &Office, actor: &str, to: &str, body: &str) -> String {
    let sent = ok(office.chat_send(
        actor,
        ChatSendParams {
            id: next_uuid(),
            to: to.into(),
            kind: org_proto::msg::KIND_MSG.into(),
            priority: None,
            subject: None,
            body: body.into(),
            meta: Default::default(),
            reply_to: None,
            also_in_channel: false,
            reason: None,
            attachment_ids: Vec::new(),
        },
    ));
    sent.message.conversation_id.to_string()
}

fn note(
    office: &Office,
    author: &str,
    kind: PrivateNoteKind,
    title: Option<&str>,
    body: &str,
) -> String {
    let note_id = next_uuid();
    ok(office.task_note_add(
        author,
        PrivateNoteAddParams {
            command_id: next_uuid(),
            note_id: note_id.clone(),
            task: None,
            project_id: None,
            kind,
            title: title.map(str::to_owned),
            why: (kind == PrivateNoteKind::Decision).then(|| "capybara is fast".into()),
            alternatives: None,
            tags: vec!["db".into()],
            body_markdown: body.into(),
        },
    ));
    note_id
}

fn task(
    office: &Office,
    owner: &str,
    title: &str,
    extra: impl FnOnce(&mut TasksCreateParams),
) -> String {
    let team = office.org_snapshot().unwrap().teams[0].id.clone();
    let mut params = TasksCreateParams {
        command_id: next_uuid(),
        task_id: next_uuid(),
        team_id: Some(team),
        project_id: None,
        assigned_team_id: None,
        assignee_id: None,
        task_type: Some("task".into()),
        work_type: None,
        title: title.into(),
        body_markdown: String::new(),
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
    };
    extra(&mut params);
    ok(office.task_create(owner, params)).task.key
}

fn world(engine: EngineChoice) -> World {
    let f = seed_fixture(engine);
    let office = &f.office;
    let team = office.org_snapshot().unwrap().teams[0].id.clone();
    let project = next_uuid();
    ok(office.submit_org(
        &f.owner,
        OrgCommand::ProjectCreate(ProjectCreateParams {
            command_id: next_uuid(),
            project_id: project.clone(),
            name: "Quokka Platform".into(),
            key: None,
            description: None,
            teams: vec![],
            general_area_id: String::new(),
            general_area_name: None,
            repo: None,
            integration_branch: None,
            slug: None,
            owner_team_id: Some(team.clone()),
            create_team_id: None,
            parent_team_id: None,
            contributing_team_ids: vec![],
        }),
    ));
    let zebra_task = task(office, &f.owner, "Migrate the zebra index", |p| {
        p.project_id = Some(project.clone());
        p.assignee_id = Some(f.agent_b.clone());
        p.labels = vec!["db:sqlite".into()];
        p.modified_files = vec![CHANGED_FILE.into()];
        p.body_markdown = "# Rollout plan\nRebuild the tables offline".into();
    });
    ok(office.task_comment(
        &f.owner,
        TasksCommentParams {
            command_id: next_uuid(),
            task: zebra_task.clone(),
            comment_id: next_uuid(),
            body_markdown: "A public comment about the walrus".into(),
            attachment_ids: vec![],
        },
    ));
    ok(office.task_checklist_add(
        &f.owner,
        TasksChecklistAddParams {
            command_id: next_uuid(),
            task: zebra_task.clone(),
            step_id: next_uuid(),
            section: None,
            text: "Verify narwhal latency".into(),
            required: Some(true),
        },
    ));
    ok(office.process_write(
        &f.owner,
        ProcessWriteParams {
            command_id: next_uuid(),
            document_id: None,
            kind: DocumentKindWire::Standard,
            scope: ProcessScopeWire::Organization,
            key: None,
            title: "Ocelot Coding Standard".into(),
            status: DocumentStatusWire::Current,
            owner_employee_id: None,
            overridable: true,
            body_markdown: "# Reviews\nEvery change gets a second reader.".into(),
            expected_revision: None,
        },
    ));
    send(
        office,
        &f.owner,
        "#general",
        "Channel note about platypus deploys",
    );
    let dm = send(
        office,
        &f.owner,
        "alpha",
        "Secret DM about the axolotl budget",
    );
    note(
        office,
        &f.agent_a,
        PrivateNoteKind::Note,
        None,
        "A's private plan: capybara caching",
    );
    note(
        office,
        &f.agent_a,
        PrivateNoteKind::Decision,
        Some("Pick the capybara store"),
        "Chose it after a benchmark",
    );
    note(
        office,
        &f.agent_b,
        PrivateNoteKind::Note,
        None,
        "B's own capybara note",
    );
    World {
        f,
        project,
        zebra_task,
        dm,
    }
}

fn params(q: &str) -> SearchQueryParams {
    SearchQueryParams {
        q: q.into(),
        ..SearchQueryParams::default()
    }
}

fn search(
    office: &Office,
    actor: &str,
    p: SearchQueryParams,
) -> Result<SearchQueryResult, ApiError> {
    office.search(actor, p).expect("storage")
}

fn found(office: &Office, actor: &str, p: SearchQueryParams) -> SearchQueryResult {
    search(office, actor, p).expect("a search answer")
}

fn kinds(result: &SearchQueryResult) -> Vec<SearchEntityType> {
    result.hits.iter().map(|h| h.entity_type).collect()
}

const PRIVATE: [SearchEntityType; 2] = [
    SearchEntityType::PrivateNote,
    SearchEntityType::PrivateDecision,
];

fn private(q: &str) -> SearchQueryParams {
    SearchQueryParams {
        types: PRIVATE.to_vec(),
        ..params(q)
    }
}

fn assert_every_kind_is_found(w: &World) {
    let (office, owner) = (&w.f.office, w.f.owner.as_str());
    let first = found(office, owner, params("rotation"));
    assert_eq!(first.total, 1);
    assert_eq!(
        first.hits[0].reference.as_deref(),
        Some(w.f.task_key.as_str())
    );
    assert_eq!(first.hits[0].status.as_deref(), Some("todo"));
    for q in ["walrus", "narwhal", "zebra", "rollout"] {
        let hits = found(office, owner, params(q));
        assert_eq!(kinds(&hits), [SearchEntityType::Task], "{q}");
        assert_eq!(
            hits.hits[0].reference.as_deref(),
            Some(w.zebra_task.as_str())
        );
    }
    let by_key = found(office, owner, params(&w.zebra_task.to_lowercase()));
    assert_eq!(
        by_key.hits[0].reference.as_deref(),
        Some(w.zebra_task.as_str())
    );
    let project = found(office, owner, params("quokka"));
    assert_eq!(kinds(&project), [SearchEntityType::Project]);
    assert_eq!(project.hits[0].entity_id, w.project);
    let standard = found(office, owner, params("ocelot"));
    assert_eq!(kinds(&standard), [SearchEntityType::Process]);
    let member = found(office, owner, params("alpha"));
    assert!(member
        .hits
        .iter()
        .any(|h| h.entity_type == SearchEntityType::Member && h.entity_id == w.f.agent_a));
    let snapshot = office.org_snapshot().unwrap();
    let team = found(office, owner, params(&snapshot.teams[0].name));
    assert!(kinds(&team).contains(&SearchEntityType::Team));
    let message = found(office, owner, params("platypus"));
    assert_eq!(kinds(&message), [SearchEntityType::Message]);
    assert!(message.hits[0].conversation_id.is_some());
    assert_eq!(
        message.coverage.searched,
        [SearchSource::Shared, SearchSource::Messages]
    );
    assert!(message.hits[0]
        .excerpt
        .as_deref()
        .unwrap()
        .contains("platypus"));
}

fn assert_filters_narrow(w: &World) {
    let (office, owner) = (&w.f.office, w.f.owner.as_str());
    let with = |edit: &dyn Fn(&mut SearchQueryParams)| {
        let mut p = params("zebra");
        edit(&mut p);
        search(office, owner, p)
    };
    let one = |edit: &dyn Fn(&mut SearchQueryParams)| with(edit).unwrap().total;
    assert_eq!(one(&|p| p.modified_file = Some(CHANGED_FILE.into())), 1);
    assert_eq!(one(&|p| p.modified_file = Some("README.md".into())), 0);
    assert_eq!(one(&|p| p.label = Some("DB:SQLite".into())), 1);
    assert_eq!(one(&|p| p.label = Some("security".into())), 0);
    assert_eq!(one(&|p| p.project = Some(w.project.clone())), 1);
    assert_eq!(one(&|p| p.project = Some("P-1".into())), 1);
    assert_eq!(one(&|p| p.assignee = Some("@beta".into())), 1);
    assert_eq!(one(&|p| p.assignee = Some(w.f.agent_a.clone())), 0);
    assert_eq!(one(&|p| p.status = Some("todo".into())), 1);
    assert_eq!(one(&|p| p.status = Some("done".into())), 0);
    assert_eq!(one(&|p| p.types = vec![SearchEntityType::Project]), 0);
    assert_eq!(one(&|p| p.channel = Some("#general".into())), 0);
    assert_eq!(
        found(
            office,
            owner,
            SearchQueryParams {
                channel: Some("#general".into()),
                ..params("platypus")
            }
        )
        .total,
        1
    );
    for (refused, code) in [
        (
            with(&|p| p.status = Some("bogus".into())),
            ErrorCode::InvalidRequest,
        ),
        (
            with(&|p| p.label = Some("Not A Label!".into())),
            ErrorCode::InvalidRequest,
        ),
        (
            with(&|p| p.modified_file = Some("../etc/passwd".into())),
            ErrorCode::InvalidRequest,
        ),
        (
            with(&|p| p.author_id = Some("not-a-uuid".into())),
            ErrorCode::InvalidRequest,
        ),
        (with(&|p| p.limit = Some(0)), ErrorCode::InvalidRequest),
        (with(&|p| p.limit = Some(101)), ErrorCode::InvalidRequest),
        (
            with(&|p| p.cursor = Some("forged".into())),
            ErrorCode::InvalidRequest,
        ),
        (with(&|p| p.q = "x".repeat(513)), ErrorCode::InvalidRequest),
        (with(&|p| p.q = "   ".into()), ErrorCode::InvalidRequest),
        (
            with(&|p| p.project = Some("P-999".into())),
            ErrorCode::NotFound,
        ),
        (
            with(&|p| p.assignee = Some("nobody".into())),
            ErrorCode::NotFound,
        ),
        (
            with(&|p| p.channel = Some("#no-such-channel".into())),
            ErrorCode::NotFound,
        ),
    ] {
        assert_eq!(refused.unwrap_err().code, code);
    }
}

fn assert_dms_never_reach_an_outsider(w: &World) {
    let office = &w.f.office;
    for (caller, sees) in [(&w.f.owner, 1), (&w.f.agent_a, 1), (&w.f.agent_b, 0)] {
        let answer = found(office, caller, params("axolotl"));
        assert_eq!(answer.total, sees, "{caller}");
        assert_eq!(answer.hits.len() as u64, sees);
        let counted = found(
            office,
            caller,
            SearchQueryParams {
                count_only: true,
                ..params("axolotl")
            },
        );
        assert_eq!(counted.total, sees);
        assert!(counted.hits.is_empty());
    }
    // The DM by its own ID reads as missing to an outsider, exactly as an unknown one does.
    let outsider = search(
        office,
        &w.f.agent_b,
        SearchQueryParams {
            channel: Some(w.dm.clone()),
            ..params("axolotl")
        },
    );
    assert_eq!(outsider.unwrap_err().code, ErrorCode::NotFound);
    let unknown = search(
        office,
        &w.f.agent_b,
        SearchQueryParams {
            channel: Some(next_uuid()),
            ..params("axolotl")
        },
    );
    assert_eq!(unknown.unwrap_err().code, ErrorCode::NotFound);
    assert_eq!(
        found(
            office,
            &w.f.agent_a,
            SearchQueryParams {
                channel: Some(w.dm.clone()),
                ..params("axolotl")
            }
        )
        .total,
        1
    );
    // A DM the outsider may not read never moves the score of a message it may read.
    let before = found(office, &w.f.agent_b, params("platypus")).hits[0].score;
    send(
        office,
        &w.f.owner,
        "alpha",
        "platypus platypus platypus in a private DM",
    );
    let after = found(office, &w.f.agent_b, params("platypus"));
    assert_eq!(after.total, 1);
    assert_eq!(after.hits[0].score, before);
    assert_eq!(found(office, &w.f.agent_a, params("platypus")).total, 2);
}

fn assert_private_notes_follow_adr_0031(w: &World) {
    let (office, a, b, owner) = (&w.f.office, &w.f.agent_a, &w.f.agent_b, &w.f.owner);
    // Never beside shared knowledge, and never mixed with it.
    assert_eq!(found(office, a, params("capybara")).total, 0);
    let mixed = SearchQueryParams {
        types: vec![SearchEntityType::Task, SearchEntityType::PrivateNote],
        ..params("capybara")
    };
    assert_eq!(
        search(office, a, mixed).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    // An agent sees only its own notes and decisions; another author's ID answers nothing.
    let mine = found(office, a, private("capybara"));
    assert_eq!(mine.total, 2);
    assert!(mine
        .hits
        .iter()
        .all(|h| h.author_id.as_deref() == Some(a.as_str())));
    assert_eq!(mine.coverage.searched, [SearchSource::PrivateNotes]);
    let decisions = found(
        office,
        a,
        SearchQueryParams {
            types: vec![SearchEntityType::PrivateDecision],
            ..params("capybara")
        },
    );
    assert_eq!(kinds(&decisions), [SearchEntityType::PrivateDecision]);
    assert_eq!(
        decisions.hits[0].title.as_deref(),
        Some("Pick the capybara store")
    );
    let theirs = found(office, b, private("capybara"));
    assert_eq!(theirs.total, 1);
    assert_eq!(theirs.hits[0].author_id.as_deref(), Some(b.as_str()));
    for p in [
        SearchQueryParams {
            author_id: Some(a.clone()),
            ..private("capybara")
        },
        SearchQueryParams {
            author_id: Some(a.clone()),
            count_only: true,
            ..private("capybara")
        },
        SearchQueryParams {
            author_id: Some(a.clone()),
            ..private("benchmark")
        },
        private("benchmark"),
    ] {
        let none = found(office, b, p);
        assert_eq!((none.total, none.hits.len()), (0, 0));
    }
    // An active person reads every author's, or one author's.
    assert_eq!(found(office, owner, private("capybara")).total, 3);
    let by_a = found(
        office,
        owner,
        SearchQueryParams {
            author_id: Some(a.clone()),
            ..private("capybara")
        },
    );
    assert_eq!(by_a.total, 2);
    // Another author's notes never move this agent's own scores.
    let before = theirs.hits[0].score;
    note(
        office,
        a,
        PrivateNoteKind::Note,
        None,
        "capybara capybara capybara capybara",
    );
    let after = found(office, b, private("capybara"));
    assert_eq!((after.total, after.hits[0].score), (1, before));
    // Leading the team that owns every task changes nothing.
    let team = office.org_snapshot().unwrap().teams[0].clone();
    ok(office.submit_org(
        owner,
        OrgCommand::TeamSetLead(TeamSetLeadParams {
            command_id: next_uuid(),
            team_id: team.id,
            expected_version: team.version,
            lead_member_id: Some(b.clone()),
        }),
    ));
    let lead = found(
        office,
        b,
        SearchQueryParams {
            author_id: Some(a.clone()),
            ..private("capybara")
        },
    );
    assert_eq!(lead.total, 0);
    assert_eq!(found(office, b, private("capybara")).total, 1);
}

fn set_paused(f: &PrivateNoteFixture, member: &str, paused: bool) {
    let employee = f.office.load_employee(member).unwrap().unwrap();
    let request = Request::new::<AgentSetStatus>(
        9,
        &AgentSetStatusParams {
            command_id: next_uuid(),
            member_id: member.into(),
            expected_version: employee.meta.revision,
            paused,
        },
    )
    .unwrap();
    let answer = f
        .office
        .dispatch_hiring_rpc(Some(&f.owner), &request)
        .unwrap();
    assert!(matches!(answer.outcome, Outcome::Result(_)), "{answer:?}");
}

fn assert_only_active_members_search(w: &World) {
    let office = &w.f.office;
    for caller in ["not-a-uuid", "", &next_uuid()] {
        let refused = search(office, caller, params("zebra")).unwrap_err();
        assert_eq!(refused.code, ErrorCode::NotAuthorized, "{caller:?}");
        // Refused before anything else: an invalid request reads the same.
        let invalid = search(
            office,
            caller,
            SearchQueryParams {
                limit: Some(0),
                ..params("")
            },
        );
        assert_eq!(invalid.unwrap_err().code, ErrorCode::NotAuthorized);
    }
    set_paused(&w.f, &w.f.agent_b, true);
    for p in [params("zebra"), private("capybara")] {
        assert_eq!(
            search(office, &w.f.agent_b, p).unwrap_err().code,
            ErrorCode::NotAuthorized
        );
    }
    set_paused(&w.f, &w.f.agent_b, false);
    assert_eq!(found(office, &w.f.agent_b, params("zebra")).total, 1);
}

fn assert_hostile_text_never_fails(w: &World) {
    let (office, owner) = (&w.f.office, w.f.owner.as_str());
    for q in [
        "foo\" OR 1=1",
        "NEAR/5",
        "*",
        "---",
        "()",
        "col:val",
        "zebra\" OR \"",
        "'; DROP search_fts; --",
        "f_body:zebra",
        "^zebra*",
        "🙂",
    ] {
        let answer = found(office, owner, params(q));
        assert!(answer.total <= 1, "{q}");
        let private = found(office, owner, private(q));
        assert!(private.total <= 3, "{q}");
    }
    assert_eq!(found(office, owner, params("*")).total, 0);
    // Quotes, operators and punctuation around a word leave the word itself.
    for q in [
        "\"zebra\"",
        "zebra*",
        "(zebra)",
        "zebra:",
        "^zebra",
        "{zebra}",
    ] {
        assert_eq!(found(office, owner, params(q)).total, 1, "{q}");
    }
}

fn assert_pages_and_fields(w: &World) {
    let (office, owner) = (&w.f.office, w.f.owner.as_str());
    for n in 0..5 {
        task(office, owner, &format!("Lemur sprint {n}"), |p| {
            p.body_markdown = "lemur".repeat(n + 1)
        });
    }
    let all = found(
        office,
        owner,
        SearchQueryParams {
            limit: Some(10),
            ..params("lemur")
        },
    );
    assert_eq!(
        (all.total, all.hits.len(), all.next_cursor.as_deref()),
        (5, 5, None)
    );
    let mut paged = Vec::new();
    let mut cursor = None;
    loop {
        let page = found(
            office,
            owner,
            SearchQueryParams {
                limit: Some(2),
                cursor,
                ..params("lemur")
            },
        );
        assert_eq!(page.total, 5);
        paged.extend(page.hits.iter().map(|h| h.entity_id.clone()));
        let Some(next) = page.next_cursor else { break };
        cursor = Some(next);
    }
    let whole: Vec<String> = all.hits.iter().map(|h| h.entity_id.clone()).collect();
    assert_eq!(paged, whole);
    let scores: Vec<f64> = all.hits.iter().map(|h| h.score.unwrap()).collect();
    assert!(
        scores.windows(2).all(|pair| pair[0] >= pair[1]),
        "{scores:?}"
    );
    let pruned = found(
        office,
        owner,
        SearchQueryParams {
            fields: vec![SearchHitField::Ref],
            ..params("lemur")
        },
    );
    let hit = &pruned.hits[0];
    assert!(hit.reference.is_some());
    assert!(hit.title.is_none() && hit.excerpt.is_none() && hit.score.is_none());
    assert!(hit.updated_at.is_none() && hit.status.is_none());
}

/// One hit as both engines must answer it: its type, reference, title and score.
type HitPrint = (SearchEntityType, Option<String>, Option<String>, f64);

/// One search's answers: its label, total and hits.
type SearchPrint = (String, u64, Vec<HitPrint>);

/// What both engines must answer alike: every hit's type, reference, title and score.
fn fingerprint(w: &World) -> Vec<SearchPrint> {
    let office = &w.f.office;
    let (owner, a, b) = (
        ("owner", w.f.owner.as_str()),
        ("agent a", w.f.agent_a.as_str()),
        ("agent b", w.f.agent_b.as_str()),
    );
    let mut out = Vec::new();
    for ((who, caller), p) in [
        (owner, params("zebra")),
        (owner, params("capybara")),
        (owner, params("alpha agent")),
        (owner, params("platypus axolotl")),
        (owner, params("t-")),
        (owner, params("a")),
        (owner, private("capybara benchmark")),
        (a, private("capybara")),
        (b, params("axolotl")),
    ] {
        let label = format!("{who}: {}", p.q);
        let answer = found(office, caller, p);
        let hits = answer
            .hits
            .iter()
            .map(|h| {
                (
                    h.entity_type,
                    h.reference.clone(),
                    h.title.clone(),
                    h.score.unwrap(),
                )
            })
            .collect();
        out.push((label, answer.total, hits));
    }
    out
}

#[test]
fn search_finds_every_kind_filters_pages_and_isolates_on_memory_and_sqlite() {
    let mut prints = Vec::new();
    for engine in ENGINES {
        let w = world(engine);
        assert_every_kind_is_found(&w);
        assert_filters_narrow(&w);
        prints.push(fingerprint(&w));
        assert_dms_never_reach_an_outsider(&w);
        assert_private_notes_follow_adr_0031(&w);
        assert_only_active_members_search(&w);
        assert_hostile_text_never_fails(&w);
        assert_pages_and_fields(&w);
    }
    // Both engines answer alike: the same hits, in the same order, with the same scores.
    let (memory, sqlite) = (&prints[0], &prints[1]);
    assert_eq!(memory.len(), sqlite.len());
    for (m, s) in memory.iter().zip(sqlite) {
        assert_eq!((&m.0, m.1, m.2.len()), (&s.0, s.1, s.2.len()), "{}", m.0);
        for (a, b) in m.2.iter().zip(&s.2) {
            assert_eq!((a.0, &a.1, &a.2), (b.0, &b.1, &b.2), "{}", m.0);
            assert!((a.3 - b.3).abs() < 1e-9, "{}: {} != {}", m.0, a.3, b.3);
        }
    }
}

fn mode(path: &std::path::Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// The bytes of a SQLite database and of its write-ahead log, as they are on disk.
fn on_disk(db: &std::path::Path) -> (Vec<u8>, Vec<u8>) {
    let wal = std::path::PathBuf::from(format!("{}-wal", db.display()));
    (
        std::fs::read(db).unwrap_or_default(),
        std::fs::read(wal).unwrap_or_default(),
    )
}

fn holds(bytes: &(Vec<u8>, Vec<u8>), needle: &[u8]) -> bool {
    [&bytes.0, &bytes.1]
        .iter()
        .any(|b| b.windows(needle.len()).any(|w| w == needle))
}

#[test]
fn a_deleted_runtime_rebuilds_every_index_and_the_private_one_stays_masked_and_apart() {
    let w = world(EngineChoice::Sqlite);
    let before = fingerprint(&w);
    let office_id = w.f.office.info().office_id.clone();
    let runtime = w.f.roots.runtime.join(&office_id);
    let index = runtime.join("protected/private/task-notes-search.db");
    assert!(index.is_file());
    assert_eq!(mode(&index), 0o600);
    assert_eq!(mode(index.parent().unwrap()), 0o700);
    assert_eq!(mode(&runtime.join("protected")), 0o700);
    assert!(!runtime.join("private").exists());
    // Searching only ever reads the notes database: not a byte of it or of its log changes,
    // and it never holds a full-text index.
    let notes = w.f.roots.durable.join("private/task-notes.db");
    let untouched = on_disk(&notes);
    for caller in [&w.f.owner, &w.f.agent_a, &w.f.agent_b] {
        found(&w.f.office, caller, private("capybara benchmark"));
    }
    assert_eq!(on_disk(&notes), untouched);
    assert!(!holds(&untouched, b"fts5") && !holds(&untouched, b"private_search"));
    let World {
        f,
        project,
        zebra_task,
        dm,
    } = w;
    let f = rebuilt_sqlite(f);
    assert!(!index.exists() || index.is_file());
    let w = World {
        f,
        project,
        zebra_task,
        dm,
    };
    let after = fingerprint(&w);
    assert_eq!(before.len(), after.len());
    for (b, a) in before.iter().zip(&after) {
        assert_eq!((&b.0, b.1), (&a.0, a.1));
        for (x, y) in b.2.iter().zip(&a.2) {
            assert_eq!((x.0, &x.1, &x.2), (y.0, &y.1, &y.2), "{}", b.0);
            assert!((x.3 - y.3).abs() < 1e-9, "{}", b.0);
        }
    }
    assert!(index.is_file());
    assert!(!holds(&on_disk(&notes), b"fts5"));
    // A note written after the rebuild is found at once.
    note(
        &w.f.office,
        &w.f.agent_b,
        PrivateNoteKind::Plan,
        None,
        "Fresh tapir plan",
    );
    assert_eq!(found(&w.f.office, &w.f.agent_b, private("tapir")).total, 1);
    assert_eq!(found(&w.f.office, &w.f.agent_a, private("tapir")).total, 0);
    // Search never reaches the accounting or budget stores.
    for store in ["accounting.db", "budgets.db"] {
        let path = runtime.join("protected").join(store);
        assert!(!holds(&on_disk(&path), b"fts5"), "{store}");
    }
}

/// A satellite of a real HQ, its organization replicated: it searches shared knowledge, holds
/// no private notes, says so, and never makes a private search index.
#[test]
fn a_satellite_omits_private_notes_truthfully_and_never_makes_their_index() {
    let (hq, sat, roots, _dir) = satellite_of_a_real_hq();
    let owner = hq.owner_member_id().to_owned();
    let shared = found(&sat, &owner, params("hr"));
    assert_eq!(
        shared.coverage.searched,
        [SearchSource::Shared, SearchSource::Messages]
    );
    let answer = found(&sat, &owner, private("capybara"));
    assert_eq!(answer.total, 0);
    assert_eq!(
        answer.coverage,
        SearchCoverage {
            searched: vec![],
            omitted: vec![SearchOmission {
                source: SearchSource::PrivateNotes,
                reason: SearchOmissionReason::HqOnly,
            }],
        }
    );
    let counted = found(
        &sat,
        &owner,
        SearchQueryParams {
            count_only: true,
            ..private("capybara")
        },
    );
    assert_eq!(counted.total, 0);
    let runtime = roots.runtime.join(&sat.info().office_id);
    assert!(!runtime.join("protected/private").exists());
    assert!(!runtime.join("private").exists());
    assert!(!roots.durable.join("private").exists());
    assert_eq!(
        search(&sat, &next_uuid(), private("capybara"))
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
}

/// A real HQ, a satellite it invited, approved and provisioned, and the HQ's organization
/// replicated onto it (`TASK-52`).
fn satellite_of_a_real_hq() -> (Office, Office, DataRoots, tempfile::TempDir) {
    use org_pki::invitation::{sign_exporter_proof, InvitationToken};
    use org_pki::{cert::sign_satellite_key_pop, SigningKey, Zeroizing};
    use org_store::{testkit::FakeCustody, Id, OfficeIdentity};

    let hq = Office::for_test_with_real_ca(TestOffice {
        engine: EngineChoice::Sqlite,
    });
    let owner = hq.owner_member_id().to_owned();
    let org_id = hq.info().org_id.clone();
    let created = ok(hq.create_satellite_invitation(
        &owner,
        CreateInvitationParams {
            office_name: "Lisbon Studio".into(),
            hq_addresses: vec!["127.0.0.1:9443".into()],
            ttl_secs: Some(600),
            existing_office_id: None,
        },
    ));
    let token = InvitationToken::decode(&created.token).unwrap();
    let tls = SigningKey::generate().unwrap();
    let exporter = [0xABu8; 32];
    let pop = sign_satellite_key_pop(&tls, &token.org_id, &token.office_id, &token.invitation_id)
        .unwrap();
    let proof = sign_exporter_proof(
        &token.signing_key().unwrap(),
        &exporter,
        &token.invitation_id,
        &token.office_id,
        &tls.public_key(),
    )
    .unwrap();
    let accepted = ok(hq.register_satellite_join_request(JoinRequestParams {
        invitation_id: token.invitation_id.clone(),
        request_id: "req-search".into(),
        office_id: token.office_id.clone(),
        joining_pub_key: tls.public_key(),
        pop_signature_der: pop,
        exporter_binding: exporter,
        exporter_proof_der: proof,
    }));
    let JoinRequestAccepted::Pending { check_code, .. } = accepted else {
        panic!("a pending join request");
    };
    let approved = ok(hq.approve_satellite_invitation(&owner, &token.invitation_id, &check_code));
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let roots = DataRoots {
        durable: root.join("data").join(&org_id).join("durable"),
        runtime: root.join("data").join(&org_id).join("runtime"),
    };
    std::fs::create_dir_all(&roots.durable).unwrap();
    std::fs::create_dir_all(&roots.runtime).unwrap();
    let id = |text: &str| Id::parse(text).unwrap();
    let custody = Custody {
        identity: OfficeIdentity {
            instance_id: id("0195a000-0000-7000-8000-0000000000f0"),
            organization_id: id(&org_id),
            office_id: id(&token.office_id),
            journal_incarnation: id("0195a000-0000-7000-8000-0000000000f1"),
            writer_epoch: 1,
            machine_id: id("0195a000-0000-7000-8000-0000000000f2"),
        },
        authority: std::sync::Arc::new(FakeCustody::new()),
    };
    let sat = Office::provision_satellite_with(
        root.join("home/offices").join(&token.office_id),
        SatelliteProvisionSpec {
            new: crate::model::NewOffice {
                office_id: token.office_id.clone(),
                office_name: "Lisbon Studio".into(),
                role: crate::model::OfficeRole::Satellite,
                org_id: org_id.clone(),
                org_name: hq.info().org_name.clone(),
            },
            operation_id: "0195a000-0000-7000-8000-0000000000f3".into(),
            hq_office_id: hq.info().office_id.clone(),
            hq_addresses: token.hq_addresses.clone(),
            epoch: approved.epoch,
            ca_cert_der: approved.ca_cert_der.clone(),
            office_cert_der: approved.cert_der.clone(),
            office_key_der: Zeroizing::new(tls.local_der().to_vec()),
        },
        crate::Key::from_bytes([0x33; 32]),
        roots.clone(),
        custody,
        Options {
            engine: EngineChoice::Sqlite,
            clock: std::sync::Arc::new(org_store::SystemClock),
            testing: Default::default(),
            before_final_checks: None,
        },
        true,
    )
    .unwrap();
    let files = hq.collect_authoritative_state_snapshot().unwrap();
    let batch = files.into_iter().map(|(p, b)| (p, Some(b))).collect();
    sat.apply_replicated_state_files("0195a000-0000-7000-8000-0000000000f4", batch)
        .unwrap();
    sat.record_replicated_hq_owner(&hq.info().office_id, &owner)
        .unwrap();
    (hq, sat, roots, dir)
}
