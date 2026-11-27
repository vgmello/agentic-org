//! `repo.fetch` end to end (`TASK-173`): `agent-os repo fetch` through the office dispatcher
//! makes a newly pushed origin branch visible in a prepared task worktree, with no credential
//! in the agent's environment and no change to the worktree; plus structured, redacted errors,
//! the per-repository cooldown, single flight under contention, and panic-safe slot release.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Barrier};
use std::time::Duration;

use org_agent_cli::{hint_for, OfficeTransport, Refusal, TransportOffice};
use org_office::{EngineChoice, Office, TestOffice};
use org_proto::git::{
    CacheStateWire, RepoAttachParams, RepoFetchParams, RepoRegisterParams, RepoStatusParams,
    RepositoryKindWire, WorktreePrepareParams,
};
use org_proto::org::{MemberAddPersonParams, ProjectCreateParams};
use org_proto::tasks::TasksCreateParams;
use org_proto::{ErrorCode, Outcome, Request};
use serde_json::Value;

use super::fetch::{FetchGate, FETCH_COOLDOWN};
use super::GitService;
use crate::live::LiveEvents;

fn uuid(n: u64) -> String {
    format!("018bce83-1200-7000-8000-{n:012x}")
}

/// Host `git` as a person or test seeds it (never the office runner).
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Seed")
        .env("GIT_AUTHOR_EMAIL", "seed@example.test")
        .env("GIT_COMMITTER_NAME", "Seed")
        .env("GIT_COMMITTER_EMAIL", "seed@example.test")
        .args(args)
        .output()
        .expect("spawn git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim_end().to_owned()
}

/// `git` as the sandboxed agent runs it: an empty environment apart from `PATH` and an empty
/// `HOME`, so no token, askpass, SSH agent or credential helper is reachable.
fn agent_git(wt: &Path, home: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(wt)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(args)
        .output()
        .expect("spawn git");
    assert!(
        out.status.success(),
        "agent git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim_end().to_owned()
}

/// A bare origin with `main`, and the clone that pushes to it.
fn seed_origin(dir: &Path) -> (PathBuf, PathBuf) {
    let origin = dir.join("origin.git");
    let seed = dir.join("seed");
    std::fs::create_dir_all(&origin).unwrap();
    std::fs::create_dir_all(&seed).unwrap();
    git(&origin, &["init", "--bare", "--initial-branch=main"]);
    git(&seed, &["init", "--initial-branch=main"]);
    std::fs::write(seed.join("README.md"), "# Seed\n").unwrap();
    git(&seed, &["add", "README.md"]);
    git(&seed, &["commit", "-m", "initial"]);
    git(
        &seed,
        &["push", origin.to_str().unwrap(), "HEAD:refs/heads/main"],
    );
    (origin, seed)
}

struct Fixture {
    office: Arc<Office>,
    svc: GitService,
    owner: String,
    dev: String,
}

fn fixture(cooldown: Duration) -> Fixture {
    let office = Arc::new(Office::for_test(TestOffice {
        engine: EngineChoice::Sqlite,
    }));
    let owner = office.owner_member_id().to_owned();
    let live = Arc::new(LiveEvents::for_owner(Box::new(|| {}), owner.clone()));
    let mut svc = GitService::new(Arc::clone(&office), live);
    svc.fetch_gate = FetchGate::new(cooldown);
    let root_team = office.org_snapshot().unwrap().teams[0].id.clone();
    let dev = uuid(205);
    office
        .submit_org(
            &owner,
            org_office::rules::OrgCommand::MemberAddPerson(MemberAddPersonParams {
                command_id: uuid(209),
                member_id: dev.clone(),
                name: "Dev One".into(),
                handle: Some("dev1".into()),
                home_team_id: Some(root_team.clone()),
                contact: None,
            }),
        )
        .unwrap()
        .unwrap();
    let project: ProjectCreateParams = serde_json::from_value(serde_json::json!({
        "command_id": uuid(203),
        "project_id": uuid(201),
        "name": "Core",
        "slug": "core",
        "owner_team_id": root_team,
        "contributing_team_ids": [],
    }))
    .unwrap();
    office
        .submit_org(
            &owner,
            org_office::rules::OrgCommand::ProjectCreate(project),
        )
        .unwrap()
        .unwrap();
    Fixture {
        office,
        svc,
        owner,
        dev,
    }
}

impl Fixture {
    fn register_local(&self, key: &str, origin: &Path, n: u64) {
        self.svc
            .handle_register(
                &self.owner,
                RepoRegisterParams {
                    command_id: uuid(n),
                    repository_id: uuid(n + 1),
                    key: Some(key.into()),
                    kind: RepositoryKindWire::Local,
                    remote_url: None,
                    origin_path: Some(origin.to_string_lossy().into_owned()),
                },
            )
            .expect("register local repo");
    }

    /// Attaches `core-repo` to the project and prepares the developer's task worktree.
    fn prepare_worktree(&self) -> PathBuf {
        let task = self.attach_and_create_task();
        let prepared = self.try_prepare(402, &task).expect("prepare worktree");
        PathBuf::from(prepared.worktree.worktree_path)
    }

    /// Attaches `core-repo` to the project (the origin must serve `main`) and creates the
    /// developer's task; returns the task key.
    fn attach_and_create_task(&self) -> String {
        self.svc
            .handle_attach(
                &self.owner,
                RepoAttachParams {
                    command_id: uuid(304),
                    project: "core".into(),
                    repository: "core-repo".into(),
                    integration_branch: "main".into(),
                },
            )
            .expect("attach repo");
        let root_team = self.office.org_snapshot().unwrap().teams[0].id.clone();
        let task = self
            .office
            .task_create(
                &self.owner,
                TasksCreateParams {
                    command_id: uuid(401),
                    task_id: uuid(411),
                    team_id: Some(root_team),
                    assigned_team_id: None,
                    project_id: Some(uuid(201)),
                    task_type: Some("task".into()),
                    work_type: None,
                    title: "Use fresh origin branches".into(),
                    body_markdown: String::new(),
                    assignee_id: Some(self.dev.clone()),
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
                },
            )
            .unwrap()
            .unwrap();
        task.task.key
    }

    fn try_prepare(
        &self,
        command: u64,
        task: &str,
    ) -> Result<org_proto::git::WorktreePrepareResult, org_proto::ApiError> {
        self.svc.handle_worktree_prepare(
            &self.dev,
            WorktreePrepareParams {
                command_id: uuid(command),
                task: task.into(),
                repository: "core-repo".into(),
                base_branch: None,
            },
        )
    }

    fn fetch(
        &self,
        repository: &str,
    ) -> Result<org_proto::git::RepoFetchResult, org_proto::ApiError> {
        self.svc.handle_fetch(RepoFetchParams {
            repository: repository.into(),
        })
    }

    fn cache_state(&self, repository: &str) -> org_proto::git::RepoStatusResult {
        self.svc
            .handle_status(
                &self.owner,
                RepoStatusParams {
                    repository: repository.into(),
                },
            )
            .unwrap()
    }
}

/// `agent-os` talking to this office's Git dispatcher as a bound member, minus the socket.
struct BoundGit<'a> {
    svc: &'a GitService,
    member: &'a str,
}

impl OfficeTransport for BoundGit<'_> {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        let request = Request {
            id: 7,
            method: method.to_owned(),
            principal: None,
            params,
        };
        let response = self
            .svc
            .dispatch(Some(self.member), &request)
            .expect("a Git family method");
        match response.outcome {
            Outcome::Result(value) => Ok(value),
            Outcome::Error(err) => Err(Refusal::new(err.code, err.message, hint_for(err.code))),
        }
    }
}

fn agent_os(svc: &GitService, member: &str, args: &[&str]) -> (u8, Value) {
    let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
    let mut office = TransportOffice(BoundGit { svc, member });
    let out = org_agent_cli::execute_with_office("agent-os", &args, &mut office);
    let env = serde_json::from_str(&out.stdout).expect("JSON envelope");
    (out.exit_code, env)
}

#[test]
fn agent_os_repo_fetch_shows_a_newly_pushed_branch_in_the_worktree_without_touching_it() {
    let tmp = tempfile::tempdir().unwrap();
    let (origin, seed) = seed_origin(tmp.path());
    let fx = fixture(Duration::ZERO);
    fx.register_local("core-repo", &origin, 302);
    let wt = fx.prepare_worktree();
    let agent_home = tmp.path().join("agent-home");
    std::fs::create_dir_all(&agent_home).unwrap();

    // The agent has an uncommitted edit and an untracked file on its task branch.
    std::fs::write(wt.join("README.md"), "# Local edit\n").unwrap();
    std::fs::write(wt.join("notes.txt"), "draft\n").unwrap();
    let head = agent_git(&wt, &agent_home, &["rev-parse", "HEAD"]);
    let branch = agent_git(&wt, &agent_home, &["symbolic-ref", "HEAD"]);
    let status = agent_git(&wt, &agent_home, &["status", "--porcelain"]);
    assert!(
        status.contains(" M README.md") && status.contains("?? notes.txt"),
        "{status}"
    );

    // Someone pushes a new branch and moves `main` on the origin.
    std::fs::write(seed.join("fix.txt"), "fix\n").unwrap();
    git(&seed, &["add", "fix.txt"]);
    git(&seed, &["commit", "-m", "hotfix"]);
    let url = origin.to_str().unwrap();
    git(
        &seed,
        &[
            "push",
            url,
            "HEAD:refs/heads/feature/hotfix",
            "HEAD:refs/heads/main",
        ],
    );
    let new_main = git(&seed, &["rev-parse", "HEAD"]);
    let remote_before = agent_git(&wt, &agent_home, &["branch", "-r"]);
    assert!(
        !remote_before.contains("origin/feature/hotfix"),
        "{remote_before}"
    );

    let (code, env) = agent_os(&fx.svc, &fx.dev, &["repo", "fetch", "--repo", "core-repo"]);
    assert_eq!(code, 0, "{env}");
    let data = &env["data"];
    assert_eq!(data["repository_key"], "core-repo");
    assert_eq!(data["state"], "ready");
    assert_eq!(data["default_branch"], "main");
    assert_eq!(data["head_oid"], new_main.as_str());
    assert_eq!(data["added"], serde_json::json!(["feature/hotfix"]));
    assert_eq!(data["updated"], serde_json::json!(["main"]));
    assert_eq!(data["pruned_count"], 0);
    // `main` and `feature/hotfix`: the unpushed task branch is local to the cache.
    assert_eq!(data["branch_count"], 2, "{data}");
    assert!(org_proto::parse_rfc3339_ms(data["fetched_at"].as_str().unwrap()).is_some());
    // The office-local origin path and cache path never reach the agent.
    let stdout = env.to_string();
    assert!(
        !stdout.contains(url) && !stdout.contains("bare_path"),
        "{stdout}"
    );

    // The agent, still without any credential, now sees the branch and the moved `main`.
    let remote_after = agent_git(&wt, &agent_home, &["branch", "-r"]);
    assert!(
        remote_after.contains("origin/feature/hotfix"),
        "{remote_after}"
    );
    assert_eq!(
        agent_git(&wt, &agent_home, &["rev-parse", "origin/main"]),
        new_main
    );
    // Its checkout, index and working tree are exactly as before.
    assert_eq!(agent_git(&wt, &agent_home, &["rev-parse", "HEAD"]), head);
    assert_eq!(
        agent_git(&wt, &agent_home, &["symbolic-ref", "HEAD"]),
        branch
    );
    assert_eq!(
        agent_git(&wt, &agent_home, &["status", "--porcelain"]),
        status
    );
    assert_eq!(
        std::fs::read_to_string(wt.join("README.md")).unwrap(),
        "# Local edit\n"
    );
    assert!(!wt.join("fix.txt").exists());
    assert_eq!(fx.cache_state("core-repo").state, CacheStateWire::Ready);
}

#[test]
fn unknown_and_unreachable_repositories_fail_with_structured_redacted_errors() {
    use std::os::unix::fs::PermissionsExt as _;

    let tmp = tempfile::tempdir().unwrap();
    let (origin, _seed) = seed_origin(tmp.path());
    let fx = fixture(Duration::ZERO);
    fx.register_local("core-repo", &origin, 302);

    let unknown = fx.fetch("no-such-repo").unwrap_err();
    assert_eq!(unknown.code, ErrorCode::NotFound);
    let (code, env) = agent_os(
        &fx.svc,
        &fx.dev,
        &["repo", "fetch", "--repo", "no-such-repo"],
    );
    assert_ne!(code, 0);
    assert_eq!(env["error"]["code"], "not_found", "{env}");

    // A cached repository whose origin no longer serves Git: `unavailable`, cache kept `ready`.
    fx.fetch("core-repo").expect("first fetch fills the cache");
    let origin_text = origin.to_string_lossy().into_owned();
    std::fs::remove_dir_all(&origin).unwrap();
    std::fs::create_dir_all(&origin).unwrap();
    let offline = fx.fetch("core-repo").unwrap_err();
    assert_eq!(offline.code, ErrorCode::Unavailable, "{offline:?}");
    assert!(
        offline.message.contains("left unchanged"),
        "{}",
        offline.message
    );
    assert!(
        !offline.message.contains(&origin_text),
        "{}",
        offline.message
    );
    assert_eq!(fx.cache_state("core-repo").state, CacheStateWire::Ready);
    // Its origin directory gone entirely: refused before any fetch, without the path.
    std::fs::remove_dir_all(&origin).unwrap();
    let missing = fx.fetch("core-repo").unwrap_err();
    assert_eq!(missing.code, ErrorCode::StorageError, "{missing:?}");
    assert!(
        !missing.message.contains(&origin_text),
        "{}",
        missing.message
    );
    assert_eq!(fx.cache_state("core-repo").state, CacheStateWire::Ready);

    // An unreachable HTTPS origin with an office credential helper: the helper stays office-side
    // and neither it, its secret nor the URL appears in the error or the recorded reason.
    org_platform::fs::set_owner_only_dir(fx.office.folder()).unwrap();
    let host_dir = fx.office.folder().join("git-credentials/hosts/127.0.0.1");
    org_platform::fs::create_owner_only_dir_all(&host_dir).unwrap();
    let helper = host_dir.join("https-helper");
    std::fs::write(&helper, "#!/bin/sh\necho password=ghp_FETCHSECRET\n").unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let url = "https://127.0.0.1:1/acme/private-repo.git";
    fx.svc
        .handle_register(
            &fx.owner,
            RepoRegisterParams {
                command_id: uuid(310),
                repository_id: uuid(311),
                key: Some("remote-repo".into()),
                kind: RepositoryKindWire::Remote,
                remote_url: Some(url.into()),
                origin_path: None,
            },
        )
        .expect("register remote repo");
    let unreachable = fx.fetch("remote-repo").unwrap_err();
    assert_eq!(unreachable.code, ErrorCode::Unavailable, "{unreachable:?}");
    let status = fx.cache_state("remote-repo");
    assert_eq!(status.state, CacheStateWire::Failed);
    let reason = status.reason.unwrap_or_default();
    for text in [&unreachable.message, &reason] {
        for leaked in [
            "ghp_FETCHSECRET",
            "127.0.0.1",
            "private-repo",
            "https-helper",
        ] {
            assert!(!text.contains(leaked), "`{leaked}` leaked in `{text}`");
        }
    }
}

/// Asserts one `repo.fetch` of a never-filled cache fails as `unavailable` and "not ready",
/// never as the offline fallback that keeps a usable cache.
fn assert_fetch_fails_unprepared(fx: &Fixture, attempt: &str) {
    let err = fx.fetch("core-repo").unwrap_err();
    assert_eq!(err.code, ErrorCode::Unavailable, "{attempt}: {err:?}");
    assert!(
        err.message.contains("not ready"),
        "{attempt}: {}",
        err.message
    );
    assert!(
        !err.message.contains("left unchanged"),
        "{attempt}: {}",
        err.message
    );
    let status = fx.cache_state("core-repo");
    assert_eq!(status.state, CacheStateWire::Failed, "{attempt}");
    assert!(status.reason.is_some(), "{attempt}");
}

#[test]
fn a_failed_first_fill_is_never_treated_as_a_usable_offline_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let (origin, seed) = seed_origin(tmp.path());
    let fx = fixture(Duration::ZERO);
    fx.register_local("core-repo", &origin, 302);
    let task = fx.attach_and_create_task();
    // The newly registered origin stops serving Git before the office ever filled its cache.
    std::fs::remove_dir_all(&origin).unwrap();
    std::fs::create_dir_all(&origin).unwrap();
    let office_id = fx.office.info().office_id.clone();
    let repo = fx.office.resolve_repository("core-repo").unwrap().unwrap();
    let bare = org_office::git::bare_cache_path(fx.office.runtime_root(), &office_id, &repo.id);

    assert_fetch_fails_unprepared(&fx, "first fetch");
    // The failed fill left a `HEAD` behind; the second call must not mistake it for a cache.
    assert!(bare.join("HEAD").is_file(), "{}", bare.display());
    assert_fetch_fails_unprepared(&fx, "second fetch");

    // `worktree.prepare` refuses the empty cache too, whether the origin is unreachable or its
    // directory is gone so the remote cannot even be resolved.
    let unreachable = fx.try_prepare(402, &task).unwrap_err();
    assert_eq!(unreachable.code, ErrorCode::Unavailable, "{unreachable:?}");
    assert_eq!(fx.cache_state("core-repo").state, CacheStateWire::Failed);
    std::fs::remove_dir_all(&origin).unwrap();
    let unresolved = fx.try_prepare(403, &task).unwrap_err();
    assert_eq!(unresolved.code, ErrorCode::StorageError, "{unresolved:?}");
    assert_eq!(fx.cache_state("core-repo").state, CacheStateWire::Failed);

    // Once the origin serves Git again, the same cache directory fills and becomes `ready`.
    std::fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "--bare", "--initial-branch=main"]);
    git(
        &seed,
        &["push", origin.to_str().unwrap(), "HEAD:refs/heads/main"],
    );
    let fetched = fx
        .fetch("core-repo")
        .expect("fetch after the origin returns");
    assert_eq!(fetched.state, CacheStateWire::Ready);
    assert_eq!(fx.cache_state("core-repo").state, CacheStateWire::Ready);
}

#[test]
fn an_unbound_connection_gets_not_bound_for_repo_fetch() {
    let fx = fixture(Duration::ZERO);
    let request = Request {
        id: 9,
        method: "repo.fetch".to_owned(),
        principal: None,
        params: serde_json::json!({"repository": "core-repo"}),
    };
    let response = fx
        .svc
        .dispatch(None, &request)
        .expect("a Git family method");
    match response.outcome {
        Outcome::Error(err) => assert_eq!(err.code, ErrorCode::NotBound, "{err:?}"),
        Outcome::Result(value) => panic!("an unbound repo.fetch was served: {value}"),
    }
}

#[test]
fn a_repeated_fetch_inside_the_cooldown_is_rate_limited_per_repository() {
    let tmp = tempfile::tempdir().unwrap();
    let (origin, _seed) = seed_origin(tmp.path());
    let (other_origin, _other_seed) = seed_origin(&tmp.path().join("other"));
    let fx = fixture(FETCH_COOLDOWN);
    fx.register_local("core-repo", &origin, 302);
    fx.register_local("other-repo", &other_origin, 320);

    fx.fetch("core-repo").expect("first fetch");
    let limited = fx.fetch("core-repo").unwrap_err();
    assert_eq!(limited.code, ErrorCode::RateLimited);
    let retry = limited.retry_after_ms.expect("retry_after_ms");
    assert!(retry > 0 && retry <= 10_000, "{retry}");
    assert!(limited.message.contains("retry in"), "{}", limited.message);
    // The same refusal through `agent-os`, and another repository is not held back.
    let (code, env) = agent_os(&fx.svc, &fx.dev, &["repo", "fetch", "--repo", "core-repo"]);
    assert_ne!(code, 0);
    assert_eq!(env["error"]["code"], "rate_limited", "{env}");
    let hint = env["error"]["hint"].as_str().unwrap_or_default();
    assert_eq!(hint, hint_for(ErrorCode::RateLimited), "{env}");
    assert!(
        hint.contains("cooldown") && !hint.contains("--help"),
        "{env}"
    );
    fx.fetch("other-repo")
        .expect("an unrelated repository fetches");
}

#[test]
fn concurrent_fetches_of_one_repository_run_once_and_leave_consistent_refs() {
    let tmp = tempfile::tempdir().unwrap();
    let (origin, seed) = seed_origin(tmp.path());
    let url = origin.to_str().unwrap();
    for n in 0..20 {
        git(&seed, &["push", url, &format!("HEAD:refs/heads/topic-{n}")]);
    }
    let expected = git(
        &origin,
        &[
            "for-each-ref",
            "--format=%(objectname) %(refname:lstrip=2)",
            "refs/heads",
        ],
    );

    for (cooldown, label) in [
        (Duration::from_secs(60), "cooldown"),
        (Duration::ZERO, "no cooldown"),
    ] {
        let fx = fixture(cooldown);
        fx.register_local("core-repo", &origin, 302);
        let barrier = Barrier::new(6);
        let results: Vec<_> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..6)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        fx.fetch("core-repo")
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let ok = results.iter().filter(|r| r.is_ok()).count();
        assert!(ok >= 1, "{label}: {results:?}");
        for refused in results.iter().filter_map(|r| r.as_ref().err()) {
            assert_eq!(refused.code, ErrorCode::RateLimited, "{label}: {refused:?}");
        }
        if !cooldown.is_zero() {
            assert_eq!(
                ok, 1,
                "{label}: exactly one fetch reaches the origin: {results:?}"
            );
        }
        let cache = PathBuf::from(fx.cache_state("core-repo").bare_path.expect("owner path"));
        let cached = git(
            &cache,
            &[
                "for-each-ref",
                "--format=%(objectname) %(refname:lstrip=3)",
                "refs/remotes/origin",
            ],
        );
        assert_eq!(cached, expected, "{label}");
    }
}

fn refused(
    claim: Result<super::fetch::FetchPermit<'_>, org_proto::ApiError>,
) -> org_proto::ApiError {
    match claim {
        Ok(_) => panic!("the fetch slot should have been refused"),
        Err(err) => err,
    }
}

#[test]
fn a_fetch_slot_is_released_even_when_the_fetch_panics() {
    let gate = FetchGate::new(Duration::from_secs(60));
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _permit = gate.begin("repo-a").unwrap();
        let in_flight = refused(gate.begin("repo-a"));
        assert!(
            in_flight.message.contains("already running"),
            "{}",
            in_flight.message
        );
        panic!("fetch blew up");
    }));
    assert!(panicked.is_err());
    // No longer in flight, but the cooldown still applies; other repositories are free.
    let after = refused(gate.begin("repo-a"));
    assert_eq!(after.code, ErrorCode::RateLimited);
    assert!(after.message.contains("moments ago"), "{}", after.message);
    drop(gate.begin("repo-b").unwrap());

    let open = FetchGate::new(Duration::ZERO);
    drop(open.begin("repo-a").unwrap());
    drop(
        open.begin("repo-a")
            .expect("zero cooldown never limits sequential fetches"),
    );
}
