//! Adversarial security and regression tests for `file_guard` (`TASK-252` AC #1 & AC #3).

use std::fs;
use std::path::{Path, PathBuf};

use org_proto::ErrorCode;

use super::paths::{is_forbidden_root, is_under_host_user_home, path_is_within};
use super::roots::{
    expected_agent_tmpdir_slug, is_allowed_agent_home_path, is_allowed_session_tmpdir_path,
};
use super::worktrees::find_git_root;
use super::{prepare_output_path, read_upload_file, FileRoots, RootEntry};

fn make_workspace_roots(workspace: &Path) -> FileRoots {
    FileRoots {
        cwd: workspace.to_path_buf(),
        workspace_roots: vec![RootEntry::from_path(workspace.to_path_buf()).unwrap()],
        private_home: None,
        worktrees: Vec::new(),
        session_tmpdir: None,
        self_agent_id: Some("agent-self".to_owned()),
    }
}

#[test]
fn forged_git_worktree_without_bidirectional_back_pointer_or_outside_boundary_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let workspace_parent = dir.path().join("projects");
    let repo = workspace_parent.join("repo");
    let sibling_forged = workspace_parent.join("sibling-no-backptr");
    let sibling_wrong_backptr = workspace_parent.join("sibling-wrong-backptr");
    let sibling_symlink_git = workspace_parent.join("sibling-symlink-git");
    let sibling_valid = workspace_parent.join("sibling-valid");
    let outside_parent = dir.path().join("victim-docs");
    let outside_with_backptr = outside_parent.join("secrets");

    for d in [
        &repo,
        &sibling_forged,
        &sibling_wrong_backptr,
        &sibling_symlink_git,
        &sibling_valid,
        &outside_with_backptr,
    ] {
        fs::create_dir_all(d).unwrap();
    }

    let git_dir = repo.join(".git");
    for name in [
        "forged-oneway",
        "wrong-backptr",
        "symlink-dotgit",
        "outside-escape",
        "valid-wt",
        "not-dotgit-leaf",
    ] {
        fs::create_dir_all(git_dir.join("worktrees").join(name)).unwrap();
    }

    // 1. Forged one-way pointer: sibling dir has NO `.git` back-pointer file.
    fs::write(
        git_dir.join("worktrees/forged-oneway/gitdir"),
        format!("{}\n", sibling_forged.join(".git").display()),
    )
    .unwrap();

    // 2. Wrong back-pointer: `.git` points to a DIFFERENT worktree admin directory.
    fs::write(
        git_dir.join("worktrees/wrong-backptr/gitdir"),
        format!("{}\n", sibling_wrong_backptr.join(".git").display()),
    )
    .unwrap();
    fs::write(
        sibling_wrong_backptr.join(".git"),
        format!("gitdir: {}\n", git_dir.join("worktrees/valid-wt").display()),
    )
    .unwrap();

    // 3. Symlinked `.git` file inside candidate worktree must be rejected.
    fs::write(
        git_dir.join("worktrees/symlink-dotgit/gitdir"),
        format!("{}\n", sibling_symlink_git.join(".git").display()),
    )
    .unwrap();
    let real_ptr = dir.path().join("real-ptr");
    fs::write(
        &real_ptr,
        format!(
            "gitdir: {}\n",
            git_dir.join("worktrees/symlink-dotgit").display()
        ),
    )
    .unwrap();
    org_platform::fs::create_symlink(&real_ptr, &sibling_symlink_git.join(".git")).unwrap();

    // 4. Forged worktree outside `projects/` parent EVEN WITH a bidirectional `.git` back-pointer!
    fs::write(
        git_dir.join("worktrees/outside-escape/gitdir"),
        format!("{}\n", outside_with_backptr.join(".git").display()),
    )
    .unwrap();
    fs::write(
        outside_with_backptr.join(".git"),
        format!(
            "gitdir: {}\n",
            git_dir.join("worktrees/outside-escape").display()
        ),
    )
    .unwrap();

    // 5. Forged gitdir whose leaf is not `.git`.
    fs::write(
        git_dir.join("worktrees/not-dotgit-leaf/gitdir"),
        format!("{}\n", sibling_valid.join("README.md").display()),
    )
    .unwrap();

    // 6. Legitimate sibling worktree with valid bidirectional `.git` regular file.
    fs::write(
        git_dir.join("worktrees/valid-wt/gitdir"),
        format!("{}\n", sibling_valid.join(".git").display()),
    )
    .unwrap();
    fs::write(
        sibling_valid.join(".git"),
        format!("gitdir: {}\n", git_dir.join("worktrees/valid-wt").display()),
    )
    .unwrap();

    let roots = FileRoots::from_parts(repo.clone(), None, None, None);
    let wt_canons: Vec<PathBuf> = roots
        .worktrees
        .iter()
        .map(|e| e.canonical.clone())
        .collect();

    assert_eq!(
        wt_canons,
        vec![sibling_valid.canonicalize().unwrap()],
        "ONLY the verified bidirectional sibling worktree may be admitted"
    );

    // Legitimate worktree upload succeeds; forged outside worktree upload fails closed.
    fs::write(sibling_valid.join("patch.md"), b"# Patch\n").unwrap();
    fs::write(outside_with_backptr.join("secret.md"), b"secret").unwrap();
    fs::write(sibling_forged.join("secret.md"), b"secret").unwrap();

    assert!(read_upload_file(sibling_valid.join("patch.md").to_str().unwrap(), &roots).is_ok());
    assert_eq!(
        read_upload_file(
            outside_with_backptr.join("secret.md").to_str().unwrap(),
            &roots
        )
        .unwrap_err()
        .code,
        ErrorCode::NotAuthorized
    );
    assert_eq!(
        read_upload_file(sibling_forged.join("secret.md").to_str().unwrap(), &roots)
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
}

#[test]
fn forged_worktree_into_sensitive_dotdir_or_other_agent_home_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let office = dir.path().join("office");
    let self_wt_dir = office.join("teams/t1/agents/agent-self/worktrees");
    let repo = self_wt_dir.join("wt-1");
    let sibling_wt = self_wt_dir.join("wt-2");
    let dot_ssh = self_wt_dir.join(".ssh");
    let other_agent = office.join("teams/t1/agents/agent-other/worktrees/wt-x");

    for d in [&repo, &sibling_wt, &dot_ssh, &other_agent] {
        fs::create_dir_all(d).unwrap();
    }

    let git_dir = repo.join(".git");
    for (name, target) in [
        ("wt-2", &sibling_wt),
        ("ssh-wt", &dot_ssh),
        ("other-wt", &other_agent),
    ] {
        let admin = git_dir.join("worktrees").join(name);
        fs::create_dir_all(&admin).unwrap();
        fs::write(
            admin.join("gitdir"),
            format!("{}\n", target.join(".git").display()),
        )
        .unwrap();
        fs::write(
            target.join(".git"),
            format!("gitdir: {}\n", admin.display()),
        )
        .unwrap();
    }

    let roots = FileRoots::from_parts(repo, None, None, None);
    assert_eq!(roots.self_agent_id.as_deref(), Some("agent-self"));
    assert_eq!(
        roots
            .worktrees
            .iter()
            .map(|e| e.canonical.clone())
            .collect::<Vec<_>>(),
        vec![sibling_wt.canonicalize().unwrap()]
    );
}

#[test]
fn forbidden_roots_block_direct_user_homes_system_trees_and_global_temp_roots() {
    let forbidden = [
        "/",
        "/etc",
        "/etc/ssh",
        "/private/etc",
        "/var",
        "/private/var",
        "/var/folders",
        "/private/var/folders",
        "/var/folders/ab",
        "/var/folders/ab/cdefgh",
        "/var/folders/ab/cdefgh/T",
        "/var/folders/ab/cdefgh/T/",
        "/private/var/folders/ab/cdefgh/T",
        "/var/folders/aa/bb/C",
        "/var/folders/aa/bb/C/sub",
        "/var/folders/aa/bb/0",
        "/var/folders/aa/bb/0/sub",
        "/var/folders/aa/bb/X",
        "/private/var/folders/aa/bb/C",
        "/private/var/folders/aa/bb/C/sub",
        "/private/var/folders/aa/bb/0",
        "/private/var/folders/aa/bb/0/sub",
        "/var/root",
        "/var/root/Desktop",
        "/private/var/root",
        "/private/var/root/.ssh",
        "/tmp",
        "/private/tmp",
        "/var/tmp",
        "/private/var/tmp",
        "/run",
        "/run/user",
        "/run/user/1000",
        "/usr",
        "/usr/local",
        "/bin",
        "/sbin",
        "/lib",
        "/lib64",
        "/System",
        "/System/Library",
        "/Library",
        "/Library/Preferences",
        "/Applications",
        "/opt",
        "/Volumes",
        "/mnt",
        "/proc",
        "/sys",
        "/dev",
        "/root",
        "/Users",
        "/Users/alice",
        "/Users/alice/",
        "/home",
        "/home/alice",
        "/home/alice/",
    ];
    for path in forbidden {
        assert!(
            is_forbidden_root(Path::new(path)),
            "`{path}` must be a forbidden root"
        );
        assert!(
            RootEntry::from_path(PathBuf::from(path)).is_none(),
            "`{path}` must never construct a RootEntry"
        );
    }

    // Legitimate dedicated workspace & session-temp subdirectories are NOT forbidden roots.
    for allowed in [
        "/Users/alice/projects/my-repo",
        "/home/alice/projects/my-repo",
        "/tmp/agentic-org-session-123",
        "/var/folders/ab/cdefgh/T/agentic-org-session-123",
        "/run/user/1000/agentic-org/t/slug1",
        "/Users/alice/Library/Application Support/agentic-org/offices/o1/local/offices/o1/agents/a1/home",
    ] {
        assert!(
            !is_forbidden_root(Path::new(allowed)),
            "legitimate dedicated path `{allowed}` must not be a forbidden root"
        );
    }
}

#[test]
fn host_credential_dotdirs_and_office_state_files_are_unconditionally_denied_inside_allowed_root() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    let roots = make_workspace_roots(&workspace);

    // Every sensitive credential/state path MUST be rejected with NotAuthorized even inside an
    // allowed root AND even when carrying an allowlisted `.json` / `.txt` / `.md` extension.
    let blocked_paths = [
        ".codex/auth.json",
        ".CODEX/auth.json",
        ".gemini/oauth_creds.json",
        ".claude/.credentials.json",
        ".claude/settings.json",
        ".docker/config.json",
        ".ssh/id_rsa",
        ".ssh/config.txt",
        ".aws/credentials",
        ".aws/config.json",
        ".kube/config.json",
        ".gnupg/trust.txt",
        ".config/gh/hosts.json",
        ".netrc",
        ".npmrc",
        ".pypirc",
        ".git-credentials",
        "nested/host-secrets/key.json",
        "nested/git-credentials/token.txt",
        "office.db",
        "office.db-wal",
        "office.db-shm",
        "office.db.new",
        "office.db.bak",
        "host.db",
        "host.db-wal",
        "host.db-shm",
        "host.db.new",
        "host.db.bak",
        "git-state.json",
        "hq-owner.v1.json",
        "session-binding.v1",
        "session-binding-0193a5c0.v1",
        "sub/session-binding-run-42.v1",
        ".session-binding.foo.tmp",
        "sub/.session-binding.run-42.tmp",
        "agents/agent-other/worktrees/repo/a.txt",
        "agents/agent-other/workspace/a.txt",
        "agents/agent-other/tmp/a.txt",
    ];

    for rel in blocked_paths {
        let full = workspace.join(rel);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&full, b"{\"secret\":\"value\"}").unwrap();

        let read_err = read_upload_file(rel, &roots).unwrap_err();
        assert_eq!(
            read_err.code,
            ErrorCode::NotAuthorized,
            "upload of `{rel}` must fail with NotAuthorized, got {read_err:?}"
        );

        let fetch_err = prepare_output_path(rel, &roots).unwrap_err();
        assert_eq!(
            fetch_err.code,
            ErrorCode::NotAuthorized,
            "fetch output to `{rel}` must fail with NotAuthorized, got {fetch_err:?}"
        );
    }

    // Normal non-sensitive JSON/Markdown and own agent subdirs inside the same workspace remain
    // allowed.
    fs::write(workspace.join("report.json"), b"{\"ok\":true}").unwrap();
    assert!(read_upload_file("report.json", &roots).is_ok());
    fs::create_dir_all(workspace.join("agents/agent-self/worktrees/repo")).unwrap();
    fs::create_dir_all(workspace.join("agents/agent-self/tmp")).unwrap();
    fs::write(
        workspace.join("agents/agent-self/worktrees/repo/a.txt"),
        b"own wt",
    )
    .unwrap();
    fs::write(workspace.join("agents/agent-self/tmp/a.txt"), b"own tmp").unwrap();
    assert!(read_upload_file("agents/agent-self/worktrees/repo/a.txt", &roots).is_ok());
    assert!(read_upload_file("agents/agent-self/tmp/a.txt", &roots).is_ok());
}

#[test]
fn spoofing_agent_id_env_or_home_fails_closed_in_both_directions() {
    let dir = tempfile::tempdir().unwrap();
    let office_agents = dir.path().join("local/offices/off-1/agents");
    let agent_a_wt = office_agents.join("agent-a/worktrees/r");
    let agent_a_home = office_agents.join("agent-a/home");
    let agent_b_home = office_agents.join("agent-b/home");
    let agent_b_wt = office_agents.join("agent-b/worktrees/r");
    for d in [&agent_a_wt, &agent_a_home, &agent_b_home, &agent_b_wt] {
        fs::create_dir_all(d).unwrap();
    }
    fs::write(agent_a_wt.join("mine.txt"), b"my own note").unwrap();
    fs::write(agent_b_home.join("secret.txt"), b"b home secret").unwrap();
    fs::write(agent_b_wt.join("secret.txt"), b"b wt secret").unwrap();

    // Direction A: Honest `cwd` inside `agents/agent-a/worktrees/r` + spoofed
    // `AGENT_OS_AGENT_ID=agent-b` (and spoofed `HOME` at `agent-b/home`) -> `self_agent_id`
    // stays `agent-a`, `private_home` is `None`, and `agents/agent-b/` fails closed.
    let roots_cwd_wins = FileRoots::from_parts(
        agent_a_wt.clone(),
        Some("agent-b".to_owned()),
        Some(agent_b_home.clone()),
        None,
    );
    assert_eq!(
        roots_cwd_wins.self_agent_id.as_deref(),
        Some("agent-a"),
        "cwd-derived agent ID must win over spoofed AGENT_OS_AGENT_ID"
    );
    assert!(
        roots_cwd_wins.private_home.is_none(),
        "spoofed HOME pointing at another agent must never be admitted"
    );
    assert!(read_upload_file("mine.txt", &roots_cwd_wins).is_ok());
    for victim in [
        agent_b_home.join("secret.txt"),
        agent_b_wt.join("secret.txt"),
    ] {
        let err = read_upload_file(victim.to_str().unwrap(), &roots_cwd_wins).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotAuthorized, "{victim:?}: {err:?}");
    }

    // Direction B: Neutral `cwd` + honest `AGENT_OS_AGENT_ID=agent-real` + spoofed
    // `HOME=.../agents/agent-victim/home` -> FAIL CLOSED: `private_home` is `None`,
    // `self_agent_id` is `None`, and `agents/agent-victim/home` is REJECTED!
    let neutral_ws = dir.path().join("workspace");
    let victim_home = office_agents.join("agent-victim/home");
    let victim_nested = neutral_ws.join("agents/agent-victim/home");
    fs::create_dir_all(&victim_home).unwrap();
    fs::create_dir_all(&victim_nested).unwrap();
    fs::write(victim_home.join("secret.txt"), b"victim home secret").unwrap();
    fs::write(victim_nested.join("secret.txt"), b"victim nested secret").unwrap();

    let roots_spoofed_home = FileRoots::from_parts(
        neutral_ws.clone(),
        Some("agent-real".to_owned()),
        Some(victim_home.clone()),
        None,
    );
    assert!(
        roots_spoofed_home.private_home.is_none(),
        "disagreeing HOME must be rejected fail-closed"
    );
    assert!(
        roots_spoofed_home.self_agent_id.is_none(),
        "disagreeing HOME and AGENT_OS_AGENT_ID must fail closed with self_agent_id = None"
    );
    let err = read_upload_file(
        victim_home.join("secret.txt").to_str().unwrap(),
        &roots_spoofed_home,
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotAuthorized, "{err:?}");
    let err_nested =
        read_upload_file("agents/agent-victim/home/secret.txt", &roots_spoofed_home).unwrap_err();
    assert_eq!(err_nested.code, ErrorCode::NotAuthorized, "{err_nested:?}");

    // Reverse disagreement in neutral `cwd`: honest `HOME=agent-a/home` + spoofed
    // `AGENT_OS_AGENT_ID=agent-victim` ALSO fails closed (`None`/`None`).
    let roots_reverse = FileRoots::from_parts(
        neutral_ws,
        Some("agent-victim".to_owned()),
        Some(agent_a_home),
        None,
    );
    assert!(roots_reverse.self_agent_id.is_none());
    assert!(roots_reverse.private_home.is_none());
    let err_rev =
        read_upload_file("agents/agent-victim/home/secret.txt", &roots_reverse).unwrap_err();
    assert_eq!(err_rev.code, ErrorCode::NotAuthorized, "{err_rev:?}");
}

#[test]
fn ambient_home_without_authoritative_id_cannot_bootstrap_self_id_or_tmpdir() {
    let dir = tempfile::tempdir().unwrap();
    let neutral_ws = dir.path().join("workspace");
    let victim_home = dir.path().join("local/offices/o1/agents/victim/home");
    let victim_tmp = dir.path().join("local/offices/o1/agents/victim/tmp");
    for d in [&neutral_ws, &victim_home, &victim_tmp] {
        fs::create_dir_all(d).unwrap();
    }
    let secret = victim_tmp.join("secret.txt");
    fs::write(&secret, b"victim tmp secret").unwrap();

    // Neutral `cwd` + `AGENT_OS_AGENT_ID = None` + attacker-controlled `HOME` & `TMPDIR`
    // MUST NOT self-bootstrap `self_agent_id`, `private_home`, or `session_tmpdir`.
    let roots = FileRoots::from_parts(neutral_ws, None, Some(victim_home), Some(victim_tmp));
    assert!(roots.self_agent_id.is_none(), "{:?}", roots.self_agent_id);
    assert!(roots.private_home.is_none());
    assert!(roots.session_tmpdir.is_none());
    assert_eq!(
        read_upload_file(secret.to_str().unwrap(), &roots)
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
}

#[test]
fn two_hop_forged_commondir_rejected_while_legitimate_linked_worktree_discovers_sibling() {
    let dir = tempfile::tempdir().unwrap();
    let projects = dir.path().join("projects");
    let sandbox_repo = projects.join("sandbox-repo");
    let fake_admin = sandbox_repo.join("fake-admin");
    let secret_repo = projects.join("secret-repo");
    let secret_wt = projects.join("secret-wt");
    let secret_wt_admin = secret_repo.join(".git/worktrees/secret-wt");
    for d in [&fake_admin, &secret_repo, &secret_wt, &secret_wt_admin] {
        fs::create_dir_all(d).unwrap();
    }
    fs::write(secret_wt.join("secret.md"), b"top secret").unwrap();
    fs::write(
        secret_wt_admin.join("gitdir"),
        format!("{}\n", secret_wt.join(".git").display()),
    )
    .unwrap();
    fs::write(secret_wt_admin.join("commondir"), "../..\n").unwrap();
    fs::write(
        secret_wt.join(".git"),
        format!("gitdir: {}\n", secret_wt_admin.display()),
    )
    .unwrap();

    // Two-hop forgery strictly inside writable `sandbox-repo`:
    // `.git` -> `./fake-admin`, `fake-admin/gitdir` -> `<sandbox-repo>/.git`,
    // `fake-admin/commondir` -> `../../secret-repo/.git`.
    fs::write(sandbox_repo.join(".git"), "gitdir: ./fake-admin\n").unwrap();
    fs::write(
        fake_admin.join("gitdir"),
        format!("{}\n", sandbox_repo.join(".git").display()),
    )
    .unwrap();
    fs::write(fake_admin.join("commondir"), "../../secret-repo/.git\n").unwrap();

    let forged_roots = FileRoots::from_parts(sandbox_repo, None, None, None);
    assert!(
        forged_roots.worktrees.is_empty(),
        "two-hop forged commondir must not admit sibling repo worktrees: {:?}",
        forged_roots.worktrees
    );
    assert_eq!(
        read_upload_file(secret_wt.join("secret.md").to_str().unwrap(), &forged_roots)
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );

    // Positive control: genuine linked worktree `wt-a` still discovers sibling `wt-b`.
    let main_repo = projects.join("main-repo");
    let wt_a = projects.join("wt-a");
    let wt_b = projects.join("wt-b");
    for (wt_name, wt_dir) in [("wt-a", &wt_a), ("wt-b", &wt_b)] {
        let admin = main_repo.join(".git/worktrees").join(wt_name);
        fs::create_dir_all(&admin).unwrap();
        fs::create_dir_all(wt_dir).unwrap();
        fs::write(
            admin.join("gitdir"),
            format!("{}\n", wt_dir.join(".git").display()),
        )
        .unwrap();
        fs::write(admin.join("commondir"), "../..\n").unwrap();
        fs::write(
            wt_dir.join(".git"),
            format!("gitdir: {}\n", admin.display()),
        )
        .unwrap();
    }
    fs::write(wt_b.join("sibling.txt"), b"hello sibling").unwrap();

    let linked_roots = FileRoots::from_parts(wt_a, None, None, None);
    assert!(
        read_upload_file(wt_b.join("sibling.txt").to_str().unwrap(), &linked_roots).is_ok(),
        "legitimate linked worktree must still discover its sibling worktree"
    );
}

#[test]
fn host_home_and_root_tmpdir_env_are_strictly_allowlisted() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    let arbitrary_dir = dir.path().join("arbitrary-dir");
    fs::create_dir_all(&workspace).unwrap();
    fs::create_dir_all(&arbitrary_dir).unwrap();

    // Direct host HOME, arbitrary non-office HOME, and global TMPDIR roots are never admitted.
    for bad_home in [
        PathBuf::from("/root"),
        PathBuf::from("/home/alice"),
        PathBuf::from("/Users/alice"),
        PathBuf::from("/var/root"),
        PathBuf::from("/private/var/root"),
        arbitrary_dir,
    ] {
        let roots = FileRoots::from_parts(
            workspace.clone(),
            Some("agent-1".to_owned()),
            Some(bad_home.clone()),
            Some(PathBuf::from("/var/folders/ab/cd/T")),
        );
        assert!(roots.private_home.is_none(), "HOME={bad_home:?}");
        assert!(roots.session_tmpdir.is_none());
    }

    // Genuine office-scoped agent home and owner-only 0700 dedicated session temp dir ARE admitted.
    let agent_home = dir.path().join("local/offices/off-1/agents/agent-1/home");
    let agent_tmp = dir.path().join("local/offices/off-1/agents/agent-1/tmp");
    fs::create_dir_all(&agent_home).unwrap();
    org_platform::fs::create_owner_only_dir_all(&agent_tmp).unwrap();
    let ok_roots = FileRoots::from_parts(
        workspace,
        Some("agent-1".to_owned()),
        Some(agent_home.clone()),
        Some(agent_tmp.clone()),
    );
    assert_eq!(
        ok_roots.private_home.as_ref().map(|e| &e.canonical),
        Some(&agent_home.canonicalize().unwrap())
    );
    assert_eq!(
        ok_roots.session_tmpdir.as_ref().map(|e| &e.canonical),
        Some(&agent_tmp.canonicalize().unwrap())
    );

    // `is_under_host_user_home` includes macOS `/var/root` and `/private/var/root`.
    for root_home in [
        "/var/root",
        "/var/root/Documents",
        "/private/var/root",
        "/private/var/root/.ssh",
        "/root",
        "/Users/alice",
        "/home/alice",
    ] {
        assert!(
            is_under_host_user_home(Path::new(root_home)),
            "`{root_home}` must be recognized as under host user home"
        );
    }

    // Strict allowlist rejects broad host/office/mount/macOS-cache paths for BOTH HOME & TMPDIR.
    let rejected_both = [
        "/Users/alice/Library/Application Support/agentic-org",
        "/home/alice/.local/share/agentic-org",
        "/home/alice/.cache/agentic-org/tmp/ao/2oe/t",
        "/Users/t/Documents",
        "/Volumes/disk",
        "/mnt/share",
        "/opt/app",
        "/var/log",
        "/var/root",
        "/var/root/agents/agent-1/home",
        "/private/var/root",
        "/private/var/root/agents/agent-1/tmp",
        "/var/folders/aa/bb/C",
        "/var/folders/aa/bb/C/sub",
        "/var/folders/aa/bb/0/sub",
        "/private/var/folders/aa/bb/C/sub",
        "/var/folders/ab/cdefgh/T",
        "/Users/alice",
        "/Users/alice/Documents",
        "/home/alice",
        "/home/alice/Desktop",
        "/ao/t/slug",
        "/run/user/notuid/sub",
        "/Users/alice/Library/Application Support/agentic-org/local/offices/o1/agents/other/home",
        "/Users/alice/Library/Application Support/agentic-org/local/offices/o1/agents/other/tmp",
    ];
    for bad in rejected_both {
        assert!(
            !is_allowed_agent_home_path(Path::new(bad), "agent-1"),
            "HOME allowlist must reject `{bad}`"
        );
        assert!(
            !is_allowed_session_tmpdir_path(Path::new(bad), Some("agent-1"), Some("o1")),
            "TMPDIR allowlist must reject `{bad}`"
        );
    }

    // Additional HOME-only rejections (non-home agent subdirs).
    for bad_home in [
        "/Users/alice/Library/Application Support/agentic-org/local/offices/o1/agents/agent-1",
        "/Users/alice/Library/Application Support/agentic-org/local/offices/o1/agents/agent-1/worktrees/r",
        "/tmp/session-1",
    ] {
        assert!(
            !is_allowed_agent_home_path(Path::new(bad_home), "agent-1"),
            "HOME allowlist must reject `{bad_home}`"
        );
    }

    // Valid allowlisted HOME paths.
    for good_home in [
        "/Users/alice/Library/Application Support/agentic-org/local/offices/o1/agents/agent-1/home",
        "/home/alice/.local/share/agentic-org/local/offices/o1/agents/agent-1/private_home",
    ] {
        assert!(
            is_allowed_agent_home_path(Path::new(good_home), "agent-1"),
            "HOME allowlist must permit `{good_home}`"
        );
    }

    // Valid allowlisted TMPDIR paths (own agent tmp, own office runtime `t/<slug>`, OS temp subdirs).
    let own_slug = expected_agent_tmpdir_slug("o1", "agent-1");
    let cache_own = format!("/home/alice/.cache/agentic-org/tmp/ao/2oe/t/{own_slug}");
    let run_own = format!("/run/user/1000/ao/2oe/t/{own_slug}");
    for good_tmp in [
        "/Users/alice/Library/Application Support/agentic-org/local/offices/o1/agents/agent-1/tmp",
        "/Users/alice/Library/Application Support/agentic-org/local/offices/o1/agents/agent-1/tmp/run-1",
        cache_own.as_str(),
        "/tmp/agentic-org-session-1",
        "/private/tmp/agentic-org-session-1",
        "/var/tmp/agentic-org-session-1",
        "/private/var/tmp/agentic-org-session-1",
        "/var/folders/ab/cdefgh/T/agentic-org-session-1",
        "/private/var/folders/ab/cdefgh/T/agentic-org-session-1",
        run_own.as_str(),
    ] {
        assert!(
            is_allowed_session_tmpdir_path(Path::new(good_tmp), Some("agent-1"), Some("o1")),
            "TMPDIR allowlist must permit `{good_tmp}`"
        );
    }
}

#[test]
fn forged_dotgit_file_and_symlink_or_ancestor_escape_cannot_admit_secret_worktrees() {
    let dir = tempfile::tempdir().unwrap();
    let projects = dir.path().join("projects");
    let sandbox_repo = projects.join("sandbox-repo");
    let secret_repo = projects.join("secret-repo");
    let secret_wt = projects.join("secret-wt");
    let secret_wt_admin = secret_repo.join(".git/worktrees/secret-wt");
    for d in [&sandbox_repo, &secret_repo, &secret_wt, &secret_wt_admin] {
        fs::create_dir_all(d).unwrap();
    }
    fs::write(secret_wt.join("secret.md"), b"top secret").unwrap();
    fs::write(
        secret_wt_admin.join("gitdir"),
        format!("{}\n", secret_wt.join(".git").display()),
    )
    .unwrap();
    fs::write(secret_wt_admin.join("commondir"), "../..\n").unwrap();
    fs::write(
        secret_wt.join(".git"),
        format!("gitdir: {}\n", secret_wt_admin.display()),
    )
    .unwrap();

    // Attack 1: forged `sandbox-repo/.git` pointing at `../secret-repo/.git` (no back-pointer).
    fs::write(sandbox_repo.join(".git"), "gitdir: ../secret-repo/.git\n").unwrap();
    let roots_direct = FileRoots::from_parts(sandbox_repo.clone(), None, None, None);
    assert!(
        roots_direct.worktrees.is_empty(),
        "forged gitdir to sibling repo .git must not admit secret-wt: {:?}",
        roots_direct.worktrees
    );
    assert_eq!(
        read_upload_file(secret_wt.join("secret.md").to_str().unwrap(), &roots_direct)
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );

    // Attack 2: forged `sandbox-repo/.git` pointing at `../secret-repo/.git/worktrees/secret-wt`
    // (admin's `gitdir` points to `secret-wt/.git`, NOT `sandbox-repo/.git`).
    fs::write(
        sandbox_repo.join(".git"),
        "gitdir: ../secret-repo/.git/worktrees/secret-wt\n",
    )
    .unwrap();
    let roots_wt_admin = FileRoots::from_parts(sandbox_repo.clone(), None, None, None);
    assert!(
        roots_wt_admin.worktrees.is_empty(),
        "forged gitdir to another worktree's admin dir must not admit secret-wt: {:?}",
        roots_wt_admin.worktrees
    );

    // Attack 3: symlink `.git` inside `sandbox-repo` is ignored by `find_git_root`.
    fs::remove_file(sandbox_repo.join(".git")).unwrap();
    org_platform::fs::create_symlink(&secret_repo.join(".git"), &sandbox_repo.join(".git"))
        .unwrap();
    assert!(
        find_git_root(&sandbox_repo.canonicalize().unwrap()).is_none(),
        "symlink .git must never be followed by find_git_root"
    );

    // Attack 4: `find_git_root` stops at `agents/<id>` boundary and never climbs into an office
    // ancestor `.git`.
    let office = dir.path().join("local/offices/o1");
    let agent_plain_dir = office.join("agents/agent-a/worktrees/no-git");
    fs::create_dir_all(office.join(".git")).unwrap();
    fs::create_dir_all(&agent_plain_dir).unwrap();
    assert!(
        find_git_root(&agent_plain_dir.canonicalize().unwrap()).is_none(),
        "find_git_root must stop at agents/<id> boundary"
    );

    // Attack 5: symlinked `<common_dir>/worktrees/symlink-admin` is refused by
    // `open_regular_beneath`.
    let real_repo = projects.join("real-repo");
    let linked_target = projects.join("linked-wt");
    let external_admin = dir.path().join("external-admin");
    for d in [
        real_repo.join(".git/worktrees"),
        linked_target.clone(),
        external_admin.clone(),
    ] {
        fs::create_dir_all(d).unwrap();
    }
    fs::write(
        external_admin.join("gitdir"),
        format!("{}\n", linked_target.join(".git").display()),
    )
    .unwrap();
    fs::write(
        linked_target.join(".git"),
        format!("gitdir: {}\n", external_admin.display()),
    )
    .unwrap();
    org_platform::fs::create_symlink(
        &external_admin,
        &real_repo.join(".git/worktrees/symlink-admin"),
    )
    .unwrap();
    let roots_symlink_admin = FileRoots::from_parts(real_repo, None, None, None);
    assert!(
        roots_symlink_admin.worktrees.is_empty(),
        "symlinked worktree admin dir must be refused: {:?}",
        roots_symlink_admin.worktrees
    );
}

#[test]
fn path_is_within_requires_matching_absolute_and_relative_kinds() {
    assert!(!path_is_within(Path::new("foo/bar"), Path::new("/foo")));
    assert!(!path_is_within(Path::new("/foo/bar"), Path::new("foo")));
    assert!(path_is_within(Path::new("/foo/bar"), Path::new("/foo")));
    assert!(path_is_within(Path::new("foo/bar"), Path::new("foo")));
}

#[test]
fn host_registry_all_durable_families_restore_backups_and_cargo_credentials_are_denied() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    let roots = make_workspace_roots(&workspace);

    let denied = [
        // host-registry.* (DB, WAL, SHM, backups, sentinel, case variants)
        "host-registry.db",
        "host-registry.db-wal",
        "host-registry.db-shm",
        "host-registry.db.bak",
        "host-registry.db.before-restore",
        "host-registry.created",
        "office-state/HOST-REGISTRY.DB",
        // All 10 DURABLE_FAMILIES plus durable/protected and durable/local companion stores
        "durable/state/org.json",
        "durable/journal/0001.json",
        "durable/authority/policy.json",
        "durable/private/notes.json",
        "durable/messaging/events.json",
        "durable/budgets/limits.json",
        "durable/accounting/ledger.json",
        "durable/recovery/key.json",
        "durable/blobs/sha256/ab.txt",
        "durable/checkpoints/snap.json",
        "durable/protected/messaging.db",
        "durable/local/offices/o1/sessions/state.json",
        "durable/local/offices/o1/auth/vault.json",
        "durable/local/offices/o1/security/git-ssh-key",
        "durable/local/offices/o1/tls/federation.v1.json",
        "durable/local/offices/o1/agents/agent-other/home/secret.txt",
        "durable/local/stray.txt",
        // restore-backups anywhere in path
        "restore-backups/before-1/office.json",
        "nested/restore-backups/snap.txt",
        // Cargo registry credentials (both legacy and .toml, case-insensitive)
        ".cargo/credentials",
        ".cargo/credentials.toml",
        ".CARGO/CREDENTIALS.TOML",
        "nested/.cargo/credentials.toml",
    ];

    for rel in denied {
        let full = workspace.join(rel);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&full, b"{\"secret\":true}").unwrap();

        let read_err = read_upload_file(rel, &roots).unwrap_err();
        assert_eq!(
            read_err.code,
            ErrorCode::NotAuthorized,
            "read of `{rel}` must be denied: {read_err:?}"
        );
        let write_err = prepare_output_path(rel, &roots).unwrap_err();
        assert_eq!(
            write_err.code,
            ErrorCode::NotAuthorized,
            "write to `{rel}` must be denied: {write_err:?}"
        );
    }

    // Positive controls: non-sensitive `.cargo/config.toml` and own agent home under
    // `durable/local/offices/<o>/agents/<self>/...` remain permitted.
    let cargo_cfg = workspace.join(".cargo/config.toml");
    fs::write(&cargo_cfg, b"[build]\njobs = 1\n").unwrap();
    let own_durable_home =
        workspace.join("durable/local/offices/o1/agents/agent-self/home/note.txt");
    fs::create_dir_all(own_durable_home.parent().unwrap()).unwrap();
    fs::write(&own_durable_home, b"own note").unwrap();
    assert!(read_upload_file(
        "durable/local/offices/o1/agents/agent-self/home/note.txt",
        &roots
    )
    .is_ok());
    assert!(prepare_output_path(".cargo/config.toml", &roots).is_ok());
}

#[test]
fn tmpdir_admission_enforces_own_office_agent_slug_and_owner_only_0700_permissions() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();

    let office_id = "0195a000-0000-7000-8000-00000000a001";
    let self_agent = "0195a000-0000-7000-8000-00000000b001";
    let other_agent = "0195a000-0000-7000-8000-00000000b002";
    let own_slug = expected_agent_tmpdir_slug(office_id, self_agent);
    let sibling_slug = expected_agent_tmpdir_slug(office_id, other_agent);
    assert_ne!(own_slug, sibling_slug);

    // 1. Path-shape checks: sibling `t/<other-slug>`, bare `ao/.../t`, parent `ao/2oe`,
    //    and missing office/agent identity ALL fail closed (never falling through to OS temp).
    for rejected in [
        format!("/run/user/1000/ao/2oe/t/{sibling_slug}"),
        format!("/tmp/ao/2oe/t/{sibling_slug}"),
        format!("/var/folders/ab/cd/T/ao/2oe/t/{sibling_slug}"),
        format!("/home/alice/.cache/agentic-org/tmp/ao/2oe/t/{sibling_slug}"),
        "/run/user/1000/ao/2oe/t".to_owned(),
        "/run/user/1000/ao/2oe".to_owned(),
        "/tmp/ao/2oe".to_owned(),
    ] {
        assert!(
            !is_allowed_session_tmpdir_path(
                Path::new(&rejected),
                Some(self_agent),
                Some(office_id)
            ),
            "sibling or broad runtime TMPDIR `{rejected}` must be rejected"
        );
    }
    let own_run_path = format!("/run/user/1000/ao/2oe/t/{own_slug}");
    assert!(is_allowed_session_tmpdir_path(
        Path::new(&own_run_path),
        Some(self_agent),
        Some(office_id)
    ));
    assert!(
        !is_allowed_session_tmpdir_path(Path::new(&own_run_path), Some(self_agent), None),
        "t/<slug> without office_id must fail closed"
    );
    assert!(
        !is_allowed_session_tmpdir_path(Path::new(&own_run_path), None, Some(office_id)),
        "t/<slug> without self_agent_id must fail closed"
    );

    // 2. Real filesystem admission: 0700 own `t/<own-slug>` is admitted; 0700 sibling
    //    `t/<other-slug>` is rejected; non-0700 (e.g. 0755 under umask 022) is rejected.
    let own_t_dir = dir.path().join("ao/2oe/t").join(&own_slug);
    let sibling_t_dir = dir.path().join("ao/2oe/t").join(&sibling_slug);
    org_platform::fs::create_owner_only_dir_all(&own_t_dir).unwrap();
    org_platform::fs::create_owner_only_dir_all(&sibling_t_dir).unwrap();

    let admitted = FileRoots::from_parts_with_office(
        workspace.clone(),
        Some(self_agent.to_owned()),
        Some(office_id.to_owned()),
        None,
        Some(own_t_dir.clone()),
    );
    assert_eq!(
        admitted.session_tmpdir.as_ref().map(|e| &e.canonical),
        Some(&own_t_dir.canonicalize().unwrap())
    );

    let sibling_roots = FileRoots::from_parts_with_office(
        workspace.clone(),
        Some(self_agent.to_owned()),
        Some(office_id.to_owned()),
        None,
        Some(sibling_t_dir),
    );
    assert!(
        sibling_roots.session_tmpdir.is_none(),
        "sibling agent's 0700 t/<other-slug> must never be admitted"
    );

    // Symlink pointing at own 0700 `t/<own-slug>` is rejected.
    let link_to_own = dir.path().join("ao/2oe/t/symlink-tmp");
    org_platform::fs::create_symlink(&own_t_dir, &link_to_own).unwrap();
    let link_roots = FileRoots::from_parts_with_office(
        workspace.clone(),
        Some(self_agent.to_owned()),
        Some(office_id.to_owned()),
        None,
        Some(link_to_own),
    );
    assert!(link_roots.session_tmpdir.is_none());

    // Loose (non-0700) directory created via `fs::create_dir_all` under `umask 022` is rejected
    // for both OS temp subdirs and own `t/<own-slug>` directories.
    let loose_os_tmp = dir.path().join("loose-session-tmp");
    fs::create_dir_all(&loose_os_tmp).unwrap();
    let uid = org_platform::fs::effective_uid().unwrap();
    if org_platform::fs::verify_owner_only_dir(&loose_os_tmp, uid).is_err() {
        let loose_roots = FileRoots::from_parts_with_office(
            workspace.clone(),
            Some(self_agent.to_owned()),
            Some(office_id.to_owned()),
            None,
            Some(loose_os_tmp.clone()),
        );
        assert!(
            loose_roots.session_tmpdir.is_none(),
            "non-0700 TMPDIR must be rejected fail-closed"
        );
    }

    // Tightening `loose_os_tmp` to owner-only 0700 admits it.
    org_platform::fs::set_owner_only_dir(&loose_os_tmp).unwrap();
    let tight_roots = FileRoots::from_parts_with_office(
        workspace,
        Some(self_agent.to_owned()),
        Some(office_id.to_owned()),
        None,
        Some(loose_os_tmp.clone()),
    );
    assert_eq!(
        tight_roots.session_tmpdir.as_ref().map(|e| &e.canonical),
        Some(&loose_os_tmp.canonicalize().unwrap())
    );
}
