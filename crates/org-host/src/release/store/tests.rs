use super::*;
use ring::signature::{Ed25519KeyPair, KeyPair};
use std::{fs, os::unix::fs::PermissionsExt};
mod process;

// Generated keys authenticate private synthetic payloads for journal mechanics
// only. No test injects these keys or this platform callback into production.
struct Fixture {
    temp: tempfile::TempDir,
    input: PathBuf,
    store: PathBuf,
    key: Ed25519KeyPair,
    key_id: String,
    keys: TrustedKeys,
    policy: TrustedPolicy,
}
impl Fixture {
    fn new() -> Self {
        let temp =
            tempfile::tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap()).unwrap();
        let input = temp.path().join("input");
        fs::create_dir(&input).unwrap();
        fs::create_dir(input.join("ui")).unwrap();
        for path in org_release::EXECUTABLE_PATHS
            .into_iter()
            .chain(["ui/index.html"])
        {
            fs::write(input.join(path), b"abc").unwrap();
        }
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
        let key = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let key_id = hex(&Sha256::digest(key.public_key().as_ref()));
        let registry=serde_json::to_vec(&serde_json::json!({"version":1,"keys":[{"key_id":key_id,"public_key_hex":hex(key.public_key().as_ref()),"min_sequence":1,"max_sequence":100}]})).unwrap();
        let keys = TrustedKeys::from_registry_bytes(&registry).unwrap();
        let policy = TrustedPolicy::new(
            org_release::Platform::Linux,
            org_release::Architecture::X86_64,
            1,
            1,
        )
        .unwrap();
        let store = temp.path().join("store");
        Self {
            temp,
            input,
            store,
            key,
            key_id,
            keys,
            policy,
        }
    }
    fn bundle(&self, id: &str, sequence: u64) -> DirectoryBundle {
        let files=org_release::EXECUTABLE_PATHS.into_iter().chain(["ui/index.html"]).map(|path|serde_json::json!({"path":path,"size":3,"sha256":hex(&Sha256::digest(b"abc")),"role":if path.starts_with("ui/"){"data"}else{"executable"},"macos_cdhash":null})).collect::<Vec<_>>();
        let bytes=serde_json::to_vec_pretty(&serde_json::json!({"format":org_release::FORMAT,"release_id":id,"sequence":sequence,"platform":"linux","architecture":"x86_64","installer_contract":1,"key_id":self.key_id,"files":files})).unwrap();
        fs::write(self.input.join("release.v1.json"), &bytes).unwrap();
        fs::write(
            self.input.join("release.v1.sig"),
            self.key.sign(&bytes).as_ref(),
        )
        .unwrap();
        DirectoryBundle::with_policy(&self.input, &self.policy, &self.keys).unwrap()
    }
    fn open(&self) -> ReleaseStore {
        ReleaseStore::with_policy(&self.store, self.policy.clone(), self.keys.clone()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fn private_cleanup(path: &Path) {
            if fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()) {
                fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
                for entry in fs::read_dir(path).unwrap() {
                    private_cleanup(&entry.unwrap().path());
                }
            }
        }
        private_cleanup(self.temp.path());
    }
}
fn synthetic_platform(_: &Directory, _: &VerifiedManifest) -> Result<(), Error> {
    Ok(())
}
fn install(store: &mut ReleaseStore, bundle: &DirectoryBundle) -> ReleasePin {
    store
        .install_directory_with(bundle, synthetic_platform, |_| Ok(()))
        .unwrap()
}

#[test]
fn durable_pin_reopens_exact_retry_keeps_bytes_and_refuses_conflicts_downgrades() {
    super::super::files::take_flush_counts();
    let flushed_before = org_platform::fs::testing::full_sync_counts();
    let f = Fixture::new();
    let first = f.bundle("first", 1);
    let mut store = f.open();
    let pin = install(&mut store, &first);
    let original = fs::read(f.store.join("releases/first/release.v1.json")).unwrap();
    assert_eq!(install(&mut store, &first), pin);
    assert_eq!(records(&store.pins).unwrap(), 1);
    assert!(matches!(
        ReleaseStore::with_policy(&f.store, f.policy.clone(), f.keys.clone()),
        Err(Error::Busy)
    ));
    drop(store);
    let mut store = f.open();
    assert_eq!(
        store.recover_with(synthetic_platform, true).unwrap(),
        Some(pin)
    );
    assert!(store
        .install_directory_with(&f.bundle("first", 2), synthetic_platform, |_| Ok(()))
        .is_err());
    let second = install(&mut store, &f.bundle("second", 2));
    assert_eq!(second.sequence, 2);
    assert!(store
        .install_directory_with(&f.bundle("third", 1), synthetic_platform, |_| Ok(()))
        .is_err());
    assert_eq!(
        fs::read(f.store.join("releases/first/release.v1.json")).unwrap(),
        original
    );
    assert_eq!(records(&store.pins).unwrap(), 2);
    // Public production platform checks cannot accept the synthetic payloads.
    assert!(store.recover().is_err());
    let counts = super::super::files::take_flush_counts();
    assert!(counts[0] > 0 && counts[2] > 0);
    // A device flush for every file flush on a system that has one (the platform counted
    // them on this thread), and none elsewhere.
    let device_flushes =
        org_platform::fs::testing::full_sync_counts().device_flush - flushed_before.device_flush;
    assert_eq!(counts[1] as u64, device_flushes);
    assert!(counts[1] == 0 || counts[1] == counts[0], "{counts:?}");
    println!("generated release store actual successful flush calls: file_fsync={}, macOS_F_FULLFSYNC={}, directory_fsync={}; process/reopen evidence, not physical power-loss proof",counts[0],counts[1],counts[2]);
}

#[test]
fn each_frozen_crash_boundary_reconciles_only_original_intent_without_recopying_input() {
    for boundary in [Step::Staged, Step::Intent, Step::Published, Step::Pinned] {
        let f = Fixture::new();
        let mut store = f.open();
        let first = install(&mut store, &f.bundle("first", 1));
        let second = f.bundle("second", 2);
        assert!(store
            .install_directory_with(&second, synthetic_platform, |step| if step == boundary {
                Err(Error::Io(std::io::ErrorKind::Interrupted))
            } else {
                Ok(())
            })
            .is_err());
        // Recovery must consume only the frozen private bytes, not source input.
        fs::write(
            f.input.join("ui/index.html"),
            b"changed after frozen transaction",
        )
        .unwrap();
        drop(store);
        let mut store = f.open();
        let recovered = store
            .recover_with(synthetic_platform, true)
            .unwrap()
            .unwrap();
        assert_eq!(
            recovered.release_id,
            if boundary == Step::Staged {
                "first"
            } else {
                "second"
            }
        );
        assert_eq!(
            records(&store.pins).unwrap(),
            if boundary == Step::Staged { 1 } else { 2 }
        );
        assert_eq!(
            fs::read(f.store.join("releases/first/ui/index.html")).unwrap(),
            b"abc"
        );
        assert_eq!(first.sequence, 1);
        if boundary != Step::Staged {
            assert_eq!(
                fs::read(f.store.join("releases/second/ui/index.html")).unwrap(),
                b"abc"
            );
        }
    }
}

#[test]
fn missing_pending_material_and_malformed_chain_refuse_without_previous_pin_replacement() {
    let f = Fixture::new();
    let mut store = f.open();
    install(&mut store, &f.bundle("first", 1));
    assert!(store
        .install_directory_with(&f.bundle("second", 2), synthetic_platform, |step| {
            if step == Step::Intent {
                Err(Error::Io(std::io::ErrorKind::Interrupted))
            } else {
                Ok(())
            }
        })
        .is_err());
    let first = fs::read(f.store.join("pins/00000000000000000001.json")).unwrap();
    let intent: Intent =
        decode(&store.intents.read(&record_name(2), MAX_CONTROL).unwrap()).unwrap();
    fs::rename(
        f.store.join("releases").join(stage_name(&intent.staging)),
        f.store.join("releases/.retained-incomplete"),
    )
    .unwrap();
    assert!(store.recover_with(synthetic_platform, true).is_err());
    assert_eq!(
        fs::read(f.store.join("pins/00000000000000000001.json")).unwrap(),
        first
    );
    assert!(!f.store.join("pins/00000000000000000002.json").exists());
}

#[test]
fn higher_build_floor_can_install_new_signed_input_without_admitting_old_image() {
    let f = Fixture::new();
    let mut store = f.open();
    install(&mut store, &f.bundle("first", 1));
    drop(store);
    let policy = TrustedPolicy::new(
        org_release::Platform::Linux,
        org_release::Architecture::X86_64,
        1,
        3,
    )
    .unwrap();
    let mut store = ReleaseStore::with_policy(&f.store, policy, f.keys.clone()).unwrap();
    assert!(store.recover_with(synthetic_platform, true).is_err());
    assert_eq!(install(&mut store, &f.bundle("new", 3)).sequence, 3);
    assert!(f.store.join("releases/first/release.v1.json").is_file());
}

#[test]
fn changed_store_lock_and_missing_record_are_detected_before_mutation() {
    for lock in [false, true] {
        let f = Fixture::new();
        let mut store = f.open();
        install(&mut store, &f.bundle("first", 1));
        install(&mut store, &f.bundle("second", 2));
        if lock {
            fs::rename(
                f.store.join("store.lock"),
                f.temp.path().join("retained-lock"),
            )
            .unwrap();
            fs::write(f.store.join("store.lock"), b"").unwrap();
        } else {
            fs::rename(
                f.store.join("pins/00000000000000000001.json"),
                f.temp.path().join("retained-pin"),
            )
            .unwrap();
        }
        assert!(store.recover_with(synthetic_platform, true).is_err());
    }
}
