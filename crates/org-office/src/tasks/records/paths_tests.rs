use org_store::{DurablePath, Id};

use super::*;

fn id(n: u64) -> Id {
    Id::parse(&format!("01900000-0000-7000-8000-{n:012x}")).unwrap()
}

fn path(text: &str) -> DurablePath {
    DurablePath::parse(text).unwrap()
}

#[test]
fn a_project_task_folder_is_under_its_project() {
    let location = Location::Project(id(1));
    let task = id(2);
    let folder = format!("state/projects/{}/work/tasks/{}", id(1), id(2));
    assert_eq!(task_folder(&location, &task), folder);
    let md = task_md_path(&location, &task).unwrap();
    assert_eq!(md.as_str(), format!("{folder}/task.md"));
    let parsed = parse_path(&md).unwrap();
    assert_eq!(parsed.kind, Kind::Task(task.clone()));
    assert_eq!(
        parsed.dir,
        Some(TaskDir::new(location.clone(), task.clone()))
    );
    let comment = comment_path(&location, &task, &id(3)).unwrap();
    assert_eq!(comment.as_str(), format!("{folder}/comments/{}.md", id(3)));
    assert_eq!(kind_of(&comment), Some(Kind::Comment(task.clone(), id(3))));
    assert_eq!(
        kind_of(&checklist_path(&location, &task).unwrap()),
        Some(Kind::Checklist(task.clone()))
    );
    assert_eq!(
        kind_of(&operations_path(&location, &task).unwrap()),
        Some(Kind::Operations(task))
    );
}

#[test]
fn a_projectless_task_folder_is_under_its_owning_team_chain() {
    let chain = vec![id(10), id(11), id(12)];
    let location = Location::Team(chain);
    let md = task_md_path(&location, &id(2)).unwrap();
    assert_eq!(
        md.as_str(),
        format!(
            "state/teams/{}/teams/{}/teams/{}/work/tasks/{}/task.md",
            id(10),
            id(11),
            id(12),
            id(2)
        )
    );
    let dir = task_dir_of(&md).unwrap();
    assert_eq!(dir.location, location);
    assert_eq!(dir.folder(), task_folder(&location, &id(2)));
}

#[test]
fn history_is_outside_the_task_folder_with_a_decimal_revision() {
    let event = event_path(&id(2), 12);
    assert_eq!(
        event.as_str(),
        format!("state/work-history/tasks/{}/12.json", id(2))
    );
    let parsed = parse_path(&event).unwrap();
    assert_eq!(parsed.kind, Kind::Event(id(2), 12));
    assert_eq!(parsed.dir, None);
    assert!(event.is_under(&history_folder(&id(2))));
}

#[test]
fn the_counter_is_office_local_and_not_a_state_file() {
    let counter = counter_path(&id(7));
    assert_eq!(
        counter.as_str(),
        format!("local/offices/{}/task-counter.yaml", id(7))
    );
    assert!(is_counter_path(&counter, &id(7)));
    assert!(!is_counter_path(&counter, &id(8)));
    assert_eq!(kind_of(&counter), None);
}

#[test]
fn paths_of_no_task_file_and_legacy_paths_are_not_the_familys() {
    let t = id(2);
    let team = id(10);
    for text in [
        // The flat pre-v1 layout has no reader (`doc-77`).
        format!("state/work/tasks/{t}/task.json"),
        format!("state/work/tasks/{t}/events/0000000001.json"),
        "state/work/office-task-counters/a.json".to_owned(),
        // Not a task file, or not where one is.
        format!("state/teams/{team}/work/tasks/{t}/task.json"),
        format!("state/teams/{team}/work/tasks/{t}/notes.md"),
        format!("state/teams/{team}/work/tasks/not-an-id/task.md"),
        format!("state/teams/{team}/work/tasks/{t}/comments/x.md"),
        format!("state/teams/{team}/team.yaml"),
        format!("state/teams/{team}/agents/{t}/execution.md"),
        format!("state/projects/{team}/project.yaml"),
        format!("state/work/tasks/{t}/task.md"),
        format!("state/teams/work/tasks/{t}/task.md"),
        // History file names are canonical decimals from 1.
        format!("state/work-history/tasks/{t}/0.json"),
        format!("state/work-history/tasks/{t}/01.json"),
        format!("state/work-history/tasks/{t}/0000000001.json"),
        format!("state/work-history/tasks/{t}/1.yaml"),
        format!("state/work-history/tasks/{t}/{}.json", u64::MAX),
    ] {
        assert_eq!(kind_of(&path(&text)), None, "{text}");
    }
}

#[test]
fn every_family_other_than_tasks_disowns_task_paths() {
    let location = Location::Team(vec![id(10), id(11)]);
    let t = id(2);
    for file in [
        task_md_path(&location, &t).unwrap(),
        comment_path(&location, &t, &id(3)).unwrap(),
        checklist_path(&location, &t).unwrap(),
        operations_path(&location, &t).unwrap(),
        task_md_path(&Location::Project(id(5)), &t).unwrap(),
        event_path(&t, 1),
    ] {
        assert!(
            crate::organization::records::kind_of(&file).is_none(),
            "{file}"
        );
        assert!(crate::projects::records::kind_of(&file).is_none(), "{file}");
        assert!(
            crate::processes::records::parse_path(&file).is_none(),
            "{file}"
        );
        assert!(
            crate::messaging::records::kind_of(&file).is_none(),
            "{file}"
        );
    }
}

#[test]
fn the_deepest_team_chain_holds_every_task_file() {
    let chain: Vec<Id> = (0..crate::organization::ports::MAX_TEAM_DEPTH as u64)
        .map(|n| id(0x100 + n))
        .collect();
    let location = Location::Team(chain);
    let comment = comment_path(&location, &id(2), &id(3)).unwrap();
    assert_eq!(comment.as_str().len(), 790);
    // Over 200 bytes are left for anything longer the contract adds.
    assert!(comment.as_str().len() + 200 <= DurablePath::MAX_BYTES);
    assert_eq!(kind_of(&comment), Some(Kind::Comment(id(2), id(3))));
    for file in [
        task_md_path(&location, &id(2)),
        checklist_path(&location, &id(2)),
        operations_path(&location, &id(2)),
    ] {
        assert!(file.is_ok());
    }
}
