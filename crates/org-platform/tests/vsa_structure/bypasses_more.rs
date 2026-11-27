//! The ways around the checks found in the fifth review (`TASK-213`), closed like those in
//! `bypasses.rs`: synthetic crates that try them and fail.

use super::bypasses::office;
use super::tests::{agent, agent_with, assert_names};

const ALIAS_LIB: &str = "extern crate self as me; mod features; mod registry; mod runtime;";

#[test]
fn a_value_use_a_macro_definition_or_a_macro_group_never_hides_a_module_name() {
    let values =
        "pub fn run() {} pub const N: u8 = 0; macro_rules! mac { () => {} } pub(crate) use mac;";
    for source in [
        // A `use` of a crate function, constant or macro binds no `name::`.
        "fn f() { use crate::messaging::features::run as t; t::features::helper(); }",
        "fn f() { use crate::messaging::features::N as t; t::features::helper(); }",
        "fn f() { use crate::messaging::features::mac as t; t::features::helper(); }",
        // What a `macro_rules!` or a macro call makes of a `use` is unknown.
        "macro_rules! m { ($($a:meta)*) => {{ $(#[$a])* use crate::messaging as t; t::features::N }} }",
        "fn f() { sneaky!(use x::{ t::features::helper };); }",
        "fn f() { cfg_if!(use crate::tasks::{features::helper};); }",
        // `$crate` starts a tree of a `use {..}` group.
        "macro_rules! m { () => { use {$crate::tasks::{features::helper}}; } }",
    ] {
        let source = format!("use crate::tasks as t; {source}");
        let found = office(&[
            ("o/src/lib.rs", "mod tasks; mod messaging;"),
            ("o/src/messaging/mod.rs", "pub mod features; mod case;"),
            ("o/src/messaging/features.rs", values),
            ("o/src/messaging/case.rs", &source),
        ]);
        assert_eq!(found, ["o/src/messaging/case.rs -> tasks"], "{source}");
    }
}

#[test]
fn a_value_use_a_cfg_module_file_or_a_macro_group_never_hides_the_crate_alias() {
    for (auth, me) in [
        (
            "use crate::runtime::input::go as me; fn f() { me::features::work::go(); }",
            "",
        ),
        (
            "fn f() { sneaky!(use x::{ me::features::work::go };); }",
            "",
        ),
        // An inner `#![cfg]` may leave the module file out.
        (
            "mod me; fn f() { me::features::work::go(); }",
            "#![cfg(any())]",
        ),
        (
            "mod me; fn f() { me::features::work::go(); }",
            "#![cfg(test)]",
        ),
    ] {
        let found = agent_with(&[
            ("cli/src/lib.rs", ALIAS_LIB),
            ("cli/src/features/auth/mod.rs", auth),
            ("cli/src/features/auth/me.rs", me),
        ]);
        assert_names(&found, &["crate::features::work::go"]);
    }
}

#[test]
fn a_module_use_of_the_crate_alias_binds_the_crate_root() {
    let found = agent_with(&[
        ("cli/src/lib.rs", ALIAS_LIB),
        (
            "cli/src/runtime/mod.rs",
            "pub(crate) mod input; pub use me;",
        ),
        (
            "cli/src/features/auth/mod.rs",
            "fn f() { crate::runtime::me::features::work::go(); }",
        ),
    ]);
    assert_names(&found, &["crate::features::work::go"]);
}

#[test]
fn a_public_use_merged_with_a_conditional_one_stays_public() {
    let found = agent(&[
        (
            "cli/src/lib.rs",
            "mod features; mod registry; mod runtime; pub mod api;",
        ),
        (
            "cli/src/api.rs",
            "#[cfg(any())] use crate::runtime as r; pub use crate::runtime as r;",
        ),
        ("cli/src/registry.rs", ""),
        ("cli/src/runtime.rs", ""),
        ("cli/src/features.rs", ""),
    ]);
    assert_names(&found, &["`pub use` re-exports all of `crate::runtime`"]);
}
