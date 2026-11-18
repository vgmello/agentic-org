//! `org-platform` is linked by all four executables, so it depends on the C library
//! and nothing else (ADR-0017 decision 6, ADR-0008).
//!
//! A crate's link directives reach every executable that links it. An Apple framework
//! here would be loaded by the web process before it confines itself, and by the
//! office worker and `agent-os`. A capability that needs one belongs in
//! `crates/org-platform-host`, which only the host links.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

/// Each workspace member's package name and the package names of its normal and build
/// dependencies, under every target, as Cargo reads the manifests: a dependency in its
/// own table, renamed with `package` or inherited from the workspace is named by the
/// package it is. `[dev-dependencies]` are not linked into an executable and are left
/// out. `--no-deps` reads only the workspace's manifests: no registry is needed.
fn linked_dependencies() -> BTreeMap<String, Vec<String>> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let output = Command::new(cargo)
        .args([
            "metadata",
            "--no-deps",
            "--offline",
            "--format-version",
            "1",
        ])
        .arg("--manifest-path")
        .arg(&root)
        .output()
        .expect("cargo runs");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata prints JSON");
    let packages = metadata["packages"].as_array().expect("a package list");
    packages
        .iter()
        .map(|package| {
            let name = package["name"].as_str().expect("a package name").to_owned();
            let linked = package["dependencies"]
                .as_array()
                .expect("a dependency list")
                .iter()
                .filter(|dependency| dependency["kind"].as_str() != Some("dev"))
                .map(|dependency| {
                    dependency["name"]
                        .as_str()
                        .expect("a dependency's package name")
                        .to_owned()
                })
                .collect();
            (name, linked)
        })
        .collect()
}

#[test]
fn org_platform_links_the_c_library_only() {
    let workspace = linked_dependencies();
    assert_eq!(
        workspace["org-platform"],
        ["libc"],
        "org-platform is linked by every executable; a capability that needs more than \
         libc goes into crates/org-platform-host"
    );
}

/// The host executable, the host library and the host-only platform crate: the only
/// members that may link the host's own capabilities.
const HOST_ONLY: [&str; 3] = ["agentic-org", "org-host", "org-platform-host"];

#[test]
fn only_the_host_links_org_platform_host() {
    // Every other member can be built into the web process, the office worker or
    // agent-os. A dependency on org-platform-host in any of them would put Apple
    // frameworks into an executable that must not load them. A new member is checked
    // without being added here.
    let workspace = linked_dependencies();
    for host in HOST_ONLY {
        assert!(
            workspace.contains_key(host),
            "{host} is no longer a workspace member"
        );
    }
    // The packages each executable outside the host is built from are among the
    // checked ones.
    for checked in [
        "agentic-org-office",
        "agentic-org-web",
        "agent-os",
        "org-store",
    ] {
        assert!(
            workspace.contains_key(checked) && !HOST_ONLY.contains(&checked),
            "{checked} is among the checked members"
        );
    }
    for (member, linked) in workspace
        .iter()
        .filter(|(member, _)| !HOST_ONLY.contains(&member.as_str()))
    {
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
                "{member} depends on {forbidden}"
            );
        }
    }
}
