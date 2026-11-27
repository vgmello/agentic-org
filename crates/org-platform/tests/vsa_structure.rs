//! The architecture guard of `TASK-213` (`.agents/rules/architecture-and-layout.md`): what the
//! folder layout promises is checked on every build, so the layout cannot drift back.
//!
//! The test reads the production modules of every crate target of the workspace (`src/lib.rs`,
//! `src/main.rs` and `src/bin/*.rs` of each `crates/*` and `apps/*` package). `syn` gives each
//! file's modules (`mod x;`, `#[path]`, `r#` names, inline `mod x { .. }`, also inside function,
//! `impl`, `const` and `static` bodies), the `use` bindings and globs of each module, and the
//! exact line/column extent of every test-only item (`#[cfg(test)]`, `#![cfg(test)]`). The
//! file's tokens give every path it names: `crate::..`, `$crate::..`, `super::..`, `self::..`
//! (also `super::` inside a `use` group), and plain `name::..` whose `name` the module binds
//! (`use crate::tasks as t;`, a glob, `extern crate self as me;`), in `use` trees, signatures,
//! bodies and macro definitions alike. Each path is resolved from the crate root through the
//! `use` bindings of every module it passes (root re-exports, inline facades such as
//! `org-office`'s `organization_family`, globs). Test files are left out by name (`tests`,
//! `*_tests`, `*-tests`, `testing`, `testkit`, `test_support`, as `file_size_ratchet` decides)
//! only when their `mod` is gated on `test` or a test-only feature. Whatever the reader cannot
//! place (a path climbing past the root, a `mod` inside a macro, an item without an end) fails.
//!
//! It enforces five invariants:
//!
//! 1. **`crates/org-agent-cli` is a two-tier VSA:** no horizontal layer file or directory
//!    (`table`, `dispatch`, `*_table`, `*_dispatch`, `help_guidance`, `aliases`); `runtime` names
//!    nothing outside `runtime`; each `features::<domain>` names only itself and `runtime` (no
//!    other domain, bare `features`, `registry` or crate-root item); `lib.rs` declares no
//!    `pub mod features|runtime|registry` and re-exports none of them whole.
//! 2. **`apps/agentic-org` is a 1:1 VSA:** only the crate root, `cli` (the composition root) and
//!    `features` name `features`; no `features::<a>` names `features::<b>` or bare `features`.
//! 3. **Intra-crate module cycles only shrink:** the strongly connected components (two or more
//!    top-level modules) of every crate target are exactly [`KNOWN_CYCLES`]. A new cycle, a listed
//!    cycle that grows or shrinks, and a listed cycle that is gone all fail: the entry is changed
//!    or removed in the same change, so the list always says what is left.
//! 4. **`org-office` domains do not reach into each other's `features`:** a file of a journaled
//!    family (a top-level module with a `features` module), or a store operation file
//!    (`store/open/<name>_ops.rs`, owned by the top-level module `<name>` or `<name>s`), naming
//!    another family's `features::*` is exactly [`OFFICE_REACH_INS`], which only shrinks.
//! 5. **Every module is where its file says:** no `include!`, no `#[path]` outside the declaring
//!    module's own directory, no production module under a test file name, and no glob import of
//!    a crate module outside the crate root.
//!
//! To print what the scan finds: `VSA_STRUCTURE_PRINT=1 cargo test -p org-platform --test
//! vsa_structure -- --nocapture`.

// A test target's root resolves `mod x;` beside itself; its modules live in `vsa_structure/`.
#[path = "vsa_structure/graph.rs"]
mod graph;
#[path = "vsa_structure/parse.rs"]
mod parse;
#[path = "vsa_structure/rules.rs"]
mod rules;
#[path = "vsa_structure/tests.rs"]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use graph::{cycles, load_target};
use rules::{
    agent_cli_violations, cycle_violations, layout_violations, office_reach_ins,
    operator_cli_violations,
};

const AGENT_CLI: &str = "crates/org-agent-cli/src/lib.rs";
const OPERATOR_CLI: &str = "apps/agentic-org/src/lib.rs";
const OFFICE: &str = "crates/org-office/src/lib.rs";

/// Module names that are horizontal layers in the agent CLI (as a file stem or a directory): a
/// command lives in its domain slice.
const AGENT_CLI_LAYER_NAMES: [&str; 4] = ["table", "dispatch", "help_guidance", "aliases"];
const AGENT_CLI_LAYER_SUFFIXES: [&str; 2] = ["_table", "_dispatch"];
/// The agent CLI modules that are never public API.
const AGENT_CLI_PRIVATE: [&str; 3] = ["features", "registry", "runtime"];
/// The operator CLI's top-level modules that may name `features`: the composition root and the
/// features themselves. Every other module (a shell or helper module) never does.
const OPERATOR_COMPOSERS: [&str; 2] = ["cli", "features"];

/// Every strongly connected component of top-level production modules that exists today:
/// `(crate root, members in name order)`. Shrink only.
const KNOWN_CYCLES: &[(&str, &[&str])] = &[
    (
        "crates/org-agent-runtime/src/lib.rs",
        &["caps", "event", "spec", "usage"],
    ),
    ("crates/org-agent-runtime/src/lib.rs", &["path", "token"]),
    (
        "crates/org-host/src/lib.rs",
        &["admin", "recovery", "service"],
    ),
    (
        "crates/org-office-sessions/src/lib.rs",
        &["external", "lifecycle", "manager", "pump", "recovery"],
    ),
    ("crates/org-store/src/lib.rs", &["change", "error"]),
    ("crates/org-supervise/src/lib.rs", &["child", "stop"]),
];

/// Every `org-office` file that names another domain's `features` today: `(file, domain)`.
/// Shrink only.
const OFFICE_REACH_INS: &[(&str, &str)] = &[
    (
        "crates/org-office/src/messaging/features/mod.rs",
        "organization",
    ),
    (
        "crates/org-office/src/projects/features/contributing_teams.rs",
        "organization",
    ),
    (
        "crates/org-office/src/projects/features/create_project.rs",
        "organization",
    ),
    (
        "crates/org-office/src/projects/features/mod.rs",
        "organization",
    ),
    (
        "crates/org-office/src/projects/features/repositories.rs",
        "organization",
    ),
    ("crates/org-office/src/store/open/git_ops.rs", "projects"),
    ("crates/org-office/src/store/open/git_ops.rs", "tasks"),
    // TASK-218 moves the shared helpers out of `tasks::features`.
    ("crates/org-office/src/store/open/process_ops.rs", "tasks"),
];

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/org-platform is two levels below the repository root")
        .to_path_buf()
}

/// Every crate target root of the workspace's packages, relative to the repository.
fn target_roots(repo: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for group in ["apps", "crates"] {
        let Ok(entries) = fs::read_dir(repo.join(group)) else {
            continue;
        };
        let mut packages: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.join("Cargo.toml").is_file())
            .collect();
        packages.sort();
        for package in packages {
            let relative = package.strip_prefix(repo).expect("under the repository");
            for root in ["src/lib.rs", "src/main.rs"] {
                if package.join(root).is_file() {
                    roots.push(relative.join(root));
                }
            }
            if let Ok(bins) = fs::read_dir(package.join("src/bin")) {
                let mut bins: Vec<PathBuf> = bins
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x == "rs"))
                    .collect();
                bins.sort();
                roots.extend(
                    bins.iter()
                        .map(|b| b.strip_prefix(repo).unwrap().to_path_buf()),
                );
            }
        }
    }
    roots
}

fn files_under(repo: &Path, dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![repo.join(dir)];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).expect("a readable source directory") {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|x| x == "rs") {
                found.push(path.strip_prefix(repo).unwrap().to_path_buf());
            }
        }
    }
    found.sort();
    found
}

fn print_requested() -> bool {
    std::env::var_os("VSA_STRUCTURE_PRINT").is_some()
}

#[test]
fn the_workspace_keeps_its_vsa_boundaries_and_only_shrinks_its_module_cycles() {
    let repo = repository();
    let read = |path: &Path| fs::read_to_string(repo.join(path)).ok();
    let mut failures = Vec::new();
    let mut targets = BTreeMap::new();
    for root in target_roots(&repo) {
        match load_target(&root, &read) {
            Ok(target) => {
                if print_requested() {
                    for cycle in cycles(&target.graph()) {
                        println!("cycle {} {:?}", root.display(), cycle);
                    }
                }
                failures.extend(layout_violations(&target));
                failures.extend(cycle_violations(&target, KNOWN_CYCLES));
                targets.insert(root.display().to_string(), target);
            }
            Err(error) => failures.push(error),
        }
    }
    for (root, _) in KNOWN_CYCLES {
        if !targets.contains_key(*root) {
            failures.push(format!(
                "{root}: listed in KNOWN_CYCLES but not a crate target"
            ));
        }
    }

    if let Some(agent_cli) = targets.get(AGENT_CLI) {
        let agent_files = files_under(&repo, Path::new("crates/org-agent-cli/src"));
        failures.extend(agent_cli_violations(agent_cli, &agent_files));
    }
    if let Some(operator) = targets.get(OPERATOR_CLI) {
        failures.extend(operator_cli_violations(operator));
    }
    if let Some(office) = targets.get(OFFICE) {
        let found = office_reach_ins(office);
        if print_requested() {
            for (file, domain) in &found {
                println!("reach-in {file} -> {domain}::features");
            }
        }
        let listed: BTreeSet<(String, String)> = OFFICE_REACH_INS
            .iter()
            .map(|(f, d)| ((*f).to_owned(), (*d).to_owned()))
            .collect();
        for (file, domain) in found.difference(&listed) {
            failures.push(format!(
                "{file}: names `{domain}::features` from another domain; share a narrow capability or a domain-neutral helper instead"
            ));
        }
        for (file, domain) in listed.difference(&found) {
            failures.push(format!(
                "{file}: no longer names `{domain}::features`; remove its OFFICE_REACH_INS entry"
            ));
        }
    }
    for root in [AGENT_CLI, OPERATOR_CLI, OFFICE] {
        if !targets.contains_key(root) {
            failures.push(format!("{root}: a guarded crate target that did not load"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
