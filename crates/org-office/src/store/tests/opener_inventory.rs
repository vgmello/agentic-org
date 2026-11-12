use super::opener_scope::SourceScope;
use super::{bounded_output, check_clippy_config, check_opener_config, opener_policy_config};
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

fn command(program: &str) -> Command {
    let mut command = Command::new(program);
    command.env_clear();
    for name in ["PATH", "HOME"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
}

pub(super) fn tracked(root: &Path) -> SourceScope {
    let root = root.canonicalize().unwrap();
    let paths = bounded_output(
        command("git").current_dir(&root).args(["ls-files", "-z"]),
        "tracked policy inventory",
    )
    .split('\0')
    .filter(|path| !path.is_empty())
    .map(|path| root.join(path))
    .collect::<Vec<_>>();
    for path in &paths {
        assert!(
            !path.is_dir(),
            "tracked directory symlink: {}",
            path.display()
        );
    }
    let scope = SourceScope {
        files: paths
            .iter()
            .filter(|path| {
                path.extension().is_some_and(|ext| ext == "rs")
                    || opener_policy_config(path)
                    || matches!(
                        path.file_name().and_then(|s| s.to_str()),
                        Some("clippy.toml" | ".clippy.toml")
                    )
            })
            .cloned()
            .collect(),
        root,
    };
    scope.require(&scope.root.join("clippy.toml"));
    for path in &scope.files {
        scope.require(path);
        if opener_policy_config(path) {
            check_opener_config(path, &std::fs::read_to_string(path).unwrap());
        } else if path.extension().is_none_or(|ext| ext != "rs") {
            check_clippy_config(path, &std::fs::read_to_string(path).unwrap());
        }
    }
    let directories = paths
        .iter()
        .flat_map(|path| {
            path.parent()
                .unwrap()
                .ancestors()
                .take_while(|path| path.starts_with(&scope.root))
        })
        .collect::<BTreeSet<_>>();
    for directory in directories {
        for name in [
            ".cargo",
            ".cargo/config",
            ".cargo/config.toml",
            "clippy.toml",
            ".clippy.toml",
        ] {
            let path = directory.join(name);
            match std::fs::symlink_metadata(&path) {
                Ok(metadata) => {
                    assert!(
                        !metadata.file_type().is_symlink(),
                        "symlinked policy config: {}",
                        path.display()
                    );
                    if name != ".cargo" {
                        scope.require(&path);
                    }
                }
                Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::NotFound),
            }
        }
    }
    scope
}

fn fixture(files: &[(&str, &str)]) -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::write(
        root.join("clippy.toml"),
        include_str!("../../../../../clippy.toml"),
    )
    .unwrap();
    for (name, text) in files {
        let file = root.join(name);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    bounded_output(
        command("git").current_dir(root).args(["init", "--quiet"]),
        "fixture init",
    );
    bounded_output(
        command("git")
            .current_dir(root)
            .args(["add", "--force", "."]),
        "fixture index",
    );
    directory
}

#[test]
fn sqlcipher_tracked_inventory_covers_all_names_and_config_classes() {
    let allowed = fixture(&[
        (".gitignore", "target/\nbuild/\ndist/\n"),
        ("src/lib.rs", "pub struct Allowed;"),
        ("build/source.rs", "pub struct Build;"),
        ("dist/Cargo.toml", "[package]\nname='nested'"),
        ("nested/.cargo/config", "[build]\nrustflags='-Dclippy::all'"),
        (
            "target/.cargo/config.toml",
            "[build]\nrustflags='-Dclippy::all'",
        ),
        (
            "nested/.clippy.toml",
            include_str!("../../../../../clippy.toml"),
        ),
        (
            "text.txt",
            "this is not Rust and is outside the input kinds",
        ),
    ]);
    let scope = tracked(allowed.path());
    for name in [
        "src/lib.rs",
        "build/source.rs",
        "dist/Cargo.toml",
        "target/.cargo/config.toml",
        "nested/.clippy.toml",
    ] {
        assert!(
            scope.files.contains(&scope.root.join(name)),
            "missed {name}"
        );
    }
    assert!(!scope.files.contains(&scope.root.join("text.txt")));
    super::check_source_inventory(allowed.path());
    for (name, source) in [
        ("build/rogue.rs", "#![allow(clippy::all)]"),
        ("dist/rogue.rs", "#![allow(clippy::all)]"),
        ("target/rogue.rs", "#![allow(clippy::all)]"),
        ("node_modules/rogue.rs", "#![allow(clippy::all)]"),
        (".svelte-kit/rogue.rs", "#![allow(clippy::all)]"),
        (
            "nested/Cargo.toml",
            "[lints.clippy]\ndisallowed_methods='allow'",
        ),
        ("nested/.cargo/config", "[build]\nrustflags='-Aclippy::all'"),
        (
            "nested/.cargo/config.toml",
            "[build]\nrustflags='-Aclippy::all'",
        ),
        ("nested/clippy.toml", "disallowed-methods=[]"),
        ("nested/.clippy.toml", "disallowed-methods=[]"),
    ] {
        let directory = fixture(&[(name, source)]);
        assert!(
            std::panic::catch_unwind(|| super::check_source_inventory(directory.path())).is_err(),
            "missed {name}"
        );
    }
    let untracked = allowed.path().join("unreferenced.rs");
    std::fs::write(untracked, "#![allow(clippy::all)]").unwrap();
    super::check_source_inventory(allowed.path());
    for name in [
        "nested/.cargo/config",
        "nested/.cargo/config.toml",
        "nested/clippy.toml",
        "nested/.clippy.toml",
    ] {
        let directory = fixture(&[("nested/asset.txt", "tracked ancestor")]);
        let path = directory.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
        assert!(
            std::panic::catch_unwind(|| tracked(directory.path())).is_err(),
            "untracked config {name}"
        );
    }
    let outer = tempfile::tempdir().unwrap();
    std::fs::create_dir(outer.path().join(".cargo")).unwrap();
    std::fs::write(
        outer.path().join(".cargo/config.toml"),
        "[build]\nrustflags='-Aclippy::all'",
    )
    .unwrap();
    let directory = fixture(&[("src/lib.rs", "pub struct Allowed;")]);
    let inner = outer.path().join("repo");
    std::fs::rename(directory.path(), &inner).unwrap();
    super::check_source_inventory(&inner);
    let missing = tempfile::tempdir().unwrap();
    assert!(std::panic::catch_unwind(|| tracked(missing.path())).is_err());
}

#[cfg(unix)]
#[test]
fn sqlcipher_inventory_refuses_real_directory_and_config_symlinks() {
    use std::os::unix::fs::symlink;
    let source = tempfile::tempdir().unwrap();
    std::fs::write(source.path().join("rogue.rs"), "#![allow(clippy::all)]").unwrap();
    let directory = fixture(&[("src/lib.rs", "pub struct Allowed;")]);
    super::check_source_inventory(directory.path());
    symlink(source.path(), directory.path().join("linked_source")).unwrap();
    bounded_output(
        command("git")
            .current_dir(directory.path())
            .args(["add", "linked_source"]),
        "symlink index",
    );
    assert!(std::panic::catch_unwind(|| super::check_source_inventory(directory.path())).is_err());
    for name in [
        ".cargo",
        "src/.cargo",
        "src/clippy.toml",
        "src/.clippy.toml",
    ] {
        let directory = fixture(&[("src/lib.rs", "pub struct Allowed;")]);
        symlink(source.path(), directory.path().join(name)).unwrap();
        assert!(
            std::panic::catch_unwind(|| super::check_source_inventory(directory.path())).is_err(),
            "symlink {name}"
        );
    }
    let directory = fixture(&[("src/lib.rs", "pub struct Allowed;")]);
    std::fs::remove_file(directory.path().join("src/lib.rs")).unwrap();
    symlink(
        source.path().join("rogue.rs"),
        directory.path().join("src/lib.rs"),
    )
    .unwrap();
    assert!(std::panic::catch_unwind(|| super::check_source_inventory(directory.path())).is_err());
    let directory = fixture(&[("src/lib.rs", "pub struct Allowed;")]);
    std::fs::rename(
        directory.path().join("src"),
        directory.path().join("real_source"),
    )
    .unwrap();
    symlink(
        directory.path().join("real_source"),
        directory.path().join("src"),
    )
    .unwrap();
    assert!(std::panic::catch_unwind(|| super::check_source_inventory(directory.path())).is_err());
    let directory = fixture(&[("src/lib.rs", "pub struct Allowed;")]);
    symlink(
        directory.path().join("src/lib.rs"),
        directory.path().join("alias.rs"),
    )
    .unwrap();
    bounded_output(
        command("git")
            .current_dir(directory.path())
            .args(["add", "alias.rs"]),
        "alias index",
    );
    assert!(std::panic::catch_unwind(|| super::check_source_inventory(directory.path())).is_err());
    let config = tempfile::tempdir().unwrap();
    std::fs::write(
        config.path().join("config.toml"),
        "[build]\nrustflags='-Aclippy::all'",
    )
    .unwrap();
    let directory = fixture(&[("src/lib.rs", "pub struct Allowed;")]);
    symlink(config.path(), directory.path().join(".cargo")).unwrap();
    assert!(std::panic::catch_unwind(|| super::check_source_inventory(directory.path())).is_err());
}

#[test]
fn sqlcipher_inventory_requires_regular_tracked_inputs_and_a_git_repository() {
    let directory = fixture(&[("src/lib.rs", "pub struct Allowed;")]);
    let scope = tracked(directory.path());
    let path = scope.root.join("src/lib.rs");
    assert_eq!(scope.require(&path), path);
    let unknown = scope.root.join("untracked.rs");
    std::fs::write(&unknown, "").unwrap();
    assert!(std::panic::catch_unwind(|| scope.require(&unknown)).is_err());
    let outside = tempfile::tempdir().unwrap();
    assert!(std::panic::catch_unwind(|| scope.require(outside.path())).is_err());
    std::fs::remove_file(&path).unwrap();
    assert!(std::panic::catch_unwind(|| scope.require(&path)).is_err());
    std::fs::create_dir(&path).unwrap();
    assert!(std::panic::catch_unwind(|| scope.require(&path)).is_err());
    let directory = fixture(&[(".cargo", "a file cannot be a config directory")]);
    assert!(std::panic::catch_unwind(|| tracked(directory.path())).is_err());
    let directory = fixture(&[("src/lib.rs", "pub struct Allowed;")]);
    bounded_output(
        command("git")
            .current_dir(directory.path())
            .args(["rm", "--cached", "clippy.toml"]),
        "remove root config",
    );
    std::fs::remove_file(directory.path().join("clippy.toml")).unwrap();
    assert!(std::panic::catch_unwind(|| tracked(directory.path())).is_err());
    let empty = fixture(&[]);
    bounded_output(
        command("git")
            .current_dir(empty.path())
            .args(["rm", "--cached", "clippy.toml"]),
        "empty index",
    );
    assert!(std::panic::catch_unwind(|| tracked(empty.path())).is_err());
    assert!(std::panic::catch_unwind(|| {
        bounded_output(
            command("git").arg("--definitely-invalid"),
            "failed inventory command",
        );
    })
    .is_err());
    assert!(std::panic::catch_unwind(|| {
        bounded_output(command("printf").arg(r"\377"), "non-UTF8 inventory output");
    })
    .is_err());
    let output = bounded_output(&mut command("env"), "policy command environment");
    let names = output
        .lines()
        .map(|line| line.split_once('=').unwrap().0)
        .collect::<BTreeSet<_>>();
    assert!(names.iter().all(|name| ["PATH", "HOME"].contains(name)));
    for name in ["PATH", "HOME"] {
        assert_eq!(names.contains(name), std::env::var_os(name).is_some());
    }
}
