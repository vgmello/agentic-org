//! Build-script inventory regressions use the same metadata entry point as the fence.
use super::opener_inventory::{fixture, tracked};
use super::opener_scope::SourceScope;
use super::opener_sources::metadata;
use serde_json::{json, Value};

const HOST: &str = "apps/org-office-host/build.rs";
const PIN: &str = "crates/org-office/src/store/tests/opener_build_pins.rs";
const HOST_SOURCE: &str = include_str!("../../../../../apps/org-office-host/build.rs");

fn package(scope: &SourceScope, id: &str, path: &str, kind: Value) -> Value {
    json!({"id":id,"manifest_path":scope.root.join("Cargo.toml"),
        "dependencies":[],"targets":[{"src_path":scope.root.join(path),"kind":kind}]})
}

fn inventory(scope: &SourceScope, packages: Vec<Value>) -> Value {
    json!({"version":1,"workspace_root":scope.root,
        "workspace_members":packages.iter().map(|p| p["id"].clone()).collect::<Vec<_>>(),
        "packages":packages})
}

fn host_fixture() -> tempfile::TempDir {
    fixture(&[
        ("Cargo.toml", "[package]\nname='fixture'\nversion='0.1.0'"),
        ("src/lib.rs", "pub struct Allowed;"),
        (HOST, HOST_SOURCE),
        ("build.rs", HOST_SOURCE),
        ("other.rs", HOST_SOURCE),
        ("scripts/helper.rs", HOST_SOURCE),
        (
            PIN,
            "// Protected pin fixture. The compiled policy supplies the reviewed tokens.",
        ),
    ])
}

fn error(action: impl FnOnce() + std::panic::UnwindSafe) -> String {
    let error = std::panic::catch_unwind(action).expect_err("accepted unreviewed build input");
    error
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| error.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap()
}

#[test]
fn sqlcipher_build_inventory_refuses_unknown_scripts_in_every_position() {
    let dir = host_fixture();
    let scope = tracked(dir.path());
    let good = package(&scope, "good", "src/lib.rs", json!(["lib"]));
    for path in ["build.rs", "other.rs", "scripts/helper.rs"] {
        for kind in [
            json!(["custom-build"]),
            json!(["custom-build", "bin"]),
            json!(["bin", "custom-build"]),
        ] {
            let bad = package(&scope, "unreviewed-package", path, kind);
            for packages in [
                vec![bad.clone(), good.clone()],
                vec![good.clone(), bad.clone()],
            ] {
                let value = inventory(&scope, packages);
                let message = error(|| {
                    metadata(&scope, &value);
                });
                assert!(
                    message.contains("unreviewed-package") && message.contains(path),
                    "{message}"
                );
            }
            let mut value = inventory(&scope, vec![good.clone()]);
            let target = bad["targets"][0].clone();
            for targets in [
                json!([target, good["targets"][0]]),
                json!([good["targets"][0], target]),
            ] {
                value["packages"][0]["targets"] = targets;
                error(|| {
                    metadata(&scope, &value);
                });
            }
        }
    }
}

#[test]
fn sqlcipher_build_inventory_pins_whole_script_tokens_and_protected_input() {
    let dir = host_fixture();
    let scope = tracked(dir.path());
    let value = inventory(
        &scope,
        vec![package(&scope, "host", HOST, json!(["custom-build"]))],
    );
    metadata(&scope, &value);
    for source in [
        format!("// ordinary comment\n{HOST_SOURCE}\n// another"),
        HOST_SOURCE.replace("fn main() {", "fn  main ( )  { /* whitespace */"),
    ] {
        std::fs::write(scope.root.join(HOST), source).unwrap();
        metadata(&scope, &value);
    }
    for source in [
        HOST_SOURCE.replace("ORG_BUNDLE", "OTHER_BUNDLE"),
        HOST_SOURCE.replace("fn main() {", "fn main() { println!(\"cargo:rustc-env=CLIPPY_CONF_DIR=/tmp\");"),
        HOST_SOURCE.replace("fn main() {", "fn main() { println!(\"cargo::rustc-env=CLIPPY_CONF_DIR=/tmp\");"),
        format!("{HOST_SOURCE}\nfn helper() {{ let key=[\"CLIPPY\",\"CONF\",\"DIR\"].join(\"_\"); println!(\"cargo::rustc-env={{key}}=/tmp\"); }}"),
        format!("{HOST_SOURCE}\ninclude!(\"helper.rs\");"),
        format!("{HOST_SOURCE}\nmod helper;"),
        format!("{HOST_SOURCE}\nextern crate helper;"),
        "fn main( {".to_string(),
    ] {
        std::fs::write(scope.root.join(HOST), &source).unwrap();
        let message = error(|| { metadata(&scope, &value); });
        assert!(message.contains("host") && message.contains(HOST), "{message}");
        if source == "fn main( {" {
            assert!(message.contains("invalid build tokens"), "{message}");
        }
    }
    std::fs::write(scope.root.join(HOST), HOST_SOURCE).unwrap();
    let mut untracked = tracked(dir.path());
    untracked.files.remove(&scope.root.join(PIN));
    error(|| {
        metadata(&untracked, &value);
    });
    let mut wrong_kind = tracked(dir.path());
    wrong_kind.files.insert(
        scope.root.join(PIN),
        super::opener_scope::SourceKind::Config,
    );
    error(|| {
        metadata(&wrong_kind, &value);
    });
    std::fs::remove_file(scope.root.join(PIN)).unwrap();
    error(|| {
        metadata(&scope, &value);
    });
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(scope.root.join("src/lib.rs"), scope.root.join(PIN)).unwrap();
        error(|| {
            metadata(&scope, &value);
        });
    }
}

#[test]
fn sqlcipher_build_inventory_refuses_build_helpers_and_unknown_metadata_kinds() {
    let dir = host_fixture();
    let scope = tracked(dir.path());
    let good = package(&scope, "fixture", "src/lib.rs", json!(["lib"]));
    let mut value = inventory(&scope, vec![good.clone()]);
    for kind in [Value::Null, json!("dev")] {
        value["packages"][0]["dependencies"] = json!([{"name":"helper","kind":kind}]);
        metadata(&scope, &value);
    }
    for dependency in [
        json!({"name":"helper","kind":"build"}),
        json!({"name":"helper"}),
        json!({"kind":"unknown"}),
        json!({"kind":5}),
    ] {
        value["packages"][0]["dependencies"] = json!([dependency]);
        error(|| {
            metadata(&scope, &value);
        });
    }
    for kind in [
        Value::Null,
        json!([]),
        json!([null]),
        json!("custom-build"),
        json!(["unknown"]),
        json!(["lib", "unknown"]),
        json!(["unknown", "lib"]),
    ] {
        let value = inventory(&scope, vec![package(&scope, "fixture", "src/lib.rs", kind)]);
        error(|| {
            metadata(&scope, &value);
        });
    }
    for kind in [
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
    ] {
        metadata(
            &scope,
            &inventory(
                &scope,
                vec![package(&scope, "fixture", "src/lib.rs", json!([kind]))],
            ),
        );
    }
    // A bad build dependency must be caught before/after good entries and in any package.
    let mut bad = good.clone();
    bad["id"] = json!("bad");
    for dependencies in [
        json!([{"kind":"build"},{"kind":null}]),
        json!([{"kind":null},{"kind":"build"}]),
    ] {
        bad["dependencies"] = dependencies;
        for packages in [
            vec![bad.clone(), good.clone()],
            vec![good.clone(), bad.clone()],
        ] {
            error(|| {
                metadata(&scope, &inventory(&scope, packages));
            });
        }
    }
}

#[test]
fn sqlcipher_build_inventory_cargo_discovers_custom_default_and_disabled_scripts() {
    for (setting, script, admitted) in [
        ("", "build.rs", false),
        ("build='other.rs'", "other.rs", false),
        ("build='scripts/custom.rs'", "scripts/custom.rs", false),
        ("build=false", "build.rs", true),
        ("build='apps/org-office-host/build.rs'", HOST, true),
    ] {
        let manifest = format!(
            "[workspace]\n[package]\nname='fixture'\nversion='0.1.0'\nedition='2021'\n{setting}\n"
        );
        let dir = fixture(&[
            ("Cargo.toml", &manifest),
            (
                "Cargo.lock",
                "version=4\n[[package]]\nname='fixture'\nversion='0.1.0'\n",
            ),
            ("src/lib.rs", "pub struct Allowed;"),
            (
                script,
                if script == HOST {
                    HOST_SOURCE
                } else {
                    "fn main() {}"
                },
            ),
            (PIN, "// Protected pin fixture."),
        ]);
        let lock = std::fs::read(dir.path().join("Cargo.lock")).unwrap();
        if admitted {
            super::check_source_inventory(dir.path());
        } else {
            let message = error(|| super::check_source_inventory(dir.path()));
            assert!(
                message.contains("fixture") && message.contains(script),
                "{message}"
            );
        }
        assert_eq!(std::fs::read(dir.path().join("Cargo.lock")).unwrap(), lock);
    }
}

#[test]
fn sqlcipher_build_inventory_cargo_refuses_all_build_dependency_spellings() {
    for declaration in [
        "[build-dependencies]\nhelper={path='helper'}",
        "[build_dependencies.helper]\npath='helper'",
        "[target.'cfg(any())'.build-dependencies.helper]\npath='helper'",
        "[target.'cfg(any())'.build_dependencies]\nhelper={path='helper'}",
        "[workspace.dependencies]\nhelper={path='helper'}\n[build-dependencies]\nhelper.workspace=true",
        "[build-dependencies.alias]\npackage='helper'\npath='helper'",
    ] {
        let manifest = format!("[workspace]\nmembers=['helper']\n[package]\nname='fixture'\nversion='0.1.0'\nedition='2021'\n{declaration}\n");
        let dir = fixture(&[
            ("Cargo.toml", &manifest),
            ("Cargo.lock", "version=4\n[[package]]\nname='fixture'\nversion='0.1.0'\ndependencies=['helper']\n[[package]]\nname='helper'\nversion='0.1.0'\n"),
            ("src/lib.rs", "pub struct Allowed;"),
            ("helper/Cargo.toml", "[package]\nname='helper'\nversion='0.1.0'\nedition='2021'"),
            ("helper/src/lib.rs", "pub struct Helper;"),
        ]);
        let message = error(|| super::check_source_inventory(dir.path()));
        assert!(message.contains("unreviewed build dependency"), "{declaration}: {message}");
    }
}
