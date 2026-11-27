//! The checks, each over a loaded [`Target`].

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::graph::{cycles, display_path, site, Reference, Target};
use crate::{
    AGENT_CLI_LAYER_NAMES, AGENT_CLI_LAYER_SUFFIXES, AGENT_CLI_PRIVATE, OPERATOR_COMPOSERS,
};

fn at(reference: &Reference) -> String {
    format!(
        "{} (`{}` names `{}`)",
        site(&reference.file, reference.at),
        display_path(&reference.from),
        display_path(&reference.to)
    )
}

/// Every crate: no `include!`, no `#[path]` out of the declaring module's directory, no
/// production module under a test file name, and no glob import of a crate module outside the
/// crate root (the composition root), so every module names what it imports.
pub fn layout_violations(target: &Target) -> Vec<String> {
    let mut out = target.problems.clone();
    for (module, scope) in &target.scopes {
        if module.is_empty() {
            continue;
        }
        for glob in &scope.globs {
            let Ok(Some(source)) = target.absolute(module, &glob.path) else {
                continue;
            };
            let Ok(source) = target.resolve(&source) else {
                continue;
            };
            if target.is_module(&source) {
                out.push(format!(
                    "{}: `{}` glob-imports the crate module `{}`; name what it uses (only the crate root may glob a module)",
                    site(&glob.file, glob.at),
                    display_path(module),
                    display_path(&source)
                ));
            }
        }
    }
    out
}

/// Whether a file of the agent CLI is (or is in) a horizontal layer: `table.rs`, `*_table.rs`,
/// `dispatch/mod.rs`, `aliases/`..
fn is_layer(relative: &Path) -> bool {
    relative.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        let stem = name.strip_suffix(".rs").unwrap_or(&name);
        AGENT_CLI_LAYER_NAMES.contains(&stem)
            || AGENT_CLI_LAYER_SUFFIXES.iter().any(|s| stem.ends_with(s))
    })
}

/// Invariant 1: `crates/org-agent-cli` (`files` are its source files under `src/`).
pub fn agent_cli_violations(target: &Target, files: &[PathBuf]) -> Vec<String> {
    let mut out = Vec::new();
    let src = target.root.parent().unwrap_or(Path::new(""));
    for file in files {
        if is_layer(file.strip_prefix(src).unwrap_or(file)) {
            out.push(format!(
                "{}: a horizontal layer file; put each command's row, guidance and handler in its `features/<domain>/` slice",
                file.display()
            ));
        }
    }
    for (name, line) in &target.root_pub_mods {
        if AGENT_CLI_PRIVATE.contains(&name.as_str()) {
            out.push(format!(
                "{}: `pub mod {name}` makes an internal tier public API; re-export the narrow items instead",
                site(&target.root, *line)
            ));
        }
    }
    out.extend(private_re_exports(target));
    for reference in &target.references {
        let Some(tier) = reference.from.first() else {
            continue;
        };
        let to_tier = reference.to.first().map(String::as_str);
        match tier.as_str() {
            "runtime" if to_tier != Some("runtime") => out.push(format!(
                "{}: `runtime` is the leaf kernel and names nothing outside `runtime`",
                at(reference)
            )),
            "features" => {
                let own = reference.from.get(1);
                let allowed = to_tier == Some("runtime")
                    || (to_tier == Some("features")
                        && (own.is_none() || reference.to.get(1) == own));
                if !allowed {
                    out.push(format!(
                        "{}: a feature slice names only itself and `runtime` (no other slice, bare `features`, `registry` or crate-root item)",
                        at(reference)
                    ));
                }
            }
            _ => {}
        }
    }
    out
}

/// `pub use` in the agent CLI's `lib.rs` that re-exports a whole internal tier
/// (`pub use runtime;`, `pub use crate::registry as r;`) or globs anything inside one.
fn private_re_exports(target: &Target) -> Vec<String> {
    let mut out = Vec::new();
    let private = |path: &[String]| {
        path.first()
            .is_some_and(|tier| AGENT_CLI_PRIVATE.contains(&tier.as_str()))
    };
    for (module, scope) in &target.scopes {
        let uses = scope.uses.values().map(|binding| (binding, false));
        let globs = scope.globs.iter().map(|binding| (binding, true));
        for (binding, glob) in uses.chain(globs) {
            if !binding.public || binding.file != target.root {
                continue;
            }
            let Ok(Some(to)) = target.absolute(module, &binding.path) else {
                continue;
            };
            let Ok(to) = target.resolve(&to) else {
                continue;
            };
            if private(&to) && (glob || to.len() == 1) {
                out.push(format!(
                    "{}: `pub use` re-exports all of `{}`, an internal tier; re-export the narrow items instead",
                    site(&binding.file, binding.at),
                    display_path(&to)
                ));
            }
        }
    }
    out
}

/// Invariant 2: `apps/agentic-org`. Only the crate root, `cli` (the composition root) and
/// `features` itself name `features`; a feature names no other feature and never bare `features`.
pub fn operator_cli_violations(target: &Target) -> Vec<String> {
    let mut out = Vec::new();
    for reference in &target.references {
        let (from, to) = (&reference.from, &reference.to);
        if to.first().map(String::as_str) != Some("features") {
            continue;
        }
        let Some(from_top) = from.first() else {
            continue;
        };
        if from_top != "features" {
            if !OPERATOR_COMPOSERS.contains(&from_top.as_str()) {
                out.push(format!(
                    "{}: only `cli` (the composition root) names `features`; a root helper module never does",
                    at(reference)
                ));
            }
        } else if from.len() >= 2 && to.len() < 2 {
            out.push(format!(
                "{}: a feature never names bare `features`",
                at(reference)
            ));
        } else if from.len() >= 2 && from[1] != to[1] {
            out.push(format!(
                "{}: a feature never names another feature",
                at(reference)
            ));
        }
    }
    out
}

/// The journaled families: top-level modules with a `features` module.
fn office_families(target: &Target) -> BTreeSet<String> {
    target
        .modules
        .iter()
        .filter(|m| m.path.len() == 2 && m.path[1] == "features")
        .map(|m| m.path[0].clone())
        .collect()
}

/// The top-level module that owns an `org-office` file for invariant 4, if any.
fn office_owner(target: &Target, families: &BTreeSet<String>, file: &Path) -> Option<String> {
    let src = target.root.parent()?;
    let relative = file.strip_prefix(src).ok()?;
    let parts: Vec<String> = relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if parts.len() >= 2 && families.contains(&parts[0]) {
        return Some(parts[0].clone());
    }
    if parts.len() >= 3 && parts[0] == "store" && parts[1] == "open" {
        let stem = parts[2].strip_suffix(".rs").unwrap_or(&parts[2]);
        let name = stem.strip_suffix("_ops")?;
        let tops = target.top_level();
        return [name.to_owned(), format!("{name}s"), format!("{name}es")]
            .into_iter()
            .find(|candidate| tops.contains(candidate));
    }
    None
}

/// Invariant 4: every `(file, domain)` reach into another family's `features`.
pub fn office_reach_ins(target: &Target) -> BTreeSet<(String, String)> {
    let families = office_families(target);
    let mut out = BTreeSet::new();
    for reference in &target.references {
        let Some(owner) = office_owner(target, &families, &reference.file) else {
            continue;
        };
        let to = &reference.to;
        if to.len() >= 2 && to[1] == "features" && families.contains(&to[0]) && to[0] != owner {
            out.insert((reference.file.display().to_string(), to[0].clone()));
        }
    }
    out
}

/// Invariant 3 for one target: what differs from the known cycles.
pub fn cycle_violations(target: &Target, known: &[(&str, &[&str])]) -> Vec<String> {
    let root = target.root.display().to_string();
    let found = cycles(&target.graph());
    let listed: Vec<Vec<String>> = known
        .iter()
        .filter(|(r, _)| *r == root)
        .map(|(_, members)| members.iter().map(|m| (*m).to_owned()).collect())
        .collect();
    let mut out = Vec::new();
    for component in &found {
        if !listed.contains(component) {
            out.push(format!(
                "{root}: the top-level modules {{{}}} depend on each other in a cycle; break it (a listed cycle never grows, and none is added)",
                component.join(", ")
            ));
        }
    }
    for component in &listed {
        if !found.contains(component) {
            out.push(format!(
                "{root}: the listed cycle {{{}}} is gone or changed; update or remove its KNOWN_CYCLES entry",
                component.join(", ")
            ));
        }
    }
    out
}
