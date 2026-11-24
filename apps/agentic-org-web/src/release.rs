//! Publisher-authenticated immutable preload while the web process is still alone
//! and has no host control capability, listener, session or TLS material.
mod files;
use crate::assets::{BuildError, BuildFiles};
use org_host_control::web::WebRelease;
use org_platform::image::{ReleaseArchitecture, ReleasePlatform};
use org_release::{Architecture, Platform, TrustedKeys, TrustedPolicy, VerifiedManifest};
use std::path::Path;
// Public policy data only: the reviewed build script validates the registry
// and always supplies this compile-time floor. No generated Rust is included.
const RELEASE_REGISTRY: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/release-trust-registry.json"));
const RELEASE_MIN_SEQUENCE: u64 = build_floor(env!("ORG_EMBEDDED_RELEASE_MIN_SEQUENCE"));
const fn build_floor(raw: &str) -> u64 {
    let bytes = raw.as_bytes();
    assert!(!bytes.is_empty(), "missing compiled release floor");
    assert!(
        bytes.len() == 1 || bytes[0] != b'0',
        "noncanonical compiled release floor"
    );
    let mut value = 0u64;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        assert!(
            byte >= b'0' && byte <= b'9',
            "invalid compiled release floor"
        );
        value = match value.checked_mul(10) {
            Some(n) => n,
            None => panic!("compiled release floor overflow"),
        };
        value = match value.checked_add((byte - b'0') as u64) {
            Some(n) => n,
            None => panic!("compiled release floor overflow"),
        };
        index += 1;
    }
    value
}
#[test]
fn compiled_release_floor_is_exact_checked_decimal_with_only_zero_development_sentinel() {
    assert_eq!(build_floor("0"), 0);
    assert_eq!(build_floor("1"), 1);
    assert_eq!(build_floor("18446744073709551615"), u64::MAX);
    for invalid in [
        "",
        "00",
        "01",
        "+1",
        "-1",
        "1.0",
        "1e0",
        " 1",
        "18446744073709551616",
    ] {
        assert!(
            std::panic::catch_unwind(|| build_floor(invalid)).is_err(),
            "{invalid}"
        );
    }
}

fn denied<T>(_: T) -> BuildError {
    BuildError::UnauthenticatedRelease
}
fn policy() -> Result<(TrustedPolicy, TrustedKeys), BuildError> {
    if RELEASE_REGISTRY.is_empty() || RELEASE_MIN_SEQUENCE == 0 {
        return Err(BuildError::UnauthenticatedRelease);
    }
    // The platform names the release this build belongs to; a target without one refuses.
    let target = org_platform::image::release_target().map_err(denied)?;
    let platform = match target.platform {
        ReleasePlatform::Macos => Platform::Macos,
        ReleasePlatform::Linux => Platform::Linux,
    };
    let architecture = match target.architecture {
        ReleaseArchitecture::Aarch64 => Architecture::Aarch64,
        ReleaseArchitecture::X86_64 => Architecture::X86_64,
    };
    Ok((
        TrustedPolicy::new(platform, architecture, 1, RELEASE_MIN_SEQUENCE).map_err(denied)?,
        TrustedKeys::from_registry_bytes(RELEASE_REGISTRY).map_err(denied)?,
    ))
}

pub(crate) fn load(assets: &Path, pin: Option<&WebRelease>) -> Result<BuildFiles, BuildError> {
    let Some(pin) = pin else {
        #[cfg(debug_assertions)]
        {
            // Explicit unpinned debug-build startup frames carry no
            // production release assertion. This branch does not exist in a
            // release build and cannot substitute runtime trust configuration.
            eprintln!("agentic-org-web: explicit debug unpinned fixture preload; no production release admission");
            return BuildFiles::from_directory(assets);
        }
        #[cfg(not(debug_assertions))]
        return Err(BuildError::UnauthenticatedRelease);
    };
    let (policy, keys) = policy()?;
    let root = files::Directory::open_root(assets.parent().ok_or(BuildError::UnsafeBuildEntry)?)?;
    let (manifest, signature) = authenticate(&root, pin, &policy, &keys)?;
    verify_running_image(&manifest)?;
    preload(&root, &manifest, &signature)
}

/// The running image must be the manifest's `agentic-org-web`. The image adapter
/// (`org_platform::image`) refuses on a platform without a loaded-image query, so a
/// valid manifest alone never admits this process or permits asset preload.
fn verify_running_image(manifest: &VerifiedManifest) -> Result<(), BuildError> {
    let expected = manifest
        .file("agentic-org-web")
        .and_then(|f| f.macos_cdhash())
        .ok_or(BuildError::ReleaseImageMismatch)?;
    if &org_platform::image::own_loaded_cdhash().map_err(|_| BuildError::ReleaseImageMismatch)?
        != expected
    {
        return Err(BuildError::ReleaseImageMismatch);
    }
    Ok(())
}

fn authenticate(
    root: &files::Directory,
    pin: &WebRelease,
    policy: &TrustedPolicy,
    keys: &TrustedKeys,
) -> Result<(VerifiedManifest, Vec<u8>), BuildError> {
    let bytes = root.read_control("release.v1.json", org_release::MAX_MANIFEST_BYTES)?;
    let signature = root.read_control("release.v1.sig", 64)?;
    let manifest =
        org_release::verify_manifest(&bytes, &signature, policy, keys).map_err(denied)?;
    let actual = manifest
        .manifest_sha256()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    if pin.release_id != manifest.manifest().release_id()
        || pin.sequence != manifest.manifest().sequence()
        || pin.manifest_sha256 != actual
    {
        return Err(BuildError::UnauthenticatedRelease);
    }
    Ok((manifest, signature))
}

fn preload(
    root: &files::Directory,
    manifest: &VerifiedManifest,
    signature: &[u8],
) -> Result<BuildFiles, BuildError> {
    root.verify_inventory(manifest)?;
    let mut loaded = Vec::new();
    let mut remaining = org_release::MAX_UI_BYTES;
    for entry in manifest.manifest().files() {
        if let Some(key) = entry.path().strip_prefix("ui/") {
            // Authentication/semantic budgets already succeeded, independently
            // enforce actual bytes before allocation and before deriving CSP.
            let bytes = root.read_asset(entry, &mut remaining)?;
            loaded.push((key.to_owned(), bytes));
        }
    }
    // Refuse changed controls/inventory rather than accepting a valid prefix.
    root.verify_inventory(manifest)?;
    if root.read_control("release.v1.json", org_release::MAX_MANIFEST_BYTES)?
        != manifest.manifest_bytes()
        || root.read_control("release.v1.sig", 64)? != signature
    {
        return Err(BuildError::UnauthenticatedRelease);
    }
    BuildFiles::from_files(loaded)
}

#[cfg(test)]
mod tests;
