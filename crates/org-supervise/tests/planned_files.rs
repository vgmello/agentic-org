use std::fs;
use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use org_runtime::{
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
    let name = tempfile::Builder::new().prefix("mark-").tempdir()?;
    let mark = SessionMark::new(
        name.path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .replace('_', "-"),
    )?;
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
        .push(org_runtime::StopStep::CloseStdin { grace_ms: 100 });
    launch.cleanup_after_exit.push(
        org_runtime::CleanupPath::new(path(&root), &format!("{}/{{pid}}.sock", root.display()))
            .unwrap(),
    );
    launch
        .secret_env_names
        .push(org_runtime::EnvName::try_from("RUNTIME_SECRET").unwrap());
    let expected = launch.clone();
    let mapped = prepare(&root, launch, &context(&root)).unwrap();
    assert_eq!(mapped.lifecycle.stop, expected.stop);
    assert_eq!(
        mapped.lifecycle.cleanup_after_exit,
        expected.cleanup_after_exit
    );
    assert_eq!(mapped.lifecycle.secret_env_names, expected.secret_env_names);
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
