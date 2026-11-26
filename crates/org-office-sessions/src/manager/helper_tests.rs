//! Unit tests for manager helpers (instructions, binding files, bwrap pruning, Rust caches).

use super::*;
use std::os::unix::fs::PermissionsExt as _;

#[test]
fn combine_agent_instructions_merges_and_deduplicates_cleanly() {
    assert_eq!(combine_agent_instructions("", None), "");
    assert_eq!(
        combine_agent_instructions("  role body  ", None),
        "role body"
    );
    assert_eq!(
        combine_agent_instructions("", Some("  exec prompt  ")),
        "exec prompt"
    );
    assert_eq!(
        combine_agent_instructions("same text", Some("same text")),
        "same text"
    );
    assert_eq!(
        combine_agent_instructions("profile", Some("exec")),
        "exec\n\nprofile"
    );
}

#[test]
fn binding_file_is_owner_only_and_removed_on_guard_drop() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("session-binding-run-1.v1");
    let bindings = RunBindingRegistry::new();
    let issued = bindings.issue("emp-1", "run-1", "sess-1", 0).unwrap();
    {
        let _guard = BindingGuard::new(&bindings, "emp-1", "run-1", &file);
        write_owner_only_binding_file(&file, issued.expose_token()).unwrap();
        let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            issued.expose_token()
        );
    }
    assert!(!file.exists(), "BindingGuard drop must unlink binding file");
}

#[test]
fn prune_nonexistent_bwrap_denies_drops_absent_paths_and_preserves_policy_enforcement() {
    use crate::preset::{decide_escalation_with_roots, EscalationDisposition};
    use org_agent_runtime::{
        AgentRuntime, ApprovalId, ApprovalKind, ApprovalRequest, Escalation, ToolCapability,
    };

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let workspace = root.join("workspace");
    let existing_db = root.join("office.db");
    let existing_dir = root.join("objects").join("info");
    let existing_sibling = root.join("agents").join("sibling-live");
    let absent_journal = root.join("office.db-journal");
    let absent_sibling = root.join("agents").join("sibling-1");
    let absent_hooks = root.join("repo.git").join("hooks");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::create_dir_all(&existing_dir).unwrap();
    std::fs::create_dir_all(existing_sibling.join("home")).unwrap();
    std::fs::write(&existing_db, b"sqlite").unwrap();
    // Plant a symlink inside an existing sibling agent's home pointing to a shared host path
    // (`TASK-163`): Linux bwrap denyRead must mask `sibling-live` at its directory boundary
    // without `/**` traversal so bwrap never follows the symlink onto host `.rustup`/`.cargo`.
    let fake_host_rustup = root.join("host-rustup");
    std::fs::create_dir_all(&fake_host_rustup).unwrap();
    org_platform::fs::create_symlink(&fake_host_rustup, &existing_sibling.join("home/.rustup"))
        .unwrap();

    let denied = vec![
        PathPattern::try_from(existing_db.to_str().unwrap()).unwrap(),
        PathPattern::try_from(format!("{}/**", existing_dir.display()).as_str()).unwrap(),
        PathPattern::try_from(format!("{}/**", existing_sibling.display()).as_str()).unwrap(),
        PathPattern::try_from(absent_journal.to_str().unwrap()).unwrap(),
        PathPattern::try_from(format!("{}/**", absent_sibling.display()).as_str()).unwrap(),
        PathPattern::try_from("/run/user/*/bus").unwrap(),
    ];
    let ws_spec = SpecPath::try_from(workspace.to_str().unwrap()).unwrap();
    let mut preset_cfg = build_session_policy(
        PermissionPreset::AutonomousSandboxed,
        true,
        &ws_spec,
        None,
        &denied,
        1,
    )
    .unwrap();
    let absent_hooks_pat =
        PathPattern::try_from(format!("{}/**", absent_hooks.display()).as_str()).unwrap();
    apply_worktree_sandbox_rules(&mut preset_cfg, &[], &[absent_hooks_pat]);

    // Exercise `apply_linux_claude_bwrap_pruning` directly with `AcceptEdits` and a non-empty
    // allow rule so we verify absent-path pruning clears `spec.policy.allow` and downgrades
    // `AcceptEdits -> Ask`, while `preset_cfg.policy` stays 100% intact for
    // `decide_escalation_with_roots`.
    let mut spec_policy = preset_cfg.policy.clone();
    spec_policy.allow.push(org_agent_runtime::Rule {
        tool: org_agent_runtime::RuleTool::try_from("Read").unwrap(),
        scope: org_agent_runtime::RuleScope::Any,
    });
    let mut spec = SessionSpec {
        runtime: RuntimeId::try_from("claude").unwrap(),
        kind: SessionKind::Home {},
        agent: AgentIdentity {
            name: AgentName::try_from("eng").unwrap(),
            description: "Engineer".to_owned(),
            prompt: "Engineer".to_owned(),
        },
        model: None,
        effort: None,
        workspace: ws_spec,
        read_only_dirs: Vec::new(),
        instructions: String::new(),
        permission_mode: org_agent_runtime::PermissionMode::AcceptEdits,
        policy: spec_policy,
        sandbox: preset_cfg.sandbox.clone(),
        mcp_servers: Default::default(),
        tools: preset_cfg.allowed_tools.clone(),
    };
    apply_linux_claude_bwrap_pruning(&mut spec);
    assert_eq!(spec.permission_mode, org_agent_runtime::PermissionMode::Ask);
    assert!(spec.policy.allow.is_empty());

    let deny_read: Vec<&str> = spec
        .sandbox
        .deny_read
        .iter()
        .map(PathPattern::as_str)
        .collect();
    let deny_write: Vec<&str> = spec
        .sandbox
        .deny_write
        .iter()
        .map(PathPattern::as_str)
        .collect();
    assert!(deny_read.contains(&existing_db.to_str().unwrap()));
    // Concrete existing directory globs normalize to directory paths without trailing `/**`
    // so Linux `bwrap` masks them atomically without traversing symlinks (`TASK-163`).
    assert!(deny_read.contains(&existing_dir.to_str().unwrap()));
    assert!(!deny_read.iter().any(|p| p.ends_with("objects/info/**")));
    assert!(deny_read.contains(&existing_sibling.to_str().unwrap()));
    assert!(!deny_read.iter().any(|p| p.ends_with("sibling-live/**")));
    assert!(deny_read.contains(&"/run/user/*/bus"));
    assert!(!deny_read.contains(&absent_journal.to_str().unwrap()));
    assert!(!deny_read.iter().any(|p| p.contains("sibling-1")));
    assert!(!deny_write.iter().any(|p| p.contains("repo.git/hooks")));
    assert!(!deny_write.iter().any(|p| p.contains(".git/hooks")));
    let runtime_dir = root.join("rt");
    let tmpdir = root.join("tmp");
    std::fs::create_dir_all(&runtime_dir).unwrap();
    std::fs::create_dir_all(&tmpdir).unwrap();
    let ctx = LaunchContext {
        runtime_path: PathBuf::from("/usr/local/bin/claude"),
        runtime_version: None,
        runtime_dir: SpecPath::try_from(runtime_dir.to_str().unwrap()).unwrap(),
        tmpdir: SpecPath::try_from(tmpdir.to_str().unwrap()).unwrap(),
        office_env_names: office_env_names(),
        new_session_id: Some(SessionId::try_from("11111111-1111-4111-8111-111111111111").unwrap()),
        resume: None,
        config_dir: None,
    };
    let (plan, _) = org_agent_runtime_claude::ClaudeRuntime::new()
        .plan_session(&spec, &ctx)
        .unwrap();
    let settings_bytes = &plan
        .files
        .iter()
        .find(|f| f.rel_path.as_str() == "claude-settings.json")
        .unwrap()
        .contents;
    let settings: serde_json::Value = serde_json::from_slice(settings_bytes).unwrap();
    let perm_denies: Vec<&str> = settings["permissions"]["deny"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    assert!(perm_denies.contains(&format!("Read(/{})", existing_db.display()).as_str()));
    assert!(!perm_denies.contains(&format!("Read(/{})", absent_journal.display()).as_str()));
    assert!(!perm_denies.contains(&format!("Read(/{}/**)", absent_sibling.display()).as_str()));
    assert!(!perm_denies.contains(&format!("Write(/{}/**)", absent_hooks.display()).as_str()));
    assert_eq!(settings["permissions"]["allow"], serde_json::json!([]));
    assert!(
        plan.args
            .windows(2)
            .any(|w| w == ["--permission-mode", "manual"]),
        "pruned Linux Claude spec must stay pinned to --permission-mode manual: {:?}",
        plan.args
    );

    let mut write_req = ApprovalRequest::new(
        ApprovalId::new("req-absent-sibling"),
        ApprovalKind::FileChange,
        Escalation::None,
        "Write",
        serde_json::json!({
            "file_path": format!("{}/secret.txt", absent_sibling.display())
        }),
    );
    write_req.capability = Some(ToolCapability::Edit);
    assert!(matches!(
        decide_escalation_with_roots(
            &write_req,
            &preset_cfg.policy,
            preset_cfg.preset,
            None,
            Some(&preset_cfg.sandbox.writable_roots),
        ),
        EscalationDisposition::AutoDeny { .. }
    ));
}

#[test]
fn cargo_net_offline_and_zero_copy_rust_cache_symlinks_never_expose_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let cargo_dir = root.join("host-cargo");
    let rustup_dir = root.join("host-rustup");
    let helper_dir = root.join("helpers");
    let home_dir = root.join("home");
    let ws_dir = root.join("ws");
    for d in [
        &cargo_dir,
        &cargo_dir.join("registry"),
        &cargo_dir.join("git"),
        &rustup_dir,
        &helper_dir,
        &home_dir,
        &ws_dir,
    ] {
        std::fs::create_dir_all(d).unwrap();
    }
    // Production creates agent homes owner-only (`ensure_owner_only_dir`).
    let owner_only = |p: &Path| {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700)).unwrap();
    };
    owner_only(&home_dir);
    std::fs::write(cargo_dir.join("credentials"), b"secret-1").unwrap();
    std::fs::write(cargo_dir.join("credentials.toml"), b"secret-2").unwrap();
    std::fs::write(cargo_dir.join("config.toml"), b"host-config").unwrap();
    let search = SearchPath::new(
        &helper_dir,
        std::ffi::OsStr::new("/usr/bin"),
        &[home_dir.clone(), ws_dir.clone()],
    )
    .unwrap();

    let mut sandboxed = Vec::new();
    append_rust_toolchain_env(
        &mut sandboxed,
        Some(&home_dir),
        Some(rustup_dir.as_os_str().to_os_string()),
        Some(cargo_dir.as_os_str().to_os_string()),
        None,
        &[home_dir.clone(), ws_dir.clone()],
        true,
    );
    assert!(sandboxed
        .iter()
        .any(|(k, v)| k == "CARGO_NET_OFFLINE" && v == "true"));
    assert!(sandboxed
        .iter()
        .any(|(k, v)| k == "CARGO_HOME" && v == home_dir.join(".cargo").as_os_str()));
    assert!(Environment::new(search, &sandboxed).is_ok());

    // Safe Cargo cache dirs are symlinked; credentials, config, and redundant `.rustup` are
    // NEVER linked into agent home (`TASK-159`, `TASK-163`: `RUSTUP_HOME` is preserved in env).
    assert_eq!(
        std::fs::read_link(home_dir.join(".cargo").join("registry")).unwrap(),
        cargo_dir.join("registry")
    );
    assert_eq!(
        std::fs::read_link(home_dir.join(".cargo").join("git")).unwrap(),
        cargo_dir.join("git")
    );
    assert!(!home_dir.join(".rustup").exists());
    assert!(sandboxed
        .iter()
        .any(|(k, v)| k == "RUSTUP_HOME" && v == rustup_dir.as_os_str()));
    assert!(!home_dir.join(".cargo").join("credentials").exists());
    assert!(!home_dir.join(".cargo").join("credentials.toml").exists());
    assert!(!home_dir.join(".cargo").join("config.toml").exists());

    // Legacy `<agent_home>/.rustup` symlink is automatically cleaned up (`TASK-163`).
    org_platform::fs::create_symlink(&rustup_dir, &home_dir.join(".rustup")).unwrap();
    assert!(home_dir.join(".rustup").symlink_metadata().is_ok());
    assert!(link_host_rust_caches(
        &home_dir,
        Some(&rustup_dir),
        Some(&cargo_dir),
        &[home_dir.clone(), ws_dir.clone()],
    )
    .is_some());
    assert!(home_dir.join(".rustup").symlink_metadata().is_err());

    // B1: symlinked `<agent_home>/.cargo` must be refused without planting symlinks outside home
    // or exporting raw host CARGO_HOME.
    let escape_target = root.join("external-target");
    std::fs::create_dir_all(&escape_target).unwrap();
    let tampered_home = root.join("tampered-home");
    std::fs::create_dir_all(&tampered_home).unwrap();
    owner_only(&tampered_home);
    org_platform::fs::create_symlink(&escape_target, &tampered_home.join(".cargo")).unwrap();
    let mut tampered_env = Vec::new();
    append_rust_toolchain_env(
        &mut tampered_env,
        Some(&tampered_home),
        Some(rustup_dir.as_os_str().to_os_string()),
        Some(cargo_dir.as_os_str().to_os_string()),
        None,
        &[tampered_home.clone(), ws_dir.clone()],
        true,
    );
    assert!(
        !escape_target.join("registry").exists(),
        "office must never plant symlinks through a symlinked .cargo dir"
    );
    assert!(
        !tampered_env.iter().any(|(k, _)| k == "CARGO_HOME"),
        "tampered .cargo must not fall back to raw host CARGO_HOME: {tampered_env:?}"
    );
    assert!(tampered_env
        .iter()
        .any(|(k, v)| k == "CARGO_NET_OFFLINE" && v == "true"));

    // B1: regular file at `<agent_home>/.cargo/registry` must NOT be treated as a valid cache.
    let file_home = root.join("file-home");
    std::fs::create_dir_all(file_home.join(".cargo")).unwrap();
    owner_only(&file_home);
    owner_only(&file_home.join(".cargo"));
    std::fs::write(file_home.join(".cargo").join("registry"), b"not-a-dir").unwrap();
    std::fs::write(file_home.join(".cargo").join("git"), b"not-a-dir").unwrap();
    assert!(
        link_host_rust_caches(
            &file_home,
            None,
            Some(&cargo_dir),
            &[file_home.clone(), ws_dir.clone()],
        )
        .is_none(),
        "regular file at registry/git must not count as linked cache"
    );

    // A `.cargo` the office account owns but a tool widened (cargo creates it 0755) is
    // tightened back to 0700 through its descriptor and linked, not refused.
    let wide_cargo_home = root.join("wide-cargo-home");
    std::fs::create_dir_all(wide_cargo_home.join(".cargo")).unwrap();
    owner_only(&wide_cargo_home);
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(
            wide_cargo_home.join(".cargo"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        assert!(link_host_rust_caches(
            &wide_cargo_home,
            None,
            Some(&cargo_dir),
            &[wide_cargo_home.clone(), ws_dir.clone()],
        )
        .is_some());
        let mode = std::fs::metadata(wide_cargo_home.join(".cargo"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o7777, 0o700, "{mode:o}");
    }

    // Dedicated-user / tamper hardening: a home that is not an owner-only directory of the
    // office's own account (here: widened to 0755), or a home path that is itself a symlink,
    // is refused before anything is created or linked inside it.
    for (label, refused_home) in [
        ("widened-mode", root.join("wide-home")),
        ("symlinked-home", root.join("link-home")),
    ] {
        if label == "widened-mode" {
            std::fs::create_dir_all(&refused_home).unwrap();
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&refused_home, std::fs::Permissions::from_mode(0o755))
                .unwrap();
        } else {
            org_platform::fs::create_symlink(&home_dir, &refused_home).unwrap();
        }
        let mut refused_env = Vec::new();
        append_rust_toolchain_env(
            &mut refused_env,
            Some(&refused_home),
            None,
            Some(cargo_dir.as_os_str().to_os_string()),
            None,
            &[refused_home.clone(), ws_dir.clone()],
            true,
        );
        assert!(
            !refused_env.iter().any(|(k, _)| k == "CARGO_HOME"),
            "{label}: no CARGO_HOME for a refused home: {refused_env:?}"
        );
    }
    assert!(!root.join("wide-home").join(".cargo").exists());

    let mut unsandboxed = Vec::new();
    append_rust_toolchain_env(
        &mut unsandboxed,
        None,
        None,
        Some(cargo_dir.as_os_str().to_os_string()),
        None,
        &[home_dir, ws_dir],
        false,
    );
    assert!(!unsandboxed.iter().any(|(k, _)| k == "CARGO_NET_OFFLINE"));
}

#[test]
fn pattern_exists_for_bwrap_preserves_root_glob_and_agy_credential_denies_match_adapter() {
    assert!(pattern_exists_for_bwrap(
        &PathPattern::try_from("/**").unwrap()
    ));
    assert!(pattern_exists_for_bwrap(
        &PathPattern::try_from("/*").unwrap()
    ));
    let mut agy_denies = Vec::new();
    append_agy_credential_denies(&mut agy_denies);
    if std::env::var_os("HOME").is_some() {
        for rel in org_agent_runtime_agy::AGY_CONFIG_CREDENTIAL_FILES {
            assert!(
                agy_denies.iter().any(|p| p.as_str().ends_with(rel)),
                "missing agy credential deny for `{rel}`: {agy_denies:?}"
            );
        }
    }
}

#[test]
fn agy_read_only_dirs_mounts_release_store_and_toolchain_subdirs_never_cargo_root() {
    use org_agent_runtime::AgentRuntime as _;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let release_bin = root.join("release-store/releases/0.1.0-v14");
    let shim_bin = root.join("bin");
    let rustup_dir = root.join("host-rustup");
    let cargo_dir = root.join("host-cargo");
    let home_dir = root.join("agent-home");
    let ws_dir = root.join("workspace");
    let rt_dir = root.join("runtime");
    let tmp_dir = root.join("tmp");
    for d in [
        &release_bin,
        &shim_bin,
        &rustup_dir,
        &cargo_dir.join("bin"),
        &cargo_dir.join("registry"),
        &cargo_dir.join("git"),
        &home_dir,
        &ws_dir,
        &rt_dir,
        &tmp_dir,
    ] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(release_bin.join("agent-os"), b"#!/bin/sh\n").unwrap();
    org_platform::fs::create_symlink(&release_bin.join("agent-os"), &shim_bin.join("agent-os"))
        .unwrap();
    std::fs::write(cargo_dir.join("credentials.toml"), b"secret").unwrap();

    let forbidden = vec![home_dir.clone(), ws_dir.clone()];
    let ro_dirs = collect_agy_read_only_dirs(
        &shim_bin,
        Some(rustup_dir.as_os_str().to_os_string()),
        Some(cargo_dir.as_os_str().to_os_string()),
        None,
        &forbidden,
    );
    let ro_strs: Vec<&str> = ro_dirs.iter().map(SpecPath::as_str).collect();
    assert!(ro_strs.contains(&shim_bin.to_str().unwrap()));
    assert!(ro_strs.contains(&release_bin.to_str().unwrap()));
    assert!(ro_strs.contains(&rustup_dir.to_str().unwrap()));
    assert!(ro_strs.contains(&cargo_dir.join("bin").to_str().unwrap()));
    assert!(ro_strs.contains(&cargo_dir.join("registry").to_str().unwrap()));
    assert!(ro_strs.contains(&cargo_dir.join("git").to_str().unwrap()));
    assert!(
        !ro_strs.contains(&cargo_dir.to_str().unwrap()),
        "must NEVER mount raw host CARGO_HOME root into Agy sandbox: {ro_strs:?}"
    );
    assert!(!ro_strs.contains(&"/"));

    // Ancestors of forbidden dirs (`root` contains `home_dir`/`ws_dir`), raw `$HOME`, and
    // symlinked `CARGO_HOME/bin -> CARGO_HOME` MUST all be refused (`TASK-164`).
    let symlink_cargo = root.join("symlink-cargo");
    std::fs::create_dir_all(&symlink_cargo).unwrap();
    org_platform::fs::create_symlink(&symlink_cargo, &symlink_cargo.join("bin")).unwrap();
    let hostile_ro = collect_agy_read_only_dirs(
        &root,
        Some(root.as_os_str().to_os_string()),
        Some(symlink_cargo.as_os_str().to_os_string()),
        Some(root.as_os_str().to_os_string()),
        &forbidden,
    );
    assert!(
        hostile_ro.is_empty(),
        "ancestors of forbidden dirs, $HOME, and escaped CARGO_HOME/bin must be refused: {hostile_ro:?}"
    );

    // Verify `AgyRuntime::plan_session` emits `--add-dir` for every collected RO dir.
    let ws_spec = SpecPath::try_from(ws_dir.to_str().unwrap()).unwrap();
    let preset_cfg = build_session_policy(
        PermissionPreset::AutonomousSandboxed,
        true,
        &ws_spec,
        None,
        &[],
        1,
    )
    .unwrap();
    let spec = SessionSpec {
        runtime: RuntimeId::try_from("agy").unwrap(),
        kind: SessionKind::Home {},
        agent: AgentIdentity {
            name: AgentName::try_from("worker-eu-agy").unwrap(),
            description: "Worker".to_owned(),
            prompt: "Worker".to_owned(),
        },
        model: None,
        effort: None,
        workspace: ws_spec,
        read_only_dirs: ro_dirs,
        instructions: String::new(),
        permission_mode: preset_cfg.permission_mode,
        policy: preset_cfg.policy,
        sandbox: preset_cfg.sandbox,
        mcp_servers: Default::default(),
        tools: preset_cfg.allowed_tools,
    };
    let ctx = LaunchContext {
        runtime_path: PathBuf::from("/usr/local/bin/agy"),
        runtime_version: None,
        runtime_dir: SpecPath::try_from(rt_dir.to_str().unwrap()).unwrap(),
        tmpdir: SpecPath::try_from(tmp_dir.to_str().unwrap()).unwrap(),
        office_env_names: office_env_names(),
        new_session_id: None,
        resume: None,
        config_dir: None,
    };
    let (plan, _) = org_agent_runtime_agy::AgyRuntime::new()
        .plan_session(&spec, &ctx)
        .unwrap();
    for expected in [
        release_bin.to_str().unwrap(),
        rustup_dir.to_str().unwrap(),
        cargo_dir.join("bin").to_str().unwrap(),
        cargo_dir.join("registry").to_str().unwrap(),
    ] {
        assert!(
            plan.args.windows(2).any(|w| w == ["--add-dir", expected]),
            "Agy LaunchPlan missing --add-dir {expected}: {:?}",
            plan.args
        );
    }
    assert!(
        !plan
            .args
            .windows(2)
            .any(|w| w == ["--add-dir", cargo_dir.to_str().unwrap()]),
        "Agy LaunchPlan must NEVER emit --add-dir for host CARGO_HOME root: {:?}",
        plan.args
    );
}

#[test]
fn agent_git_identity_allows_plain_git_commit_without_global_config() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let helper_dir = root.join("helpers");
    let home_dir = root.join("home");
    let repo_dir = root.join("repo");
    for d in [&helper_dir, &home_dir, &repo_dir] {
        std::fs::create_dir_all(d).unwrap();
    }
    let (git_name, git_email) = agent_git_identity("Worker EU\nAgy", "Worker_EU.Agy");
    assert_eq!(git_name, "Worker EU Agy");
    assert_eq!(git_email, "worker_eu-agy@agents.agentic-org.local");
    assert_eq!(
        agent_git_identity("<>...", "Worker_EU.Agy").0,
        "worker_eu-agy"
    );

    let search = SearchPath::new(
        &helper_dir,
        std::ffi::OsStr::new("/usr/bin:/bin"),
        &[home_dir.clone(), repo_dir.clone()],
    )
    .unwrap();
    let git_context = [
        (OsString::from("GIT_AUTHOR_NAME"), OsString::from(&git_name)),
        (
            OsString::from("GIT_AUTHOR_EMAIL"),
            OsString::from(&git_email),
        ),
        (
            OsString::from("GIT_COMMITTER_NAME"),
            OsString::from(&git_name),
        ),
        (
            OsString::from("GIT_COMMITTER_EMAIL"),
            OsString::from(&git_email),
        ),
    ];
    let env = Environment::new(
        search,
        &[(OsString::from("HOME"), home_dir.as_os_str().to_os_string())],
    )
    .unwrap()
    .with_context(&git_context)
    .unwrap();
    let env_debug = format!("{env:?}");
    let names = office_env_names();
    for required in [
        "GIT_AUTHOR_NAME",
        "GIT_AUTHOR_EMAIL",
        "GIT_COMMITTER_NAME",
        "GIT_COMMITTER_EMAIL",
    ] {
        assert!(
            names.iter().any(|n| n.as_str() == required),
            "office_env_names missing {required}"
        );
        assert!(
            env_debug.contains(required),
            "Environment missing {required}: {env_debug}"
        );
    }

    // Run a real plain `git commit` with global & system gitconfigs disabled and zero `-c user.*`.
    let init = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo_dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .unwrap();
    assert!(init.success());
    std::fs::write(repo_dir.join("hello.txt"), b"hi\n").unwrap();
    let add = std::process::Command::new("git")
        .args(["add", "hello.txt"])
        .current_dir(&repo_dir)
        .status()
        .unwrap();
    assert!(add.success());

    let mut commit_cmd = std::process::Command::new("git");
    commit_cmd
        .args(["commit", "-q", "-m", "plain commit"])
        .current_dir(&repo_dir)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", &home_dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1");
    for (k, v) in &git_context {
        commit_cmd.env(k, v);
    }
    let commit_out = commit_cmd.output().unwrap();
    assert!(
        commit_out.status.success(),
        "plain git commit failed: {}",
        String::from_utf8_lossy(&commit_out.stderr)
    );
    let log = std::process::Command::new("git")
        .args(["log", "-1", "--format=%an|%ae|%cn|%ce"])
        .current_dir(&repo_dir)
        .output()
        .unwrap();
    assert!(log.status.success());
    assert_eq!(
        String::from_utf8_lossy(&log.stdout).trim(),
        format!("{git_name}|{git_email}|{git_name}|{git_email}")
    );
}
