//! Workspace build-script pins and proc-macro refusal. Third-party/toolchain code remains trusted.
use super::opener_scope::{SourceKind, SourceScope};
use sha2::{Digest, Sha256};
pub(super) const NATIVE: &str = "crates/org-host/src/recovery/presence.m";
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
    if relative == Path::new("crates/org-host/build.rs") {
        native(scope, &scope.root.join(NATIVE));
    }
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

pub(super) fn native(scope: &SourceScope, path: &Path) {
    assert_eq!(
        path.strip_prefix(&scope.root).unwrap(),
        Path::new(NATIVE),
        "unreviewed native input"
    );
    let path = scope.require(path);
    assert_eq!(scope.files[&path], SourceKind::Native, "native input kind");
    let bytes = std::fs::read(&path).expect("native input");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        pins::NATIVE_SHA256,
        "changed native input"
    );
}
pub(super) fn dependency(
    scope: &SourceScope,
    package: &str,
    name: Option<&str>,
    manifest: &Path,
    value: &serde_json::Value,
) {
    use serde_json::json;
    match value.get("kind") {
        Some(serde_json::Value::Null) => return,
        Some(serde_json::Value::String(kind)) if kind == "dev" => return,
        Some(serde_json::Value::String(kind)) if kind == "build" => {}
        _ => panic!("unreviewed build dependency or invalid kind for {package}: {value}"),
    }
    // Bind the actual package name AND confined owner manifest, never a spoofed
    // metadata id or a helper name alone. Compare the complete approved tuple.
    let host =
        name == Some("org-host") && manifest == scope.root.join("crates/org-host/Cargo.toml");
    let web = name == Some("agentic-org-web")
        && manifest == scope.root.join("apps/agentic-org-web/Cargo.toml");
    let common = json!({"name":"org-release","source":null,"req":"*","kind":"build","rename":null,
        "optional":false,"uses_default_features":true,"features":[],"target":null,"registry":null,
        "path":scope.root.join("crates/org-release")});
    let cc = json!({"name":"cc","source":"registry+https://github.com/rust-lang/crates.io-index","req":"=1.11.1",
        "kind":"build","rename":null,"optional":false,"uses_default_features":true,"features":[],"target":null,"registry":null});
    assert!(
        ((host || web) && value == &common) || (host && value == &cc),
        "unreviewed build dependency for {package}: {value}"
    );
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
