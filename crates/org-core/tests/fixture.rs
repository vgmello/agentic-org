//! Loads the synthetic fixture workspace (tests/fixtures/workspace) and checks
//! every part of the model, then exercises `refresh` on a temp copy.

mod common;

use std::fs;

use common::{fixture, TempDir};
use org_core::{
    AgentStatus, BacklogItem, Error, FileKind, ProblemKind, Severity, TaskStatus, Tools, Workspace,
    WorkspaceModel,
};

const IVO: &str = "shop-backend-engineer-ivo";
const NOA: &str = "shop-designer-noa";
const OTTO: &str = "shop-backend-engineer-otto";
const KAI: &str = "blog-writer-kai";
const IVO_BACKLOG: &str = "projects/shop/agents/shop-backend-engineer-ivo/tasks/BACKLOG.md";

fn load() -> WorkspaceModel {
    Workspace::load(fixture()).expect("fixture loads")
}

fn problem_summary(model: &WorkspaceModel) -> Vec<(String, ProblemKind, Severity)> {
    model
        .problems
        .iter()
        .map(|p| (p.path.clone(), p.kind, p.severity))
        .collect()
}

#[test]
fn projects_and_briefs() {
    let model = load();
    assert_eq!(model.projects.keys().collect::<Vec<_>>(), ["blog", "shop"]);

    let shop = model.project("shop").unwrap();
    assert_eq!(shop.dir, "projects/shop");
    let profile = shop.profile.as_ref().unwrap();
    assert_eq!(profile.key.as_deref(), Some("shop"));
    assert_eq!(profile.name.as_deref(), Some("Shop"));
    assert_eq!(profile.created_on.as_deref(), Some("2026-01-02"));
    assert_eq!(profile.home.as_deref(), Some("projects/shop"));

    let brief = shop.brief.as_ref().unwrap();
    assert_eq!(brief.title.as_deref(), Some("Shop"));
    assert_eq!(
        brief.goals.as_deref(),
        Some("- Take the first order by March.")
    );
    assert!(brief
        .context
        .as_deref()
        .unwrap()
        .ends_with("- Payments go through a sandbox provider."));
    let team = brief.team.as_ref().unwrap();
    let members: Vec<_> = team
        .members
        .iter()
        .map(|m| (m.agent.as_str(), m.title.as_str()))
        .collect();
    assert_eq!(
        members,
        [(IVO, "Senior Backend Engineer"), (NOA, "Designer")]
    );
    assert_eq!(team.members[0].focus.as_deref(), Some("Checkout API"));
    assert_eq!(team.members[1].focus, None);
    assert_eq!(team.members[1].reports_to, IVO);
    assert!(team.unreadable.is_empty());

    let blog = model.project("blog").unwrap();
    assert_eq!(blog.profile.as_ref().unwrap().description, None);
    let blog_brief = blog.brief.as_ref().unwrap();
    assert_eq!(blog_brief.goals, None, "an empty Goals section is None");
    assert!(blog_brief.team.as_ref().unwrap().members.is_empty());
}

#[test]
fn agents_including_offboarded_and_garbled() {
    let model = load();
    let shop = model.project("shop").unwrap();
    assert_eq!(
        shop.agents.keys().map(String::as_str).collect::<Vec<_>>(),
        [IVO, OTTO, NOA]
    );

    let ivo = shop.agents[IVO].profile.as_ref().unwrap();
    assert!(ivo.is_active());
    assert_eq!(ivo.title.as_deref(), Some("Senior Backend Engineer"));
    assert_eq!(ivo.tools, Some(Tools::Inherit));
    assert_eq!(ivo.extra["timezone"], "UTC");

    let noa = shop.agents[NOA].profile.as_ref().unwrap();
    assert_eq!(noa.focus, None);
    assert_eq!(noa.reports_to.as_deref(), Some(IVO));
    assert!(matches!(&noa.tools, Some(Tools::List(t)) if t.len() == 5));
    assert!(shop.agents[NOA].backlog.is_none(), "Noa has no BACKLOG.md");

    let otto = shop.agents[OTTO].profile.as_ref().unwrap();
    assert_eq!(otto.status, Some(AgentStatus::Offboarded));
    assert!(!otto.is_active());
    assert_eq!(otto.offboarded_on.as_deref(), Some("2026-01-20"));
    assert_eq!(
        shop.agents[OTTO].backlog.as_ref().unwrap().tasks().count(),
        2
    );

    let kai = model.agent(KAI).unwrap();
    assert!(kai.profile.is_none(), "garbled agent.json");
    assert_eq!(
        kai.backlog.as_ref().unwrap().tasks().count(),
        1,
        "the backlog still loads"
    );
}

#[test]
fn backlog_rows() {
    let model = load();
    let backlog = model.agent(IVO).unwrap().backlog.as_ref().unwrap();
    assert_eq!(backlog.owner.as_deref(), Some("Ivo"));
    assert_eq!(backlog.items.len(), 8);

    let tasks: Vec<_> = backlog.tasks().collect();
    let summary: Vec<_> = tasks
        .iter()
        .map(|t| (t.number.unwrap(), t.status.clone()))
        .collect();
    assert_eq!(
        summary,
        [
            (1, TaskStatus::Done),
            (2, TaskStatus::InProgress),
            (3, TaskStatus::Todo),
            (5, TaskStatus::Blocked),
            (6, TaskStatus::Other("parked".into())),
            (7, TaskStatus::Todo),
            (8, TaskStatus::Done),
        ]
    );
    assert_eq!(tasks[1].status_raw, "in-progress (awaiting review)");
    assert_eq!(tasks[1].requested_by, NOA);
    assert_eq!(tasks[1].notes, "PR open");
    assert_eq!(tasks[3].status_raw, "blocked: waiting for sandbox keys");
    assert_eq!(tasks[5].task, "Support `a | b` filters");
    assert_eq!(
        tasks[6].requested_by, "user. Shipped in 1.2",
        "row with a missing Notes cell"
    );
    assert_eq!(tasks[6].notes, "");
    assert_eq!(tasks[6].warnings.len(), 1);
    assert!(tasks[..6].iter().all(|t| t.warnings.is_empty()));
    assert_eq!(tasks[0].line_no, 7);

    match &backlog.items[3] {
        BacklogItem::Unreadable {
            line_no,
            raw,
            reason,
        } => {
            assert_eq!(*line_no, 10);
            assert_eq!(raw, "| 4 | a row an agent cut short");
            assert!(!reason.is_empty());
        }
        other => panic!("row 4 should be unreadable, got {other:?}"),
    }
}

#[test]
fn roles_and_hr() {
    let model = load();
    assert_eq!(
        model.roles.keys().collect::<Vec<_>>(),
        ["backend-engineer", "designer"],
        "no _template, no broken"
    );
    let backend = &model.roles["backend-engineer"];
    assert_eq!(backend.title.as_deref(), Some("Backend Engineer"));
    assert_eq!(backend.aliases, ["backend", "api engineer"]);
    assert_eq!(
        backend.mission.as_deref(),
        Some("Ship reliable services for the shop.")
    );
    assert_eq!(backend.tools, None);
    assert_eq!(backend.extra["salary_band"], "B2");
    let designer = &model.roles["designer"];
    assert_eq!(designer.model, "sonnet");
    assert_eq!(
        designer.directories["knowledge/research"],
        "User research notes"
    );

    let hr = model.hr.as_ref().unwrap();
    assert_eq!(hr.path, ".claude/agents/hr.md");
    assert_eq!(hr.name.as_deref(), Some("hr"));
    assert_eq!(
        hr.description.as_deref(),
        Some("HR agent for the fixture workspace. Creates projects and \"hires\" agents.")
    );
    assert_eq!(hr.tools.as_ref().unwrap().len(), 4);
}

#[test]
fn problems_are_per_file_and_never_fatal() {
    let model = load();
    assert_eq!(
        problem_summary(&model),
        [
            (
                "hr/roles/backend-engineer.toml".into(),
                ProblemKind::InvalidField,
                Severity::Warning
            ),
            (
                "hr/roles/broken.toml".into(),
                ProblemKind::Malformed,
                Severity::Error
            ),
            (
                "projects/blog/agents/blog-writer-kai/agent.json".into(),
                ProblemKind::Malformed,
                Severity::Error
            ),
            (
                IVO_BACKLOG.into(),
                ProblemKind::UnreadableRows,
                Severity::Warning
            ),
            (
                "projects/shop/agents/shop-designer-noa/tasks/BACKLOG.md".into(),
                ProblemKind::Missing,
                Severity::Warning
            ),
        ]
    );
    assert!(model
        .problems_for(IVO_BACKLOG)
        .next()
        .unwrap()
        .message
        .contains("line 10"));
}

#[test]
fn serializes_to_json() {
    let model = load();
    let json = serde_json::to_value(&model).unwrap();
    let items = &json["projects"]["shop"]["agents"][IVO]["backlog"]["items"];
    assert_eq!(items[1]["kind"], "task");
    assert_eq!(items[1]["status"], "in-progress");
    assert_eq!(items[3]["kind"], "unreadable");
    assert_eq!(items[3]["line_no"], 10);
    assert_eq!(
        json["projects"]["shop"]["agents"][OTTO]["profile"]["status"],
        "offboarded"
    );
    assert_eq!(
        json["projects"]["shop"]["agents"][IVO]["profile"]["tools"],
        "inherit"
    );
    assert_eq!(json["problems"][1]["kind"], "malformed");
}

#[test]
fn per_file_functions_ignore_names_that_are_not_one_plain_segment() {
    let ws = Workspace::open(fixture()).unwrap();
    for bad in ["", "..", ".hidden", "../shop", "shop/agents", "a\\b"] {
        assert_eq!(ws.load_project(bad), (None, vec![]), "{bad:?}");
        assert_eq!(ws.load_agent("shop", bad), (None, vec![]), "{bad:?}");
        assert_eq!(ws.read_role(bad), (None, vec![]), "{bad:?}");
    }
    assert!(ws.load_project("shop").0.is_some());
}

#[test]
fn bad_roots_are_errors() {
    assert!(matches!(
        Workspace::load("/nonexistent/org-core/root"),
        Err(Error::RootNotFound(_))
    ));
    let file = fixture().join("projects/shop/project.json");
    assert!(matches!(
        Workspace::load(file),
        Err(Error::RootNotDirectory(_))
    ));
    // An empty folder is a workspace with nothing in it, not an error.
    let empty = TempDir::new("empty");
    let model = Workspace::load(empty.path()).unwrap();
    assert!(model.projects.is_empty() && model.roles.is_empty() && model.hr.is_none());
    assert_eq!(model.problems.len(), 3, "{:?}", model.problems);
}

#[test]
fn refresh_matches_a_full_reload_after_each_change() {
    let ws = TempDir::with_fixture("refresh");
    let mut model = Workspace::load(ws.path()).unwrap();
    let check = |model: &WorkspaceModel| {
        let fresh = Workspace::load(ws.path()).unwrap();
        assert_eq!(model, &fresh, "refresh diverged from a full reload");
    };

    // Fix the cut-short row.
    ws.write(
        IVO_BACKLOG,
        fs::read_to_string(fixture().join(IVO_BACKLOG))
            .unwrap()
            .replace(
                "| 4 | a row an agent cut short\n",
                "| 4 | a row an agent cut short | todo | user | |\n",
            ),
    );
    assert_eq!(
        model.refresh(IVO_BACKLOG),
        Some(FileKind::Backlog {
            project: "shop".into(),
            agent: IVO.into()
        })
    );
    assert!(model
        .agent(IVO)
        .unwrap()
        .backlog
        .as_ref()
        .unwrap()
        .unreadable()
        .next()
        .is_none());
    assert_eq!(model.problems_for(IVO_BACKLOG).count(), 0);
    check(&model);

    // Noa gets a backlog: the folder event, then the file event.
    ws.write(
        "projects/shop/agents/shop-designer-noa/tasks/BACKLOG.md",
        "| # | Task | Status |\n|-|-|-|\n| 1 | Wireframes | todo |\n",
    );
    model.refresh("projects/shop/agents/shop-designer-noa/tasks");
    model.refresh("projects/shop/agents/shop-designer-noa/tasks/BACKLOG.md");
    assert_eq!(
        model
            .agent(NOA)
            .unwrap()
            .backlog
            .as_ref()
            .unwrap()
            .tasks()
            .count(),
        1
    );
    check(&model);

    // Kai's agent.json is repaired.
    let kai_json = "projects/blog/agents/blog-writer-kai/agent.json";
    ws.write(kai_json, r#"{"id":"blog-writer-kai","name":"Kai","project":"blog","role":"writer","title":"Writer","status":"active","home":"projects/blog/agents/blog-writer-kai"}"#);
    model.refresh(kai_json);
    assert_eq!(
        model
            .agent(KAI)
            .unwrap()
            .profile
            .as_ref()
            .unwrap()
            .name
            .as_deref(),
        Some("Kai")
    );
    check(&model);

    // A new agent appears through its agent.json alone.
    let new_json = "projects/blog/agents/blog-editor-max/agent.json";
    ws.write(new_json, r#"{"id":"blog-editor-max","name":"Max","project":"blog","role":"editor","title":"Editor","status":"active","home":"projects/blog/agents/blog-editor-max"}"#);
    model.refresh(new_json);
    assert!(model.agent("blog-editor-max").is_some());
    check(&model);

    // Otto's folder is purged.
    fs::remove_dir_all(
        ws.path()
            .join("projects/shop/agents/shop-backend-engineer-otto"),
    )
    .unwrap();
    model.refresh("projects/shop/agents/shop-backend-engineer-otto/agent.json");
    assert!(model.agent(OTTO).is_none());
    check(&model);

    // The broken role is fixed, a role is deleted, the template changes.
    ws.write("hr/roles/broken.toml", "title = \"Fixed\"\ndepartment = \"Ops\"\nsummary = \"Use for ops.\"\nmission = \"Run it.\"\nresponsibilities = [\"Deploy\"]\n");
    model.refresh("hr/roles/broken.toml");
    fs::remove_file(ws.path().join("hr/roles/designer.toml")).unwrap();
    model.refresh("hr/roles/designer.toml");
    assert_eq!(model.refresh("hr/roles/_template.toml"), None);
    assert_eq!(
        model.roles.keys().collect::<Vec<_>>(),
        ["backend-engineer", "broken"]
    );
    check(&model);

    // HR's definition is deleted; the brief loses its roster markers.
    fs::remove_file(ws.path().join(".claude/agents/hr.md")).unwrap();
    model.refresh(".claude/agents/hr.md");
    assert!(model.hr.is_none());
    ws.write("projects/shop/CLAUDE.md", "# Shop\n\nNo roster here.\n");
    model.refresh("projects/shop/CLAUDE.md");
    assert_eq!(
        model.project("shop").unwrap().brief.as_ref().unwrap().team,
        None
    );
    check(&model);

    // A whole new project, then the blog project is removed.
    ws.write(
        "projects/docs/project.json",
        r#"{"key":"docs","name":"Docs"}"#,
    );
    model.refresh("projects/docs/project.json");
    fs::remove_dir_all(ws.path().join("projects/blog")).unwrap();
    model.refresh("projects/blog");
    assert_eq!(model.projects.keys().collect::<Vec<_>>(), ["docs", "shop"]);
    check(&model);

    // Stray files where folders are expected, and a folder named like a role.
    ws.write("projects/README.md", "# Projects\n");
    ws.write("projects/shop/agents/notes.txt", "notes\n");
    fs::create_dir_all(ws.path().join("hr/roles/odd.toml")).unwrap();
    model.refresh("projects/README.md");
    model.refresh("projects/shop/agents/notes.txt");
    model.refresh("hr/roles/odd.toml");
    assert!(model.project("README.md").is_none() && model.agent("notes.txt").is_none());
    check(&model);

    // A top-level folder disappears and comes back: the event for the folder
    // itself re-reads everything.
    fs::rename(ws.path().join("hr"), ws.path().join("hr-away")).unwrap();
    assert_eq!(model.refresh("hr"), Some(FileKind::Workspace));
    assert!(model.roles.is_empty());
    check(&model);
    fs::rename(ws.path().join("hr-away"), ws.path().join("hr")).unwrap();
    model.refresh("hr");
    assert_eq!(model.roles.len(), 2);
    check(&model);

    // Watchers report absolute paths under the canonical root.
    ws.write(
        IVO_BACKLOG,
        "| # | Task | Status |\n|-|-|-|\n| 1 | Only task | done |\n",
    );
    let absolute = model.root().join(IVO_BACKLOG);
    assert!(matches!(
        model.refresh(&absolute),
        Some(FileKind::Backlog { .. })
    ));
    assert_eq!(
        model
            .agent(IVO)
            .unwrap()
            .backlog
            .as_ref()
            .unwrap()
            .items
            .len(),
        1
    );
    check(&model);
    assert_eq!(
        model.refresh("/somewhere/else/projects/shop/project.json"),
        None
    );

    // Paths outside the model change nothing.
    let before = model.clone();
    assert_eq!(
        model.refresh("projects/shop/agents/shop-backend-engineer-ivo/journal/2026-01-05.md"),
        None
    );
    assert_eq!(model.refresh("../etc/passwd"), None);
    assert_eq!(model, before);
}
