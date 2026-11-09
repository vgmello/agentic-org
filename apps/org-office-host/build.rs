//! Marks release-profile builds, so the crate refuses to compile one with debug assertions on.
//!
//! The host's development paths (the dev key folder instead of the Keychain, no check of the
//! office servers' signatures, the unsigned development app, `--home`/`--server-bin`/
//! `--app-team-id`) are compiled in exactly when `debug_assertions` is. A release build with
//! debug assertions on would ship all of them. They can be turned on many ways (the profile's
//! `debug-assertions`, `-C debug-assertions` or `-C opt-level=0` in `RUSTFLAGS` or config
//! rustflags), so this script doesn't try to read the flags: it only sets `release_profile`,
//! and `lib.rs` has `compile_error!` under `all(release_profile, debug_assertions)`, which the
//! compiler decides with every flag applied.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rustc-check-cfg=cfg(release_profile)");
    if std::env::var("PROFILE").is_ok_and(|profile| profile == "release") {
        println!("cargo::rustc-cfg=release_profile");
    }
}
