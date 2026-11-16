//! Build-script inventory regressions use the same metadata entry point as the fence.
use super::opener_inventory::{fixture, tracked};
use super::opener_scope::SourceScope;
use super::opener_sources::metadata;
use serde_json::{json, Value};

const HOST: &str = "crates/org-host/build.rs";
const PIN: &str = "crates/org-office/src/store/tests/opener_build_pins.rs";
const HOST_SOURCE: &str = include_str!("../../../../../crates/org-host/build.rs");

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
        (
            super::opener_build::NATIVE,
            include_str!("../../../../../crates/org-host/src/recovery/presence.m"),
        ),
        ("build.rs", HOST_SOURCE),
        ("other.rs", HOST_SOURCE),
        ("scripts/helper.rs", HOST_SOURCE),
        ("vendor/crates/org-host/build.rs", HOST_SOURCE),
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
    for path in [
        "build.rs",
        "other.rs",
        "scripts/helper.rs",
        "vendor/crates/org-host/build.rs",
    ] {
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
        let message = error(|| {
            metadata(&scope, &value);
        });
        assert!(message.contains("fixture"), "{message}");
    }
    for kind in [
        "lib",
        "rlib",
        "dylib",
        "staticlib",
        "cdylib",
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
        ("build='crates/org-host/build.rs'", HOST, true),
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
            (
                super::opener_build::NATIVE,
                include_str!("../../../../../crates/org-host/src/recovery/presence.m"),
            ),
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

#[test]
fn sqlcipher_build_inventory_names_external_script_package_and_path() {
    let dir = host_fixture();
    let scope = tracked(dir.path());
    let path = scope.root.parent().unwrap().join("outside-build.rs");
    let value = inventory(
        &scope,
        vec![package(
            &scope,
            "external-package",
            path.to_str().unwrap(),
            json!(["custom-build"]),
        )],
    );
    let message = error(|| {
        metadata(&scope, &value);
    });
    assert!(
        message.contains("external-package") && message.contains(path.to_str().unwrap()),
        "{message}"
    );
}

#[test]
fn sqlcipher_build_script_checks_source_scope_before_reading() {
    let dir = host_fixture();
    let scope = tracked(dir.path());
    let path = scope.root.join(HOST);
    super::opener_build::script(&scope, "host", &path);
    let mut untracked = tracked(dir.path());
    untracked.files.remove(&path);
    error(|| super::opener_build::script(&untracked, "host", &path));
    let mut wrong_kind = tracked(dir.path());
    wrong_kind
        .files
        .insert(path.clone(), super::opener_scope::SourceKind::Config);
    error(|| super::opener_build::script(&wrong_kind, "host", &path));
    #[cfg(unix)]
    {
        // The same bytes behind a symlink must still be refused, before reading them.
        let target = scope.root.join("src/copied.rs");
        std::fs::write(&target, HOST_SOURCE).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        error(|| super::opener_build::script(&scope, "host", &path));
    }
}

#[test]
fn sqlcipher_build_inventory_refuses_workspace_proc_macros_in_every_position() {
    let dir = host_fixture();
    let scope = tracked(dir.path());
    let good = package(&scope, "good", "src/lib.rs", json!(["lib"]));
    for kinds in [
        json!(["proc-macro"]),
        json!(["proc-macro", "lib"]),
        json!(["lib", "proc-macro"]),
    ] {
        let bad = package(&scope, "workspace-macro", "src/lib.rs", kinds);
        for packages in [
            vec![bad.clone(), good.clone()],
            vec![good.clone(), bad.clone()],
        ] {
            let message = error(|| {
                metadata(&scope, &inventory(&scope, packages));
            });
            assert!(
                message.contains("workspace-macro") && message.contains("proc-macro"),
                "{message}"
            );
        }
        for targets in [
            json!([bad["targets"][0], good["targets"][0]]),
            json!([good["targets"][0], bad["targets"][0]]),
        ] {
            let mut value = inventory(&scope, vec![good.clone()]);
            value["packages"][0]["id"] = json!("workspace-macro");
            value["packages"][0]["targets"] = targets;
            error(|| {
                metadata(&scope, &value);
            });
        }
    }
}

#[test]
fn sqlcipher_build_inventory_cargo_refuses_workspace_proc_macros() {
    for declaration in [
        "[dependencies]\nmacro-helper={path='macro-helper'}",
        "[dev-dependencies.macro-helper]\npath='macro-helper'",
        "[dev_dependencies]\nmacro-helper={path='macro-helper'}",
        "[target.'cfg(any())'.dependencies]\nmacro-helper={path='macro-helper'}",
        "[target.'cfg(any())'.dev-dependencies.macro-helper]\npath='macro-helper'",
        "[build-dependencies]\nmacro-helper={path='macro-helper'}",
        "[target.'cfg(any())'.build_dependencies.macro-helper]\npath='macro-helper'",
        "[dependencies.alias]\npackage='macro-helper'\npath='macro-helper'",
        "[workspace.dependencies]\nmacro-helper={path='macro-helper'}\n[dependencies]\nmacro-helper.workspace=true",
    ] {
        let manifest = format!("[workspace]\nmembers=['macro-helper']\n[package]\nname='fixture'\nversion='0.1.0'\nedition='2021'\n{declaration}\n");
        let dir = fixture(&[
            ("Cargo.toml", &manifest),
            ("Cargo.lock", "version=4\n[[package]]\nname='fixture'\nversion='0.1.0'\ndependencies=['macro-helper']\n[[package]]\nname='macro-helper'\nversion='0.1.0'\n"),
            ("src/lib.rs", "pub struct Allowed;"),
            ("macro-helper/Cargo.toml", "[package]\nname='macro-helper'\nversion='0.1.0'\nedition='2021'\n[lib]\nproc-macro=true\n"),
            ("macro-helper/src/lib.rs", ""),
        ]);
        let lock = std::fs::read(dir.path().join("Cargo.lock")).unwrap();
        let message = error(|| super::check_source_inventory(dir.path()));
        assert!(message.contains("macro-helper") && (message.contains("proc-macro") || message.contains("build dependency")), "{declaration}: {message}");
        assert_eq!(std::fs::read(dir.path().join("Cargo.lock")).unwrap(), lock);
    }
}

#[test]
fn sqlcipher_build_helpers_bind_exact_owner_manifest_name_and_complete_tuple() {
    let dir = fixture(&[
        ("Cargo.toml", "[workspace]"),
        ("src/lib.rs", "pub struct Entry;"),
        (
            "crates/org-host/Cargo.toml",
            "[package]\nname='org-host'\nversion='0.0.0'",
        ),
        (
            "apps/agentic-org-web/Cargo.toml",
            "[package]\nname='agentic-org-web'\nversion='0.0.0'",
        ),
        (
            "crates/org-release/Cargo.toml",
            "[package]\nname='org-release'\nversion='0.0.0'",
        ),
    ]);
    let scope = tracked(dir.path());
    let release = json!({"name":"org-release","source":null,"req":"*","kind":"build","rename":null,"optional":false,
        "uses_default_features":true,"features":[],"target":null,"registry":null,"path":scope.root.join("crates/org-release")});
    let cc = json!({"name":"cc","source":"registry+https://github.com/rust-lang/crates.io-index","req":"=1.11.1","kind":"build",
        "rename":null,"optional":false,"uses_default_features":true,"features":[],"target":null,"registry":null});
    let host = scope.root.join("crates/org-host/Cargo.toml");
    let web = scope.root.join("apps/agentic-org-web/Cargo.toml");
    let check = |name, manifest: &std::path::Path, d: &Value| {
        super::opener_build::dependency(&scope, "untrusted metadata id", Some(name), manifest, d)
    };
    check("org-host", &host, &release);
    check("agentic-org-web", &web, &release);
    check("org-host", &host, &cc);
    error(|| check("agentic-org-web", &web, &cc));
    error(|| check("spoofed", &host, &cc));
    error(|| check("org-host", &web, &cc));
    for (field, bad) in [
        ("name", json!("helper")),
        ("source", json!("git+https://example.test/cc")),
        ("req", json!("^1.11.1")),
        ("path", json!(scope.root.join("outside"))),
        ("rename", json!("other")),
        ("features", json!(["parallel"])),
        ("optional", json!(true)),
        ("uses_default_features", json!(false)),
        ("target", json!("cfg(unix)")),
        ("registry", json!("other")),
    ] {
        let mut bad_cc = cc.clone();
        bad_cc[field] = bad;
        error(|| check("org-host", &host, &bad_cc));
    }
    for (field, bad) in [
        ("path", json!(scope.root.join("../org-release"))),
        (
            "source",
            json!("registry+https://github.com/rust-lang/crates.io-index"),
        ),
        ("req", json!("=0.0.0")),
        ("features", json!(["new-feature"])),
    ] {
        let mut bad_release = release.clone();
        bad_release[field] = bad;
        error(|| check("org-host", &host, &bad_release));
    }
}
#[test]
fn sqlcipher_native_bridge_requires_exact_bytes_fixed_kind_confined_tracked_regular_source() {
    use super::opener_build::{native, NATIVE};
    let dir = host_fixture();
    let scope = tracked(dir.path());
    let path = scope.root.join(NATIVE);
    native(&scope, &path);
    let original = std::fs::read(&path).unwrap();
    std::fs::write(&path, [original.as_slice(), b"\n// changed bytes"].concat()).unwrap();
    error(|| native(&scope, &path));
    std::fs::write(&path, &original).unwrap();
    let mut untracked = tracked(dir.path());
    untracked.files.remove(&path);
    error(|| native(&untracked, &path));
    let mut wrong = tracked(dir.path());
    wrong
        .files
        .insert(path.clone(), super::opener_scope::SourceKind::Rust);
    error(|| native(&wrong, &path));
    error(|| native(&scope, &scope.root.join("../presence.m")));
    error(|| {
        scope.require_rust(&path);
    });
    std::fs::remove_file(&path).unwrap();
    error(|| native(&scope, &path));
    #[cfg(unix)]
    {
        let other = scope.root.join("other-native.txt");
        std::fs::write(&other, &original).unwrap();
        std::os::unix::fs::symlink(&other, &path).unwrap();
        error(|| native(&scope, &path));
    }
}
