//! Cargo source roots over the tracked, typed source scope.
use super::opener_scope::SourceScope;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub(super) fn cargo_roots(scope: &SourceScope) -> BTreeSet<PathBuf> {
    let lock = scope.root.join("Cargo.lock");
    let before = std::fs::read(&lock).expect("missing Cargo.lock");
    let manifests = scope
        .files
        .keys()
        .filter(|p| p.ends_with("Cargo.toml"))
        .collect::<Vec<_>>();
    // Validate all manifests before invoking Cargo, including unused workspace declarations.
    for manifest in &manifests {
        super::opener_policy::manifest_paths(
            scope,
            manifest,
            &std::fs::read_to_string(manifest).unwrap(),
        );
    }
    let mut seen = BTreeSet::new();
    let mut roots = BTreeSet::new();
    for manifest in manifests {
        if !seen.contains(manifest) {
            let mut command = std::process::Command::new("cargo");
            command.env_clear().current_dir(&scope.root);
            for name in ["PATH", "HOME", "RUSTUP_HOME", "RUSTUP_TOOLCHAIN"] {
                if let Some(value) = std::env::var_os(name) {
                    command.env(name, value);
                }
            }
            let output = super::bounded_output(
                command
                    .args([
                        "metadata",
                        "--locked",
                        "--offline",
                        "--no-deps",
                        "--format-version",
                        "1",
                        "--manifest-path",
                    ])
                    .arg(manifest),
                "Cargo source inventory",
            );
            let value = serde_json::from_str(&output).expect("invalid Cargo metadata JSON");
            let (packages, targets) = metadata(scope, &value);
            assert!(
                packages.contains(manifest),
                "Cargo omitted requested manifest"
            );
            seen.extend(packages);
            roots.extend(targets);
        }
    }
    assert!(!roots.is_empty(), "Cargo inventory has no source roots");
    assert_eq!(
        std::fs::read(lock).unwrap(),
        before,
        "Cargo changed Cargo.lock"
    );
    roots
}

pub(super) fn metadata(
    scope: &SourceScope,
    value: &serde_json::Value,
) -> (BTreeSet<PathBuf>, BTreeSet<PathBuf>) {
    assert_eq!(value["version"], 1, "unsupported Cargo metadata version");
    assert_eq!(
        Path::new(
            value["workspace_root"]
                .as_str()
                .expect("missing workspace root")
        ),
        scope.root,
        "Cargo workspace escapes repository"
    );
    let mut manifests = BTreeSet::from([scope.require(&scope.root.join("Cargo.toml"))]);
    let mut roots = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for package in value["packages"]
        .as_array()
        .expect("missing Cargo packages")
    {
        assert!(
            ids.insert(package["id"].as_str().expect("missing Cargo package id")),
            "duplicate Cargo package"
        );
        let manifest = Path::new(
            package["manifest_path"]
                .as_str()
                .expect("missing package manifest"),
        );
        assert!(
            manifest.ends_with("Cargo.toml"),
            "not a Cargo manifest: {}",
            manifest.display()
        );
        manifests.insert(scope.require(manifest));
        for dependency in package["dependencies"]
            .as_array()
            .expect("missing Cargo dependencies")
        {
            if let Some(path) = dependency.get("path") {
                scope.require(
                    &Path::new(path.as_str().expect("invalid local dependency path"))
                        .join("Cargo.toml"),
                );
            }
        }
        let targets = package["targets"]
            .as_array()
            .expect("missing Cargo targets");
        assert!(!targets.is_empty(), "Cargo package has no targets");
        for target in targets {
            roots.insert(scope.require_rust(Path::new(
                target["src_path"].as_str().expect("missing target source"),
            )));
        }
    }
    assert!(!ids.is_empty(), "Cargo inventory has no packages");
    let members = value["workspace_members"]
        .as_array()
        .expect("missing Cargo members");
    assert!(!members.is_empty(), "Cargo workspace has no members");
    for member in members {
        assert!(
            ids.contains(member.as_str().expect("invalid Cargo member")),
            "unknown Cargo member"
        );
    }
    (manifests, roots)
}
