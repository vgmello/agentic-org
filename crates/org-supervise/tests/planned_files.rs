mod support;
use std::fs;
use std::io::Read;
use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use org_agent_runtime::{
    FileLocation, LaunchContext, LaunchPlan, PlannedDir, PlannedFile, RelPath, SpecPath, Stdio,
    StopPlan,
};
use org_supervise::{runtime, Environment, SearchPath, SessionMark};

fn path(p: &Path) -> SpecPath {
    SpecPath::try_from(p.to_str().unwrap()).unwrap()
}

fn context(root: &Path) -> LaunchContext {
    LaunchContext {
        runtime_path: "/bin/cat".into(),
        runtime_version: None,
        runtime_dir: path(root),
        tmpdir: path(root),
        office_env_names: vec![],
        new_session_id: None,
        resume: None,
        config_dir: None,
    }
}

fn plan(root: &Path) -> LaunchPlan {
    LaunchPlan {
        program: "/bin/cat".into(),
        args: vec![],
        cwd: path(root),
        env_add: vec![],
        dirs: vec![],
        files: vec![],
        stdio: Stdio::Piped,
        runtime_session_id: None,
        stop: StopPlan { steps: vec![] },
        cleanup_after_exit: vec![],
        secret_env_names: vec![],
    }
}

fn prepare(
    root: &Path,
    plan: LaunchPlan,
    ctx: &LaunchContext,
) -> std::io::Result<runtime::RuntimeLaunch> {
    let env = Environment::new(
        SearchPath::new(Path::new("/usr/bin"), "/bin".as_ref(), &[root.to_owned()])?,
        &[],
    )?;
    let mut random = [0u8; 16];
    fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
    let mark = SessionMark::new(format!("file-test-{:032x}", u128::from_ne_bytes(random)))?;
    runtime::prepare(plan, ctx, env, mark)
}

fn file(relative: &str, contents: &[u8]) -> PlannedFile {
    PlannedFile {
        location: FileLocation::RuntimeDir,
        rel_path: RelPath::try_from(relative).unwrap(),
        contents: contents.to_owned(),
        git_exclude: false,
    }
}

#[test]
fn writes_private_dirs_and_replaces_symlink_and_hardlink_leaves_atomically() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let protected = outside.path().join("protected");
    fs::write(&protected, b"original").unwrap();
    fs::create_dir(root.join("nested")).unwrap();
    fs::set_permissions(root.join("nested"), fs::Permissions::from_mode(0o750)).unwrap();
    symlink(&protected, root.join("nested/link")).unwrap();
    fs::hard_link(&protected, root.join("nested/hard")).unwrap();
    let mut launch = plan(&root);
    launch.dirs.push(PlannedDir {
        location: FileLocation::RuntimeDir,
        rel_path: RelPath::try_from("new/deep").unwrap(),
    });
    launch.files = vec![
        file("nested/link", b"new-link"),
        file("nested/hard", b"new-hard"),
        file("new/deep/config", b"settings"),
    ];
    prepare(&root, launch, &context(&root)).unwrap();
    assert_eq!(fs::read(&protected).unwrap(), b"original");
    assert_eq!(fs::read(root.join("nested/link")).unwrap(), b"new-link");
    assert_eq!(fs::read(root.join("nested/hard")).unwrap(), b"new-hard");
    for name in ["new", "new/deep"] {
        assert_eq!(fs::metadata(root.join(name)).unwrap().mode() & 0o777, 0o700);
    }
    assert_eq!(
        fs::metadata(root.join("nested")).unwrap().mode() & 0o777,
        0o750
    );
    assert_eq!(
        fs::metadata(root.join("new/deep/config")).unwrap().mode() & 0o777,
        0o600
    );
}

#[test]
fn refuses_symlink_in_root_or_intermediate_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let real = outside.path().canonicalize().unwrap();
    symlink(&real, root.join("nested")).unwrap();
    let mut launch = plan(&root);
    launch.files.push(file("nested/config", b"no"));
    assert!(prepare(&root, launch, &context(&root)).is_err());
    assert!(!real.join("config").exists());
    let alias = root.join("alias");
    symlink(&real, &alias).unwrap();
    let mut launch = plan(&root);
    launch.files.push(file("config", b"no"));
    let mut ctx = context(&root);
    ctx.runtime_dir = path(&alias);
    assert!(prepare(&root, launch, &ctx).is_err());
    assert!(!real.join("config").exists());
}

#[test]
fn unsupported_plan_parts_fail_before_file_writes_and_lifecycle_is_retained() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    for pty in [false, true] {
        let mut launch = plan(&root);
        launch.files.push(file("config", b"no"));
        if pty {
            launch.stdio = Stdio::Pty { cols: 80, rows: 24 };
        } else {
            launch.files[0].git_exclude = true;
        }
        assert!(prepare(&root, launch, &context(&root)).is_err());
        assert!(!root.join("config").exists());
    }
    let mut launch = plan(&root);
    launch
        .stop
        .steps
        .push(org_agent_runtime::StopStep::CloseStdin { grace_ms: 100 });
    launch.cleanup_after_exit.push(
        org_agent_runtime::CleanupPath::new(
            path(&root),
            &format!("{}/{{pid}}.sock", root.display()),
        )
        .unwrap(),
    );
    launch
        .secret_env_names
        .push(org_agent_runtime::EnvName::try_from("RUNTIME_SECRET").unwrap());
    let expected = launch.clone();
    let mapped = prepare(&root, launch, &context(&root)).unwrap();
    assert_eq!(mapped.lifecycle.stop, expected.stop);
    assert_eq!(
        mapped.lifecycle.cleanup_after_exit,
        expected.cleanup_after_exit
    );
    assert_eq!(mapped.lifecycle.secret_env_names, expected.secret_env_names);
    let mut mismatched = plan(&root);
    mismatched.program = "/bin/sh".into();
    mismatched.files.push(file("wrong-runtime", b"no"));
    assert!(prepare(&root, mismatched, &context(&root)).is_err());
    assert!(!root.join("wrong-runtime").exists());
}

#[test]
fn failed_replacement_removes_its_temporary_file_and_workspace_location_works() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    fs::create_dir(root.join("directory")).unwrap();
    let mut launch = plan(&root);
    launch.files.push(file("directory", b"invalid"));
    assert!(prepare(&root, launch, &context(&root)).is_err());
    let entries: Vec<PathBuf> = fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(entries, [root.join("directory")]);
    let mut launch = plan(&root);
    let mut planned = file("workspace-config", b"valid");
    planned.location = FileLocation::Workspace;
    launch.files.push(planned);
    prepare(&root, launch, &context(&root)).unwrap();
    assert_eq!(fs::read(root.join("workspace-config")).unwrap(), b"valid");
}

#[test]
fn restrictive_umask_child() {
    let Ok(mask) = std::env::var("ORG_TEST_UMASK") else {
        return;
    };
    let root = std::env::current_dir().unwrap();
    let mask: libc::mode_t = mask.parse().unwrap();
    // SAFETY: only this isolated helper's process-wide umask changes; the main test runner
    // never changes its umask while other tests/threads create files.
    unsafe {
        libc::umask(mask);
    }
    // No directories need creating here: even an owner-masking umask must leave the
    // newly O_EXCL-created regular file at0600 through its pinned descriptor.
    let mut file_only = plan(&root);
    file_only.files.push(file("existing/private", b"settings"));
    prepare(&root, file_only, &context(&root)).unwrap();
    assert_eq!(
        fs::metadata(root.join("existing/private")).unwrap().mode() & 0o777,
        0o600
    );
    let mut launch = plan(&root);
    launch.dirs.push(PlannedDir {
        location: FileLocation::RuntimeDir,
        rel_path: RelPath::try_from("existing").unwrap(),
    });
    launch.dirs.push(PlannedDir {
        location: FileLocation::RuntimeDir,
        rel_path: RelPath::try_from("new/deep").unwrap(),
    });
    launch.files.push(file("new/deep/config", b"settings"));
    let result = prepare(&root, launch, &context(&root));
    if mask & 0o700 != 0 {
        // Ada's v1.0 decision: never chmod a directory whose creation provenance is unknown.
        let error = result.err().expect("extreme umask must fail closed");
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(error.to_string().contains("umask"));
        assert!(!root.join("new/deep/config").exists());
        assert_eq!(
            fs::metadata(root.join("new")).unwrap().mode() & 0o777,
            0o700 & !(mask as u32)
        );
        assert_eq!(
            fs::metadata(root.join("existing")).unwrap().mode() & 0o777,
            0o750
        );
        // Test-only cleanup in this isolated helper's private tree; no concurrent actor.
        fs::set_permissions(root.join("new"), fs::Permissions::from_mode(0o700)).unwrap();
        return;
    }
    result.unwrap();
    for name in ["new", "new/deep"] {
        assert_eq!(fs::metadata(root.join(name)).unwrap().mode() & 0o777, 0o700);
    }
    assert_eq!(
        fs::metadata(root.join("new/deep/config")).unwrap().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(root.join("existing")).unwrap().mode() & 0o777,
        0o750
    );
}

#[test]
fn planned_modes_or_explicit_refusal_under_restrictive_umask() {
    use org_supervise::{PreparedSpawn, SpawnSpec};
    use std::time::Duration;
    for mask in [0o002, 0o022, 0o027, 0o077, 0o100, 0o700, 0o777] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        fs::create_dir(root.join("existing")).unwrap();
        fs::set_permissions(root.join("existing"), fs::Permissions::from_mode(0o750)).unwrap();
        let env = Environment::new(
            SearchPath::new(
                Path::new("/usr/bin"),
                "/bin".as_ref(),
                std::slice::from_ref(&root),
            )
            .unwrap(),
            &[],
        )
        .unwrap()
        .with_context(&[("ORG_TEST_UMASK".into(), mask.to_string().into())])
        .unwrap();
        let spec = SpawnSpec {
            program: std::env::current_exe().unwrap(),
            args: vec![
                "--exact".into(),
                "restrictive_umask_child".into(),
                "--nocapture".into(),
                "--test-threads=1".into(),
            ],
            cwd: root.clone(),
            environment: env,
        };
        let child = PreparedSpawn::new(
            spec,
            SessionMark::new(
                temp.path()
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .replace(['.', '_'], "-"),
            )
            .unwrap(),
        )
        .unwrap()
        .record_mark(|_| Ok(()))
        .unwrap()
        .spawn()
        .unwrap()
        .record_identity(|_| Ok(()))
        .unwrap();
        let (status, out, err) = support::collect(child, Duration::from_secs(10));
        assert!(status.success(), "umask {mask:o}: {out}\n{err}");
    }
}

#[test]
fn runtime_mapper_rejects_injection_and_duplicate_switches_before_writes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    for name in [
        "HOME",
        "ORG_SESSION_TOKEN",
        "GIT_CONFIG_COUNT",
        "DYLD_INSERT_LIBRARIES",
        "LD_PRELOAD",
        "NODE_OPTIONS",
        "BASH_ENV",
        "ENV",
        "PYTHONSTARTUP",
        "CLAUDE_CODE_BAD",
    ] {
        let mut launch = plan(&root);
        launch.env_add.push((
            org_agent_runtime::EnvName::try_from(name).unwrap(),
            "secret".into(),
        ));
        launch.files.push(file("must-not-exist", b"settings"));
        assert!(prepare(&root, launch, &context(&root)).is_err(), "{name}");
        assert!(!root.join("must-not-exist").exists());
    }
    for second in ["false", "true"] {
        let mut launch = plan(&root);
        launch.env_add = vec![
            (
                org_agent_runtime::EnvName::try_from("ENABLE_TOOL_SEARCH").unwrap(),
                "false".into(),
            ),
            (
                org_agent_runtime::EnvName::try_from("ENABLE_TOOL_SEARCH").unwrap(),
                second.into(),
            ),
        ];
        assert!(prepare(&root, launch, &context(&root)).is_err());
    }
    let mut launch = plan(&root);
    launch.env_add = [
        "DISABLE_AUTOUPDATER",
        "ENABLE_TOOL_SEARCH",
        "CLAUDE_CONFIG_DIR",
        "CODEX_HOME",
    ]
    .map(|name| {
        (
            org_agent_runtime::EnvName::try_from(name).unwrap(),
            "false".into(),
        )
    })
    .into();
    prepare(&root, launch, &context(&root)).unwrap();
}

#[test]
fn runtime_auth_allowlist_and_agent_os_context_and_probe_config_dir_switches() {
    use org_agent_runtime::{EnvName, ProbeContext, ProbePlan};

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let probe_tmp = root.join("probe-tmp");
    fs::create_dir(&probe_tmp).unwrap();
    fs::set_permissions(&probe_tmp, fs::Permissions::from_mode(0o700)).unwrap();

    let make_env = || {
        Environment::new(
            SearchPath::new(
                Path::new("/usr/bin"),
                "/bin".as_ref(),
                std::slice::from_ref(&root),
            )
            .unwrap(),
            &[],
        )
        .unwrap()
    };

    // Reviewed runtime auth names are accepted and do not mark the environment as carrying
    // agent context (so auth probes may use them).
    let auth_env = make_env()
        .with_runtime_auth(&[
            ("CLAUDE_CODE_OAUTH_TOKEN".into(), "tok-1".into()),
            ("CLAUDE_SECURESTORAGE_CONFIG_DIR".into(), "/tmp/sec".into()),
            ("ANTHROPIC_API_KEY".into(), "sk-ant-test".into()),
            ("OPENAI_API_KEY".into(), "sk-openai-test".into()),
            ("CODEX_API_KEY".into(), "sk-codex-test".into()),
        ])
        .unwrap();

    // Unreviewed names must be refused by with_runtime_auth.
    for bad in ["PATH", "HOME", "ORG_SESSION_TOKEN", "AGENT_OS_TOKEN", "FOO"] {
        assert!(
            make_env()
                .with_runtime_auth(&[(bad.into(), "val".into())])
                .is_err(),
            "with_runtime_auth must reject {bad}"
        );
    }

    // AGENT_OS_* is accepted by with_context and marks the env as carrying context, so
    // prepare_probe refuses it while prepare accepts it.
    let ctx_env = make_env()
        .with_context(&[("AGENT_OS_SESSION_TOKEN".into(), "sess-tok".into())])
        .unwrap();

    let probe_ctx = ProbeContext::new("/bin/cat".into());
    let probe_plan = ProbePlan {
        program: "/bin/cat".into(),
        args: vec![],
        env_add: vec![
            (
                EnvName::try_from("CLAUDE_CONFIG_DIR").unwrap(),
                root.to_str().unwrap().to_owned(),
            ),
            (
                EnvName::try_from("CODEX_HOME").unwrap(),
                root.to_str().unwrap().to_owned(),
            ),
        ],
        stdin_lines: vec![],
        timeout_ms: 5_000,
    };
    let mark = SessionMark::new("probe-rtp3-00000000000000000000000000000001".to_owned()).unwrap();
    runtime::prepare_probe(
        probe_plan.clone(),
        &probe_ctx,
        auth_env,
        mark.clone(),
        &probe_tmp,
        runtime::ProbeMode::OneShot,
    )
    .expect("prepare_probe accepts reviewed CLAUDE_CONFIG_DIR, CODEX_HOME, and runtime auth");

    assert!(
        runtime::prepare_probe(
            probe_plan,
            &probe_ctx,
            ctx_env,
            mark,
            &probe_tmp,
            runtime::ProbeMode::OneShot,
        )
        .is_err(),
        "prepare_probe must refuse AGENT_OS_* context variables"
    );
}
