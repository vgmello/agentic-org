//! Reads a real workspace (read-only) when `ORG_CORE_REAL_WORKSPACE` points at
//! one, e.g.:
//!
//! ```text
//! ORG_CORE_REAL_WORKSPACE=/path/to/agents cargo test -p org-core --test real_workspace -- --nocapture
//! ```
//!
//! Skipped (with a note) when the variable isn't set.

use org_core::{BacklogItem, Workspace};

const AC_AGENTS: &[&str] = &[
    "ac-staff-engineer-ada",
    "ac-product-designer-eden",
    "ac-software-engineer-remy",
    "ac-software-engineer-riley",
];

#[test]
fn reads_the_real_workspace() {
    let Some(root) = std::env::var_os("ORG_CORE_REAL_WORKSPACE") else {
        println!("note: ORG_CORE_REAL_WORKSPACE isn't set; skipping the real-workspace test");
        return;
    };
    let model = Workspace::load(&root).expect("the real workspace loads");

    let ac = model.project("ac").expect("project ac exists");
    for id in AC_AGENTS {
        assert!(ac.agents.contains_key(*id), "agent {id} is missing");
    }
    let hr = model.hr.as_ref().expect("HR is present");
    assert_eq!(hr.name.as_deref(), Some("hr"));
    assert!(hr.description.as_deref().is_some_and(|d| !d.is_empty()));

    let mut tasks = 0;
    for agent in model.agents() {
        let backlog = agent
            .backlog
            .as_ref()
            .unwrap_or_else(|| panic!("{} has a readable backlog", agent.id));
        let unreadable: Vec<_> = backlog.unreadable().collect();
        assert!(
            unreadable.is_empty(),
            "{}: unreadable rows {unreadable:#?}",
            agent.id
        );
        tasks += backlog.tasks().count();
        assert!(agent.profile.is_some(), "{}: agent.json reads", agent.id);
    }

    println!(
        "real workspace {}: {} project(s), {} agent(s) ({} active), {} role(s), HR {:?}, {} task(s), {} problem(s)",
        model.root().display(),
        model.projects.len(),
        model.agents().count(),
        model.agents().filter(|a| a.profile.as_ref().is_some_and(|p| p.is_active())).count(),
        model.roles.len(),
        hr.name.as_deref().unwrap_or("?"),
        tasks,
        model.problems.len(),
    );
    for agent in model.agents() {
        let backlog = agent.backlog.as_ref().unwrap();
        let statuses: Vec<String> = backlog
            .items
            .iter()
            .filter_map(|item| match item {
                BacklogItem::Task(t) => Some(format!(
                    "#{}:{}",
                    t.number.map_or("?".into(), |n| n.to_string()),
                    t.status
                )),
                BacklogItem::Unreadable { .. } => None,
            })
            .collect();
        let warned = backlog.tasks().filter(|t| !t.warnings.is_empty()).count();
        println!(
            "  {}: {} [{}], {} row(s) with warnings",
            agent.id,
            statuses.len(),
            statuses.join(" "),
            warned
        );
    }
    for problem in &model.problems {
        println!(
            "  problem: {} [{:?}/{:?}] {}",
            problem.path, problem.severity, problem.kind, problem.message
        );
    }
}
