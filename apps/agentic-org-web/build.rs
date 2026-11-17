//! Embed trusted public publisher policy; no runtime trust environment.
//!
//! Also marks release-profile (`release_profile`) and app-bundle (`ORG_BUNDLE=1`,
//! `bundle_build`) builds, so `lib.rs` refuses to compile one with debug assertions on:
//! the unpinned development startup path is compiled in exactly when they are.
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
    embed_release_trust();
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
