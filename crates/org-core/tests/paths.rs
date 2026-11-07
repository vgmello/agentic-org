//! Path safety: nothing outside the workspace root is resolved or read, even
//! through a symlink.

mod common;

use std::fs;

use common::TempDir;
use org_core::{resolve_in_workspace, Error, ProblemKind, Workspace};

#[test]
fn plain_paths_resolve_inside_the_root() {
    let tmp = TempDir::new("paths-ok");
    tmp.write("root/projects/a/project.json", "{}");
    let root = tmp.path().join("root");
    let canonical_root = fs::canonicalize(&root).unwrap();

    let resolved = resolve_in_workspace(&root, "projects/a/project.json").unwrap();
    assert_eq!(resolved, canonical_root.join("projects/a/project.json"));
    assert_eq!(
        resolve_in_workspace(&root, "./projects/a").unwrap(),
        canonical_root.join("projects/a")
    );
    assert_eq!(resolve_in_workspace(&root, "").unwrap(), canonical_root);
    assert!(matches!(
        resolve_in_workspace(&root, "projects/missing"),
        Err(Error::NotFound(_))
    ));
}

#[test]
fn parent_and_absolute_paths_are_refused() {
    let tmp = TempDir::new("paths-dots");
    tmp.write("root/projects/a/project.json", "{}");
    tmp.write("secret.txt", "secret");
    let root = tmp.path().join("root");

    assert!(matches!(
        resolve_in_workspace(&root, "../secret.txt"),
        Err(Error::ParentComponent(_))
    ));
    // Even a `..` that would stay inside the root is refused.
    assert!(matches!(
        resolve_in_workspace(&root, "projects/../projects/a"),
        Err(Error::ParentComponent(_))
    ));
    let absolute = tmp.path().join("secret.txt");
    assert!(matches!(
        resolve_in_workspace(&root, &absolute),
        Err(Error::AbsolutePath(_))
    ));
}

#[cfg(unix)]
#[test]
fn symlinks_that_escape_are_refused() {
    use std::os::unix::fs::symlink;

    let tmp = TempDir::new("paths-symlink");
    tmp.write("root/projects/a/project.json", "{}");
    tmp.write("outside/secret.txt", "secret");
    let root = tmp.path().join("root");
    let outside = tmp.path().join("outside");

    symlink(outside.join("secret.txt"), root.join("escape.txt")).unwrap();
    symlink(&outside, root.join("escape-dir")).unwrap();
    symlink("../outside/secret.txt", root.join("relative-escape.txt")).unwrap();
    symlink(root.join("projects"), root.join("inner")).unwrap();

    for rel in [
        "escape.txt",
        "escape-dir",
        "escape-dir/secret.txt",
        "relative-escape.txt",
    ] {
        assert!(
            matches!(
                resolve_in_workspace(&root, rel),
                Err(Error::OutsideWorkspace(_))
            ),
            "{rel} must be refused"
        );
    }
    // A symlink that stays inside the workspace is fine.
    let canonical_root = fs::canonicalize(&root).unwrap();
    assert_eq!(
        resolve_in_workspace(&root, "inner/a/project.json").unwrap(),
        canonical_root.join("projects/a/project.json")
    );
    // The root itself may be reached through a symlink.
    let root_link = tmp.path().join("root-link");
    symlink(&root, &root_link).unwrap();
    assert!(resolve_in_workspace(&root_link, "projects/a/project.json").is_ok());
}

#[cfg(unix)]
#[test]
fn refresh_under_an_escaping_folder_matches_a_full_load() {
    use std::os::unix::fs::symlink;

    let tmp = TempDir::new("refresh-symlink");
    tmp.write(
        "outside/agents/evil-a/agent.json",
        r#"{"id":"evil-a","name":"SECRET"}"#,
    );
    tmp.write("outside-roles/x.toml", "title = \"SECRET\"\n");
    tmp.write(
        "root/projects/ok/project.json",
        r#"{"key":"ok","name":"Ok"}"#,
    );
    fs::create_dir_all(tmp.path().join("root/hr")).unwrap();
    let root = tmp.path().join("root");
    symlink(tmp.path().join("outside"), root.join("projects/evil")).unwrap();
    symlink(tmp.path().join("outside-roles"), root.join("hr/roles")).unwrap();

    let mut model = Workspace::load(&root).unwrap();
    model.refresh("projects/evil/agents/evil-a/agent.json");
    model.refresh("projects/evil/agents/evil-a/tasks/BACKLOG.md");
    model.refresh("hr/roles/x.toml");
    assert_eq!(model, Workspace::load(&root).unwrap());
    assert!(model.project("evil").unwrap().agents.is_empty());
    assert!(!serde_json::to_string(&model).unwrap().contains("SECRET"));
}

#[cfg(unix)]
#[test]
fn a_fifo_does_not_hang_the_loader() {
    let ws = TempDir::with_fixture("fifo");
    let fifo = ws
        .path()
        .join("projects/shop/agents/shop-designer-noa/tasks/BACKLOG.md");
    fs::create_dir_all(fifo.parent().unwrap()).unwrap();
    let status = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("mkfifo runs");
    assert!(status.success());

    let root = ws.path().to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(Workspace::load(root));
    });
    let model = rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the load returns instead of blocking on the FIFO")
        .unwrap();
    let problem = model
        .problems_for("projects/shop/agents/shop-designer-noa/tasks/BACKLOG.md")
        .next()
        .unwrap();
    assert_eq!(problem.kind, ProblemKind::Unreadable);
    assert!(
        problem.message.contains("regular file"),
        "{}",
        problem.message
    );
}

#[cfg(unix)]
#[test]
fn the_loader_never_reads_through_an_escaping_symlink() {
    use std::os::unix::fs::symlink;

    let tmp = TempDir::new("load-symlink");
    tmp.write(
        "outside/BACKLOG.md",
        "| # | Task | Status |\n|-|-|-|\n| 1 | TOP SECRET | todo |\n",
    );
    tmp.write(
        "outside/project.json",
        r#"{"key":"evil","name":"TOP SECRET"}"#,
    );
    tmp.write(
        "root/projects/ok/agents/ok-dev-a/agent.json",
        r#"{"id":"ok-dev-a"}"#,
    );
    let root = tmp.path().join("root");
    let outside = tmp.path().join("outside");
    fs::create_dir_all(root.join("projects/ok/agents/ok-dev-a/tasks")).unwrap();
    symlink(
        outside.join("BACKLOG.md"),
        root.join("projects/ok/agents/ok-dev-a/tasks/BACKLOG.md"),
    )
    .unwrap();
    symlink(&outside, root.join("projects/evil")).unwrap();

    let model = Workspace::load(&root).unwrap();
    let json = serde_json::to_string(&model).unwrap();
    assert!(
        !json.contains("TOP SECRET"),
        "content from outside the workspace leaked: {json}"
    );

    let agent = model.agent("ok-dev-a").unwrap();
    assert!(agent.backlog.is_none());
    let backlog_problem = model
        .problems_for("projects/ok/agents/ok-dev-a/tasks/BACKLOG.md")
        .next()
        .unwrap();
    assert_eq!(backlog_problem.kind, ProblemKind::Unreadable);
    assert!(backlog_problem.message.contains("outside the workspace"));

    let evil = model
        .project("evil")
        .expect("the project shows up, flagged");
    assert!(evil.profile.is_none() && evil.agents.is_empty());
    assert_eq!(
        model.problems_for("projects/evil").next().unwrap().kind,
        ProblemKind::Unreadable
    );
}
