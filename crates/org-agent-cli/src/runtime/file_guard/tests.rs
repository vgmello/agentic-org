//! Adversarial security and regression tests for `file_guard` (`TASK-252` AC #1 & AC #3).

use std::fs;
use std::path::{Path, PathBuf};

use org_proto::ErrorCode;

use super::roots::{
    host_home_policy_allows_agent_home, host_home_policy_allows_tmpdir, is_forbidden_root,
};
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
        "host.db",
        "host.db-wal",
        "host.db-shm",
        "git-state.json",
        "hq-owner.v1.json",
        "session-binding-0193a5c0.v1",
        "sub/session-binding-run-42.v1",
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

    // Normal non-sensitive JSON/Markdown inside the same workspace remains allowed.
    fs::write(workspace.join("report.json"), b"{\"ok\":true}").unwrap();
    assert!(read_upload_file("report.json", &roots).is_ok());
}

#[test]
fn spoofing_agent_id_env_cannot_bypass_other_agent_home_isolation() {
    let dir = tempfile::tempdir().unwrap();
    let office_agents = dir.path().join("local/offices/off-1/agents");
    let self_home = office_agents.join("agent-real/home");
    let victim_home = office_agents.join("agent-victim/home");
    fs::create_dir_all(&self_home).unwrap();
    fs::create_dir_all(&victim_home).unwrap();
    fs::write(self_home.join("mine.txt"), b"my own note").unwrap();
    fs::write(victim_home.join("secret.txt"), b"victim secret").unwrap();

    // Attacker runs inside `agent-real`'s home but spoofs `AGENT_OS_AGENT_ID=agent-victim` and
    // points `HOME` at `agent-victim`'s home.
    let roots = FileRoots::from_parts(
        self_home.clone(),
        Some("agent-victim".to_owned()),
        Some(victim_home.clone()),
        None,
    );

    assert_eq!(
        roots.self_agent_id.as_deref(),
        Some("agent-real"),
        "cwd-derived agent ID must win over spoofed AGENT_OS_AGENT_ID"
    );
    assert!(
        roots.private_home.is_none(),
        "spoofed HOME pointing at another agent must never be admitted"
    );

    // Own note succeeds; victim's file fails closed with NotAuthorized.
    assert!(read_upload_file("mine.txt", &roots).is_ok());
    let err =
        read_upload_file(victim_home.join("secret.txt").to_str().unwrap(), &roots).unwrap_err();
    assert_eq!(err.code, ErrorCode::NotAuthorized, "{err:?}");

    // Even when `cwd` is a neutral workspace, if `HOME` is `agent-real`'s office home and
    // `AGENT_OS_AGENT_ID` spoofs `agent-victim`, `self_agent_id` resolves to `agent-real` and
    // `agent-victim` stays denied.
    let neutral_ws = dir.path().join("workspace");
    fs::create_dir_all(neutral_ws.join("agents/agent-victim/private_home")).unwrap();
    fs::write(
        neutral_ws.join("agents/agent-victim/private_home/secret.txt"),
        b"victim secret",
    )
    .unwrap();
    let roots_from_home = FileRoots::from_parts(
        neutral_ws,
        Some("agent-victim".to_owned()),
        Some(self_home),
        None,
    );
    assert_eq!(roots_from_home.self_agent_id.as_deref(), Some("agent-real"));
    let err = read_upload_file(
        "agents/agent-victim/private_home/secret.txt",
        &roots_from_home,
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotAuthorized, "{err:?}");
}

#[test]
fn host_home_and_root_tmpdir_env_are_never_admitted_as_roots() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();

    // Direct host HOME (e.g. `/root`, `/home/alice`, `/Users/alice`) and global TMPDIR roots
    // (`/tmp`, `/var/tmp`, `/var/folders/.../T`) are never admitted even with an agent ID.
    for bad_home in ["/root", "/home/alice", "/Users/alice"] {
        let roots = FileRoots::from_parts(
            workspace.clone(),
            Some("agent-1".to_owned()),
            Some(PathBuf::from(bad_home)),
            Some(PathBuf::from("/var/folders/ab/cd/T")),
        );
        assert!(roots.private_home.is_none(), "HOME={bad_home}");
        assert!(roots.session_tmpdir.is_none());
    }

    // A dedicated non-host-home office-scoped agent home IS admitted.
    let agent_home = dir.path().join("local/offices/off-1/agents/agent-1/home");
    let agent_tmp = dir.path().join("session-tmp-agent-1");
    fs::create_dir_all(&agent_home).unwrap();
    fs::create_dir_all(&agent_tmp).unwrap();
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

    // Arbitrary directories inside host user homes are refused for HOME and TMPDIR, whereas
    // genuine office-scoped agent homes and office-scoped temp dirs remain permitted.
    assert!(!host_home_policy_allows_agent_home(
        Path::new("/Users/alice"),
        "agent-1"
    ));
    assert!(!host_home_policy_allows_agent_home(
        Path::new("/Users/alice/Documents"),
        "agent-1"
    ));
    assert!(!host_home_policy_allows_agent_home(
        Path::new("/home/alice/Desktop"),
        "agent-1"
    ));
    assert!(!host_home_policy_allows_agent_home(
        Path::new("/Users/alice/Library/Application Support/agentic-org/local/offices/o1/agents/other/home"),
        "agent-1"
    ));
    assert!(host_home_policy_allows_agent_home(
        Path::new("/Users/alice/Library/Application Support/agentic-org/local/offices/o1/agents/agent-1/home"),
        "agent-1"
    ));
    assert!(!host_home_policy_allows_tmpdir(
        Path::new("/Users/alice/Documents"),
        Some("agent-1")
    ));
    assert!(!host_home_policy_allows_tmpdir(
        Path::new("/var/folders/ab/cdefgh/T"),
        Some("agent-1")
    ));
    assert!(host_home_policy_allows_tmpdir(
        Path::new("/home/alice/.cache/agentic-org/tmp/ao/2oe/t/slug"),
        Some("agent-1")
    ));
}
