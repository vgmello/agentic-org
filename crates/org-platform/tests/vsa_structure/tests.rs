//! Synthetic crates held in memory: each check catches the violation it exists for, and each
//! way around it found in review (`TASK-213` pass 2) is closed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use proc_macro2::{TokenStream, TokenTree};

use super::graph::{cycles, is_test_path, load_target, Target};
use super::parse::{requires_test, scan, Head};
use super::rules::{
    agent_cli_violations, cycle_violations, layout_violations, office_reach_ins,
    operator_cli_violations,
};

fn load(root: &str, files: &[(&str, &str)]) -> Result<Target, String> {
    let files: BTreeMap<PathBuf, String> = files
        .iter()
        .map(|(p, s)| (PathBuf::from(p), (*s).to_owned()))
        .collect();
    let read = |path: &Path| files.get(path).cloned();
    load_target(Path::new(root), &read)
}

fn target(root: &str, files: &[(&str, &str)]) -> Target {
    load(root, files).expect("the synthetic crate loads")
}

fn agent(files: &[(&str, &str)]) -> Vec<String> {
    let t = target("cli/src/lib.rs", files);
    let paths: Vec<PathBuf> = files.iter().map(|(p, _)| PathBuf::from(p)).collect();
    agent_cli_violations(&t, &paths)
}

const CLEAN_LIB: &str = "mod features; mod registry; mod runtime; pub use runtime::input::Thing;";
const FEATURES: &str = "pub(crate) mod auth; pub(crate) mod work;";

/// A clean agent CLI (`registry`, `runtime::input`, the `auth` and `work` slices) with
/// `changed` files replaced or added.
fn agent_with(changed: &[(&'static str, &'static str)]) -> Vec<String> {
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

fn assert_names(found: &[String], expected: &[&str]) {
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

// ---------------------------------------------------------------------------------------------
// The ways around the checks found in review, each closed

#[test]
fn a_use_group_is_read_once_with_its_prefix() {
    let tokens: TokenStream =
        "use super::{super::work::Row, self::x::{Y}}; m!(pub(in crate::a) z);"
            .parse()
            .unwrap();
    let tokens: Vec<TokenTree> = tokens.into_iter().collect();
    let mut named = Vec::new();
    scan(&tokens, &mut named);
    let found: Vec<(Head, String, bool)> = named
        .into_iter()
        .map(|n| (n.path.head, n.path.segments.join("::"), n.restriction))
        .collect();
    assert_eq!(
        found,
        [
            (Head::Up(2), "work::Row".to_owned(), false),
            (Head::Up(1), "x::Y".to_owned(), false),
            (Head::Root, "a".to_owned(), true),
        ]
    );
}

#[test]
fn super_inside_a_use_group_climbs_from_the_prefix() {
    let found = agent_with(&[
        ("cli/src/features/auth/mod.rs", "mod sub;"),
        (
            "cli/src/features/auth/sub.rs",
            "use super::{super::work::Row};
             use super::{self as me, super::super::registry::Table};",
        ),
    ]);
    assert_names(
        &found,
        &["crate::features::work::Row", "crate::registry::Table"],
    );
}

#[test]
fn module_aliases_and_globs_are_followed() {
    let changed = [
        (
            "cli/src/features/auth/mod.rs",
            "use crate::features; use crate::features as f;
             fn g() { features::work::Row::new(); f::work::go(); }",
        ),
        (
            "cli/src/features/work/mod.rs",
            "use super::*; pub struct Row; fn g() { auth::x(); }",
        ),
    ];
    assert_names(
        &agent_with(&changed),
        &[
            "names `crate::features`)",
            "names `crate::features`)",
            "crate::features::work::Row::new",
            "crate::features::work::go",
            "names `crate::features`)",
            "crate::features::auth::x",
        ],
    );
    let t = target(
        "x/src/lib.rs",
        &[
            ("x/src/lib.rs", "mod a; mod b;"),
            ("x/src/a.rs", "use crate::*; fn f() { b::B::new(); }"),
            ("x/src/b.rs", "pub struct B; fn g() { crate::a::f(); }"),
        ],
    );
    assert_eq!(cycles(&t.graph()), vec![vec!["a".to_owned(), "b".into()]]);
    assert_names(
        &layout_violations(&t),
        &["glob-imports the crate module `crate`"],
    );
}

#[test]
fn a_test_item_or_inline_module_on_the_same_line_hides_nothing() {
    let found = agent_with(&[(
        "cli/src/features/auth/mod.rs",
        "#[cfg(test)] const _T: () = (); use crate::features::work::Row;
         mod m {} use super::work::Row as R2;
         #[cfg(test)] mod t {} use crate::registry::X;
         #[cfg(test)] fn t2() { crate::registry::x(); }",
    )]);
    assert_names(
        &found,
        &[
            "crate::features::work::Row",
            "crate::features::work::Row",
            "crate::registry::X",
        ],
    );
}

#[test]
fn modules_in_bodies_are_placed_and_a_path_past_the_root_fails() {
    let found = agent_with(&[
        (
            "cli/src/features/auth/mod.rs",
            "mod sub; impl X { fn f() { #[path = \"sneaky.rs\"] mod sneaky; } }
             const C: () = { mod k { fn z() { crate::features::work::Row; } } };",
        ),
        (
            "cli/src/features/auth/sub.rs",
            "pub fn leak() { mod inner { pub fn go() { super::super::super::super::registry::secret(); } } }",
        ),
        (
            "cli/src/features/auth/sneaky.rs",
            "pub fn s() { crate::registry::x(); }",
        ),
    ]);
    assert_names(
        &found,
        &[
            "crate::features::auth::k` names `crate::features::work::Row",
            "crate::registry::x",
            "crate::features::auth::sub::inner` names `crate::registry::secret",
        ],
    );
    for (source, error) in [
        ("fn f() { super::super::y(); }", "past the crate root"),
        ("use super::super::*;", "past the crate root"),
        ("macro_rules! m { () => { mod hidden {} } }", "cannot place"),
    ] {
        let loaded = load(
            "x/src/lib.rs",
            &[("x/src/lib.rs", "mod a;"), ("x/src/a.rs", source)],
        );
        let failure = loaded.expect_err(source);
        assert!(failure.contains("x/src/a.rs:1:"), "{failure}");
        assert!(failure.contains(error), "{failure}");
    }
}

#[test]
fn root_re_exports_resolve_through_self_globs_and_raw_names() {
    let t = target(
        "x/src/lib.rs",
        &[
            (
                "x/src/lib.rs",
                "mod a; mod b; mod model; mod r#type;
                 pub use self::b::B; pub use model::*; pub use r#type::Kind;",
            ),
            (
                "x/src/a.rs",
                "pub struct A; fn f() { crate::B::new(); crate::Info::new(); }",
            ),
            ("x/src/b.rs", "pub struct B; fn g() { crate::Kind::One; }"),
            ("x/src/model.rs", "pub struct Info; fn h() { crate::a::A; }"),
            (
                "x/src/type.rs",
                "pub enum Kind { One } fn k() { crate::b::B; }",
            ),
        ],
    );
    assert_eq!(
        cycles(&t.graph()),
        vec![
            vec!["a".to_owned(), "model".into()],
            vec!["b".to_owned(), "type".into()]
        ]
    );
    assert!(layout_violations(&t).is_empty(), "the crate root may glob");
}

#[test]
fn an_extern_crate_self_alias_is_the_crate_root() {
    let found = agent_with(&[
        (
            "cli/src/lib.rs",
            "extern crate self as me; mod features; mod registry; mod runtime;",
        ),
        (
            "cli/src/features/auth/mod.rs",
            "fn f() { me::features::work::Row::new(); } use me::registry;",
        ),
    ]);
    assert_names(
        &found,
        &["crate::features::work::Row::new", "crate::registry"],
    );
}

#[test]
fn a_macro_group_after_pub_is_a_path_unless_it_is_a_visibility() {
    let found = agent_with(&[
        (
            "cli/src/features/auth/mod.rs",
            "m!(pub (crate::features::work::Row)); pub(crate) fn a() {} pub(super) fn b() {}
             pub(in crate::features) fn c() {} pub(self) fn d() {}",
        ),
        (
            "cli/src/features/work/mod.rs",
            "pub struct Row; pub(in crate::features::auth) fn e() {}",
        ),
    ]);
    assert_names(
        &found,
        &[
            "crate::features::work::Row",
            "names `crate::features::auth`",
        ],
    );
}

#[test]
fn a_test_file_name_needs_a_test_cfg() {
    let t = target(
        "t/src/lib.rs",
        &[
            (
                "t/src/lib.rs",
                "mod testing; #[cfg(any(test, feature = \"testkit\"))] mod testkit;
                 #[cfg(all(feature = \"test-support\", debug_assertions))] pub mod test_support;
                 #[cfg(not(test))] mod tests; mod checks_tests; #[cfg(test)] mod wake_tests;",
            ),
            ("t/src/testing.rs", ""),
            ("t/src/testkit.rs", ""),
            ("t/src/test_support.rs", ""),
            ("t/src/tests.rs", ""),
            ("t/src/checks_tests.rs", ""),
        ],
    );
    assert_names(
        &layout_violations(&t),
        &["mod testing", "mod tests", "mod checks_tests"],
    );
}

#[test]
fn include_and_a_path_out_of_the_module_directory_are_caught() {
    let t = target(
        "t/src/lib.rs",
        &[
            (
                "t/src/lib.rs",
                "mod a; mod feat; include!(\"gen.rs\"); #[cfg(test)] fn t() { include!(\"x.rs\"); }",
            ),
            (
                "t/src/a.rs",
                "#[path = \"a/ok.rs\"] mod ok; #[path = \"../b.rs\"] mod up; #[path = \"sibling.rs\"] mod sib;",
            ),
            ("t/src/a/ok.rs", ""),
            ("t/b.rs", ""),
            ("t/src/sibling.rs", ""),
            ("t/src/feat/mod.rs", "mod auth;"),
            (
                "t/src/feat/auth/mod.rs",
                "#[path = \"../work/rows.rs\"] mod rows; mod inner { #[path = \"deep.rs\"] mod deep; }",
            ),
            ("t/src/feat/work/rows.rs", ""),
            ("t/src/feat/auth/inner/deep.rs", ""),
        ],
    );
    let mut found = layout_violations(&t);
    found.sort();
    assert_names(
        &found,
        &[
            "t/b.rs from outside",
            "t/src/sibling.rs from outside",
            "rows.rs from outside",
            "`include!`",
        ],
    );
}

#[test]
fn only_the_crate_root_glob_imports_a_crate_module() {
    let t = target(
        "g/src/lib.rs",
        &[
            ("g/src/lib.rs", "mod a; mod b; fn run() { use b::*; c(); }"),
            (
                "g/src/a.rs",
                "use crate::*; use super::b::*; pub enum Kind { One } use self::Kind::*;
                 use std::collections::*;",
            ),
            ("g/src/b.rs", "pub fn c() {}"),
        ],
    );
    assert_names(
        &layout_violations(&t),
        &["the crate module `crate`", "the crate module `crate::b`"],
    );
}
