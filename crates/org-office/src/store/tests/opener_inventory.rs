use super::opener_scope::{SourceKind, SourceScope};
use super::{bounded_output, check_clippy_config, check_opener_config, opener_policy_config};
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

const POLICY_NAMES: &[&str] = &[
    "Cargo.toml",
    ".cargo",
    ".cargo/config",
    ".cargo/config.toml",
    "clippy.toml",
    ".clippy.toml",
];

fn require_policy_spelling(path: &Path) {
    for relative in path.ancestors() {
        let folded = std::path::PathBuf::from(relative.to_string_lossy().to_ascii_lowercase());
        for &name in POLICY_NAMES {
            assert!(
                !folded.ends_with(name.to_ascii_lowercase()) || relative.ends_with(name),
                "case-variant policy name: {}",
                path.display()
            );
        }
    }
}

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
            "tracked input resolves to a directory (symlink or gitlink): {}",
            path.display()
        );
        require_policy_spelling(path.strip_prefix(&root).unwrap());
    }
    let scope = SourceScope {
        files: paths
            .iter()
            .filter(|path| {
                path.extension().is_some_and(|ext| ext == "rs")
                    || opener_policy_config(path)
                    || POLICY_NAMES.iter().any(|name| path.ends_with(name))
            })
            .map(|path| {
                (
                    path.clone(),
                    if path.extension().is_some_and(|ext| ext == "rs") {
                        SourceKind::Rust
                    } else {
                        SourceKind::Config
                    },
                )
            })
            .collect(),
        root,
    };
    scope.require(&scope.root.join("clippy.toml"));
    for path in scope.files.keys() {
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
        for &name in POLICY_NAMES {
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

pub(super) fn fixture(files: &[(&str, &str)]) -> tempfile::TempDir {
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

fn check_inventory(root: &Path) {
    super::check_tracked_sources(&tracked(root));
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
            scope.files.contains_key(&scope.root.join(name)),
            "missed {name}"
        );
    }
    assert!(!scope.files.contains_key(&scope.root.join("text.txt")));
    check_inventory(allowed.path());
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
            std::panic::catch_unwind(|| check_inventory(directory.path())).is_err(),
            "missed {name}"
        );
    }
    let untracked = allowed.path().join("unreferenced.rs");
    std::fs::write(untracked, "#![allow(clippy::all)]").unwrap();
    check_inventory(allowed.path());
    for name in [
        "Cargo.toml",
        "nested/Cargo.toml",
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
    check_inventory(&inner);
    let missing = tempfile::tempdir().unwrap();
    assert!(std::panic::catch_unwind(|| tracked(missing.path())).is_err());
}

#[test]
fn sqlcipher_inventory_refuses_policy_name_case_variants_on_every_filesystem() {
    let clippy = include_str!("../../../../../clippy.toml");
    let canonical = fixture(&[
        ("nested/Cargo.toml", ""),
        ("nested/.cargo/config", ""),
        ("nested/.cargo/config.toml", ""),
        ("nested/clippy.toml", clippy),
        ("nested/.clippy.toml", clippy),
        ("nested/NotCargo.toml", "not a policy input"),
        ("nested/Cargo.toml.extra", "not a policy input"),
    ]);
    tracked(canonical.path());
    for (name, source) in [
        (
            "nested/cargo.toml",
            "[lints.clippy]\ndisallowed_methods='allow'",
        ),
        (
            "nested/CARGO.TOML",
            "[lints.clippy]\ndisallowed_methods='allow'",
        ),
        ("nested/.Cargo/config.toml", ""),
        ("nested/.CARGO/config", ""),
        ("nested/.Cargo/asset.txt", "not a policy input"),
        ("nested/.cargo/Config.toml", ""),
        ("nested/.cargo/CONFIG", ""),
        ("nested/Clippy.toml", clippy),
        ("nested/.Clippy.toml", clippy),
    ] {
        assert!(
            std::panic::catch_unwind(|| require_policy_spelling(Path::new(name))).is_err(),
            "accepted case-variant spelling {name}"
        );
        let directory = fixture(&[(name, source)]);
        assert!(
            std::panic::catch_unwind(|| tracked(directory.path())).is_err(),
            "accepted case-variant policy name {name}"
        );
    }
}

#[cfg(unix)]
#[test]
fn sqlcipher_inventory_refuses_real_directory_and_config_symlinks() {
    use std::os::unix::fs::symlink;
    let source = tempfile::tempdir().unwrap();
    std::fs::write(source.path().join("rogue.rs"), "#![allow(clippy::all)]").unwrap();
    let directory = fixture(&[("src/lib.rs", "pub struct Allowed;")]);
    check_inventory(directory.path());
    symlink(source.path(), directory.path().join("linked_source")).unwrap();
    bounded_output(
        command("git")
            .current_dir(directory.path())
            .args(["add", "linked_source"]),
        "symlink index",
    );
    assert!(std::panic::catch_unwind(|| check_inventory(directory.path())).is_err());
    for name in [
        ".cargo",
        "src/.cargo",
        "src/clippy.toml",
        "src/.clippy.toml",
    ] {
        let directory = fixture(&[("src/lib.rs", "pub struct Allowed;")]);
        symlink(source.path(), directory.path().join(name)).unwrap();
        assert!(
            std::panic::catch_unwind(|| check_inventory(directory.path())).is_err(),
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
    assert!(std::panic::catch_unwind(|| check_inventory(directory.path())).is_err());
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
    assert!(std::panic::catch_unwind(|| check_inventory(directory.path())).is_err());
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
    assert!(std::panic::catch_unwind(|| check_inventory(directory.path())).is_err());
    let config = tempfile::tempdir().unwrap();
    std::fs::write(
        config.path().join("config.toml"),
        "[build]\nrustflags='-Aclippy::all'",
    )
    .unwrap();
    let directory = fixture(&[("src/lib.rs", "pub struct Allowed;")]);
    symlink(config.path(), directory.path().join(".cargo")).unwrap();
    assert!(std::panic::catch_unwind(|| check_inventory(directory.path())).is_err());
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

#[test]
fn sqlcipher_cargo_metadata_rejects_invalid_shape_members_and_source_kinds() {
    let dir = fixture(&[
        ("Cargo.toml", "[package]\nname='fixture'\nversion='0.1.0'"),
        ("src/lib.rs", "pub struct Allowed;"),
        ("src/source.RS", "pub struct Unsupported;"),
        (".cargo/config", ""),
    ]);
    std::fs::create_dir(dir.path().join("untracked")).unwrap();
    std::fs::write(
        dir.path().join("untracked/Cargo.toml"),
        "[package]\nname='untracked'\nversion='0.1.0'",
    )
    .unwrap();
    let scope = tracked(dir.path());
    let root = &scope.root;
    let value = serde_json::json!({
        "version": 1, "workspace_root": root,
        "workspace_members": ["fixture"],
        "packages": [{"id": "fixture", "manifest_path": root.join("Cargo.toml"),
            "dependencies": [], "targets": [{"kind":["lib"],"src_path": root.join("src/lib.rs")}]}]
    });
    let (manifests, roots) = super::opener_sources::metadata(&scope, &value);
    let mut local_and_remote = value.clone();
    local_and_remote["packages"][0]["dependencies"] = serde_json::json!([
        {"kind":null,"path": root, "rename": "self_reference"}, {"kind":null,"name": "remote", "source": "registry"}
    ]);
    assert_eq!(
        super::opener_sources::metadata(&scope, &local_and_remote),
        (manifests.clone(), roots.clone())
    );
    assert!(manifests.contains(&root.join("Cargo.toml")));
    assert_eq!(roots, BTreeSet::from([root.join("src/lib.rs")]));
    for (pointer, replacement) in [
        ("/version", serde_json::json!(0)),
        ("/version", serde_json::json!("1")),
        ("/workspace_root", serde_json::json!(root.parent().unwrap())),
        ("/workspace_root", serde_json::Value::Null),
        ("/packages", serde_json::json!([])),
        ("/packages", serde_json::Value::Null),
        ("/packages/0/id", serde_json::Value::Null),
        (
            "/packages/0/manifest_path",
            serde_json::json!(root.join("clippy.toml")),
        ),
        ("/packages/0/manifest_path", serde_json::Value::Null),
        (
            "/packages/0/manifest_path",
            serde_json::json!(root.join("untracked/Cargo.toml")),
        ),
        (
            "/packages/0/manifest_path",
            serde_json::json!(root.parent().unwrap().join("Cargo.toml")),
        ),
        ("/packages/0/dependencies", serde_json::Value::Null),
        (
            "/packages/0/dependencies",
            serde_json::json!([{"kind":null,"path": null}]),
        ),
        (
            "/packages/0/dependencies",
            serde_json::json!([{"kind":null,"path": root.join("missing")} ]),
        ),
        (
            "/packages/0/dependencies",
            serde_json::json!([{"kind":null,"path": root.parent().unwrap()}]),
        ),
        ("/packages/0/targets", serde_json::Value::Null),
        ("/packages/0/targets", serde_json::json!([])),
        ("/packages/0/targets/0/src_path", serde_json::Value::Null),
        (
            "/packages/0/targets/0/src_path",
            serde_json::json!(root.join("src/missing.rs")),
        ),
        (
            "/packages/0/targets/0/src_path",
            serde_json::json!(root.join(".cargo/config")),
        ),
        (
            "/packages/0/targets/0/src_path",
            serde_json::json!(root.join("src/source.RS")),
        ),
        ("/workspace_members", serde_json::Value::Null),
        ("/workspace_members", serde_json::json!([])),
        ("/workspace_members", serde_json::json!([null])),
        ("/workspace_members", serde_json::json!(["unknown"])),
        (
            "/packages",
            serde_json::json!([value["packages"][0], value["packages"][0]]),
        ),
    ] {
        let mut invalid = value.clone();
        *invalid.pointer_mut(pointer).unwrap() = replacement;
        assert!(
            std::panic::catch_unwind(|| super::opener_sources::metadata(&scope, &invalid)).is_err(),
            "accepted {pointer}: {invalid}"
        );
    }
}

#[test]
fn sqlcipher_manifest_source_declarations_validate_every_local_spelling() {
    let dir = fixture(&[
        ("Cargo.toml", "[workspace]\nmembers=['member']"),
        (
            "member/Cargo.toml",
            "[package]\nname='member'\nversion='0.1.0'",
        ),
    ]);
    std::fs::create_dir(dir.path().join("untracked")).unwrap();
    std::fs::write(
        dir.path().join("untracked/Cargo.toml"),
        "[package]\nname='untracked'\nversion='0.1.0'",
    )
    .unwrap();
    let scope = tracked(dir.path());
    let manifest = scope.root.join("Cargo.toml");
    for source in [
        "[workspace]\nmembers=['member']\ndefault-members=['member']\nexclude=['member']",
        "workspace={members=['member'],default-members=['member'],exclude=['member']}",
        "workspace.members=['member',]",
        "package.metadata={}",
        "workspace.members=['member']",
        "workspace.default-members=['member']",
        "workspace.exclude=['member']",
        "dependencies={renamed={package='member',path='member'}}",
        "[target.'cfg(unix)'.dev-dependencies]\nhelper={path='member'}",
        "[target.'cfg(unix)'.build-dependencies.helper]\npath='member'",
        "[dev-dependencies.self_ref]\npath='.'",
        "[[bin]]\nname='custom'\npath='src/main.rs'",
        "[workspace.dependencies]\nunused={path='member'}",
        "workspace.dependencies.unused.path='member'",
        "[dependencies.renamed]\npackage='member'\npath='member'\nversion='0.1.0'",
        "[dev-dependencies]\nrenamed={package='member',path='member',optional=true}",
        "[build-dependencies.helper]\npath='member'",
        "[target.'cfg(unix)'.dependencies]\nhelper={path='member'}",
        "[package]\nworkspace='.'",
        "[dependencies]\nremote={git='https://example.test/remote'}",
        "[package.metadata.dependencies.example]\npath='untracked'\ngit='file:///documentation'",
        "[package.metadata]\ndependencies={example={path='untracked'}}",
    ] {
        super::opener_policy::manifest_paths(&scope, &manifest, source);
        // Every local source declaration must catch the corresponding untracked manifest too.
        let missing = source
            .replace("'member'", "'untracked'")
            .replace("workspace='.'", "workspace='untracked'");
        if missing != source {
            assert!(
                std::panic::catch_unwind(|| super::opener_policy::manifest_paths(
                    &scope, &manifest, &missing
                ))
                .is_err(),
                "accepted {missing}"
            );
        }
    }
    for source in [
        "[patch]",
        "['patch'.'crates-io']",
        "patch={crates-io={member={path='member'}}}",
        "patch.crates-io.member={package='member',path='member'}",
        "[replace]",
        "replace={'member:0.1.0'={path='member'}}",
        "[dependencies]\nmember={git='file:///tmp/repo'}",
        "[dependencies.member]\ngit='FILE:///tmp/repo'",
        "[dependencies]\nhelper={git='file:/tmp/repo'}",
        "[workspace]\nmembers=['*']",
        "workspace.default-members=['member?']",
        "workspace.exclude=['[ab]']",
        "workspace.members='member'",
        "[workspace.dependencies]\nunused={path='../outside'}",
    ] {
        assert!(
            std::panic::catch_unwind(|| super::opener_policy::manifest_paths(
                &scope, &manifest, source
            ))
            .is_err(),
            "accepted {source}"
        );
    }
}

#[test]
fn sqlcipher_cargo_closure_covers_real_dependency_spellings_and_unused_paths() {
    for (declaration, dependency) in [
        ("[dependencies]\nhelper={path='helper',version='0.1.0'}", true),
        ("[dependencies.renamed]\npackage='helper'\npath='helper'", true),
        ("[dev-dependencies]\nhelper={path='helper'}", true),
        ("[target.'cfg(any())'.dependencies]\nhelper={path='helper'}", true),
        ("[dependencies]\nhelper={path='helper',optional=true}", true),
        ("[workspace.dependencies]\nhelper={path='helper'}\n[dependencies]\nhelper.workspace=true", true),
        ("[workspace.dependencies]\nunused={path='helper'}", false),
        ("[workspace.dependencies.unused]\npath='helper'", false),
    ] {
        let manifest = format!("[workspace]\nmembers=['helper']\n[package]\nname='fixture'\nversion='0.1.0'\nedition='2021'\n{declaration}\n");
        let lock = format!("version=4\n[[package]]\nname='fixture'\nversion='0.1.0'\n{}\n[[package]]\nname='helper'\nversion='0.1.0'\n", if dependency { "dependencies=['helper']" } else { "" });
        let dir = fixture(&[
            ("Cargo.toml", &manifest), ("Cargo.lock", &lock), ("src/lib.rs", "pub struct Allowed;"),
            ("helper/Cargo.toml", "[package]\nname='helper'\nversion='0.1.0'\nedition='2021'"),
            ("helper/src/lib.rs", "pub struct Helper;"),
        ]);
        let before = std::fs::read(dir.path().join("Cargo.lock")).unwrap();
        super::check_source_inventory(dir.path());
        assert_eq!(std::fs::read(dir.path().join("Cargo.lock")).unwrap(), before);
        std::fs::create_dir(dir.path().join("untracked")).unwrap();
        std::fs::write(dir.path().join("untracked/Cargo.toml"), "[package]\nname='helper'\nversion='0.1.0'").unwrap();
        // Change only the local path, preserving member/package names and the valid lock.
        let missing = manifest.replace("path='helper'", "path='untracked'");
        std::fs::write(dir.path().join("Cargo.toml"), missing).unwrap();
        assert!(std::panic::catch_unwind(|| super::check_source_inventory(dir.path())).is_err(), "accepted {declaration}");
        assert_eq!(std::fs::read(dir.path().join("Cargo.lock")).unwrap(), before);
    }
}

#[test]
fn sqlcipher_cargo_closure_covers_virtual_workspace_and_every_target_kind() {
    let dir = fixture(&[
        ("Cargo.toml", "[workspace]\nmembers=['member']\ndefault-members=['member']\nresolver='2'"),
        ("Cargo.lock", "version=4\n[[package]]\nname='member'\nversion='0.1.0'\n"),
        ("member/Cargo.toml", "[package]\nname='member'\nversion='0.1.0'\nedition='2021'\n[features]\nextra=[]\n[lib]\npath='custom.rs'\n[[bin]]\nname='gated'\npath='gated.rs'\nrequired-features=['extra']\n[[test]]\nname='explicit'\npath='explicit-test.rs'\n[[example]]\nname='explicit'\npath='explicit-example.rs'\n[[bench]]\nname='explicit'\npath='explicit-bench.rs'"),
        ("member/custom.rs", "pub struct Allowed;"), ("member/gated.rs", "fn main() {}"),
        ("member/explicit-test.rs", ""), ("member/explicit-example.rs", "fn main() {}"),
        ("member/explicit-bench.rs", "fn main() {}"),
        ("member/src/bin/auto.rs", "fn main() {}"), ("member/tests/auto.rs", ""),
        ("member/examples/auto.rs", "fn main() {}"), ("member/benches/auto.rs", "fn main() {}"),
    ]);
    let scope = tracked(dir.path());
    let roots = super::opener_sources::cargo_roots(&scope);
    for name in [
        "custom.rs",
        "gated.rs",
        "explicit-test.rs",
        "explicit-example.rs",
        "explicit-bench.rs",
        "src/bin/auto.rs",
        "tests/auto.rs",
        "examples/auto.rs",
        "benches/auto.rs",
    ] {
        assert!(
            roots.contains(&scope.root.join("member").join(name)),
            "missed {name}"
        );
    }
    let before = std::fs::read(dir.path().join("Cargo.lock")).unwrap();
    // A default, auto-discovered build script is also a source root, even if untracked.
    std::fs::write(
        dir.path().join("member/Cargo.toml"),
        "[package]\nname='member'\nversion='0.1.0'\nedition='2021'\n[lib]\npath='custom.rs'",
    )
    .unwrap();
    std::fs::write(dir.path().join("member/build.rs"), "fn main() {}").unwrap();
    assert!(
        std::panic::catch_unwind(|| super::opener_sources::cargo_roots(&tracked(dir.path())))
            .is_err()
    );
    assert_eq!(
        std::fs::read(dir.path().join("Cargo.lock")).unwrap(),
        before
    );
}

#[test]
fn sqlcipher_cargo_source_closure_refuses_untracked_and_non_rust_target_roots() {
    for (name, tracked_input) in [
        ("hidden/entry.rs", false),
        (".cargo/config", true),
        ("src/entry.RS", true),
    ] {
        let manifest = format!(
            "[package]\nname='fixture'\nversion='0.1.0'\nedition='2021'\n[lib]\npath='{name}'"
        );
        let mut files = vec![
            ("Cargo.toml", manifest.as_str()),
            (
                "Cargo.lock",
                "version=4\n[[package]]\nname='fixture'\nversion='0.1.0'",
            ),
            ("src/lib.rs", "pub struct Allowed;"),
        ];
        if tracked_input {
            files.push((name, ""));
        }
        let dir = fixture(&files);
        if !tracked_input {
            let file = dir.path().join(name);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, "#![allow(warnings)]").unwrap();
        }
        let before = std::fs::read(dir.path().join("Cargo.lock")).unwrap();
        assert!(
            std::panic::catch_unwind(|| super::check_source_inventory(dir.path())).is_err(),
            "accepted {name}"
        );
        assert_eq!(
            std::fs::read(dir.path().join("Cargo.lock")).unwrap(),
            before
        );
    }
}

#[test]
fn sqlcipher_cargo_cut_preserves_baseline_module_admission() {
    let dir = fixture(&[("src/lib.rs", ""), ("src/custom.rs", "pub struct Allowed;")]);
    let file = dir.path().join("src/lib.rs");
    for source in [
        "mod external;",
        "mod inline { mod external; }",
        "#[r#path=\"custom.rs\"] mod custom;",
        "#[path=\"custom.rs\"] mod custom;",
    ] {
        super::check_rust_file(dir.path(), &file, source);
    }
    for source in [
        "#[path=\"missing.rs\"] mod custom;",
        "#[path=\"../build/rogue.rs\"] mod custom;",
        "mod inline { #[path=\"custom.rs\"] mod custom; }",
        "include!(\"custom.rs\");",
    ] {
        assert!(
            std::panic::catch_unwind(|| super::check_rust_file(dir.path(), &file, source)).is_err(),
            "baseline behaviour changed: {source}"
        );
    }
}

#[cfg(unix)]
fn shell_path(path: &Path) -> String {
    format!("'{}'", path.to_str().unwrap().replace('\'', "'\\''"))
}

#[cfg(unix)]
#[test]
fn sqlcipher_cargo_checks_config_first_and_refuses_failed_or_invalid_metadata() {
    use std::os::unix::fs::PermissionsExt;
    if let Some(root) = std::env::var_os("QUINN_CARGO_FIXTURE_CHILD") {
        assert!(
            std::panic::catch_unwind(|| super::check_source_inventory(Path::new(&root))).is_err()
        );
        return;
    }
    for (config, script, invoked, changed_lock) in [
        (
            "[build]\nrustflags='-Aclippy::all'",
            "exit 31",
            false,
            false,
        ),
        ("", "exit 31", true, false),
        ("", "printf invalid-json", true, false),
        ("", "printf '{} invalid-json'", true, false),
        ("", "change-lock", true, true),
        ("", "manifest-invalid", false, false),
        ("", "manifest-invalid-url", false, false),
        ("", "manifest-invalid-legacy", false, false),
    ] {
        for toolchain in [false, true] {
            let manifest = match script {
                "manifest-invalid" => "[package]\nname='fixture'\nversion='0.1.0'\nedition='2021'\n[workspace.dependencies]\nunused={path='untracked'}",
                "manifest-invalid-url" => "[package]\nname='fixture'\nversion='0.1.0'\nedition='2021'\n[dependencies]\nhelper={git=' file:///tmp/repo'}",
                "manifest-invalid-legacy" => "[package]\nname='fixture'\nversion='0.1.0'\nedition='2021'\n[target.'cfg(any())'.build_dependencies]\nhelper={git='file:///tmp/repo'}",
                _ => "[package]\nname='fixture'\nversion='0.1.0'\nedition='2021'",
            };
            let dir = fixture(&[
                ("Cargo.toml", manifest),
                (
                    "Cargo.lock",
                    "version=4\n[[package]]\nname='fixture'\nversion='0.1.0'",
                ),
                ("src/lib.rs", "pub struct Allowed;"),
                (".cargo/config", config),
            ]);
            let root = dir.path().canonicalize().unwrap();
            let fake = tempfile::tempdir().unwrap();
            let marker = fake.path().join("invoked");
            let args = fake.path().join("args");
            let environment = fake.path().join("environment");
            let payload = fake.path().join("metadata.json");
            std::fs::write(
                &payload,
                serde_json::to_vec(&serde_json::json!({
                    "version": 1, "workspace_root": root, "workspace_members": ["fixture"],
                    "packages": [{"id": "fixture", "manifest_path": root.join("Cargo.toml"),
                        "dependencies": [], "targets": [{"kind":["lib"],"src_path": root.join("src/lib.rs")}]}]
                }))
                .unwrap(),
            )
            .unwrap();
            let action = if changed_lock {
                format!(
                    "/bin/cat {}\nprintf corrupt > {}",
                    shell_path(&payload),
                    shell_path(&root.join("Cargo.lock"))
                )
            } else {
                script.to_owned()
            };
            let cargo = fake.path().join("cargo");
            std::fs::write(&cargo, format!(
                "#!/bin/sh\n: > {}\nprintf '%s\\n' \"$@\" > {}\nprintf '%s\\n' \"${{HOME-unset}}\" \"${{PWD-unset}}\" \"${{RUSTUP_HOME-unset}}\" \"${{RUSTUP_TOOLCHAIN-unset}}\" \"${{RUSTFLAGS-unset}}\" \"${{RUSTC-unset}}\" \"${{CARGO_HOME-unset}}\" \"${{QUINN_UNLISTED-unset}}\" > {}\n{action}\n",
                shell_path(&marker), shell_path(&args), shell_path(&environment),
            )).unwrap();
            std::fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o755)).unwrap();
            let before = std::fs::read(root.join("Cargo.lock")).unwrap();
            let search = std::env::join_paths(
                std::iter::once(fake.path().to_owned())
                    .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
            )
            .unwrap();
            let mut child = Command::new(std::env::current_exe().unwrap());
            child
                .args([
                    "sqlcipher_cargo_checks_config_first_and_refuses_failed_or_invalid_metadata",
                    "--nocapture",
                ])
                .env("PATH", &search)
                .env("HOME", fake.path())
                .env("RUSTFLAGS", "fixture-flags")
                .env("RUSTC", "fixture-compiler")
                .env("CARGO_HOME", "fixture-cargo")
                .env("QUINN_UNLISTED", "fixture-unlisted")
                .env("QUINN_CARGO_FIXTURE_CHILD", &root);
            if toolchain {
                child
                    .env("RUSTUP_HOME", "fixture-rustup")
                    .env("RUSTUP_TOOLCHAIN", "fixture-toolchain");
            } else {
                child
                    .env_remove("RUSTUP_HOME")
                    .env_remove("RUSTUP_TOOLCHAIN");
            }
            let output = bounded_output(&mut child, "isolated Cargo refusal fixture");
            assert!(
                output.contains("1 passed; 0 failed"),
                "child did not run: {output}"
            );
            assert_eq!(marker.exists(), invoked, "Cargo invocation order changed");
            if invoked {
                let values = std::fs::read_to_string(environment).unwrap();
                assert_eq!(
                    values.lines().collect::<Vec<_>>(),
                    [
                        fake.path().to_str().unwrap(),
                        root.to_str().unwrap(),
                        if toolchain { "fixture-rustup" } else { "unset" },
                        if toolchain {
                            "fixture-toolchain"
                        } else {
                            "unset"
                        },
                        "unset",
                        "unset",
                        "unset",
                        "unset"
                    ]
                );
                assert_eq!(std::fs::read_to_string(args).unwrap(), format!("metadata\n--locked\n--offline\n--no-deps\n--format-version\n1\n--manifest-path\n{}\n", root.join("Cargo.toml").display()));
            }
            assert_eq!(
                std::fs::read(root.join("Cargo.lock")).unwrap() != before,
                changed_lock
            );
        }
    }
}

#[test]
fn sqlcipher_cargo_source_closure_requires_a_manifest_and_existing_lock() {
    for missing in ["Cargo.toml", "Cargo.lock"] {
        let mut files = vec![("src/lib.rs", "pub struct Allowed;")];
        for (name, body) in [
            ("Cargo.toml", "[package]\nname='fixture'\nversion='0.1.0'"),
            (
                "Cargo.lock",
                "version=4\n[[package]]\nname='fixture'\nversion='0.1.0'",
            ),
        ] {
            if name != missing {
                files.push((name, body));
            }
        }
        let dir = fixture(&files);
        assert!(
            std::panic::catch_unwind(|| super::check_source_inventory(dir.path())).is_err(),
            "accepted missing {missing}"
        );
    }
}

#[cfg(unix)]
#[test]
fn sqlcipher_cargo_metadata_queries_each_workspace_once_and_refuses_omitted_manifests() {
    use std::os::unix::fs::PermissionsExt;
    if let Some(root) = std::env::var_os("QUINN_CARGO_COUNTER_CHILD") {
        let result = std::panic::catch_unwind(|| super::check_source_inventory(Path::new(&root)));
        assert_eq!(
            result.is_ok(),
            std::env::var_os("QUINN_CARGO_COMPLETE").is_some()
        );
        return;
    }
    for complete in [false, true] {
        let dir = fixture(&[
            ("Cargo.toml", "[workspace]\nmembers=['helper']\n[package]\nname='fixture'\nversion='0.1.0'"),
            ("Cargo.lock", "version=4\n[[package]]\nname='fixture'\nversion='0.1.0'\n[[package]]\nname='helper'\nversion='0.1.0'"),
            ("src/lib.rs", "pub struct Allowed;"),
            ("helper/Cargo.toml", "[package]\nname='helper'\nversion='0.1.0'"), ("helper/src/lib.rs", "pub struct Helper;"),
        ]);
        let root = dir.path().canonicalize().unwrap();
        let mut packages = vec![
            serde_json::json!({"id": "fixture", "manifest_path": root.join("Cargo.toml"), "dependencies": [], "targets": [{"kind":["lib"],"src_path": root.join("src/lib.rs")} ]}),
        ];
        let mut members = vec!["fixture"];
        if complete {
            packages.push(serde_json::json!({"id": "helper", "manifest_path": root.join("helper/Cargo.toml"), "dependencies": [], "targets": [{"kind":["lib"],"src_path": root.join("helper/src/lib.rs")} ]}));
            members.push("helper");
        }
        let fake = tempfile::tempdir().unwrap();
        let marker = fake.path().join("invocations");
        let payload = fake.path().join("metadata.json");
        std::fs::write(&payload, serde_json::to_vec(&serde_json::json!({"version":1,"workspace_root":root,"workspace_members":members,"packages":packages})).unwrap()).unwrap();
        let cargo = fake.path().join("cargo");
        std::fs::write(
            &cargo,
            format!(
                "#!/bin/sh\nprintf 'called\\n' >> {}\n/bin/cat {}\n",
                shell_path(&marker),
                shell_path(&payload)
            ),
        )
        .unwrap();
        std::fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o755)).unwrap();
        let search = std::env::join_paths(
            std::iter::once(fake.path().to_owned())
                .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap());
        child.args(["sqlcipher_cargo_metadata_queries_each_workspace_once_and_refuses_omitted_manifests", "--nocapture"]).env("PATH", search).env("QUINN_CARGO_COUNTER_CHILD", &root);
        if complete {
            child.env("QUINN_CARGO_COMPLETE", "1");
        } else {
            child.env_remove("QUINN_CARGO_COMPLETE");
        }
        let output = bounded_output(&mut child, "isolated Cargo workspace fixture");
        assert!(
            output.contains("1 passed; 0 failed"),
            "child did not run: {output}"
        );
        assert_eq!(
            std::fs::read_to_string(marker)
                .unwrap()
                .matches("called")
                .count(),
            if complete { 1 } else { 2 }
        );
    }
}

#[cfg(unix)]
#[test]
fn sqlcipher_manifest_glob_refusal_is_independent_of_tracked_path_membership() {
    let dir = fixture(&[
        ("Cargo.toml", "[workspace]"),
        (
            "star*/Cargo.toml",
            "[package]\nname='star'\nversion='0.1.0'",
        ),
        (
            "question?/Cargo.toml",
            "[package]\nname='question'\nversion='0.1.0'",
        ),
        (
            "open[/Cargo.toml",
            "[package]\nname='open'\nversion='0.1.0'",
        ),
        (
            "close]/Cargo.toml",
            "[package]\nname='close'\nversion='0.1.0'",
        ),
    ]);
    let scope = tracked(dir.path());
    for field in ["members", "default-members", "exclude"] {
        for name in ["star*", "question?", "open[", "close]"] {
            let manifest = scope.root.join("Cargo.toml");
            let source = format!("workspace.{field}=['{name}']");
            assert!(
                std::panic::catch_unwind(|| super::opener_policy::manifest_paths(
                    &scope, &manifest, &source
                ))
                .is_err(),
                "accepted tracked glob {source}"
            );
        }
    }
}

#[test]
fn sqlcipher_cargo_uses_disabled_discovery_and_refuses_broken_manifests() {
    let manifest = "[package]\nname='fixture'\nversion='0.1.0'\nedition='2021'\nbuild=false\nautobins=false\nautotests=false\nautoexamples=false\nautobenches=false\n[lib]\npath='src/lib.rs'";
    let dir = fixture(&[
        ("Cargo.toml", manifest),
        (
            "Cargo.lock",
            "version=4\n[[package]]\nname='fixture'\nversion='0.1.0'",
        ),
        ("src/lib.rs", "pub struct Allowed;"),
    ]);
    for name in [
        "build.rs",
        "src/bin/automatic.rs",
        "tests/automatic.rs",
        "examples/automatic.rs",
        "benches/automatic.rs",
    ] {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "#![allow(warnings)]").unwrap();
    }
    let scope = tracked(dir.path());
    assert_eq!(
        super::opener_sources::cargo_roots(&scope),
        BTreeSet::from([scope.root.join("src/lib.rs")])
    );
    let before = std::fs::read(dir.path().join("Cargo.lock")).unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        manifest.replace("build=false", "build='build.rs'"),
    )
    .unwrap();
    assert!(std::panic::catch_unwind(|| super::check_source_inventory(dir.path())).is_err());
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname='fixture'\nversion='not-a-version'",
    )
    .unwrap();
    assert!(std::panic::catch_unwind(|| super::check_source_inventory(dir.path())).is_err());
    assert_eq!(
        std::fs::read(dir.path().join("Cargo.lock")).unwrap(),
        before
    );
}

#[test]
fn sqlcipher_rust_source_kind_refuses_noncanonical_extension_aliases() {
    let dir = fixture(&[("src/lib.rs", "pub struct Allowed;")]);
    let scope = tracked(dir.path());
    let source = scope.root.join("src/lib.rs");
    assert_eq!(scope.require_rust(&source), source);
    for extension in ["RS", "Rs", "rS"] {
        let alias = source.with_extension(extension);
        assert!(
            std::panic::catch_unwind(|| scope.require_rust(&alias)).is_err(),
            "accepted Rust extension alias {}",
            alias.display()
        );
    }
}

#[test]
fn sqlcipher_metadata_requires_the_tracked_virtual_workspace_manifest() {
    let dir = fixture(&[
        (
            "helper/Cargo.toml",
            "[package]\nname='helper'\nversion='0.1.0'",
        ),
        ("helper/src/lib.rs", "pub struct Helper;"),
    ]);
    let scope = tracked(dir.path());
    let value = serde_json::json!({"version":1,"workspace_root":scope.root,"workspace_members":["helper"],"packages":[{
        "id":"helper","manifest_path":scope.root.join("helper/Cargo.toml"),"dependencies":[],"targets":[{"kind":["lib"],"src_path":scope.root.join("helper/src/lib.rs")}]
    }]});
    assert!(std::panic::catch_unwind(|| super::opener_sources::metadata(&scope, &value)).is_err());
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[workspace]\nmembers=['helper']",
    )
    .unwrap();
    bounded_output(
        command("git")
            .current_dir(dir.path())
            .args(["add", "Cargo.toml"]),
        "track workspace root",
    );
    let scope = tracked(dir.path());
    let (manifests, _) = super::opener_sources::metadata(&scope, &value);
    assert!(manifests.contains(&scope.root.join("Cargo.toml")));
}

#[test]
fn sqlcipher_source_inventory_records_only_the_supported_kinds() {
    let dir = fixture(&[
        ("Cargo.toml", "[package]\nname='fixture'\nversion='0.1.0'"),
        ("src/lib.rs", "pub struct Allowed;"),
        ("build/script.rs", "pub struct Build;"),
        (".cargo/config", ""),
        ("src/unsupported.RS", ""),
        ("readme.txt", ""),
    ]);
    let scope = tracked(dir.path());
    for (name, kind) in [
        ("Cargo.toml", SourceKind::Config),
        ("clippy.toml", SourceKind::Config),
        (".cargo/config", SourceKind::Config),
        ("src/lib.rs", SourceKind::Rust),
        ("build/script.rs", SourceKind::Rust),
    ] {
        assert_eq!(
            scope.files.get(&scope.root.join(name)),
            Some(&kind),
            "wrong source kind: {name}"
        );
    }
    for name in ["src/unsupported.RS", "readme.txt"] {
        assert!(
            !scope.files.contains_key(&scope.root.join(name)),
            "unsupported input selected: {name}"
        );
    }
}

#[test]
fn sqlcipher_cargo_validates_declarations_in_already_seen_members() {
    let member = "[package]\nname='member'\nversion='0.1.0'\nedition='2021'";
    let dir = fixture(&[
        (
            "Cargo.toml",
            "[workspace]\nmembers=['member']\nresolver='2'",
        ),
        (
            "Cargo.lock",
            "version=4\n[[package]]\nname='member'\nversion='0.1.0'",
        ),
        ("member/Cargo.toml", member),
        ("member/src/lib.rs", "pub struct Member;"),
    ]);
    super::check_source_inventory(dir.path());
    // Cargo ignores a patch in a member; the source policy still rejects the whole family.
    std::fs::write(
        dir.path().join("member/Cargo.toml"),
        format!("{member}\n[patch.crates-io]\nunused={{path='untracked'}}"),
    )
    .unwrap();
    assert!(std::panic::catch_unwind(|| super::check_source_inventory(dir.path())).is_err());
}

#[test]
fn sqlcipher_cargo_refuses_a_separate_nested_workspace() {
    let dir = fixture(&[
        ("Cargo.toml", "[workspace]\nexclude=['nested']\n[package]\nname='fixture'\nversion='0.1.0'\nedition='2021'"),
        ("Cargo.lock", "version=4\n[[package]]\nname='fixture'\nversion='0.1.0'"), ("src/lib.rs", "pub struct Allowed;"),
        ("nested/Cargo.toml", "[workspace]\n[package]\nname='nested'\nversion='0.1.0'\nedition='2021'"),
        ("nested/Cargo.lock", "version=4\n[[package]]\nname='nested'\nversion='0.1.0'"), ("nested/src/lib.rs", "pub struct Nested;"),
    ]);
    let before = std::fs::read(dir.path().join("Cargo.lock")).unwrap();
    assert!(std::panic::catch_unwind(|| super::check_source_inventory(dir.path())).is_err());
    assert_eq!(
        std::fs::read(dir.path().join("Cargo.lock")).unwrap(),
        before
    );
}

#[test]
fn sqlcipher_manifest_git_urls_refuse_whitespace_normalization() {
    let dir = fixture(&[("Cargo.toml", "[package]\nname='fixture'\nversion='0.1.0'")]);
    let scope = tracked(dir.path());
    super::opener_policy::manifest_paths(
        &scope,
        &scope.root.join("Cargo.toml"),
        "[dependencies]\nremote={git='https://example.test/remote'}",
    );
    let mut admitted = Vec::new();
    for url in [
        "' file:///tmp/repo'",
        "'file:///tmp/repo '",
        "\"\\tfile:///tmp/repo\"",
        "\"f\\ti\\rl\\ne:///tmp/repo\"",
        "\"\\u0000file:///tmp/repo\"",
    ] {
        let source = format!("[dependencies]\nhelper={{git={url}}}");
        if std::panic::catch_unwind(|| {
            super::opener_policy::manifest_paths(&scope, &scope.root.join("Cargo.toml"), &source)
        })
        .is_ok()
        {
            admitted.push(url);
        }
    }
    assert!(admitted.is_empty(), "admitted local Git URLs: {admitted:?}");
}

#[test]
fn sqlcipher_manifest_validates_legacy_dependency_tables() {
    let dir = fixture(&[
        ("Cargo.toml", "[workspace]\nmembers=['member']"),
        (
            "member/Cargo.toml",
            "[package]\nname='member'\nversion='0.1.0'",
        ),
    ]);
    let scope = tracked(dir.path());
    let mut admitted = Vec::new();
    for table in [
        "dev_dependencies",
        "build_dependencies",
        "target.'cfg(any())'.dev_dependencies",
        "target.'cfg(any())'.build_dependencies",
    ] {
        for form in [
            format!("[{table}]\nhelper={{path='member'}}"),
            format!("[{table}.helper]\npath='member'"),
            format!("{table}.helper.path='member'"),
        ] {
            super::opener_policy::manifest_paths(&scope, &scope.root.join("Cargo.toml"), &form);
            let invalid = form.replace("'member'", "'untracked'");
            if std::panic::catch_unwind(|| {
                super::opener_policy::manifest_paths(
                    &scope,
                    &scope.root.join("Cargo.toml"),
                    &invalid,
                )
            })
            .is_ok()
            {
                admitted.push(invalid);
            }
        }
        let remote = format!("[{table}]\nhelper={{git='https://example.test/remote'}}");
        super::opener_policy::manifest_paths(&scope, &scope.root.join("Cargo.toml"), &remote);
        let local = remote.replace("https://example.test/remote", "file:///tmp/repo");
        if std::panic::catch_unwind(|| {
            super::opener_policy::manifest_paths(&scope, &scope.root.join("Cargo.toml"), &local)
        })
        .is_ok()
        {
            admitted.push(local);
        }
    }
    assert!(
        admitted.is_empty(),
        "admitted legacy source declarations: {admitted:?}"
    );
}

#[test]
fn sqlcipher_cargo_normalized_local_git_aliases_are_refused() {
    let mut admitted = Vec::new();
    for (table, url) in [
        ("dependencies", "' file:///tmp/repo'"),
        ("dependencies", "\"f\\ti\\rl\\ne:///tmp/repo\""),
        ("dev_dependencies", "'file:///tmp/repo'"),
        ("build_dependencies", "'file:///tmp/repo'"),
        ("target.'cfg(any())'.dev_dependencies", "'file:///tmp/repo'"),
        (
            "target.'cfg(any())'.build_dependencies",
            "'file:///tmp/repo'",
        ),
    ] {
        let manifest = format!("[package]\nname='fixture'\nversion='0.1.0'\nedition='2021'\n[{table}]\nhelper={{git={url}}}");
        let dir = fixture(&[
            ("Cargo.toml", &manifest),
            (
                "Cargo.lock",
                "version=4\n[[package]]\nname='fixture'\nversion='0.1.0'",
            ),
            ("src/lib.rs", "pub struct Allowed;"),
        ]);
        let output = bounded_output(
            Command::new("cargo").current_dir(dir.path()).args([
                "metadata",
                "--locked",
                "--offline",
                "--no-deps",
                "--format-version",
                "1",
            ]),
            "real Cargo local Git normalization fixture",
        );
        let metadata: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(
            metadata["packages"][0]["dependencies"][0]["source"]
                .as_str()
                .unwrap()
                .starts_with("git+file:"),
            "Cargo did not normalize {table}: {url}"
        );
        let before = std::fs::read(dir.path().join("Cargo.lock")).unwrap();
        if std::panic::catch_unwind(|| super::check_source_inventory(dir.path())).is_ok() {
            admitted.push(format!("{table}: {url}"));
        }
        assert_eq!(
            std::fs::read(dir.path().join("Cargo.lock")).unwrap(),
            before
        );
    }
    assert!(
        admitted.is_empty(),
        "inventory admitted local Git: {admitted:?}"
    );
}
