//! TASK-155: setup's mode audit is strict on the host's and offices' records and checks the
//! trees agents and Git write at their own folder only.

use super::{mode_exceptions, written_by_agents_or_git};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const ORG: &str = "0193a5c0-0000-7000-8000-000000000001";
const OFFICE: &str = "0193a5c0-0000-7000-8000-000000000002";
const EMPLOYEE: &str = "0193a5c0-0000-7000-8000-000000000003";

fn dir(path: &Path, mode: u32) {
    std::fs::create_dir_all(path).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

fn file(path: &Path, mode: u32) {
    std::fs::write(path, b"x").unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// Every folder from `root` to `path` made private, then `path` with `mode`.
fn private_dirs(root: &Path, path: &Path, mode: u32) {
    let below = path.strip_prefix(root).unwrap();
    let mut at = root.to_path_buf();
    for part in below.components() {
        at.push(part);
        dir(&at, 0o700);
    }
    dir(path, mode);
}

struct Layout {
    _temp: tempfile::TempDir,
    root: PathBuf,
    durable: PathBuf,
    runtime: PathBuf,
}

impl Layout {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap().join("instance");
        dir(&root, 0o700);
        let durable = root.join(ORG).join("durable");
        let runtime = root.join(ORG).join("runtime");
        Self {
            _temp: temp,
            root,
            durable,
            runtime,
        }
    }

    fn office_local(&self) -> PathBuf {
        self.durable.join("local/offices").join(OFFICE)
    }

    fn agent(&self) -> PathBuf {
        self.office_local().join("agents").join(EMPLOYEE)
    }

    fn worktrees(&self) -> PathBuf {
        self.runtime
            .join(OFFICE)
            .join("teams/t1/agents")
            .join(EMPLOYEE)
            .join("tasks/k1/repositories")
    }
}

/// Normal work under a 700 agent home or Git tree (a UPG account's `0002` mask, a checkout's
/// executable bit, a cache) is no exception; loose modes on the host's and offices' records,
/// on an agent's runtime folder and on the boundary folders themselves still are.
#[test]
fn agent_homes_and_git_trees_are_checked_at_their_folder_only() {
    let layout = Layout::new();
    let root = &layout.root;

    // An agent's home: private folder, whatever its tools wrote inside.
    let home = layout.agent().join("home");
    private_dirs(root, &home, 0o700);
    dir(&home.join(".cargo/registry/src"), 0o775);
    file(&home.join(".cargo/registry/src/lib.rs"), 0o664);
    dir(&home.join("project/target/debug"), 0o775);
    file(&home.join("project/target/debug/tool"), 0o775);
    // The office's bare caches and a task worktree: private folders, Git's modes inside.
    let caches = layout.runtime.join(OFFICE).join("repositories");
    private_dirs(root, &caches, 0o700);
    dir(&caches.join("r1.git/objects/ab"), 0o775);
    file(&caches.join("r1.git/config"), 0o664);
    dir(&caches.join("r1.git/worktrees/wt1"), 0o775);
    let worktrees = layout.worktrees();
    private_dirs(root, &worktrees, 0o700);
    dir(&worktrees.join("r1/src"), 0o775);
    file(&worktrees.join("r1/src/main.rs"), 0o664);
    file(&worktrees.join("r1/run.sh"), 0o775);
    assert_eq!(mode_exceptions(root).unwrap(), (vec![], vec![]));

    // Security-critical records are still checked file by file.
    let security = layout.office_local().join("security");
    private_dirs(root, &security, 0o700);
    file(&security.join("ca-key.v1.json"), 0o644);
    let sessions = layout.office_local().join("sessions");
    private_dirs(root, &sessions, 0o755);
    let private = layout.durable.join("private");
    private_dirs(root, &private, 0o700);
    file(&private.join("task-notes.db"), 0o664);
    let agent_runtime = layout.agent().join("runtime");
    private_dirs(root, &agent_runtime, 0o700);
    file(&agent_runtime.join("state.json"), 0o644);
    // The office's record of where a repository lives is no Git tree.
    let origins = layout.office_local().join("repositories");
    private_dirs(root, &origins, 0o700);
    file(&origins.join("r1.json"), 0o644);
    let office_db = layout.runtime.join(OFFICE).join("office.db");
    file(&office_db, 0o664);
    // The boundary folders themselves are checked: an open home or cache folder is reported
    // (and what is inside is still not walked).
    dir(&home, 0o755);
    dir(&caches, 0o775);
    dir(&layout.agent(), 0o755);

    let (exceptions, unreadable) = mode_exceptions(root).unwrap();
    assert!(unreadable.is_empty(), "{unreadable:?}");
    let mut expected = vec![
        (security.join("ca-key.v1.json"), 0o644),
        (sessions.clone(), 0o755),
        (private.join("task-notes.db"), 0o664),
        (agent_runtime.join("state.json"), 0o644),
        (origins.join("r1.json"), 0o644),
        (office_db, 0o664),
        (home.clone(), 0o755),
        (caches.clone(), 0o775),
        (layout.agent(), 0o755),
    ];
    expected.sort();
    assert_eq!(exceptions, expected);
}

/// A boundary that is a symbolic link is reported and never followed, so an agent cannot
/// point its home elsewhere to hide or expose anything.
#[test]
fn a_linked_agent_home_is_reported_and_not_followed() {
    let layout = Layout::new();
    let outside = layout.root.parent().unwrap().join("outside");
    dir(&outside, 0o777);
    file(&outside.join("secret"), 0o666);
    let home = layout.agent().join("home");
    private_dirs(&layout.root, &layout.agent(), 0o700);
    std::os::unix::fs::symlink(&outside, &home).unwrap();
    let (exceptions, _) = mode_exceptions(&layout.root).unwrap();
    assert_eq!(exceptions.len(), 1, "{exceptions:?}");
    assert_eq!(exceptions[0].0, home);
}

#[test]
fn only_the_agent_and_git_trees_of_the_layout_are_boundaries() {
    let root = Path::new("/i");
    let o = format!("{ORG}/durable/local/offices/{OFFICE}");
    let r = format!("{ORG}/runtime/{OFFICE}");
    let s = format!("{ORG}/offices/{OFFICE}/runtime/{OFFICE}");
    for (path, expected) in [
        (format!("{o}/agents/{EMPLOYEE}/home"), true),
        (format!("{r}/repositories"), true),
        (format!("{s}/repositories"), true),
        (
            format!("{r}/teams/t/agents/{EMPLOYEE}/tasks/k/repositories"),
            true,
        ),
        (
            format!("{r}/teams/t/teams/u/agents/{EMPLOYEE}/projects/p/tasks/k/repositories"),
            true,
        ),
        // Not boundaries: the folders above, the office's and agents' own records, and the
        // same names anywhere else.
        (format!("{o}/agents/{EMPLOYEE}"), false),
        (format!("{o}/agents/{EMPLOYEE}/runtime"), false),
        (format!("{o}/agents/{EMPLOYEE}/home/.cargo"), false),
        (format!("{o}/repositories"), false),
        (
            format!("{o}/agents/{EMPLOYEE}/runtime/x/repositories"),
            false,
        ),
        (format!("{r}/teams/t/repositories"), false),
        (format!("{r}/teams/t/agents/{EMPLOYEE}/repositories"), false),
        (
            format!("{ORG}/durable/private/agents/{EMPLOYEE}/home"),
            false,
        ),
        (format!("{r}/protected"), false),
        ("repositories".into(), false),
    ] {
        assert_eq!(
            written_by_agents_or_git(&root.join(&path), root),
            expected,
            "{path}"
        );
    }
    assert!(!written_by_agents_or_git(
        Path::new("/elsewhere/runtime/o/repositories"),
        root
    ));
}
