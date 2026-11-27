//! Marks release-profile builds, so the crate refuses to compile one with debug assertions on.
//!
//! The host's development paths (the dev key folder instead of the Keychain, no check of the
//! office servers' signatures, the unsigned development app, `--home`/`--server-bin`/
//! `--app-team-id`) are compiled in exactly when `debug_assertions` is. A release build with
//! debug assertions on would ship all of them. They can be turned on many ways (the profile's
//! `debug-assertions`, `-C debug-assertions` or `-C opt-level=0` in `RUSTFLAGS` or config
//! rustflags), so this script doesn't try to read the flags: it only sets `release_profile`,
//! and `lib.rs` has `compile_error!` under `all(release_profile, debug_assertions)`, which the
//! compiler decides with every flag applied. `ORG_BUNDLE=1` (set by the app's bundling) marks
//! `bundle_build` the same way for any profile. Documentation and doctests are exempt.

#![forbid(unsafe_code)]

fn main() {
    embed_release_trust();
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-env-changed=ORG_BUNDLE");
    println!("cargo::rustc-check-cfg=cfg(release_profile)");
    println!("cargo::rustc-check-cfg=cfg(bundle_build)");
    if std::env::var("PROFILE").is_ok_and(|profile| profile == "release") {
        println!("cargo::rustc-cfg=release_profile");
    }
    // The app's bundling sets ORG_BUNDLE=1: then any profile (a custom one that inherits dev,
    // say) with debug assertions on is refused too.
    match std::env::var_os("ORG_BUNDLE") {
        None => {}
        Some(value) if value == "1" => println!("cargo::rustc-cfg=bundle_build"),
        // Anything else is a mistake that would silently skip the check: refuse it.
        Some(_) => panic!("ORG_BUNDLE must be unset or exactly 1"),
    }
}

// Trusted public build inputs only. Never called from the runtime or bundle.
fn embed_release_trust() {
    use std::{fs, io::Read, path::PathBuf};
    println!("cargo::rerun-if-env-changed=ORG_RELEASE_TRUST_REGISTRY");
    println!("cargo::rerun-if-env-changed=ORG_RELEASE_MIN_SEQUENCE");
    let registry = std::env::var_os("ORG_RELEASE_TRUST_REGISTRY");
    let floor = std::env::var("ORG_RELEASE_MIN_SEQUENCE").ok();
    let (bytes, minimum) = match (registry, floor) {
        (None, None) => (Vec::new(), 0),
        (Some(path), Some(raw)) => {
            if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) || raw.starts_with('0') {
                panic!("invalid release freshness build input");
            }
            let minimum = raw
                .parse::<u64>()
                .expect("invalid release freshness build input");
            let path = PathBuf::from(path);
            println!("cargo::rerun-if-changed={}", path.display());
            let file =
                fs::File::open(path).expect("release public registry build input unavailable");
            let metadata = file
                .metadata()
                .expect("release public registry metadata unavailable");
            assert!(
                metadata.is_file() && metadata.len() <= org_release::MAX_REGISTRY_BYTES as u64,
                "release public registry exceeds build profile"
            );
            let mut bytes = Vec::new();
            file.take(org_release::MAX_REGISTRY_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .expect("release public registry read failed");
            org_release::TrustedKeys::from_registry_bytes(&bytes)
                .expect("invalid release public registry build input");
            (bytes, minimum)
        }
        _ => panic!("release public registry and freshness floor must be configured together"),
    };
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo build output"));
    fs::write(out.join("release-trust-registry.json"), bytes)
        .expect("release public registry embedding failed");
    println!("cargo::rustc-env=ORG_EMBEDDED_RELEASE_MIN_SEQUENCE={minimum}");
}
