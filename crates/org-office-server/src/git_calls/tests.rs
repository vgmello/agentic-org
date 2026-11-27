use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use crate::live::LiveEvents;
use org_git::{AllowedProtocol, GitCredential, GitError};
use org_office::{EngineChoice, Office, TestOffice};
use org_proto::git::{
    CacheStateWire, GitPushParams, RepoAttachParams, RepoRegisterParams, RepoStatusParams,
    RepositoryKindWire, WorktreeListParams, WorktreePrepareParams,
};
use org_proto::org::{MemberAddPersonParams, ProjectCreateParams, TeamCreateParams};
use org_proto::tasks::{TaskStatus, TasksCreateParams, TasksTransitionParams};
use org_proto::ErrorCode;

use super::credentials::{extract_remote_host, https_origin_scope, resolve_office_git_credential};
use super::redaction::{redact_git_error_for_log, scrub_git_log_text};
use super::*;

fn run_host_git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args(args)
        .output()
        .expect("spawn git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn seed_bare_origin(dir: &Path) -> PathBuf {
    let bare = dir.join("origin.git");
    std::fs::create_dir_all(&bare).unwrap();
    run_host_git(&bare, &["init", "--bare", "--initial-branch=main"]);
    let seed = dir.join("seed");
    std::fs::create_dir_all(&seed).unwrap();
    run_host_git(&seed, &["init", "--initial-branch=main"]);
    run_host_git(&seed, &["config", "user.name", "Seed"]);
    run_host_git(&seed, &["config", "user.email", "seed@example.test"]);
    std::fs::write(seed.join("README.md"), "# Seed\n").unwrap();
    run_host_git(&seed, &["add", "README.md"]);
    run_host_git(&seed, &["commit", "-m", "initial"]);
    run_host_git(&seed, &["remote", "add", "origin", bare.to_str().unwrap()]);
    run_host_git(&seed, &["push", "origin", "main"]);
    bare
}

#[test]
fn real_office_register_attach_prepare_commit_and_push_end_to_end() {
    let office = Arc::new(Office::for_test(TestOffice {
        engine: EngineChoice::Sqlite,
    }));
    let owner = office.owner_member_id().to_owned();
    let live = Arc::new(LiveEvents::for_owner(Box::new(|| {}), owner.clone()));
    let svc = GitService::new(Arc::clone(&office), live);

    let origin_tmp = tempfile::tempdir().unwrap();
    let bare_origin = seed_bare_origin(origin_tmp.path());

    let uuid = |n: u64| format!("018bce83-1200-7000-8000-{n:012x}");
    let root_team = office.org_snapshot().unwrap().teams[0].id.clone();
    let project_id = uuid(201);
    let team_id = uuid(202);
    let child_member_id = uuid(205);
    let outsider_team_id = uuid(206);
    let outsider_member_id = uuid(207);

    let _ = office
        .submit_org(
            &owner,
            org_office::rules::OrgCommand::TeamCreate(TeamCreateParams {
                command_id: uuid(200),
                team_id: team_id.clone(),
                name: "Platform".into(),
                owner_team_id: Some(root_team.clone()),
            }),
        )
        .unwrap()
        .unwrap();
    let _ = office
        .submit_org(
            &owner,
            org_office::rules::OrgCommand::TeamCreate(TeamCreateParams {
                command_id: uuid(208),
                team_id: outsider_team_id.clone(),
                name: "Design".into(),
                owner_team_id: Some(root_team.clone()),
            }),
        )
        .unwrap()
        .unwrap();
    let _ = office
        .submit_org(
            &owner,
            org_office::rules::OrgCommand::MemberAddPerson(MemberAddPersonParams {
                command_id: uuid(209),
                member_id: child_member_id.clone(),
                name: "Dev One".into(),
                handle: Some("dev1".into()),
                home_team_id: Some(team_id.clone()),
                contact: None,
            }),
        )
        .unwrap()
        .unwrap();
    let _ = office
        .submit_org(
            &owner,
            org_office::rules::OrgCommand::MemberAddPerson(MemberAddPersonParams {
                command_id: uuid(210),
                member_id: outsider_member_id.clone(),
                name: "Outsider".into(),
                handle: Some("out1".into()),
                home_team_id: Some(outsider_team_id.clone()),
                contact: None,
            }),
        )
        .unwrap()
        .unwrap();

    let proj_params: ProjectCreateParams = serde_json::from_value(serde_json::json!({
        "command_id": uuid(203),
        "project_id": project_id,
        "name": "Core",
        "slug": "core",
        "owner_team_id": team_id,
        "contributing_team_ids": [],
    }))
    .unwrap();
    let _ = office
        .submit_org(
            &owner,
            org_office::rules::OrgCommand::ProjectCreate(proj_params),
        )
        .unwrap()
        .unwrap();

    // Non-owner cannot register a repository (even probing nonexistent local path gets NotAuthorized).
    let non_owner_reg = svc.handle_register(
        &child_member_id,
        RepoRegisterParams {
            command_id: uuid(299),
            repository_id: uuid(298),
            key: Some("probe".into()),
            kind: RepositoryKindWire::Local,
            remote_url: None,
            origin_path: Some("/nonexistent/secret/path".into()),
        },
    );
    assert_eq!(non_owner_reg.unwrap_err().code, ErrorCode::NotAuthorized);

    let repo_id = uuid(301);
    let reg = svc
        .handle_register(
            &owner,
            RepoRegisterParams {
                command_id: uuid(302),
                repository_id: repo_id.clone(),
                key: Some("core-repo".into()),
                kind: RepositoryKindWire::Local,
                remote_url: None,
                origin_path: Some(bare_origin.to_string_lossy().into_owned()),
            },
        )
        .expect("register local repo");
    assert_eq!(reg.repository.key.as_deref(), Some("core-repo"));

    // Verify local origin_path NEVER enters shared durable `state/repositories.yaml`.
    let repos_yaml =
        std::fs::read_to_string(office.durable_root().join("state/repositories.yaml")).unwrap();
    assert!(!repos_yaml.contains(&bare_origin.to_string_lossy().to_string()));

    // Replaying `repo.register` with same command_id but different `origin_path` fails with OperationReused.
    let other_dir = origin_tmp.path().join("other");
    std::fs::create_dir_all(&other_dir).unwrap();
    let reused = svc.handle_register(
        &owner,
        RepoRegisterParams {
            command_id: uuid(302),
            repository_id: repo_id.clone(),
            key: Some("core-repo".into()),
            kind: RepositoryKindWire::Local,
            remote_url: None,
            origin_path: Some(other_dir.to_string_lossy().into_owned()),
        },
    );
    assert_eq!(reused.unwrap_err().code, ErrorCode::OperationReused);

    // Attach with non-existent branch must be refused before commit!
    let bad_attach = svc.handle_attach(
        &owner,
        RepoAttachParams {
            command_id: uuid(303),
            project: "core".into(),
            repository: "core-repo".into(),
            integration_branch: "does-not-exist".into(),
        },
    );
    assert_eq!(bad_attach.unwrap_err().code, ErrorCode::NotFound);

    // Attach with verified `main` branch succeeds.
    let attached = svc
        .handle_attach(
            &owner,
            RepoAttachParams {
                command_id: uuid(304),
                project: "core".into(),
                repository: "core-repo".into(),
                integration_branch: "main".into(),
            },
        )
        .expect("attach repo");
    assert_eq!(attached.link.integration_branch, "main");

    // Owner creates task assigned to `child_member_id` (in child team `team_id`) and prepares
    // worktree: path MUST use assignee's nested team chain (`root_team` -> `team_id`)!
    let task_out = office
        .task_create(
            &owner,
            TasksCreateParams {
                command_id: uuid(401),
                task_id: uuid(411),
                team_id: Some(team_id.clone()),
                assigned_team_id: None,
                project_id: Some(project_id.clone()),
                task_type: Some("task".into()),
                work_type: None,
                title: "Implement C7 Git Worktrees".into(),
                body_markdown: String::new(),
                assignee_id: Some(child_member_id.clone()),
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
            },
        )
        .unwrap()
        .unwrap();

    let prepared = svc
        .handle_worktree_prepare(
            &owner,
            WorktreePrepareParams {
                command_id: uuid(402),
                task: task_out.task.key.clone(),
                repository: "core-repo".into(),
                base_branch: None,
            },
        )
        .expect("prepare worktree");
    let expected_chain_segment =
        format!("teams/{root_team}/teams/{team_id}/agents/{child_member_id}/projects/{project_id}");
    assert!(
        prepared
            .worktree
            .worktree_path
            .contains(&expected_chain_segment),
        "worktree path {} must use assignee team chain {expected_chain_segment}",
        prepared.worktree.worktree_path
    );
    assert!(prepared
        .worktree
        .branch
        .starts_with(&format!("task/{}-", task_out.task.key)));
    assert_eq!(prepared.writable_roots.len(), 5);

    // Non-owner outsider cannot see `child_member_id`'s worktree or bare_path.
    let outsider_list = svc
        .handle_worktree_list(&outsider_member_id, WorktreeListParams::default())
        .unwrap();
    assert!(outsider_list.worktrees.is_empty());
    let non_owner_status = svc
        .handle_status(
            &outsider_member_id,
            RepoStatusParams {
                repository: "core-repo".into(),
            },
        )
        .unwrap();
    assert!(non_owner_status.bare_path.is_none());

    // Outsider cannot prepare an unassigned task owned by `team_id`.
    let unassigned = office
        .task_create(
            &owner,
            TasksCreateParams {
                command_id: uuid(420),
                task_id: uuid(421),
                team_id: Some(team_id.clone()),
                assigned_team_id: None,
                project_id: Some(project_id.clone()),
                task_type: Some("task".into()),
                work_type: None,
                title: "Unassigned task".into(),
                body_markdown: String::new(),
                assignee_id: None,
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
            },
        )
        .unwrap()
        .unwrap();
    let outsider_prep = svc.handle_worktree_prepare(
        &outsider_member_id,
        WorktreePrepareParams {
            command_id: uuid(422),
            task: unassigned.task.key.clone(),
            repository: "core-repo".into(),
            base_branch: None,
        },
    );
    assert_eq!(outsider_prep.unwrap_err().code, ErrorCode::NotAuthorized);

    // Commit inside worktree and push as assignee (`child_member_id`).
    let wt = PathBuf::from(&prepared.worktree.worktree_path);
    std::fs::write(wt.join("feature.txt"), "hello c7\n").unwrap();
    run_host_git(&wt, &["add", "feature.txt"]);
    run_host_git(
        &wt,
        &[
            "-c",
            "user.name=Dev One",
            "-c",
            "user.email=dev1@example.test",
            "commit",
            "-m",
            "add feature",
        ],
    );

    let pushed = svc
        .handle_git_push(
            &child_member_id,
            GitPushParams {
                task: task_out.task.key.clone(),
                repository: None,
            },
        )
        .expect("push task branch");
    assert_eq!(pushed.branch, prepared.worktree.branch);
    assert_eq!(pushed.commit.len(), 40);

    // Closing the task blocks subsequent push!
    let current_rev = office
        .task_inspect(&task_out.task.key)
        .unwrap()
        .unwrap()
        .task
        .revision;
    let _ = office
        .task_transition(
            &owner,
            TasksTransitionParams {
                command_id: uuid(430),
                task: task_out.task.key.clone(),
                status: TaskStatus::Done,
                resolution: Some("fixed".into()),
                blocked_reason: None,
                note: None,
                expected_revision: current_rev,
                done_anyway_reason: None,
                final_summary: None,
            },
        )
        .unwrap()
        .unwrap();
    let closed_push = svc.handle_git_push(
        &child_member_id,
        GitPushParams {
            task: task_out.task.key.clone(),
            repository: None,
        },
    );
    assert_eq!(closed_push.unwrap_err().code, ErrorCode::InvalidRequest);

    // Verify origin bare repository received `refs/heads/<branch>`.
    let verify_out = Command::new("git")
        .current_dir(&bare_origin)
        .args(["rev-parse", &format!("refs/heads/{}", pushed.branch)])
        .output()
        .unwrap();
    assert!(verify_out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&verify_out.stdout).trim(),
        pushed.commit
    );

    let owner_status = svc
        .handle_status(
            &owner,
            RepoStatusParams {
                repository: "core-repo".into(),
            },
        )
        .unwrap();
    assert_eq!(owner_status.state, CacheStateWire::Ready);
    assert!(owner_status.bare_path.is_some());

    // Offline fallback: delete origin repo and verify `worktree.prepare` for a new task still
    // succeeds from the cached bare repository without clobbering `Ready` state!
    std::fs::remove_dir_all(&bare_origin).unwrap();
    let offline_task = office
        .task_create(
            &owner,
            TasksCreateParams {
                command_id: uuid(440),
                task_id: uuid(441),
                team_id: Some(team_id),
                assigned_team_id: None,
                project_id: Some(project_id),
                task_type: Some("task".into()),
                work_type: None,
                title: "Offline branch".into(),
                body_markdown: String::new(),
                assignee_id: Some(child_member_id),
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
            },
        )
        .unwrap()
        .unwrap();
    let offline_wt = svc
        .handle_worktree_prepare(
            &owner,
            WorktreePrepareParams {
                command_id: uuid(442),
                task: offline_task.task.key,
                repository: "core-repo".into(),
                base_branch: None,
            },
        )
        .expect("offline worktree prepare should fall back to cached branch");
    assert!(offline_wt.worktree.branch.starts_with("task/"));
    assert_eq!(
        svc.handle_status(
            &owner,
            RepoStatusParams {
                repository: "core-repo".into(),
            },
        )
        .unwrap()
        .state,
        CacheStateWire::Ready
    );
}

#[test]
fn resolve_office_git_credential_enforces_host_allowlist_ancestors_and_stderr_redaction() {
    use std::os::unix::fs::PermissionsExt as _;

    let office = Office::for_test(TestOffice::default());
    org_platform::fs::set_owner_only_dir(office.folder()).unwrap();
    let url = "https://github.com/vgmello/agentic-org.git";
    assert!(matches!(
        resolve_office_git_credential(&office, url, AllowedProtocol::Https).unwrap(),
        GitCredential::None
    ));

    // Legacy `durable/.../security/git-https-helper` MUST NEVER be loaded (TASK-168).
    let legacy_sec = office
        .durable_root()
        .join("local")
        .join("offices")
        .join(&office.info().office_id)
        .join("security");
    org_platform::fs::create_owner_only_dir_all(&legacy_sec).unwrap();
    let legacy_helper = legacy_sec.join("git-https-helper");
    std::fs::write(&legacy_helper, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&legacy_helper, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(matches!(
        resolve_office_git_credential(&office, url, AllowedProtocol::Https).unwrap(),
        GitCredential::None
    ));

    // Per-host credentials live outside `durable/` under `<office.folder()>/git-credentials/hosts/<host>/`.
    let creds_root = office.folder().join("git-credentials");
    let hosts_dir = creds_root.join("hosts");
    let gh_dir = hosts_dir.join("github.com");
    org_platform::fs::create_owner_only_dir_all(&gh_dir).unwrap();
    let helper = gh_dir.join("https-helper");
    std::fs::write(&helper, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();

    // Loose ancestor directory (`git-credentials` 0755) must fail closed even when leaf is 0700.
    std::fs::set_permissions(&creds_root, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(resolve_office_git_credential(&office, url, AllowedProtocol::Https).is_err());
    org_platform::fs::set_owner_only_dir(&creds_root).unwrap();

    // Loose host directory permissions (0755) must fail closed.
    std::fs::set_permissions(&gh_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(resolve_office_git_credential(&office, url, AllowedProtocol::Https).is_err());
    org_platform::fs::set_owner_only_dir(&gh_dir).unwrap();

    // Loose helper (0755) and non-executable (0600) must fail closed.
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(resolve_office_git_credential(&office, url, AllowedProtocol::Https).is_err());
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(resolve_office_git_credential(&office, url, AllowedProtocol::Https).is_err());

    // Exact owner-only executable (0700) resolves ONLY for matching host (`github.com`),
    // NEVER for an unrelated remote host (`evil.example`).
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
    match resolve_office_git_credential(&office, url, AllowedProtocol::Https).unwrap() {
        GitCredential::HttpsHelper {
            helper_program,
            scope_url,
        } => {
            assert_eq!(helper_program, helper.canonicalize().unwrap());
            assert_eq!(scope_url, "https://github.com");
        }
        other => panic!("expected HttpsHelper, got {other:?}"),
    }
    assert!(
        matches!(
            resolve_office_git_credential(
                &office,
                "https://evil.example/attacker/repo.git",
                AllowedProtocol::Https,
            )
            .unwrap(),
            GitCredential::None
        ),
        "per-host github.com credential must NEVER attach to unrelated host"
    );

    // Symlink helper must fail closed.
    std::fs::remove_file(&helper).unwrap();
    let target = gh_dir.join("real-helper");
    std::fs::write(&target, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700)).unwrap();
    org_platform::fs::create_symlink(&target, &helper).unwrap();
    assert!(resolve_office_git_credential(&office, url, AllowedProtocol::Https).is_err());

    // SSH key without known-hosts fails closed; with owner-only known-hosts resolves Ssh ONLY for github.com.
    let key = gh_dir.join("ssh-key");
    org_platform::fs::write_owner_only_file(&key, b"PRIVATE").unwrap();
    let ssh_url = "ssh://git@github.com/vgmello/agentic-org.git";
    assert!(resolve_office_git_credential(&office, ssh_url, AllowedProtocol::Ssh).is_err());
    let kh = gh_dir.join("known-hosts");
    org_platform::fs::write_owner_only_file(&kh, b"github.com ssh-ed25519 AAAA\n").unwrap();
    assert!(matches!(
        resolve_office_git_credential(&office, ssh_url, AllowedProtocol::Ssh).unwrap(),
        GitCredential::Ssh { .. }
    ));
    assert!(matches!(
        resolve_office_git_credential(
            &office,
            "git@github.com:vgmello/agentic-org.git",
            AllowedProtocol::Ssh,
        )
        .unwrap(),
        GitCredential::Ssh { .. }
    ));
    assert!(matches!(
        resolve_office_git_credential(
            &office,
            "git@evil.example:attacker/repo.git",
            AllowedProtocol::Ssh,
        )
        .unwrap(),
        GitCredential::None
    ));

    // B3: https_origin_scope rejects `=`, `*`, bad ports, and trims whitespace cleanly; accepts IPv6.
    assert_eq!(
        https_origin_scope("  https://github.com/vgmello/agentic-org.git  ").unwrap(),
        "https://github.com"
    );
    assert_eq!(
        https_origin_scope("https://[::1]:8443/org/repo.git").unwrap(),
        "https://[::1]:8443"
    );
    for bad_url in [
        "https://github.com=evil/org/repo.git",
        "https://*.github.com/org/repo.git",
        "https://github.com:0/org/repo.git",
        "https://github.com:abc/org/repo.git",
        "https://[::::]/org/repo.git",
        "https://user@github.com/org/repo.git",
    ] {
        assert!(
            https_origin_scope(bad_url).is_err(),
            "expected invalid https_origin_scope for `{bad_url}`"
        );
    }

    // B4: setgid/setuid bits (02700 / 04700), EACCES on 0000 dir, and symlink host dir fail closed.
    let _ = std::fs::remove_file(&helper);
    std::fs::write(&helper, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o4700)).unwrap();
    assert!(resolve_office_git_credential(&office, url, AllowedProtocol::Https).is_err());
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::set_permissions(&gh_dir, std::fs::Permissions::from_mode(0o2700)).unwrap();
    assert!(resolve_office_git_credential(&office, url, AllowedProtocol::Https).is_err());
    std::fs::set_permissions(&gh_dir, std::fs::Permissions::from_mode(0o000)).unwrap();
    let eacces = resolve_office_git_credential(&office, url, AllowedProtocol::Https);
    std::fs::set_permissions(&gh_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(eacces.is_err(), "EACCES on host dir must fail closed");

    std::fs::remove_dir_all(&gh_dir).unwrap();
    let alt_dir = office.folder().join("alt-sec");
    org_platform::fs::create_owner_only_dir_all(&alt_dir).unwrap();
    org_platform::fs::create_symlink(&alt_dir, &gh_dir).unwrap();
    assert!(
        resolve_office_git_credential(&office, url, AllowedProtocol::Https).is_err(),
        "symlink host dir must fail closed"
    );
}

#[test]
fn git_stderr_redaction_and_strict_ssh_host_validation() {
    // TASK-168 AC #3: git stderr redaction scrubs embedded URL creds, query secrets, PATs,
    // PEM private keys, case-insensitive headers/env dumps, preserves `task-123`, and never
    // panics on multi-byte UTF-8 straddling byte 512.
    let leaked = GitError::CommandFailed {
        status: 128,
        stderr: "fatal on task-123: HTTPS://x-access-token:ghp_SUPERSECRET123@github.com/org/repo.git?token=QSECRET99 Proxy-Authorization: Basic BASE64SECRET\nX-Access-Token: HDRSECRET1\npassword=\"DQSECRET\" token='SQSECRET' Bearer  DSPACE_SECRET\nGIT_SSH_COMMAND=ssh -i /priv/secret_key\nGITHUB_TOKEN=\"gh_env_secret\" -----BEGIN OPENSSH PRIVATE KEY-----\nAAAASECRETKEYBYTES\n-----END OPENSSH PRIVATE KEY-----".into(),
    };
    let redacted = redact_git_error_for_log(&leaked);
    for secret in [
        "ghp_SUPERSECRET123",
        "QSECRET99",
        "BASE64SECRET",
        "HDRSECRET1",
        "DQSECRET",
        "SQSECRET",
        "DSPACE_SECRET",
        "/priv/secret_key",
        "gh_env_secret",
        "AAAASECRETKEYBYTES",
    ] {
        assert!(
            !redacted.contains(secret),
            "leaked `{secret}` in: {redacted}"
        );
    }
    assert!(
        redacted.contains("task-123"),
        "benign task-123 corrupted: {redacted}"
    );
    assert!(
        redacted.contains("HTTPS://[REDACTED]@github.com/org/repo.git?"),
        "{redacted}"
    );
    let utf8_boundary = format!("{}ééé", "a".repeat(511));
    let truncated = scrub_git_log_text(&utf8_boundary);
    assert!(truncated.ends_with("...[truncated]"), "{truncated}");
    assert!(
        truncated.len() <= 512,
        "truncated exceeds 512 bytes: {}",
        truncated.len()
    );

    // Adversarial redaction checks via org_office_sessions::redact choke-point + Git env scrubbing:
    // multi-byte UTF-8 straddling the truncation cut, mixed-case headers/env vars, multiple
    // secrets on one line, and unterminated PEM blocks failing closed.
    let utf8_cut = format!("{}é{}", "a".repeat(497), "b".repeat(30));
    let cut_out = scrub_git_log_text(&utf8_cut);
    assert!(cut_out.ends_with("...[truncated]"), "{cut_out}");
    assert!(
        cut_out.len() <= 512,
        "cut_out exceeds 512 bytes: {}",
        cut_out.len()
    );

    let adversarial = "authorization: bearer MIXED_CASE_TOK1 cookie: sid=COOKIE_SECRET_2\n\
        git_ssh_command=ssh -i /home/u/.ssh/id_ed25519\n\
        GIT_ASKPASS=/tmp/secret-askpass.sh\n\
        json_payload=\"ok\\nGIT_ASKPASS=/tmp/escaped-askpass.sh\"\n\
        -----BEGIN RSA PRIVATE KEY-----\nUNTERMINATED_KEY_MATERIAL_NEVER_LEAKS";
    let adv_out = scrub_git_log_text(adversarial);
    for secret in [
        "MIXED_CASE_TOK1",
        "COOKIE_SECRET_2",
        "id_ed25519",
        "secret-askpass.sh",
        "escaped-askpass.sh",
        "UNTERMINATED_KEY_MATERIAL_NEVER_LEAKS",
    ] {
        assert!(!adv_out.contains(secret), "leaked `{secret}` in: {adv_out}");
    }

    // Strict SSH authority validation rejects ambiguous `@`, userinfo `:`, query, bad port, `+22`.
    for bad_ssh in [
        "ssh://git@evil.com:22@github.com/org/repo.git",
        "ssh://git@github.com:evil.com/org/repo.git",
        "ssh://git:pw@github.com/org/repo.git",
        "ssh://git@github.com:0/org/repo.git",
        "ssh://git@github.com:+22/org/repo.git",
        "ssh://git@github.com/org/repo.git?token=1",
    ] {
        assert!(
            extract_remote_host(bad_ssh, AllowedProtocol::Ssh).is_err(),
            "expected invalid SSH remote host for `{bad_ssh}`"
        );
    }
}
