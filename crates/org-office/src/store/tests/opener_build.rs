//! Closed workspace build-code admission. Third-party/toolchain build code remains trusted.
use super::opener_scope::SourceScope;
use std::path::Path;

#[path = "opener_build_pins.rs"]
mod pins;

pub(super) fn script(scope: &SourceScope, package: &str, path: &Path) {
    let relative = path
        .strip_prefix(&scope.root)
        .expect("build script outside repository");
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

pub(super) fn dependency(package: &str, value: &serde_json::Value) {
    // No workspace package currently needs a build helper. Refuse rather than leave
    // an unpinned dependency/helper closure behind a pinned entry script.
    match value.get("kind") {
        Some(serde_json::Value::Null) => {}
        Some(serde_json::Value::String(kind)) if kind == "dev" => {}
        _ => panic!("unreviewed build dependency or invalid kind for {package}: {value}"),
    }
}

pub(super) fn target(scope: &SourceScope, package: &str, value: &serde_json::Value, path: &Path) {
    let kinds = value["kind"].as_array().expect("missing Cargo target kind");
    assert!(!kinds.is_empty(), "empty Cargo target kind");
    for kind in kinds {
        let kind = kind.as_str().expect("invalid Cargo target kind");
        assert!(
            [
                "lib",
                "rlib",
                "dylib",
                "staticlib",
                "cdylib",
                "proc-macro",
                "bin",
                "test",
                "example",
                "bench",
                "custom-build"
            ]
            .contains(&kind),
            "unknown Cargo target kind: {kind}"
        );
        if kind == "custom-build" {
            script(scope, package, path);
        }
    }
}
