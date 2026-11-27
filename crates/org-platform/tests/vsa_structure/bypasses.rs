//! Each way around the checks found in review is closed (`TASK-213` passes 2 and 3): synthetic
//! crates that try it and fail.

use proc_macro2::{TokenStream, TokenTree};

use super::graph::cycles;
use super::parse::Head;
use super::rules::{layout_violations, office_reach_ins};
use super::scan::scan;
use super::tests::{agent, agent_with, assert_names, load, target};

// ---------------------------------------------------------------------------------------------
// Pass 2

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

// ---------------------------------------------------------------------------------------------
// Pass 3

/// The `org-office` families `tasks` and `messaging`, with `extra` files added.
fn office(extra: &[(&'static str, &'static str)]) -> Vec<String> {
    let mut files = vec![
        ("o/src/tasks/mod.rs", "pub mod features;"),
        (
            "o/src/tasks/features.rs",
            "pub fn helper() {} pub const N: u8 = 1;",
        ),
        ("o/src/messaging/features.rs", ""),
    ];
    files.extend(extra);
    office_reach_ins(&target("o/src/lib.rs", &files))
        .into_iter()
        .map(|(file, domain)| format!("{file} -> {domain}"))
        .collect()
}

#[test]
fn a_body_local_use_or_mod_never_shadows_its_module() {
    let files = [
        (
            "o/src/lib.rs",
            "mod tasks; mod messaging; pub use crate::tasks::features as shared;
             fn hide() { mod shared {} }",
        ),
        (
            "o/src/messaging/mod.rs",
            "pub mod features; mod alias; mod reexport; mod own;
             fn f() { mod inner { fn g() { super::super::tasks::features::helper(); } } }",
        ),
        (
            "o/src/messaging/alias.rs",
            "use crate::tasks as t; fn decoy() { use crate::messaging as t; }
             fn f() { t::features::helper(); }",
        ),
        ("o/src/tasks/mod.rs", "pub mod features;"),
        ("o/src/tasks/features.rs", "pub fn helper() {}"),
        ("o/src/messaging/features.rs", ""),
        ("o/src/messaging/reexport.rs", "use crate::shared::helper;"),
        (
            "o/src/messaging/own.rs",
            "use crate::tasks as t; fn f() { use crate::messaging as t; t::features::x(); }
             fn g() { use crate::messaging::features as t; { t::x(); } }",
        ),
    ];
    let t = target("o/src/lib.rs", &files);
    assert!(!t.scopes[&Vec::new()].children.contains("shared"));
    assert!(t
        .modules
        .iter()
        .all(|m| !m.path.contains(&"inner".to_owned())));
    let found: Vec<String> = office_reach_ins(&t)
        .into_iter()
        .map(|(file, domain)| format!("{file} -> {domain}"))
        .collect();
    assert_eq!(
        found,
        [
            "o/src/messaging/alias.rs -> tasks",
            "o/src/messaging/mod.rs -> tasks",
            "o/src/messaging/reexport.rs -> tasks",
        ]
    );
}

#[test]
fn a_leading_colon_path_through_an_extern_crate_self_alias_is_the_crate_root() {
    let found = agent_with(&[
        (
            "cli/src/lib.rs",
            "extern crate self as me; mod features; mod registry; mod runtime;
             m!(extern crate self as also;);",
        ),
        (
            "cli/src/features/auth/mod.rs",
            "fn f() -> ::me::registry::Table { ::me::features::work::Row::new() }
             use ::me::runtime::input::Thing; pub use ::me::runtime as rt;
             fn g(x: u8) -> bool { match x { _ => ::me::features::work::go() } }
             fn h() -> bool { 1 > ::me::features::work::N }
             fn k() { if true {} ::also::registry::x(); }
             fn l<'a>() { let _: &'a ::me::registry::Y; }",
        ),
    ]);
    assert_names(
        &found,
        &[
            "crate::registry::Table",
            "crate::features::work::Row::new",
            "crate::features::work::go",
            "crate::features::work::N",
            "crate::registry::x",
            "crate::registry::Y",
        ],
    );
    let t = target(
        "x/src/lib.rs",
        &[
            ("x/src/lib.rs", "extern crate self as me; mod a;"),
            ("x/src/a.rs", "use ::me::*; use ::std::fmt::*;"),
        ],
    );
    assert_names(
        &layout_violations(&t),
        &["glob-imports the crate module `crate`"],
    );
}

#[test]
fn a_macro_argument_is_a_path_unless_it_is_a_whole_visibility_or_use_tree() {
    let found = agent_with(&[(
        "cli/src/features/auth/mod.rs",
        "m!(pub(in x crate::features::work::Row)); m!(x::{ let _ = crate::registry::y; });
         m!(pub(in crate::features::auth crate::registry::z)); m!(x::{crate::registry::v});
         m!(x::{super::registry::w}); pub(in crate::features) fn ok() {}",
    )]);
    assert_names(
        &found,
        &[
            "crate::features::work::Row",
            "crate::registry::y",
            "crate::registry::z",
            "crate::registry::v",
            "crate::features::registry::w",
        ],
    );
    let tokens: TokenStream = "m!(pub(in x crate::a)); pub(in ::me::b) fn f() {}"
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
            (Head::Root, "a".to_owned(), false),
            (Head::Extern, "me::b".to_owned(), true),
        ]
    );
}

#[test]
fn every_include_and_every_path_the_guard_cannot_follow_fails() {
    let t = target(
        "t/src/lib.rs",
        &[
            ("t/src/lib.rs", "mod a; mod feat; r#include!(\"gen.rs\");"),
            (
                "t/src/a.rs",
                "use core::include as inc; fn f() { use std::{include}; }
                 m!(use ::core::include as inc2;); inc!(\"x.rs\");",
            ),
            ("t/src/feat/mod.rs", "mod auth; mod work;"),
            (
                "t/src/feat/auth/mod.rs",
                "#[path = \"../work\"] mod inner { mod sub; }
                 #[cfg_attr(all(), path = \"../work/sub.rs\")] mod sub;
                 #[cfg_attr(unix, cfg_attr(all(), path = \"deep.rs\"))] mod nested;
                 #[r#path = \"../work/rows.rs\"] mod rows;",
            ),
            ("t/src/feat/auth/inner/sub.rs", ""),
            ("t/src/feat/auth/sub.rs", ""),
            ("t/src/feat/auth/nested.rs", ""),
            ("t/src/feat/work/mod.rs", ""),
            ("t/src/feat/work/sub.rs", ""),
            ("t/src/feat/work/rows.rs", ""),
        ],
    );
    let mut found = layout_violations(&t);
    found.sort();
    assert_names(
        &found,
        &[
            "t/src/a.rs:1:1: a `use` of `include`",
            "t/src/a.rs:1:36: a `use` of `include`",
            "t/src/a.rs:2:21: a `use` of `include`",
            "`#[path]` on the inline module `inner`",
            "`mod sub` takes a `path` from `cfg_attr`",
            "`mod nested` takes a `path` from `cfg_attr`",
            "rows.rs from outside",
            "t/src/lib.rs:1:18: `include!`",
        ],
    );
}

#[test]
fn a_use_in_a_macro_a_discriminant_or_an_array_length_binds_its_name() {
    let found = office(&[
        (
            "o/src/lib.rs",
            "mod tasks; mod messaging; mod projects; mod store;
             #[path = \"renamed.rs\"] mod notes; #[path = \"messaging/glue.rs\"] mod glue;",
        ),
        (
            "o/src/messaging/mod.rs",
            "pub mod features; mod rules; mod kinds; mod sizes; mod called; mod nested;
             mod cfg_first; mod cfg_last; mod masked; mod unmasked; mod item;",
        ),
        (
            "o/src/messaging/rules.rs",
            "macro_rules! sneaky { () => {{ use crate::tasks as t; t::features::helper(); }}; }",
        ),
        (
            "o/src/messaging/kinds.rs",
            "enum E { A = { use crate::tasks as t; t::features::N } }",
        ),
        (
            "o/src/messaging/sizes.rs",
            "struct S { f: [u8; { use crate::tasks as t; t::features::N as usize }] }",
        ),
        (
            "o/src/messaging/called.rs",
            "m!(use crate::tasks as t;); fn f() { t::features::helper(); }",
        ),
        (
            "o/src/messaging/nested.rs",
            "fn f() { m!({ use crate::tasks as t; t::features::helper(); }); }",
        ),
        (
            "o/src/messaging/cfg_first.rs",
            "#[cfg(feature = \"a\")] use crate::tasks as t;
             #[cfg(not(feature = \"a\"))] use crate::messaging as t;
             fn f() { t::features::helper(); }",
        ),
        (
            "o/src/messaging/cfg_last.rs",
            "#[cfg(feature = \"a\")] use crate::messaging as t;
             #[cfg(not(feature = \"a\"))] use crate::tasks as t;
             fn f() { t::features::helper(); }",
        ),
        (
            "o/src/messaging/masked.rs",
            "use crate::tasks as t; fn f() { m!(use crate::messaging as t;); t::features::x(); }",
        ),
        (
            "o/src/messaging/item.rs",
            "mod t { pub use crate::tasks as inner; }
             fn f() { m!(use crate::messaging as t;); t::inner::features::helper(); }",
        ),
        (
            "o/src/messaging/unmasked.rs",
            "use crate::messaging as t; fn f() { m!(use crate::tasks as t;); t::features::x(); }",
        ),
        (
            "o/src/projects.rs",
            "pub mod features; fn f() { crate::tasks::features::helper(); }",
        ),
        ("o/src/projects/features.rs", ""),
        (
            "o/src/renamed.rs",
            "pub mod features; fn f() { crate::tasks::features::helper(); }",
        ),
        ("o/src/renamed/features.rs", ""),
        (
            "o/src/messaging/glue.rs",
            "fn f() { crate::tasks::features::helper(); }",
        ),
        ("o/src/store/mod.rs", "mod open;"),
        ("o/src/store/open/mod.rs", "mod project_ops;"),
        (
            "o/src/store/open/project_ops.rs",
            "use crate::projects::features::x;",
        ),
    ]);
    assert_eq!(
        found,
        [
            "o/src/messaging/called.rs -> tasks",
            "o/src/messaging/cfg_first.rs -> tasks",
            "o/src/messaging/cfg_last.rs -> tasks",
            "o/src/messaging/glue.rs -> tasks",
            "o/src/messaging/item.rs -> tasks",
            "o/src/messaging/kinds.rs -> tasks",
            "o/src/messaging/masked.rs -> tasks",
            "o/src/messaging/nested.rs -> tasks",
            "o/src/messaging/rules.rs -> tasks",
            "o/src/messaging/sizes.rs -> tasks",
            "o/src/messaging/unmasked.rs -> tasks",
            "o/src/projects.rs -> tasks",
            "o/src/renamed.rs -> tasks",
        ]
    );
}

#[test]
fn a_whole_tier_re_export_from_any_public_module_is_caught() {
    let found = agent(&[
        (
            "cli/src/lib.rs",
            "mod features; mod registry; mod runtime; pub mod api; mod hidden; mod private;
             pub use hidden as exposed;",
        ),
        ("cli/src/api.rs", "pub use crate::runtime; pub mod deeper;"),
        ("cli/src/api/deeper.rs", "pub use crate::registry as r;"),
        ("cli/src/hidden.rs", "pub use crate::features as f;"),
        ("cli/src/private.rs", "pub use crate::runtime as rt;"),
        ("cli/src/registry.rs", ""),
        ("cli/src/runtime.rs", ""),
        ("cli/src/features.rs", ""),
    ]);
    assert_names(
        &found,
        &[
            "cli/src/api.rs:1:5: `pub use` re-exports all of `crate::runtime`",
            "cli/src/api/deeper.rs:1:5: `pub use` re-exports all of `crate::registry`",
            "cli/src/hidden.rs:1:5: `pub use` re-exports all of `crate::features`",
        ],
    );
}
