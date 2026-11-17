//! Workspace build-script pins and proc-macro refusal. Third-party/toolchain code remains trusted.
use super::opener_scope::SourceScope;
use std::path::Path;

#[path = "opener_build_pins.rs"]
mod pins;

pub(super) fn script(scope: &SourceScope, package: &str, path: &Path) {
    let relative = path
        .strip_prefix(&scope.root)
        .unwrap_or_else(|_| panic!("unreviewed build script for {package}: {}", path.display()));
    let (_, expected) = pins::SCRIPTS
        .iter()
        .find(|(name, _)| relative == Path::new(name))
        .unwrap_or_else(|| panic!("unreviewed build script for {package}: {}", path.display()));
    // The reviewed data is itself a required tracked, confined, regular Rust policy input.
    scope.require_rust(
        &scope
            .root
            .join("crates/org-office/src/store/tests/opener_build_pins.rs"),
    );
    let actual = std::fs::read_to_string(scope.require_rust(path)).unwrap();
    let tokens = |source: &str| {
        source
            .parse::<proc_macro2::TokenStream>()
            .unwrap_or_else(|error| {
                panic!(
                    "invalid build tokens for {package}: {}: {error}",
                    path.display()
                )
            })
            .to_string()
    };
    assert_eq!(
        tokens(&actual),
        tokens(expected),
        "changed build script for {package}: {}",
        path.display()
    );
}

pub(super) fn dependency(
    scope: &SourceScope,
    package: &str,
    manifest: &Path,
    value: &serde_json::Value,
) {
    match value.get("kind") {
        Some(serde_json::Value::Null) => return,
        Some(serde_json::Value::String(kind)) if kind == "dev" => return,
        Some(serde_json::Value::String(kind)) if kind == "build" => {}
        _ => panic!("unreviewed build dependency or invalid kind for {package}: {value}"),
    }
    assert_eq!(
        manifest,
        scope.root.join("crates/org-host/Cargo.toml"),
        "unreviewed build dependency caller for {package}: {value}"
    );
    scope.require(manifest);
    scope.require_rust(
        &scope
            .root
            .join("crates/org-office/src/store/tests/opener_build_pins.rs"),
    );
    // These are the two reviewed edges of the pinned host build script, not a
    // general build-helper allowance. Exact metadata also rejects aliases,
    // feature/target changes and local replacement of the registry compiler.
    let mut expected = serde_json::json!({
        "name": "cc", "source": "registry+https://github.com/rust-lang/crates.io-index",
        "req": "=1.11.1", "kind": "build", "rename": null, "optional": false,
        "uses_default_features": true, "features": [], "target": null, "registry": null
    });
    if value["name"] == "org-release" {
        expected["name"] = serde_json::json!("org-release");
        expected["source"] = serde_json::Value::Null;
        expected["req"] = serde_json::json!("*");
        expected["path"] = serde_json::json!(scope.root.join("crates/org-release"));
    }
    assert_eq!(
        value, &expected,
        "unreviewed build dependency for {package}"
    );
    if value["name"] == "org-release" {
        pin_release_helper(scope, package);
    }
}

fn pin_release_helper(scope: &SourceScope, package: &str) {
    use sha2::{Digest, Sha256};
    // A pinned entry script must not call mutable workspace helper code. Pin
    // the helper manifest and its complete production module closure too.
    for (relative, expected) in pins::HOST_BUILD_HELPER_PINS {
        let path = scope.root.join(relative);
        let path = if path.extension().is_some_and(|extension| extension == "rs") {
            scope.require_rust(&path)
        } else {
            scope.require(&path)
        };
        let actual = format!("{:x}", Sha256::digest(std::fs::read(&path).unwrap()));
        assert_eq!(
            &actual,
            expected,
            "changed build helper for {package}: {}",
            path.display()
        );
    }
}

pub(super) fn target(scope: &SourceScope, package: &str, value: &serde_json::Value, path: &Path) {
    let kinds = value["kind"]
        .as_array()
        .unwrap_or_else(|| panic!("missing Cargo target kind for {package}"));
    assert!(!kinds.is_empty(), "empty Cargo target kind for {package}");
    for kind in kinds {
        let kind = kind
            .as_str()
            .unwrap_or_else(|| panic!("invalid Cargo target kind for {package}"));
        assert!(
            [
                "lib",
                "rlib",
                "dylib",
                "staticlib",
                "cdylib",
                "bin",
                "test",
                "example",
                "bench",
                "custom-build"
            ]
            .contains(&kind),
            "unreviewed Cargo target kind for {package}: {kind}"
        );
        if kind == "custom-build" {
            script(scope, package, path);
        }
    }
}
