//! Regression & negative security tests for `TASK-135` AC #4 (auto-pre-prepare before Turn 1)
//! and `TASK-160` (mid-turn `worktree.prepare` sandbox refresh + deferred recycle/resume).

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicUsize, Ordering};

use org_office::git::{OfficeGitState, TaskWorkspaceRecord};

use super::*;
use crate::manager::{
    LocalOsSandbox, PreparedWorktreeNotice, SandboxPlan, SandboxPrepareContext, SandboxRunner,
    TaskWorktreePreparer, MID_TURN_SANDBOX_REFRESH_HINT,
};
use crate::wake::WakeKind;

fn shared_mgr(office: Arc<Office>, cfg: SessionManagerConfig) -> Arc<SessionManager> {
    let mgr = Arc::new(SessionManager::new(office, cfg));
    SessionManager::attach_self(&mgr);
    mgr
}

const REPO_1_ID: &str = "01a212a8-982e-71ca-a6de-322dba762e01";
const REPO_2_ID: &str = "01a212a8-982e-71ca-a6de-322dba762e02";

fn hire_sandboxed_claude_agent(
    office: &Office,
    owner_id: &str,
    root_team_id: &str,
    handle: &str,
) -> (String, String) {
    let conn_id = new_uuid_v7();
    let prof_id = new_uuid_v7();
    let conn_req = Request::new::<ProviderConnectionCreate>(
        1,
        &ProviderConnectionCreateParams {
            command_id: new_uuid_v7(),
            connection_id: conn_id.clone(),
            default_profile_id: prof_id.clone(),
            key: format!("{handle}-conn"),
            name: format!("{handle} conn"),
            provider_type: "anthropic".into(),
            auth_mode: "api_key".into(),
            subscription_identity: None,
            runtime_driver: "claude".into(),
            endpoint_url: None,
            max_concurrent_turns: 2,
        },
    )
    .unwrap();
    let resp = office
        .dispatch_hiring_rpc(Some(owner_id), &conn_req)
        .unwrap();
    assert!(matches!(resp.outcome, org_proto::Outcome::Result(_)));
    bind_and_refresh_test_connection(office, owner_id, &conn_id, "claude-sonnet-4-6");

    let hire_req = Request::new::<HrHire>(
        2,
        &HrHireParams {
            command_id: new_uuid_v7(),
            request_id: new_uuid_v7(),
            employee_id: new_uuid_v7(),
            home_team_id: root_team_id.into(),
            role_id: None,
            proposal: HireProposalWire {
                name: format!("Agent {handle}"),
                handle: handle.into(),
                title: Some("Engineer".into()),
                focus: None,
                role_text: None,
                title_in_team: Some("Engineer".into()),
                reports_to_employee_id: Some(owner_id.into()),
                office_id: None,
                model_profile_id: Some(prof_id),
                model_id: Some("claude-sonnet-4-6".into()),
                permission_preset: Some("autonomous_sandboxed".into()),
                tools_config: Vec::new(),
                system_prompt_markdown: None,
                body: Some("Work safely.".into()),
                effort: None,
            },
            reason: "Sandboxed Claude test agent".into(),
        },
    )
    .unwrap();
    let hired_resp = office
        .dispatch_hiring_rpc(Some(owner_id), &hire_req)
        .unwrap();
    match hired_resp.outcome {
        org_proto::Outcome::Result(v) => {
            let hired: org_proto::hiring::HireOpResult = serde_json::from_value(v).unwrap();
            (hired.employee_id.expect("employee_id"), conn_id)
        }
        org_proto::Outcome::Error(e) => panic!("hire failed: {e:?}"),
    }
}

fn enroll_api_key(mgr: &SessionManager, owner_id: &str, conn_id: &str) {
    let started = mgr.enrollment().start(owner_id, conn_id, "claude").unwrap();
    mgr.enrollment()
        .submit_code(owner_id, conn_id, &started.login_id, "sk-ant-test-key-160")
        .unwrap();
}

fn seed_worktree_record(
    office: &Office,
    root_team_id: &str,
    employee_id: &str,
    task_id: &str,
    task_key: &str,
    repo_id: &str,
    admin_dir_name: &str,
) -> PathBuf {
    let office_id = office.info().office_id.clone();
    let office_rt = office.runtime_root().join(&office_id);
    let cache_dir = office_rt
        .join("repositories")
        .join(format!("{repo_id}.git"));
    fs::create_dir_all(cache_dir.join("worktrees").join(admin_dir_name)).unwrap();
    fs::create_dir_all(cache_dir.join("objects")).unwrap();
    let wt_path = office_rt
        .join("teams")
        .join(root_team_id)
        .join("agents")
        .join(employee_id)
        .join("tasks")
        .join(task_id)
        .join("repositories")
        .join(repo_id);
    fs::create_dir_all(&wt_path).unwrap();
    fs::write(
        wt_path.join(".git"),
        format!(
            "gitdir: {}\n",
            cache_dir.join("worktrees").join(admin_dir_name).display()
        ),
    )
    .unwrap();
    let mut state = OfficeGitState::load(office.runtime_root(), &office_id).unwrap_or_default();
    state.workspaces.insert(
        OfficeGitState::workspace_key(task_id, repo_id, employee_id),
        TaskWorkspaceRecord {
            office_id: office_id.clone(),
            employee_id: employee_id.to_owned(),
            task_id: task_id.to_owned(),
            task_key: task_key.to_owned(),
            repository_id: repo_id.to_owned(),
            repository_key: Some("repo".into()),
            branch: format!("task/{task_key}-work"),
            worktree_path: wt_path.display().to_string(),
            admin_dir_name: admin_dir_name.to_owned(),
            base_commit: "deadbeef".into(),
            pushed_head: None,
            state: "active".into(),
        },
    );
    state.save(office.runtime_root(), &office_id).unwrap();
    wt_path
}

fn json_str_list<'a>(val: &'a serde_json::Value, pointer: &str) -> Vec<&'a str> {
    val.pointer(pointer)
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|x| x.as_str()).collect())
        .unwrap_or_default()
}

fn wait_until(timeout: Duration, mut pred: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if pred() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    pred()
}

fn assert_settings_grant_self_and_deny_peer_and_r2(
    office: &Office,
    settings: &serde_json::Value,
    self_wt: &Path,
    self_admin: &str,
    peer_admin: &str,
    untargeted_repo_id: &str,
) {
    assert_eq!(
        settings.pointer("/sandbox/enabled"),
        Some(&serde_json::json!(true))
    );
    let allow_write = json_str_list(settings, "/sandbox/filesystem/allowWrite");
    let deny_read = json_str_list(settings, "/sandbox/filesystem/denyRead");
    let perm_deny = json_str_list(settings, "/permissions/deny");

    let wt_canon = self_wt
        .canonicalize()
        .unwrap_or_else(|_| self_wt.to_path_buf());
    assert!(
        allow_write.iter().any(|p| wt_canon.starts_with(p)),
        "allowWrite must cover self worktree `{}`: {allow_write:?}",
        wt_canon.display()
    );
    assert!(
        allow_write
            .iter()
            .any(|p| p.ends_with(&format!("/worktrees/{self_admin}"))),
        "allowWrite must include concrete self bare-cache admin dir: {allow_write:?}"
    );
    assert!(
        allow_write.iter().any(|p| p.ends_with("/objects")),
        "allowWrite must include targeted bare-cache objects dir: {allow_write:?}"
    );
    assert!(
        !allow_write.iter().any(|p| p.ends_with("/worktrees")),
        "parent <bare>/worktrees must NEVER be writable: {allow_write:?}"
    );
    assert!(
        !allow_write
            .iter()
            .any(|p| p.contains(peer_admin) || p.contains(untargeted_repo_id)),
        "allowWrite must NEVER include peer admin or untargeted repo: {allow_write:?}"
    );
    assert!(
        deny_read.iter().any(|p| p.contains(peer_admin)),
        "denyRead must deny sibling worktree admin dir `{peer_admin}`: {deny_read:?}"
    );
    assert!(
        deny_read.iter().any(|p| p.contains(untargeted_repo_id)),
        "denyRead must deny untargeted repository `{untargeted_repo_id}`: {deny_read:?}"
    );
    let rt_repos = office
        .runtime_root()
        .join(&office.info().office_id)
        .join("repositories");
    let rt_repos_canon = rt_repos.canonicalize().unwrap_or(rt_repos);
    let rt_repos_str = rt_repos_canon.to_str().unwrap();
    assert!(
        !deny_read
            .iter()
            .any(|p| *p == rt_repos_str || *p == format!("{rt_repos_str}/**")),
        "runtime bare-cache root must be narrowed once self worktree exists: {deny_read:?}"
    );
    assert!(
        !deny_read.iter().any(|p| p.contains(self_admin)),
        "denyRead must NOT deny self bare-cache admin dir: {deny_read:?}"
    );
    assert!(
        perm_deny.iter().any(|p| p.contains(peer_admin))
            && perm_deny.iter().any(|p| p.contains(untargeted_repo_id)),
        "permissions.deny must mirror sibling & untargeted repo denies: {perm_deny:?}"
    );
}

struct StubTaskPreparer {
    office: Arc<Office>,
    root_team_id: String,
    expected_emp: String,
    expected_task: String,
    calls: AtomicUsize,
    created_wt: Mutex<Option<PathBuf>>,
}

impl TaskWorktreePreparer for StubTaskPreparer {
    fn preprepare_for_task(&self, employee_id: &str, task_ref: &str) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if employee_id == self.expected_emp && task_ref == self.expected_task {
            let wt = seed_worktree_record(
                &self.office,
                &self.root_team_id,
                employee_id,
                "t-135",
                "T-135",
                REPO_1_ID,
                "self-admin-135",
            );
            *self.created_wt.lock().unwrap() = Some(wt);
        }
    }
}

#[test]
fn task135_preprepare_before_turn1_grants_exact_bare_cache_and_denies_sibling_and_untargeted_repo()
{
    let _ = org_platform::process::harden_self();
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let root_team_id = office.org_snapshot().unwrap().teams[0].id.clone();

    let (self_emp, self_conn) =
        hire_sandboxed_claude_agent(&office, &owner_id, &root_team_id, "self-preprep");
    let (peer_emp, _) =
        hire_sandboxed_claude_agent(&office, &owner_id, &root_team_id, "peer-preprep");

    // Seed a sibling agent's active worktree on R1 and an untargeted R2 bare cache.
    let _peer_wt = seed_worktree_record(
        &office,
        &root_team_id,
        &peer_emp,
        "t-peer",
        "T-PEER",
        REPO_1_ID,
        "peer-admin-135",
    );
    let untargeted_cache = office
        .runtime_root()
        .join(&office.info().office_id)
        .join("repositories")
        .join(format!("{REPO_2_ID}.git"));
    fs::create_dir_all(&untargeted_cache).unwrap();

    let tmp = tempfile::tempdir().unwrap();
    let captured_settings = tmp.path().join("turn1-settings.json");
    let fake_claude = write_fake_cli(
        tmp.path(),
        "claude",
        &format!(
            r#"#!/bin/sh
settings=""
prev=""
for arg in "$@"; do
  if [ "$prev" = "--settings" ]; then settings="$arg"; fi
  prev="$arg"
done
cp "$settings" "{cap}.tmp" && mv "{cap}.tmp" "{cap}"
read init_line
echo '{{"type":"control_response","response":{{"subtype":"success","request_id":"host-1","response":{{"agents":["self-preprep"],"current_permission_mode":"default","pid":1234,"version":"2.1.311"}}}}}}'
read user_line
echo '{{"type":"system","subtype":"init","session_id":"00000000-0000-0000-0000-000000000135","permissionMode":"default"}}'
echo '{{"type":"result","subtype":"success","is_error":false,"result":"turn1-ok"}}'
"#,
            cap = captured_settings.display()
        ),
    );
    let mut registry = RuntimeRegistry::production();
    registry.register(
        Arc::new(org_agent_runtime_claude::ClaudeRuntime::new()),
        Some(fake_claude),
    );
    let mut cfg = SessionManagerConfig::production(tmp.path().join("office.sock"));
    cfg.runtimes = registry;
    cfg.helper_dir = Some(tmp.path().to_path_buf());
    let mgr = shared_mgr(Arc::clone(&office), cfg);
    enroll_api_key(&mgr, &owner_id, &self_conn);

    let preparer = Arc::new(StubTaskPreparer {
        office: Arc::clone(&office),
        root_team_id: root_team_id.clone(),
        expected_emp: self_emp.clone(),
        expected_task: "T-135".into(),
        calls: AtomicUsize::new(0),
        created_wt: Mutex::new(None),
    });
    mgr.set_task_worktree_preparer(Arc::clone(&preparer) as Arc<dyn TaskWorktreePreparer>);

    let _ = mgr.on_task_assigned(
        &self_emp,
        &owner_id,
        "Owner",
        true,
        "T-135",
        "Auto-pre-prepare worktree",
    );
    assert!(
        wait_until(Duration::from_secs(5), || captured_settings.exists()),
        "Turn 1 fake Claude should launch and snapshot claude-settings.json"
    );
    assert!(
        preparer.calls.load(Ordering::SeqCst) >= 1,
        "TaskWorktreePreparer must run BEFORE Turn 1 sandbox planning"
    );
    let self_wt = preparer
        .created_wt
        .lock()
        .unwrap()
        .clone()
        .expect("pre-preparer created worktree");
    let settings: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&captured_settings).unwrap()).unwrap();
    assert_settings_grant_self_and_deny_peer_and_r2(
        &office,
        &settings,
        &self_wt,
        "self-admin-135",
        "peer-admin-135",
        REPO_2_ID,
    );
    let _ = mgr.stop_session(&self_emp, "done");
}

#[test]
fn task160_mid_turn_worktree_prepare_refreshes_live_policy_settings_and_recycles_with_resume() {
    let _ = org_platform::process::harden_self();
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let root_team_id = office.org_snapshot().unwrap().teams[0].id.clone();

    let (self_emp, self_conn) =
        hire_sandboxed_claude_agent(&office, &owner_id, &root_team_id, "self-midturn");
    let (peer_emp, _) =
        hire_sandboxed_claude_agent(&office, &owner_id, &root_team_id, "peer-midturn");

    let _peer_wt = seed_worktree_record(
        &office,
        &root_team_id,
        &peer_emp,
        "t-peer",
        "T-PEER",
        REPO_1_ID,
        "peer-admin-160",
    );
    let untargeted_cache = office
        .runtime_root()
        .join(&office.info().office_id)
        .join("repositories")
        .join(format!("{REPO_2_ID}.git"));
    fs::create_dir_all(&untargeted_cache).unwrap();

    let tmp = tempfile::tempdir().unwrap();
    let inv_count_file = tmp.path().join("inv-count");
    let turn1_busy = tmp.path().join("turn1-busy");
    let turn1_release = tmp.path().join("turn1-release");
    let turn1_settings = tmp.path().join("turn1-settings.json");
    let turn2_args = tmp.path().join("turn2-args.txt");
    let turn2_settings = tmp.path().join("turn2-settings.json");
    let turn2_done = tmp.path().join("turn2-done");
    let session_uuid = "00000000-0000-0000-0000-000000000160";

    let fake_claude = write_fake_cli(
        tmp.path(),
        "claude",
        &format!(
            r#"#!/bin/sh
n=0
if [ -f "{cnt}" ]; then n=$(cat "{cnt}"); fi
n=$((n + 1))
echo "$n" > "{cnt}"
settings=""
prev=""
for arg in "$@"; do
  if [ "$prev" = "--settings" ]; then settings="$arg"; fi
  prev="$arg"
done
read init_line
echo '{{"type":"control_response","response":{{"subtype":"success","request_id":"host-1","response":{{"agents":["self-midturn"],"current_permission_mode":"default","pid":1234,"version":"2.1.311"}}}}}}'
read user_line
echo '{{"type":"system","subtype":"init","session_id":"{sid}","permissionMode":"default"}}'
if [ "$n" -eq 1 ]; then
  cp "$settings" "{t1s}"
  touch "{busy}"
  while [ ! -f "{rel}" ]; do sleep 0.02; done
  echo '{{"type":"result","subtype":"success","is_error":false,"result":"turn1-done"}}'
  while IFS= read -r _extra; do :; done
else
  printf '%s\n' "$@" > "{t2a}.tmp" && mv "{t2a}.tmp" "{t2a}"
  cp "$settings" "{t2s}.tmp" && mv "{t2s}.tmp" "{t2s}"
  echo '{{"type":"result","subtype":"success","is_error":false,"result":"turn2-resumed"}}'
  touch "{t2d}"
  while IFS= read -r _extra; do :; done
fi
"#,
            cnt = inv_count_file.display(),
            sid = session_uuid,
            t1s = turn1_settings.display(),
            busy = turn1_busy.display(),
            rel = turn1_release.display(),
            t2a = turn2_args.display(),
            t2s = turn2_settings.display(),
            t2d = turn2_done.display(),
        ),
    );
    let mut registry = RuntimeRegistry::production();
    registry.register(
        Arc::new(org_agent_runtime_claude::ClaudeRuntime::new()),
        Some(fake_claude),
    );
    let mut cfg = SessionManagerConfig::production(tmp.path().join("office.sock"));
    cfg.runtimes = registry;
    cfg.helper_dir = Some(tmp.path().to_path_buf());
    let mgr = shared_mgr(Arc::clone(&office), cfg);
    enroll_api_key(&mgr, &owner_id, &self_conn);

    // 1. Launch Turn 1 with ZERO active worktrees for `self_emp`.
    mgr.start_session(
        &owner_id,
        &MemberSessionStartParams {
            command_id: new_uuid_v7(),
            employee_id: self_emp.clone(),
            instruction: Some("Start work".into()),
            workspace: None,
            fresh: false,
            resume_runtime_session_id: None,
        },
    )
    .expect("Turn 1 started");
    assert!(
        wait_until(Duration::from_secs(5), || turn1_busy.exists()),
        "Turn 1 must enter busy gate"
    );
    let init_settings: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&turn1_settings).unwrap()).unwrap();
    let init_deny_read = json_str_list(&init_settings, "/sandbox/filesystem/denyRead");
    let rt_repos = office
        .runtime_root()
        .join(&office.info().office_id)
        .join("repositories");
    let rt_repos_canon = rt_repos.canonicalize().unwrap_or(rt_repos);
    let rt_repos_str = rt_repos_canon.to_str().unwrap();
    assert!(
        init_deny_read
            .iter()
            .any(|p| *p == rt_repos_str || *p == format!("{rt_repos_str}/**")),
        "Turn 1 with 0 worktrees must deny runtime repositories blanket: {init_deny_read:?}"
    );

    // 2. Mid-turn: prepare `self_emp`'s worktree and invoke `on_worktree_prepared`.
    let self_wt = seed_worktree_record(
        &office,
        &root_team_id,
        &self_emp,
        "t-160",
        "T-160",
        REPO_1_ID,
        "self-admin-160",
    );
    let notice = PreparedWorktreeNotice {
        employee_id: &self_emp,
        triggered_by_self: true,
        recycle_idle_target: true,
        task_id: "t-160",
        task_key: "T-160",
        repository_id: REPO_1_ID,
        admin_dir_name: "self-admin-160",
        branch: "task/T-160-work",
        worktree_path: self_wt.to_str().unwrap(),
    };
    let outcome = mgr.on_worktree_prepared(&notice);
    assert!(outcome.sandbox_refresh_required);
    assert_eq!(
        outcome.sandbox_refresh_hint.as_deref(),
        Some(MID_TURN_SANDBOX_REFRESH_HINT)
    );
    assert!(outcome.live_state_refreshed);
    assert!(outcome.settings_file_refreshed);
    assert!(outcome.continuation_queued);

    // Verify live approval bridge & rewritten claude-settings.json immediately reflect grants
    // while keeping launch-time `sandbox_plan` stale so turn end recycles the process.
    {
        let reg = mgr.registry.lock().unwrap();
        let handle = reg.live.get(&self_emp).expect("live handle present");
        assert!(
            mgr.sandbox_runner
                .is_plan_stale(&office, &self_emp, &handle.sandbox_plan),
            "launch-time SandboxPlan must remain stale until process recycles"
        );
        let st = handle.state.lock().unwrap();
        assert!(
            !st.allow_host_auth,
            "API-key launch must keep allow_host_auth=false on live state"
        );
        assert!(
            st.writable_roots
                .iter()
                .any(|p| p.as_str().ends_with("/worktrees/self-admin-160")),
            "live approval writable_roots must include new self admin dir"
        );
        assert!(
            !st.writable_roots
                .iter()
                .any(|p| p.as_str().contains("peer-admin-160") || p.as_str().contains(REPO_2_ID)),
            "live approval writable_roots must NEVER expose sibling or untargeted repo"
        );
    }
    let live_settings_path = office
        .durable_root()
        .join("local")
        .join("offices")
        .join(&office.info().office_id)
        .join("agents")
        .join(&self_emp)
        .join("runtime")
        .join("claude-settings.json");
    let rewritten: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&live_settings_path).unwrap()).unwrap();
    assert_settings_grant_self_and_deny_peer_and_r2(
        &office,
        &rewritten,
        &self_wt,
        "self-admin-160",
        "peer-admin-160",
        REPO_2_ID,
    );
    let queued = mgr
        .store()
        .read_pending_wakes(&self_emp)
        .unwrap()
        .expect("pending wake queue present");
    assert!(
        queued
            .items
            .iter()
            .any(|i| i.kind == WakeKind::SandboxRefresh),
        "SandboxRefresh continuation wake must be durably queued"
    );

    // 3. Release Turn 1 -> office automatically recycles stale warm session and resumes Turn 2!
    fs::write(&turn1_release, b"go").unwrap();
    assert!(
        wait_until(Duration::from_secs(6), || turn2_done.exists()),
        "Turn 2 must auto-spawn after stale warm recycle"
    );
    let args2 = fs::read_to_string(&turn2_args).unwrap();
    assert!(
        args2.contains(&format!("--resume={session_uuid}")),
        "Turn 2 after recycle must resume the same conversation session_id: {args2}"
    );
    let settings2: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&turn2_settings).unwrap()).unwrap();
    assert_settings_grant_self_and_deny_peer_and_r2(
        &office,
        &settings2,
        &self_wt,
        "self-admin-160",
        "peer-admin-160",
        REPO_2_ID,
    );
    assert!(
        wait_until(Duration::from_secs(3), || {
            let reg = mgr.registry.lock().unwrap();
            reg.live.get(&self_emp).is_some_and(|h| {
                !mgr.sandbox_runner
                    .is_plan_stale(&office, &self_emp, &h.sandbox_plan)
            })
        }),
        "freshly recycled handle must have non-stale SandboxPlan"
    );
    let after_recycle = mgr.on_worktree_prepared(&notice);
    assert!(
        !after_recycle.sandbox_refresh_required,
        "freshly recycled handle covering the worktree must not request another refresh"
    );
    let _ = mgr.stop_session(&self_emp, "done");
}

#[test]
fn task160_mid_turn_settings_rewrite_defeats_symlink_escape() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let root_team_id = office.org_snapshot().unwrap().teams[0].id.clone();
    let (self_emp, _) =
        hire_sandboxed_claude_agent(&office, &owner_id, &root_team_id, "self-symlink");

    let sock_dir = tempfile::tempdir().unwrap();
    let mgr = shared_mgr(
        Arc::clone(&office),
        SessionManagerConfig::production(sock_dir.path().join("office.sock")),
    );
    let _home = office.ensure_private_home(&self_emp).unwrap();
    let runtime_dir = office
        .durable_root()
        .join("local")
        .join("offices")
        .join(&office.info().office_id)
        .join("agents")
        .join(&self_emp)
        .join("runtime");
    fs::create_dir_all(&runtime_dir).unwrap();

    // Plant a symlink at `claude-settings.json` targeting a sentinel outside `runtime_dir`.
    let outside = tempfile::tempdir().unwrap();
    let sentinel = outside.path().join("sentinel.txt");
    fs::write(&sentinel, b"ORIGINAL-SENTINEL").unwrap();
    let settings_path = runtime_dir.join("claude-settings.json");
    std::os::unix::fs::symlink(&sentinel, &settings_path).unwrap();

    // Install a busy live Claude handle launched with 0 worktrees.
    let (handle, _stopped) = spawn_stub_bwrap_handle(
        &office.info().office_id,
        &self_emp,
        SessionState::Running,
        BTreeSet::new(),
    );
    handle.state.lock().unwrap().runtime_id = Some("claude".into());
    mgr.registry
        .lock()
        .unwrap()
        .live
        .insert(self_emp.clone(), handle);

    let self_wt = seed_worktree_record(
        &office,
        &root_team_id,
        &self_emp,
        "t-sym",
        "T-SYM",
        REPO_1_ID,
        "self-admin-sym",
    );
    let outcome = mgr.on_worktree_prepared(&PreparedWorktreeNotice {
        employee_id: &self_emp,
        triggered_by_self: false,
        recycle_idle_target: true,
        task_id: "t-sym",
        task_key: "T-SYM",
        repository_id: REPO_1_ID,
        admin_dir_name: "self-admin-sym",
        branch: "task/T-SYM",
        worktree_path: self_wt.to_str().unwrap(),
    });
    assert!(outcome.settings_file_refreshed);
    assert_eq!(
        fs::read(&sentinel).unwrap(),
        b"ORIGINAL-SENTINEL",
        "symlink target outside runtime_dir must NEVER be overwritten"
    );
    let meta = fs::symlink_metadata(&settings_path).unwrap();
    assert!(
        meta.file_type().is_file() && !meta.file_type().is_symlink(),
        "claude-settings.json must be atomically replaced by a regular file"
    );
    assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    let _ = mgr.stop_session(&self_emp, "done");
}

fn assert_worktree_digest_staleness_for_profile(
    office: &Office,
    root_team_id: &str,
    self_emp: &str,
    peer_emp: &str,
    sock: &Path,
    profile_case: (&str, org_agent_runtime::RuntimeHostProfile, u16),
) {
    let (label, profile, suffix) = profile_case;
    let runner = LocalOsSandbox;
    let prepare = || {
        runner
            .prepare_launch(&SandboxPrepareContext {
                office,
                office_socket: sock,
                agent_os_state_roots: &[],
                employee_id: self_emp,
                sandbox_enabled: true,
                host_profile: profile,
            })
            .unwrap()
    };
    let plan0 = prepare();
    assert!(plan0.worktree_sandbox_digest().is_some());
    assert!(!runner.is_plan_stale(office, self_emp, &plan0));

    let _ = seed_worktree_record(
        office,
        root_team_id,
        self_emp,
        &format!("t-self-{label}"),
        "T-SELF",
        REPO_1_ID,
        &format!("self-admin-{label}"),
    );
    assert!(runner.is_plan_stale(office, self_emp, &plan0));

    let plan1 = prepare();
    assert!(!runner.is_plan_stale(office, self_emp, &plan1));

    let _ = seed_worktree_record(
        office,
        root_team_id,
        peer_emp,
        &format!("t-peer-{label}"),
        "T-PEER",
        REPO_1_ID,
        &format!("peer-admin-{label}"),
    );
    assert!(runner.is_plan_stale(office, self_emp, &plan1));

    let plan2 = prepare();
    assert!(!runner.is_plan_stale(office, self_emp, &plan2));

    let extra_repo = format!("01a212a8-982e-71ca-a6de-322dba76{suffix:04x}");
    let extra_cache = office
        .runtime_root()
        .join(&office.info().office_id)
        .join("repositories")
        .join(format!("{extra_repo}.git"));
    fs::create_dir_all(&extra_cache).unwrap();
    assert!(runner.is_plan_stale(office, self_emp, &plan2));
}

#[test]
fn task160_sandbox_plan_worktree_digest_detects_staleness_on_host_native_and_linux_bwrap() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let root_team_id = office.org_snapshot().unwrap().teams[0].id.clone();
    let (self_emp, _) =
        hire_sandboxed_claude_agent(&office, &owner_id, &root_team_id, "self-digest");
    let (peer_emp, _) =
        hire_sandboxed_claude_agent(&office, &owner_id, &root_team_id, "peer-digest");
    let sock_dir = tempfile::tempdir().unwrap();
    let sock = sock_dir.path().join("office.sock");
    let claude = org_agent_runtime_claude::ClaudeRuntime::new();
    let codex = org_agent_runtime_codex::CodexRuntime::new();

    for (label, profile, suffix) in [
        ("host_native", codex.host_profile(), 10u16),
        ("linux_bwrap", claude.host_profile(), 11u16),
    ] {
        assert_worktree_digest_staleness_for_profile(
            &office,
            &root_team_id,
            &self_emp,
            &peer_emp,
            &sock,
            (label, profile, suffix),
        );
    }

    // Verify `on_worktree_prepared` immediately recycles an idle stale sibling handle.
    let mgr = shared_mgr(Arc::clone(&office), SessionManagerConfig::production(sock));
    let (mut idle_peer, peer_stopped) = spawn_stub_bwrap_handle(
        &office.info().office_id,
        &peer_emp,
        SessionState::Idle,
        BTreeSet::new(),
    );
    idle_peer.sandbox_plan =
        SandboxPlan::host_native().with_worktree_sandbox_digest(Some("stale-digest".into()));
    mgr.registry
        .lock()
        .unwrap()
        .live
        .insert(peer_emp.clone(), idle_peer);
    let _ = mgr.on_worktree_prepared(&PreparedWorktreeNotice {
        employee_id: &self_emp,
        triggered_by_self: false,
        recycle_idle_target: true,
        task_id: "t-self-linux_bwrap",
        task_key: "T-SELF",
        repository_id: REPO_1_ID,
        admin_dir_name: "self-admin-linux_bwrap",
        branch: "task/T-SELF",
        worktree_path: "/tmp/unused",
    });
    assert!(
        wait_until(Duration::from_secs(3), || {
            peer_stopped.load(Ordering::SeqCst)
                && !mgr.registry.lock().unwrap().live.contains_key(&peer_emp)
        }),
        "idle stale sibling handle must be recycled when another agent prepares a worktree"
    );
}

#[test]
fn task160_corrupt_git_state_fails_closed_for_worktree_digest() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let root_team_id = office.org_snapshot().unwrap().teams[0].id.clone();
    let (self_emp, _) =
        hire_sandboxed_claude_agent(&office, &owner_id, &root_team_id, "self-corrupt");
    let sock_dir = tempfile::tempdir().unwrap();
    let sock = sock_dir.path().join("office.sock");
    let runner = LocalOsSandbox;
    let claude = org_agent_runtime_claude::ClaudeRuntime::new();
    let codex = org_agent_runtime_codex::CodexRuntime::new();

    let mut plans = Vec::new();
    for profile in [codex.host_profile(), claude.host_profile()] {
        let plan = runner
            .prepare_launch(&SandboxPrepareContext {
                office: &office,
                office_socket: &sock,
                agent_os_state_roots: &[],
                employee_id: &self_emp,
                sandbox_enabled: true,
                host_profile: profile,
            })
            .expect("healthy git state prepares cleanly");
        plans.push((profile, plan));
    }

    // Corrupt `git-state.json` on disk -> BOTH `prepare_launch` and `is_plan_stale` MUST fail closed.
    let state_path = office
        .runtime_root()
        .join(&office.info().office_id)
        .join("git-state.json");
    fs::create_dir_all(state_path.parent().unwrap()).unwrap();
    fs::write(&state_path, b"{not-valid-json").unwrap();

    for (profile, plan) in plans {
        let err = runner
            .prepare_launch(&SandboxPrepareContext {
                office: &office,
                office_socket: &sock,
                agent_os_state_roots: &[],
                employee_id: &self_emp,
                sandbox_enabled: true,
                host_profile: profile,
            })
            .expect_err("corrupt git-state.json must fail closed with StorageError");
        assert_eq!(err.code, org_proto::ErrorCode::StorageError);
        assert!(
            runner.is_plan_stale(&office, &self_emp, &plan),
            "corrupt git-state.json must mark warm plan stale"
        );
    }

    // Even if `git-state.json` is healthy again, a synthetic `"err"` digest plan stays stale.
    fs::remove_file(&state_path).unwrap();
    let err_plan = SandboxPlan::host_native().with_worktree_sandbox_digest(Some("err".into()));
    assert!(runner.is_plan_stale(&office, &self_emp, &err_plan));
}

#[test]
fn task160_mid_turn_settings_rewrite_respects_allow_host_auth_false_and_refreshes_busy_sibling() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let root_team_id = office.org_snapshot().unwrap().teams[0].id.clone();
    let (self_emp, _) =
        hire_sandboxed_claude_agent(&office, &owner_id, &root_team_id, "self-auth-sib");
    let (peer_emp, _) =
        hire_sandboxed_claude_agent(&office, &owner_id, &root_team_id, "peer-auth-sib");
    let sock_dir = tempfile::tempdir().unwrap();
    let mgr = shared_mgr(
        Arc::clone(&office),
        SessionManagerConfig::production(sock_dir.path().join("office.sock")),
    );
    let _ = office.ensure_private_home(&self_emp).unwrap();
    let _ = office.ensure_private_home(&peer_emp).unwrap();

    // Seed `peer_emp` with an existing worktree on `REPO_1_ID`, then install busy handles for both.
    let _ = seed_worktree_record(
        &office,
        &root_team_id,
        &peer_emp,
        "t-peer-sib",
        "T-PSIB",
        REPO_1_ID,
        "peer-admin-sib",
    );
    let (mut self_handle, _) = spawn_stub_bwrap_handle(
        &office.info().office_id,
        &self_emp,
        SessionState::Running,
        BTreeSet::new(),
    );
    self_handle.state.lock().unwrap().allow_host_auth = false;
    self_handle.sandbox_plan =
        SandboxPlan::host_native().with_worktree_sandbox_digest(Some("old-self".into()));
    let (mut peer_handle, _) = spawn_stub_bwrap_handle(
        &office.info().office_id,
        &peer_emp,
        SessionState::Running,
        BTreeSet::new(),
    );
    peer_handle.state.lock().unwrap().allow_host_auth = false;
    peer_handle.sandbox_plan =
        SandboxPlan::host_native().with_worktree_sandbox_digest(Some("old-peer".into()));
    {
        let mut reg = mgr.registry.lock().unwrap();
        reg.live.insert(self_emp.clone(), self_handle);
        reg.live.insert(peer_emp.clone(), peer_handle);
    }

    // Prepare `self_emp`'s worktree on the same `REPO_1_ID` bare cache.
    let self_wt = seed_worktree_record(
        &office,
        &root_team_id,
        &self_emp,
        "t-self-sib",
        "T-SSIB",
        REPO_1_ID,
        "self-admin-sib",
    );
    let outcome = mgr.on_worktree_prepared(&PreparedWorktreeNotice {
        employee_id: &self_emp,
        triggered_by_self: false,
        recycle_idle_target: true,
        task_id: "t-self-sib",
        task_key: "T-SSIB",
        repository_id: REPO_1_ID,
        admin_dir_name: "self-admin-sib",
        branch: "task/T-SSIB",
        worktree_path: self_wt.to_str().unwrap(),
    });
    assert!(outcome.settings_file_refreshed);

    // 1. With `allow_host_auth = false`, rewritten `claude-settings.json` must NOT allow host `.claude`.
    let settings_path = office
        .durable_root()
        .join("local")
        .join("offices")
        .join(&office.info().office_id)
        .join("agents")
        .join(&self_emp)
        .join("runtime")
        .join("claude-settings.json");
    let settings_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
    let allow_write = json_str_list(&settings_json, "/sandbox/filesystem/allowWrite");
    let allow_read = json_str_list(&settings_json, "/sandbox/filesystem/allowRead");
    assert!(
        !allow_write
            .iter()
            .any(|p| p.contains(".credentials.json") || p.ends_with("/.claude")),
        "allow_host_auth=false must never allowWrite host Claude credentials: {allow_write:?}"
    );
    assert!(
        !allow_read
            .iter()
            .any(|p| p.contains(".credentials.json") || p.ends_with("/.claude")),
        "allow_host_auth=false must never allowRead host Claude credentials: {allow_read:?}"
    );

    // 2. Busy sibling `peer_emp`'s live approval `policy.deny` must immediately deny `self-admin-sib`
    //    while keeping `peer_emp`'s launch-time `sandbox_plan` stale for turn-end recycle.
    {
        let reg = mgr.registry.lock().unwrap();
        let peer = reg.live.get(&peer_emp).expect("busy peer still live");
        assert!(
            mgr.sandbox_runner
                .is_plan_stale(&office, &peer_emp, &peer.sandbox_plan),
            "busy sibling launch-time SandboxPlan must stay stale until turn end"
        );
        let peer_st = peer.state.lock().unwrap();
        assert!(
            peer_st
                .policy
                .deny
                .iter()
                .any(|rule| format!("{rule:?}").contains("self-admin-sib")),
            "busy sibling live policy.deny must immediately deny newly prepared peer worktree: {:?}",
            peer_st.policy.deny
        );
    }
    let _ = mgr.stop_session(&self_emp, "done");
    let _ = mgr.stop_session(&peer_emp, "done");
}

fn seed_exhausted_task_budget(
    office: &Office,
    owner_id: &str,
    root_team_id: &str,
    emp_id: &str,
) -> String {
    let task_id = new_uuid_v7();
    office
        .task_create(
            owner_id,
            org_proto::tasks::TasksCreateParams {
                command_id: new_uuid_v7(),
                task_id: task_id.clone(),
                team_id: Some(root_team_id.to_owned()),
                project_id: None,
                assigned_team_id: None,
                assignee_id: Some(emp_id.to_owned()),
                task_type: Some("task".into()),
                work_type: None,
                title: "Guarded task".into(),
                body_markdown: "Guard SandboxRefresh".into(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: Some(1),
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
        .budget_set(
            owner_id,
            org_proto::budgets::BudgetSetParams {
                operation_id: new_uuid_v7(),
                scope_kind: org_proto::budgets::BudgetScopeKind::Task,
                task_id: Some(task_id.clone()),
                agent_id: None,
                release_id: None,
                period_kind: org_proto::budgets::BudgetPeriodKind::Lifetime,
                unit: org_proto::budgets::BudgetUnit::Tokens,
                limit_amount: 1_000,
                time_zone: Some("UTC".into()),
                time_zone_source: Some(org_proto::budgets::BudgetTimeZoneSource::OwnerPerson),
                reason: None,
            },
        )
        .unwrap()
        .unwrap();
    let occurred_at =
        org_office::format_rfc3339_ms(org_office::organization_family::testkit::CREATION_AT.0);
    office
        .record_office_usage(org_proto::costs::CostUsageRecordParams {
            id: None,
            producer_id: "test-session".into(),
            source_event_id: "ev-task-budget-hit".into(),
            run_id: None,
            actor_kind: org_proto::costs::UsageActorKind::Agent,
            employee_id: Some(emp_id.to_owned()),
            runtime: "claude".into(),
            provider: None,
            model: None,
            occurred_at,
            source_metadata_json: None,
            input_tokens: Some(600),
            output_tokens: Some(400),
            cache_read_tokens: Some(0),
            cache_write_tokens: Some(0),
            reasoning_tokens: Some(0),
            token_issue: None,
            runtime_cost_micro_usd: None,
            is_subscription: false,
            scope_kind: org_proto::costs::UsageScopeKind::Task,
            task_id: Some(task_id.clone()),
            project_id: None,
            team_id: None,
            feature_id: None,
            milestone_id: None,
            release_id: None,
            task_type_snapshot: None,
            work_type: None,
            attribution_basis: org_proto::costs::AttributionBasis::ExplicitContext,
        })
        .unwrap()
        .unwrap();
    task_id
}

#[test]
fn task160_sandbox_refresh_wake_respects_task_budget_hold_pause_offboard_and_shutdown() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let root_team_id = office.org_snapshot().unwrap().teams[0].id.clone();
    let (emp_id, _) =
        hire_sandboxed_claude_agent(&office, &owner_id, &root_team_id, "self-wake-guards");
    let task_id = seed_exhausted_task_budget(&office, &owner_id, &root_team_id, &emp_id);

    let sock_dir = tempfile::tempdir().unwrap();
    let mgr = shared_mgr(
        Arc::clone(&office),
        SessionManagerConfig::production(sock_dir.path().join("office.sock")),
    );
    let make_refresh_wake = |suffix: &str| crate::wake::PendingWakeItem {
        item_id: format!("sandbox-refresh:{suffix}"),
        kind: WakeKind::SandboxRefresh,
        sender_id: owner_id.clone(),
        sender_name: "Office Supervisor".into(),
        sender_handle: None,
        sender_relation: Some("office_supervisor".into()),
        sender_is_person: true,
        conversation_id: None,
        source_id: task_id.clone(),
        summary: "Sandbox grants refreshed.".into(),
        attachment_ids: Vec::new(),
        enqueued_at_ms: crate::credentials::now_ms(),
        session_affinity: None,
    };
    // 1. Task-scoped lifetime budget exhaustion holds SandboxRefresh on BudgetHold.
    assert_eq!(
        mgr.enqueue_wake(&emp_id, make_refresh_wake("task-budget")),
        crate::WakeOutcome::Queued {
            reason: crate::WakeHoldReason::BudgetHold,
        }
    );

    // 2. Agent-level pause prevents SandboxRefresh from starting.
    let v1 = office
        .org_snapshot()
        .unwrap()
        .members
        .iter()
        .find(|m| m.id == emp_id)
        .unwrap()
        .version;
    let pause_req = Request::new::<org_proto::hiring::AgentSetStatus>(
        1,
        &org_proto::hiring::AgentSetStatusParams {
            command_id: new_uuid_v7(),
            member_id: emp_id.clone(),
            expected_version: v1,
            paused: true,
        },
    )
    .unwrap();
    let pause_resp = office
        .dispatch_hiring_rpc(Some(&owner_id), &pause_req)
        .unwrap();
    assert!(
        matches!(pause_resp.outcome, org_proto::Outcome::Result(_)),
        "{pause_resp:?}"
    );
    assert_ne!(
        mgr.enqueue_wake(&emp_id, make_refresh_wake("paused")),
        crate::WakeOutcome::Started
    );
    assert!(mgr.registry.lock().unwrap().live.is_empty());

    // 3. Offboarding rejects/ignores SandboxRefresh.
    let v2 = office
        .org_snapshot()
        .unwrap()
        .members
        .iter()
        .find(|m| m.id == emp_id)
        .unwrap()
        .version;
    mgr.on_agent_offboarded(&emp_id);
    let offboard_req = Request::new::<org_proto::hiring::AgentOffboard>(
        2,
        &org_proto::hiring::AgentOffboardParams {
            command_id: new_uuid_v7(),
            member_id: emp_id.clone(),
            expected_version: v2,
        },
    )
    .unwrap();
    let offboard_resp = office
        .dispatch_hiring_rpc(Some(&owner_id), &offboard_req)
        .unwrap();
    assert!(
        matches!(offboard_resp.outcome, org_proto::Outcome::Result(_)),
        "{offboard_resp:?}"
    );
    assert_eq!(
        mgr.enqueue_wake(&emp_id, make_refresh_wake("offboarded")),
        crate::WakeOutcome::Ignored
    );

    // 4. Manager shutdown refuses/ignores SandboxRefresh on a distinct ACTIVE agent.
    let (active_emp, _) =
        hire_sandboxed_claude_agent(&office, &owner_id, &root_team_id, "self-wake-shutdown");
    mgr.shutdown("shutdown");
    assert_eq!(
        mgr.enqueue_wake(&active_emp, make_refresh_wake("shutdown")),
        crate::WakeOutcome::Ignored
    );
}
