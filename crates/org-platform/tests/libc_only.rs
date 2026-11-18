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

/// The workspace's members, as the root manifest's `members` list names them.
fn workspace_members() -> Vec<String> {
    let root = manifest("../../Cargo.toml");
    let list = root
        .split_once("\nmembers = [")
        .and_then(|(_, rest)| rest.split_once(']'))
        .map(|(list, _)| list)
        .expect("the root manifest has a members list");
    list.lines()
        .map(|line| line.trim().trim_end_matches(','))
        .filter(|line| line.starts_with('"'))
        .map(|line| line.trim_matches('"').to_owned())
        .collect()
}

/// The host executable, the host library and the host-only platform crate: the only
/// members that may link the host's own capabilities.
const HOST_ONLY: [&str; 3] = [
    "apps/agentic-org",
    "crates/org-host",
    "crates/org-platform-host",
];

#[test]
fn only_the_host_links_org_platform_host() {
    // Every other member can be built into the web process, the office worker or
    // agent-os. A dependency on org-platform-host in any of them would put Apple
    // frameworks into an executable that must not load them. A new member is checked
    // without being added here.
    let members = workspace_members();
    for host in HOST_ONLY {
        assert!(
            members.iter().any(|member| member == host),
            "{host} is no longer a workspace member"
        );
    }
    let checked: Vec<&String> = members
        .iter()
        .filter(|member| !HOST_ONLY.contains(&member.as_str()))
        .collect();
    assert!(
        checked
            .iter()
            .any(|member| *member == "apps/agentic-org-office"),
        "the worker is among the checked members"
    );
    for member in checked {
        let crate_manifest = format!("../../{member}/Cargo.toml");
        let linked = linked_dependencies(&manifest(&crate_manifest));
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
