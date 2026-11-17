//! The browser gateway has no office database, key or host-supervisor access: its
//! normal (linked) dependency closure never reaches the host or office crates.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::process::Command;

/// Crates that own host supervision, office storage/keys, work execution or the
/// privileged client. None may be linked into the gateway.
const FORBIDDEN: &[&str] = &["org-host", "org-office", "org-work", "org-client"];

/// The package names reachable from `agentic-org-web` through normal dependency
/// edges (any target platform), from the locked workspace resolve.
fn normal_closure() -> BTreeSet<String> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args([
            "metadata",
            "--offline",
            "--locked",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(&manifest)
        .output()
        .expect("run cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata JSON");
    let names: HashMap<&str, &str> = metadata["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .map(|package| {
            (
                package["id"].as_str().expect("package id"),
                package["name"].as_str().expect("package name"),
            )
        })
        .collect();
    let root = names
        .iter()
        .find(|(_, name)| **name == "agentic-org-web")
        .map(|(id, _)| *id)
        .expect("agentic-org-web in the workspace");
    let nodes: HashMap<&str, &serde_json::Value> = metadata["resolve"]["nodes"]
        .as_array()
        .expect("a resolve graph")
        .iter()
        .map(|node| (node["id"].as_str().expect("node id"), node))
        .collect();
    let mut seen = BTreeSet::new();
    let mut pending = vec![root];
    while let Some(id) = pending.pop() {
        for dep in nodes[id]["deps"].as_array().expect("node deps") {
            let normal = dep["dep_kinds"]
                .as_array()
                .expect("dep kinds")
                .iter()
                .any(|kind| kind["kind"].is_null());
            let dep_id = dep["pkg"].as_str().expect("dep pkg");
            if normal && seen.insert(names[dep_id].to_owned()) {
                pending.push(dep_id);
            }
        }
    }
    seen
}

#[test]
fn the_gateway_links_no_host_office_work_or_client_crate() {
    let closure = normal_closure();
    // Not vacuous: the walk reaches the gateway's real workspace dependencies.
    for expected in ["org-host-control", "org-proto", "org-socket", "org-release"] {
        assert!(
            closure.contains(expected),
            "{expected} missing: {closure:?}"
        );
    }
    let linked: Vec<_> = FORBIDDEN
        .iter()
        .filter(|name| closure.contains(**name))
        .collect();
    assert!(
        linked.is_empty(),
        "agentic-org-web links forbidden crates {linked:?}"
    );
}
