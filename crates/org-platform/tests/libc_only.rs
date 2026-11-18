//! `org-platform` is linked by all four executables, so it depends on the C library
//! and nothing else (ADR-0017 decision 6, ADR-0008).
//!
//! A crate's link directives reach every executable that links it. An Apple framework
//! here would be loaded by the web process before it confines itself, and by the
//! office worker and `agent-os`. A capability that needs one belongs in
//! `crates/org-platform-host`, which only the host links.

use std::path::Path;

/// The dependency names in every `[dependencies]`, `[build-dependencies]` and
/// `[target.….dependencies]` table of a manifest. `[dev-dependencies]` are not linked
/// into an executable and are left out.
fn linked_dependencies(manifest: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut linked = false;
    for line in manifest.lines().map(str::trim) {
        if let Some(table) = line.strip_prefix('[') {
            let table = table.trim_end_matches(']');
            linked = (table == "dependencies"
                || table == "build-dependencies"
                || table.ends_with(".dependencies")
                || table.ends_with(".build-dependencies"))
                && !table.ends_with("dev-dependencies");
        } else if linked && !line.is_empty() && !line.starts_with('#') {
            let name = line.split(['=', '.', ' ']).next().unwrap_or(line);
            found.push(name.trim_matches('"').to_owned());
        }
    }
    found
}

fn manifest(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

#[test]
fn the_manifest_reader_sees_every_linked_table_and_no_dev_dependency() {
    let text = r#"
[package]
name = "x"
[dependencies]
libc.workspace = true
serde = { version = "1" }
[target.'cfg(target_os = "macos")'.dependencies]
core-foundation.workspace = true
"security-framework" = "3"
[target.'cfg(unix)'.build-dependencies]
cc = "1"
[build-dependencies]
bindgen = "0.7"
[dev-dependencies]
tempfile.workspace = true
[target.'cfg(unix)'.dev-dependencies]
proptest = "1"
[features]
x = []
"#;
    assert_eq!(
        linked_dependencies(text),
        [
            "libc",
            "serde",
            "core-foundation",
            "security-framework",
            "cc",
            "bindgen"
        ]
    );
}

#[test]
fn org_platform_links_the_c_library_only() {
    assert_eq!(
        linked_dependencies(&manifest("Cargo.toml")),
        ["libc"],
        "org-platform is linked by every executable; a capability that needs more than \
         libc goes into crates/org-platform-host"
    );
}

#[test]
fn only_the_host_links_org_platform_host() {
    // The crates the web process, the office worker and agent-os are built from. A
    // dependency on org-platform-host in any of them would put Apple frameworks into
    // an executable that must not load them.
    for crate_manifest in [
        "../org-socket/Cargo.toml",
        "../org-supervise/Cargo.toml",
        "../org-proto/Cargo.toml",
        "../org-client/Cargo.toml",
        "../org-office/Cargo.toml",
        "../org-office-launch/Cargo.toml",
        "../org-host-control/Cargo.toml",
        "../../apps/agentic-org-web/Cargo.toml",
        "../../apps/agentic-org-office/Cargo.toml",
        "../../apps/agent-os/Cargo.toml",
    ] {
        let linked = linked_dependencies(&manifest(crate_manifest));
        for forbidden in [
            "org-platform-host",
            "org-host",
            "core-foundation",
            "core-foundation-sys",
            "security-framework",
            "security-framework-sys",
        ] {
            assert!(
                !linked.iter().any(|name| name == forbidden),
                "{crate_manifest} depends on {forbidden}"
            );
        }
    }
}
