use super::*;
use std::{
    fs,
    os::unix::fs::{symlink, PermissionsExt},
    path::PathBuf,
};

const MANIFEST: &[u8] = include_bytes!("fixtures/release.v1.json");
const SIGNATURE: &[u8] = include_bytes!("fixtures/release.v1.sig");
const HTML: &[u8] = include_bytes!("fixtures/index.html");
struct Input {
    temp: tempfile::TempDir,
    path: PathBuf,
}
impl Input {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().canonicalize().unwrap().join("release");
        fs::create_dir_all(path.join("ui/_app")).unwrap();
        for executable in org_release::EXECUTABLE_PATHS {
            fs::write(path.join(executable), b"abc").unwrap();
        }
        fs::write(path.join("ui/index.html"), HTML).unwrap();
        fs::write(path.join("ui/_app/a.js"), include_bytes!("fixtures/a.js")).unwrap();
        fs::write(path.join("release.v1.json"), MANIFEST).unwrap();
        fs::write(path.join("release.v1.sig"), SIGNATURE).unwrap();
        let input = Self { temp, path };
        input.seal();
        input
    }
    fn seal(&self) {
        for name in org_release::EXECUTABLE_PATHS {
            mode(&self.path.join(name), 0o500);
        }
        for name in [
            "ui/index.html",
            "ui/_app/a.js",
            "release.v1.json",
            "release.v1.sig",
        ] {
            mode(&self.path.join(name), 0o400);
        }
        for name in ["ui/_app", "ui", ""] {
            mode(&self.path.join(name), 0o500);
        }
    }
    fn directory(&self) -> files::Directory {
        files::Directory::open_root(&self.path).unwrap()
    }
    fn verified(&self) -> (VerifiedManifest, Vec<u8>) {
        let keys =
            TrustedKeys::from_registry_bytes(include_bytes!("fixtures/registry.json")).unwrap();
        let policy = TrustedPolicy::new(Platform::Linux, Architecture::X86_64, 1, 1).unwrap();
        authenticate(&self.directory(), &pin(), &policy, &keys).unwrap()
    }
    fn replace(&self, name: &str, bytes: &[u8]) {
        let path = self.path.join(name);
        mode(&path, 0o600);
        fs::write(&path, bytes).unwrap();
        mode(&path, 0o400);
    }
}
impl Drop for Input {
    fn drop(&mut self) {
        // Only generated, owned temporary fixture directories are unsealed.
        fn unseal(path: &Path) {
            if let Ok(m) = fs::symlink_metadata(path) {
                if m.is_dir() {
                    mode(path, 0o700);
                    for e in fs::read_dir(path).unwrap() {
                        unseal(&e.unwrap().path());
                    }
                }
            }
        }
        unseal(self.temp.path());
    }
}
fn mode(path: &Path, value: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(value)).unwrap();
}
fn pin() -> WebRelease {
    use sha2::{Digest, Sha256};
    WebRelease {
        release_id: "gateway-input-fixture-1".into(),
        sequence: 1,
        manifest_sha256: Sha256::digest(MANIFEST)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    }
}

#[test]
fn signed_assets_are_verified_before_csp_and_remain_memory_only() {
    let input = Input::new();
    let root = input.directory();
    let (manifest, signature) = input.verified();
    let files = preload(&root, &manifest, &signature).unwrap();
    assert_eq!(files.file("index.html").unwrap().as_ref(), HTML);
    assert_eq!(
        files.file("_app/a.js").unwrap().as_ref(),
        include_bytes!("fixtures/a.js")
    );
    input.replace("ui/index.html", b"changed after immutable preload");
    assert_eq!(files.file("index.html").unwrap().as_ref(), HTML);
    assert!(preload(&root, &manifest, &signature).is_err());
}

#[test]
fn signed_manifest_alone_cannot_admit_the_running_image() {
    let input = Input::new();
    let (manifest, _) = input.verified();
    // This signed Linux fixture has no macOS image identity. macOS must reject
    // it, and unsupported platforms must refuse even a valid signed manifest.
    assert!(matches!(
        verify_running_image(&manifest),
        Err(BuildError::ReleaseImageMismatch)
    ));
}

#[test]
fn changed_pin_controls_assets_and_actual_byte_budget_refuse() {
    let input = Input::new();
    let (manifest, signature) = input.verified();
    let root = input.directory();
    let mut remaining = 1;
    assert!(matches!(
        root.read_asset(manifest.file("ui/index.html").unwrap(), &mut remaining),
        Err(BuildError::TooLarge)
    ));
    assert_eq!(remaining, 1);
    let keys = TrustedKeys::from_registry_bytes(include_bytes!("fixtures/registry.json")).unwrap();
    let policy = TrustedPolicy::new(Platform::Linux, Architecture::X86_64, 1, 1).unwrap();
    let mut other = pin();
    other.sequence = 2;
    assert!(authenticate(&root, &other, &policy, &keys).is_err());
    other = pin();
    other.manifest_sha256.replace_range(..1, "f");
    assert!(authenticate(&root, &other, &policy, &keys).is_err());
    let mut same_size = HTML.to_vec();
    same_size[0] ^= 1;
    input.replace("ui/index.html", &same_size);
    assert!(preload(&root, &manifest, &signature).is_err());
    input.replace("ui/index.html", HTML);
    input.replace("release.v1.sig", &[0; 64]);
    assert!(authenticate(&root, &pin(), &policy, &keys).is_err());
    assert!(preload(&root, &manifest, &signature).is_err());
    input.replace("release.v1.sig", SIGNATURE);
    input.replace("release.v1.json", b"{}");
    assert!(authenticate(&root, &pin(), &policy, &keys).is_err());
    assert!(preload(&root, &manifest, &signature).is_err());
}
#[test]
fn closed_inventory_modes_links_and_missing_paths_refuse() {
    for kind in [
        "extra",
        "empty",
        "symlink",
        "hardlink",
        "missing",
        "writable",
        "ancestor-link",
    ] {
        let input = Input::new();
        let (manifest, signature) = input.verified();
        mode(&input.path.join("ui"), 0o700);
        match kind {
            "extra" => {
                fs::write(input.path.join("ui/extra.js"), b"x").unwrap();
            }
            "empty" => {
                fs::create_dir(input.path.join("ui/empty")).unwrap();
            }
            "symlink" => {
                fs::remove_file(input.path.join("ui/index.html")).unwrap();
                symlink("_app/a.js", input.path.join("ui/index.html")).unwrap();
            }
            "hardlink" => {
                fs::hard_link(
                    input.path.join("ui/index.html"),
                    input.temp.path().join("second-link"),
                )
                .unwrap();
            }
            "missing" => {
                fs::remove_file(input.path.join("ui/index.html")).unwrap();
            }
            "writable" => mode(&input.path.join("ui/index.html"), 0o600),
            "ancestor-link" => {
                symlink(&input.path, input.temp.path().join("alias")).unwrap();
                assert!(files::Directory::open_root(&input.temp.path().join("alias")).is_err());
            }
            _ => unreachable!(),
        }
        mode(&input.path.join("ui"), 0o500);
        if kind != "ancestor-link" {
            assert!(
                preload(&input.directory(), &manifest, &signature).is_err(),
                "{kind}"
            );
        }
    }
}
#[test]
fn held_release_directory_cannot_switch_to_replaced_path() {
    let input = Input::new();
    let root = input.directory();
    let (manifest, signature) = input.verified();
    let moved = input.temp.path().join("original");
    // macOS refuses to rename a directory its owner cannot write, so it is made writable for the
    // move and sealed again at its new name; the held directory handle is what the test is about.
    mode(&input.path, 0o700);
    fs::rename(&input.path, &moved).unwrap();
    mode(&moved, 0o500);
    fs::create_dir(&input.path).unwrap();
    mode(&input.path, 0o500);
    let files = preload(&root, &manifest, &signature).unwrap();
    assert_eq!(files.file("index.html").unwrap().as_ref(), HTML);
    assert!(preload(&input.directory(), &manifest, &signature).is_err());
}
#[test]
fn missing_embedded_trust_fails_before_input_open() {
    if RELEASE_REGISTRY.is_empty() {
        assert!(matches!(
            load(Path::new("/missing/ui"), Some(&pin())),
            Err(BuildError::UnauthenticatedRelease)
        ));
    }
}
