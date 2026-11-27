//! Synthetic crates held in memory: each check catches the violation it exists for. The ways
//! around the checks found in review are in `bypasses.rs`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::graph::{cycles, is_test_path, load_target, Target};
use super::parse::requires_test;
use super::rules::{
    agent_cli_violations, cycle_violations, office_reach_ins, operator_cli_violations,
};

pub(super) fn load(root: &str, files: &[(&str, &str)]) -> Result<Target, String> {
    let files: BTreeMap<PathBuf, String> = files
        .iter()
        .map(|(p, s)| (PathBuf::from(p), (*s).to_owned()))
        .collect();
    let read = |path: &Path| files.get(path).cloned();
    load_target(Path::new(root), &read)
}

pub(super) fn target(root: &str, files: &[(&str, &str)]) -> Target {
    load(root, files).expect("the synthetic crate loads")
}

pub(super) fn agent(files: &[(&str, &str)]) -> Vec<String> {
    let t = target("cli/src/lib.rs", files);
    let paths: Vec<PathBuf> = files.iter().map(|(p, _)| PathBuf::from(p)).collect();
    agent_cli_violations(&t, &paths)
}

const CLEAN_LIB: &str = "mod features; mod registry; mod runtime; pub use runtime::input::Thing;";
const FEATURES: &str = "pub(crate) mod auth; pub(crate) mod work;";

/// A clean agent CLI (`registry`, `runtime::input`, the `auth` and `work` slices) with
/// `changed` files replaced or added.
pub(super) fn agent_with(changed: &[(&'static str, &'static str)]) -> Vec<String> {
    let mut files: BTreeMap<&str, &str> = [
        ("cli/src/lib.rs", CLEAN_LIB),
        ("cli/src/registry.rs", ""),
        ("cli/src/runtime/mod.rs", "pub(crate) mod input;"),
        (
            "cli/src/runtime/input.rs",
            "pub struct Thing; pub fn go() {}",
        ),
        ("cli/src/features/mod.rs", FEATURES),
        ("cli/src/features/auth/mod.rs", ""),
        ("cli/src/features/work/mod.rs", "pub struct Row;"),
    ]
    .into_iter()
    .collect();
    files.extend(changed.iter().copied());
    agent(&files.into_iter().collect::<Vec<_>>())
}

pub(super) fn assert_names(found: &[String], expected: &[&str]) {
    assert_eq!(found.len(), expected.len(), "{found:#?}");
    for (finding, path) in found.iter().zip(expected) {
        assert!(finding.contains(path), "`{path}` in {found:#?}");
    }
}

#[test]
fn a_clean_two_tier_layout_passes() {
    let found = agent(&[
        (
            "cli/src/lib.rs",
            "mod features; mod registry; mod runtime; pub use runtime::input::Thing;
             pub mod external_auth { pub use crate::features::auth::x; }",
        ),
        (
            "cli/src/registry.rs",
            "use crate::features::{auth, work}; fn f() { crate::runtime::input::go(); }",
        ),
        ("cli/src/runtime/mod.rs", "pub(crate) mod input;"),
        (
            "cli/src/runtime/input.rs",
            "pub struct Thing; pub fn go() { self::helper(); } fn helper() {}",
        ),
        ("cli/src/features/mod.rs", FEATURES),
        (
            "cli/src/features/auth/mod.rs",
            "use crate::runtime::input::Thing; use super::auth::x; pub mod x;",
        ),
        (
            "cli/src/features/auth/x.rs",
            "use super::super::auth::y; fn f() { super::z(); }
             pub(in crate::features) fn h() {} use crate::runtime as rt;
             use super::{self as me}; fn g() { rt::input::go(); me::z(); }",
        ),
        (
            "cli/src/features/work/mod.rs",
            "use crate::Thing; #[cfg(test)] mod tests;",
        ),
        (
            "cli/src/features/work/tests.rs",
            "use crate::features::auth::Anything;",
        ),
    ]);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_cross_feature_import_is_caught_in_use_trees_and_inline_paths() {
    let found = agent_with(&[
        (
            "cli/src/features/auth/mod.rs",
            "use crate::{runtime::input, features::{work::Row}};",
        ),
        (
            "cli/src/features/work/mod.rs",
            "fn f() { super::auth::go(); crate::registry::table(); }",
        ),
    ]);
    assert_names(
        &found,
        &[
            "crate::features::work::Row",
            "crate::features::auth::go",
            "crate::registry::table",
        ],
    );
}

#[test]
fn runtime_naming_a_feature_the_registry_or_the_root_is_caught() {
    let found = agent(&[
        (
            "cli/src/lib.rs",
            "mod features; mod registry; mod runtime; pub fn run() {}",
        ),
        ("cli/src/registry.rs", ""),
        (
            "cli/src/runtime/mod.rs",
            "use crate::features::auth; fn f() { crate::registry::x(); crate::run(); }",
        ),
        ("cli/src/features/mod.rs", "pub(crate) mod auth;"),
        (
            "cli/src/features/auth.rs",
            "#[cfg(test)] fn t() { crate::registry::x(); }",
        ),
    ]);
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(found.iter().all(|f| f.contains("leaf kernel")), "{found:?}");
}

#[test]
fn a_public_internal_tier_and_every_form_of_layer_file_are_caught() {
    let found = agent(&[
        (
            "cli/src/lib.rs",
            "pub mod features; mod registry; pub(crate) mod runtime; pub mod msg_table;",
        ),
        ("cli/src/registry.rs", ""),
        ("cli/src/runtime.rs", ""),
        ("cli/src/features.rs", ""),
        ("cli/src/msg_table.rs", ""),
        ("cli/src/features/work/table.rs", ""),
        ("cli/src/features/work/dispatch.rs", ""),
        ("cli/src/features/work/dispatch/read.rs", ""),
        ("cli/src/features/auth/msg_dispatch/mod.rs", ""),
        ("cli/src/help_guidance/mod.rs", ""),
        ("cli/src/aliases/mod.rs", ""),
        ("cli/src/features/work/timetable.rs", ""),
    ]);
    assert_names(
        &found,
        &[
            "msg_table.rs",
            "work/table.rs",
            "work/dispatch.rs",
            "dispatch/read.rs",
            "msg_dispatch/mod.rs",
            "help_guidance/mod.rs",
            "aliases/mod.rs",
            "pub mod features",
        ],
    );
}

#[test]
fn an_inline_public_tier_and_whole_tier_re_exports_are_caught() {
    let found = agent(&[
        (
            "cli/src/lib.rs",
            "mod features; mod runtime; pub mod registry {}
             pub use crate::runtime as rt; pub use self::features as f; pub use runtime::*;
             pub mod api { pub use crate::features::auth::Client; pub use crate::runtime::input; }",
        ),
        ("cli/src/features/mod.rs", "pub mod auth;"),
        ("cli/src/features/auth.rs", "pub struct Client;"),
        ("cli/src/runtime/mod.rs", "pub mod input;"),
        ("cli/src/runtime/input.rs", ""),
    ]);
    assert_names(
        &found,
        &[
            "pub mod registry",
            "all of `crate::features`",
            "all of `crate::runtime`",
            "all of `crate::runtime`",
        ],
    );
}

#[test]
fn a_module_cycle_is_found_and_the_ratchet_holds_it_exactly() {
    let t = target(
        "x/src/lib.rs",
        &[
            (
                "x/src/lib.rs",
                "mod a; mod b; mod c; mod d; pub use c::Root;",
            ),
            ("x/src/a.rs", "use crate::b::B; pub struct A;"),
            ("x/src/b.rs", "pub struct B; fn f() { crate::Root::new(); }"),
            (
                "x/src/c/mod.rs",
                "pub struct Root; mod inner { fn g() { super::super::a::A; } }",
            ),
            (
                "x/src/d.rs",
                "#[cfg(test)] use crate::a::A; pub(in crate::d) fn f() {} pub(in super) fn g() {}",
            ),
        ],
    );
    assert_eq!(
        cycles(&t.graph()),
        vec![vec!["a".to_owned(), "b".into(), "c".into()]]
    );
    let new = cycle_violations(&t, &[]);
    assert_eq!(new.len(), 1, "{new:?}");
    assert!(new[0].contains("{a, b, c}"), "{new:?}");
    assert!(cycle_violations(&t, &[("x/src/lib.rs", &["a", "b", "c"])]).is_empty());
    let stale = cycle_violations(&t, &[("x/src/lib.rs", &["a", "b"])]);
    assert_eq!(
        stale.len(),
        2,
        "a shrunk or grown entry fails both ways: {stale:?}"
    );
}

#[test]
fn the_operator_cli_rules_allow_only_cli_and_features_to_name_features() {
    let t = target(
        "op/src/lib.rs",
        &[
            (
                "op/src/lib.rs",
                "mod features; mod view; mod cli; mod fmt; fn run() { features::org::run(); }",
            ),
            ("op/src/cli.rs", "use crate::features::org::run;"),
            ("op/src/view.rs", "use crate::features::org::Card;"),
            (
                "op/src/fmt.rs",
                "pub fn f() { crate::features::project::Row::new(); }",
            ),
            (
                "op/src/features/mod.rs",
                "pub(crate) mod org; pub(crate) mod project;",
            ),
            (
                "op/src/features/org.rs",
                "use crate::view::Table; use super::project::Row;",
            ),
            (
                "op/src/features/project/mod.rs",
                "use crate::features::project::Own; use crate::features;",
            ),
        ],
    );
    let found = operator_cli_violations(&t);
    assert_names(
        &found,
        &[
            "another feature",
            "bare `features`",
            "op/src/fmt.rs",
            "op/src/view.rs",
        ],
    );
}

#[test]
fn an_office_domain_reaching_into_another_domains_features_is_caught_through_any_name() {
    let t = target(
        "o/src/lib.rs",
        &[
            (
                "o/src/lib.rs",
                "mod tasks; mod projects; mod processes; mod store; mod office; mod messaging;
                 pub mod family { pub use crate::tasks::features::{self, helper}; }
                 pub use self::tasks::features as task_features;",
            ),
            ("o/src/tasks/mod.rs", "pub mod features;"),
            ("o/src/tasks/features.rs", "pub fn helper() {}"),
            (
                "o/src/projects/mod.rs",
                "pub mod features; use crate::tasks::features::helper;",
            ),
            (
                "o/src/projects/features.rs",
                "use crate::projects::features::x;",
            ),
            ("o/src/processes/mod.rs", ""),
            ("o/src/store/mod.rs", "mod open;"),
            (
                "o/src/store/open/mod.rs",
                "mod process_ops; mod project_ops; mod operations;",
            ),
            (
                "o/src/store/open/process_ops.rs",
                "use crate::{tasks::{features::{helper}}};",
            ),
            (
                "o/src/store/open/project_ops.rs",
                "use crate::projects::features::x;",
            ),
            (
                "o/src/store/open/operations.rs",
                "use crate::tasks::features::helper;",
            ),
            ("o/src/office.rs", "use crate::tasks::features::helper;"),
            (
                "o/src/messaging/mod.rs",
                "pub mod features; mod alias; mod group; mod facade; mod reexport;",
            ),
            ("o/src/messaging/features.rs", ""),
            (
                "o/src/messaging/alias.rs",
                "use crate::tasks as t; fn f() { t::features::helper(); }",
            ),
            (
                "o/src/messaging/group.rs",
                "use crate::tasks::{self as t}; fn f() { t::features::helper(); }",
            ),
            (
                "o/src/messaging/facade.rs",
                "fn f() { crate::family::helper(); crate::family::features::helper(); }",
            ),
            (
                "o/src/messaging/reexport.rs",
                "use crate::task_features::helper;",
            ),
        ],
    );
    let found: Vec<String> = office_reach_ins(&t)
        .into_iter()
        .map(|(file, domain)| format!("{file} -> {domain}"))
        .collect();
    assert_eq!(
        found,
        [
            "o/src/messaging/alias.rs -> tasks",
            "o/src/messaging/facade.rs -> tasks",
            "o/src/messaging/group.rs -> tasks",
            "o/src/messaging/reexport.rs -> tasks",
            "o/src/projects/mod.rs -> tasks",
            "o/src/store/open/process_ops.rs -> tasks",
        ]
    );
}

#[test]
fn test_only_code_is_never_production() {
    assert!(requires_test("test".parse().unwrap()));
    assert!(requires_test("all(test, unix)".parse().unwrap()));
    assert!(requires_test("any(test, all(test, unix))".parse().unwrap()));
    assert!(!requires_test(
        "any(test, feature = \"testkit\")".parse().unwrap()
    ));
    assert!(!requires_test("not(test)".parse().unwrap()));
    assert!(is_test_path(Path::new("crates/x/src/a/tests.rs")));
    assert!(is_test_path(Path::new("crates/x/src/wake_tests.rs")));
    assert!(is_test_path(Path::new("crates/x/src/testkit/mod.rs")));
    assert!(!is_test_path(Path::new("crates/x/src/testing_ground.rs")));
    let t = target(
        "t/src/lib.rs",
        &[
            ("t/src/lib.rs", "mod a; mod b; #[cfg(test)] mod c;"),
            (
                "t/src/a.rs",
                "use crate::b::B;\nimpl X {\n    #[cfg(test)]\n    fn t() { crate::b::y(); }\n}\n",
            ),
            ("t/src/b.rs", "#![cfg(test)]\nuse crate::a::A;"),
        ],
    );
    assert!(cycles(&t.graph()).is_empty());
    assert_eq!(t.references.len(), 1, "{:?}", t.references);
}
