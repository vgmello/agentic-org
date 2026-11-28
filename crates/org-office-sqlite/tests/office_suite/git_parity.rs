//! Dual-engine (`Memory` + `Sqlite`) project & task repository parity test (`TASK-255`).

use org_office::{rules::OrgCommand, EngineChoice, Office, TestOffice};
use org_proto::{
    git::RepositoryKindWire, org::ProjectCreateParams, tasks::TasksCreateParams, ErrorCode,
};
use org_store::Id;

fn run_parity_suite(engine: EngineChoice) {
    let office = Office::for_test(TestOffice { engine });
    let owner = office.owner_member_id().to_owned();
    let root_team = office.org_snapshot().unwrap().teams[0].id.clone();
    let uuid = |n: u64| format!("018bce83-1200-7000-8000-{n:012x}");

    for (idx, bad) in [
        "file:///tmp/repo.git",
        "http://example.com/repo.git",
        "ext::sh -c evil",
        "ssh://-oProxyCommand=evil/repo",
    ]
    .into_iter()
    .enumerate()
    {
        let err = office
            .register_repository(
                &owner,
                &uuid(100 + idx as u64),
                &uuid(200 + idx as u64),
                Some("bad".into()),
                RepositoryKindWire::Remote,
                Some(bad.into()),
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidRequest, "must refuse {bad}");
    }

    let repo_id = uuid(10);
    let repo = office
        .register_repository(
            &owner,
            &uuid(11),
            &repo_id,
            Some("backend".into()),
            RepositoryKindWire::Remote,
            Some("https://github.com/acme/backend.git".into()),
        )
        .unwrap()
        .unwrap();
    assert_eq!(repo.key.as_deref(), Some("backend"));

    let project_id = uuid(20);
    let proj_params: ProjectCreateParams = serde_json::from_value(serde_json::json!({
        "command_id": uuid(21),
        "project_id": project_id,
        "name": "Alpha",
        "slug": "alpha",
        "owner_team_id": root_team,
        "contributing_team_ids": [],
    }))
    .unwrap();
    office
        .submit_org(&owner, OrgCommand::ProjectCreate(proj_params))
        .unwrap()
        .unwrap();
    let link = office
        .attach_project_repository(&owner, &uuid(22), "alpha", "backend", "main".into())
        .unwrap()
        .unwrap();
    assert_eq!(link.integration_branch, "main");

    let task_id = uuid(30);
    let task = office
        .task_create(
            &owner,
            TasksCreateParams {
                command_id: uuid(31),
                task_id: task_id.clone(),
                team_id: Some(root_team),
                assigned_team_id: None,
                project_id: Some(project_id),
                task_type: Some("task".into()),
                work_type: None,
                title: "Git parity test".into(),
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

    office
        .record_task_repository_branch(
            &owner,
            &uuid(32),
            &Id::parse(&task.task.id).unwrap(),
            &Id::parse(&repo_id).unwrap(),
            "task/T-A1-git-parity-test".into(),
            None,
        )
        .unwrap()
        .unwrap();

    let ctx = office
        .task_git_context(&owner, &task.task.key)
        .unwrap()
        .unwrap();
    assert_eq!(ctx.task.repositories.len(), 1);
    assert_eq!(
        ctx.task.repositories[0].branch.as_deref(),
        Some("task/T-A1-git-parity-test")
    );
    assert_eq!(ctx.project.unwrap().repositories.len(), 1);
}

#[test]
fn memory_and_sqlite_project_and_task_repository_parity() {
    run_parity_suite(EngineChoice::Memory);
    run_parity_suite(EngineChoice::Sqlite(org_office_sqlite::sqlite_backend()));
}
