//! Marks release-profile builds, so the crate refuses to compile one with debug assertions on.
//!
//! The server's development paths (the peer check by the parent's pid instead of the host's code
//! signature) are compiled in exactly when `debug_assertions` is. A release build with debug
//! assertions on would ship them. They can be turned on many ways (the profile's
//! `debug-assertions`, `-C debug-assertions` or `-C opt-level=0` in `RUSTFLAGS`), so this script
//! doesn't try to read the flags: it only sets `release_profile`, and `lib.rs` has
//! `compile_error!` under `all(release_profile, debug_assertions)`, which the compiler decides
//! with every flag applied. `ORG_BUNDLE=1` (set by the app's bundling) marks `bundle_build` the
//! same way for any profile. Documentation and doctests are exempt. (The host's `build.rs` is the
//! model.)

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-env-changed=ORG_BUNDLE");
    println!("cargo::rustc-check-cfg=cfg(release_profile)");
    println!("cargo::rustc-check-cfg=cfg(bundle_build)");
    if std::env::var("PROFILE").is_ok_and(|profile| profile == "release") {
        println!("cargo::rustc-cfg=release_profile");
    }
    match std::env::var_os("ORG_BUNDLE") {
        None => {}
        Some(value) if value == "1" => println!("cargo::rustc-cfg=bundle_build"),
        // Anything else is a mistake that would silently skip the check: refuse it.
        Some(_) => panic!("ORG_BUNDLE must be unset or exactly 1"),
    }
}
