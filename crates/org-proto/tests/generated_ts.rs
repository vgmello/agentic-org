//! Fails when the committed TypeScript in `app/src/lib/api/generated/` doesn't match what the
//! Rust types generate now. Fix: `cargo run -p org-proto --example export-ts`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Every entry of `dir` by name, with its content. An entry that isn't a UTF-8 text file
/// (a directory, a binary file) maps to `None`, so it shows up as unexpected, not as a panic.
fn read_dir_files(dir: &Path) -> BTreeMap<String, Option<String>> {
    let Ok(entries) = fs::read_dir(dir) else {
        return BTreeMap::new();
    };
    entries
        .map(|entry| {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            (name, entry)
        })
        // Skip dotfiles such as a local `.DS_Store`; the generator never writes any.
        .filter(|(name, _)| !name.starts_with('.'))
        .map(|(name, entry)| {
            let content = fs::read_to_string(entry.path()).ok();
            (name, content)
        })
        .collect()
}

#[test]
fn committed_typescript_is_up_to_date() {
    let committed_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../app/src/lib/api/generated");
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let fresh_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("org-proto-ts-{}-{nanos}", std::process::id()));

    let written = org_proto::typescript::export_typescript(&fresh_dir).unwrap();
    let fresh = read_dir_files(&fresh_dir);
    let committed = read_dir_files(&committed_dir);
    fs::remove_dir_all(&fresh_dir).unwrap();

    assert_eq!(written, fresh.keys().cloned().collect::<Vec<_>>());

    let mut problems = Vec::new();
    for (name, content) in &fresh {
        assert!(content.is_some(), "generated {name} is not a text file");
        match committed.get(name) {
            None => problems.push(format!("missing: {name}")),
            Some(old) if old != content => problems.push(format!("stale: {name}")),
            Some(_) => {}
        }
    }
    for name in committed.keys() {
        if !fresh.contains_key(name) {
            problems.push(format!("not generated any more: {name}"));
        }
    }

    assert!(
        problems.is_empty(),
        "the generated TypeScript in {} is out of date:\n  {}\nRegenerate it with \
         `cargo run -p org-proto --example export-ts` and commit the result.",
        committed_dir.display(),
        problems.join("\n  ")
    );
}
