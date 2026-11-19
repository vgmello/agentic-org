//! The engine boundary (ADR-0018), in the narrow form B1a guards:
//!
//! - `rusqlite` is a dependency of `org-office` and `org-host` alone;
//! - the organization family on the office store, and the seam that opens the store,
//!   never name `rusqlite`: only `src/store/sqlite/` does, among the new code.
//!
//! The scan of the whole workspace for SQL outside the engine arrives with B1b block 7.
//! The old store it would have had to allow is deleted, so its allowlist starts empty.

use std::{
    fs,
    path::{Path, PathBuf},
};

const ENGINE_CRATE: &str = "rusqlite";

/// Folders that hold no source of this workspace.
const SKIPPED: [&str; 4] = ["target", "node_modules", ".git", ".agent-kit"];

/// The new code that must not name the engine's crate, under `crates/org-office/src/`.
const ENGINE_FREE: [&str; 9] = [
    "organization/records.rs",
    "organization/records",
    "organization/ports.rs",
    "organization/projector.rs",
    "organization/projector",
    "organization/features",
    "organization/testkit.rs",
    "store/open.rs",
    "store/open",
];

fn workspace() -> PathBuf {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = here.parent().and_then(Path::parent);
    root.expect("crates/org-office is two folders below the workspace")
        .to_owned()
}

/// Every file under `root` with this name or, when `name` is `None`, a `.rs` file.
fn files(root: &Path, name: Option<&str>) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if root.is_file() {
        return vec![root.to_owned()];
    }
    let mut folders = vec![root.to_owned()];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(&folder).expect("a readable folder") {
            let path = entry.expect("an entry").path();
            let file = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if path.is_dir() {
                if !SKIPPED.contains(&file) {
                    folders.push(path);
                }
                continue;
            }
            let wanted = match name {
                Some(name) => file == name,
                None => file.ends_with(".rs"),
            };
            if wanted {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// Whether a manifest line names the crate as a dependency: `rusqlite = ...`,
/// `rusqlite.workspace = true` or a `[dependencies.rusqlite]` table.
fn names_engine(line: &str) -> bool {
    let line = line.trim();
    if line.starts_with('#') {
        return false;
    }
    let key = line.split(['=', '.', ' ']).next().unwrap_or("");
    let table = line.starts_with('[') && line.trim_end_matches(']').ends_with(".rusqlite");
    key == ENGINE_CRATE || table
}

#[test]
fn only_the_office_and_the_host_depend_on_the_engines_crate() {
    let root = workspace();
    let mut dependents = Vec::new();
    let manifests = files(&root, Some("Cargo.toml"));
    assert!(
        manifests.len() > 2,
        "the workspace's manifests were not found"
    );
    for manifest in manifests {
        // The workspace's own manifest only fixes the version its members use.
        if manifest.parent() == Some(root.as_path()) {
            continue;
        }
        let text = fs::read_to_string(&manifest).expect("a manifest");
        if text.lines().any(names_engine) {
            let folder = manifest.parent().expect("a manifest is in a folder");
            let folder = folder.strip_prefix(&root).expect("under the workspace");
            dependents.push(folder.to_str().expect("a UTF-8 path").replace('\\', "/"));
        }
    }
    dependents.sort();
    assert_eq!(dependents, ["crates/org-host", "crates/org-office"]);
}

#[test]
fn the_organization_family_and_the_store_seam_never_name_the_engines_crate() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut scanned = 0;
    let mut naming = Vec::new();
    for path in ENGINE_FREE {
        let path = source.join(path);
        assert!(
            path.exists(),
            "{} is gone: update this guard",
            path.display()
        );
        for file in files(&path, None) {
            scanned += 1;
            let text = fs::read_to_string(&file).expect("a source file");
            for (number, line) in text.lines().enumerate() {
                if line.contains(ENGINE_CRATE) {
                    naming.push(format!("{}:{}", file.display(), number + 1));
                }
            }
        }
    }
    assert!(
        scanned >= ENGINE_FREE.len(),
        "only {scanned} files were scanned"
    );
    assert!(
        naming.is_empty(),
        "the engine's crate is named in: {naming:#?}"
    );
}

#[test]
fn the_guard_recognizes_a_dependency_line() {
    for line in [
        "rusqlite.workspace = true",
        "rusqlite = \"0.42\"",
        "rusqlite = { version = \"=0.42.0\" }",
        "[dependencies.rusqlite]",
        "[target.'cfg(unix)'.dependencies.rusqlite]",
    ] {
        assert!(names_engine(line), "{line}");
    }
    for line in [
        "# rusqlite = \"1\"",
        "description = \"no rusqlite here\"",
        "rusqlite-extra = \"1\"",
    ] {
        assert!(!names_engine(line), "{line}");
    }
}
