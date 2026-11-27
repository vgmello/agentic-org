//! The web process has no office database, key or host-supervisor access: its
//! normal (linked) dependency closure never reaches the host or office crates.

use std::collections::{BTreeSet, HashMap};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// `cargo metadata --offline` normally finishes in well under a second; a hung
/// subprocess fails the test instead of stalling CI.
const METADATA_DEADLINE: Duration = Duration::from_secs(120);

/// Crates that own host supervision, office storage/keys or the privileged client. None may
/// be linked into the web process.
/// The web process opens no database (non-negotiable 7), so no SQLite crate either.
const FORBIDDEN: &[&str] = &[
    "org-host",
    "org-host-setup",
    "org-host-sqlite",
    "org-office",
    "org-client",
    "rusqlite",
    "libsqlite3-sys",
];

/// The package names reachable from `agentic-org-web` through normal dependency
/// edges (any target platform), from the locked workspace resolve.
fn normal_closure() -> BTreeSet<String> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut output = Command::new(cargo);
    output
        .args([
            "metadata",
            "--offline",
            "--locked",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(&manifest);
    // Files, not pipes, so a large metadata document can never block the child.
    let mut stdout = tempfile::tempfile().expect("stdout file");
    let mut stderr = tempfile::tempfile().expect("stderr file");
    let mut child = output
        .stdout(Stdio::from(stdout.try_clone().expect("stdout clone")))
        .stderr(Stdio::from(stderr.try_clone().expect("stderr clone")))
        .spawn()
        .expect("run cargo metadata");
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("wait for cargo metadata") {
            break status;
        }
        if started.elapsed() > METADATA_DEADLINE {
            let _ = child.kill();
            let _ = child.wait();
            panic!("cargo metadata did not finish within {METADATA_DEADLINE:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let read = |file: &mut std::fs::File| {
        let mut bytes = Vec::new();
        file.seek(SeekFrom::Start(0)).expect("rewind");
        file.read_to_end(&mut bytes).expect("read output");
        bytes
    };
    let (out, err) = (read(&mut stdout), read(&mut stderr));
    assert!(
        status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&err)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&out).expect("cargo metadata JSON");
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
    // Traverse by package id, so a second version of a crate is still walked;
    // collect names separately for the forbidden-crate check.
    let mut visited = BTreeSet::new();
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
            if normal && visited.insert(dep_id) {
                seen.insert(names[dep_id].to_owned());
                pending.push(dep_id);
            }
        }
    }
    seen
}

#[test]
fn the_web_links_no_host_office_or_client_crate() {
    let closure = normal_closure();
    // Not vacuous: the walk reaches the web process's real workspace dependencies.
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
