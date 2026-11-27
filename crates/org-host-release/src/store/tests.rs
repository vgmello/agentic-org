use super::*;
use ring::signature::{Ed25519KeyPair, KeyPair};
use std::{fs, os::unix::fs::PermissionsExt};
mod process;

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
    crate::files::take_flush_counts();
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
    assert!(store.recover().is_err());
    let counts = crate::files::take_flush_counts();
    assert!(counts[0] > 0 && counts[2] > 0);
    let per_flush = org_platform::fs::testing::device_flushes_per_full_sync().unwrap();
    assert_eq!(counts[1] as u64, counts[0] as u64 * per_flush, "{counts:?}");
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

fn build_signed_tar_gz(
    f: &Fixture,
    id: &str,
    sequence: u64,
    payload: &[u8],
    tamper_sig: bool,
    tamper_payload: bool,
    extra_entry: Option<(&str, tar::EntryType)>,
) -> PathBuf {
    let files = org_release::EXECUTABLE_PATHS
        .into_iter()
        .chain(["ui/index.html"])
        .map(|path| {
            serde_json::json!({
                "path": path,
                "size": payload.len(),
                "sha256": hex(&Sha256::digest(payload)),
                "role": if path.starts_with("ui/") { "data" } else { "executable" },
                "macos_cdhash": null
            })
        })
        .collect::<Vec<_>>();
    let manifest_bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "format": org_release::FORMAT,
        "release_id": id,
        "sequence": sequence,
        "platform": "linux",
        "architecture": "x86_64",
        "installer_contract": 1,
        "key_id": f.key_id,
        "files": files
    }))
    .unwrap();
    let mut sig = f.key.sign(&manifest_bytes).as_ref().to_vec();
    if tamper_sig {
        sig[0] ^= 0xff;
    }

    let mut archive = tar::Builder::new(Vec::new());
    for path in org_release::EXECUTABLE_PATHS
        .into_iter()
        .chain(["ui/index.html"])
    {
        let mut data = payload.to_vec();
        if tamper_payload && path == "ui/index.html" {
            data[0] ^= 0x01;
        }
        let mut header = tar::Header::new_ustar();
        header.set_size(data.len() as u64);
        header.set_mode(0o600);
        header.set_cksum();
        archive
            .append_data(&mut header, path, data.as_slice())
            .unwrap();
    }
    for (name, bytes) in [
        ("release.v1.json", manifest_bytes.as_slice()),
        ("release.v1.sig", sig.as_slice()),
    ] {
        let mut header = tar::Header::new_ustar();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o600);
        header.set_cksum();
        archive.append_data(&mut header, name, bytes).unwrap();
    }
    if let Some((name, kind)) = extra_entry {
        let mut header = tar::Header::new_ustar();
        header.set_size(3);
        header.set_mode(0o600);
        header.set_entry_type(kind);
        if kind.is_symlink() {
            header.set_link_name("agentic-org").unwrap();
        }
        header.set_cksum();
        if name.contains("..") {
            // Write raw header bytes directly when testing path traversal rejection.
            let mut raw = [0u8; 512];
            raw[..name.len()].copy_from_slice(name.as_bytes());
            raw[100..107].copy_from_slice(b"0000600");
            raw[124..135].copy_from_slice(b"00000000003");
            raw[156] = b'0';
            raw[257..263].copy_from_slice(b"ustar\0");
            raw[263..265].copy_from_slice(b"00");
            raw[148..156].fill(b' ');
            let sum: u32 = raw.iter().map(|&b| u32::from(b)).sum();
            let cksum = format!("{sum:06o}\0 ");
            raw[148..156].copy_from_slice(cksum.as_bytes());
            let mut inner = archive.into_inner().unwrap();
            let trim = inner.len().saturating_sub(1024);
            inner.truncate(trim);
            inner.extend_from_slice(&raw);
            let mut block = [0u8; 512];
            block[..3].copy_from_slice(b"abc");
            inner.extend_from_slice(&block);
            inner.extend_from_slice(&[0u8; 1024]);
            let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            std::io::Write::write_all(&mut gz, &inner).unwrap();
            let out = f.temp.path().join(format!("{id}-{sequence}-trav.tar.gz"));
            fs::write(&out, gz.finish().unwrap()).unwrap();
            return out;
        }
        archive.append_data(&mut header, name, &b"abc"[..]).unwrap();
    }
    let tar_bytes = archive.into_inner().unwrap();
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    std::io::Write::write_all(&mut gz, &tar_bytes).unwrap();
    let out = f.temp.path().join(format!(
        "{id}-{sequence}-{tamper_sig}-{tamper_payload}.tar.gz"
    ));
    fs::write(&out, gz.finish().unwrap()).unwrap();
    out
}

#[test]
fn end_to_end_signed_archive_stage_verify_install_update_and_rollback_safety() {
    let f = Fixture::new();
    let stage_root = f.temp.path().join("stage");
    org_platform::fs::create_owner_only_dir(&stage_root).unwrap();
    let stage_dir = Directory::open(&stage_root).unwrap();

    // 1. Fail-closed verification on bad signature, tampered payload, symlink, and path traversal.
    let bad_sig = build_signed_tar_gz(&f, "bad-sig", 1, b"v1-bin", true, false, None);
    let bad_hash = build_signed_tar_gz(&f, "bad-hash", 1, b"v1-bin", false, true, None);
    let bad_symlink = build_signed_tar_gz(
        &f,
        "bad-sym",
        1,
        b"v1-bin",
        false,
        false,
        Some(("ui/evil", tar::EntryType::Symlink)),
    );
    let bad_trav = build_signed_tar_gz(
        &f,
        "bad-trav",
        1,
        b"v1-bin",
        false,
        false,
        Some(("../escape", tar::EntryType::Regular)),
    );
    for evil in [&bad_sig, &bad_hash, &bad_symlink, &bad_trav] {
        let frozen = stage_dir
            .freeze_archive(evil, org_release::MAX_PAYLOAD_BYTES)
            .unwrap();
        assert!(
            ArchiveBundle::with_policy(frozen, &f.policy, &f.keys).is_err(),
            "adversarial archive must fail closed: {}",
            evil.display()
        );
    }

    // 2. Stage, verify, and install signed v1 archive (sequence 1).
    let v1_path = build_signed_tar_gz(&f, "rel-1.0.0", 1, b"v1-bin", false, false, None);
    let v1_frozen = stage_dir
        .freeze_archive(&v1_path, org_release::MAX_PAYLOAD_BYTES)
        .unwrap();
    let mut v1_bundle = ArchiveBundle::with_policy(v1_frozen, &f.policy, &f.keys).unwrap();
    let mut store = f.open();
    let pin1 = store
        .install_archive_with(&mut v1_bundle, synthetic_platform, |_| Ok(()))
        .unwrap();
    assert_eq!(pin1.release_id, "rel-1.0.0");
    assert_eq!(pin1.sequence, 1);
    assert_eq!(
        fs::metadata(f.store.join("releases/rel-1.0.0/agentic-org"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o500
    );
    assert_eq!(
        fs::metadata(f.store.join("releases/rel-1.0.0/ui/index.html"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o400
    );

    // 3. Rollback safety: an interrupted v2 staging fault before intent leaves v1 active cleanly.
    let v2_path = build_signed_tar_gz(&f, "rel-2.0.0", 2, b"v2-bin", false, false, None);
    let v2_frozen_fail = stage_dir
        .freeze_archive(&v2_path, org_release::MAX_PAYLOAD_BYTES)
        .unwrap();
    let mut v2_bundle_fail =
        ArchiveBundle::with_policy(v2_frozen_fail, &f.policy, &f.keys).unwrap();
    assert!(store
        .install_archive_with(&mut v2_bundle_fail, synthetic_platform, |step| {
            if step == Step::Staged {
                Err(Error::Io(std::io::ErrorKind::Interrupted))
            } else {
                Ok(())
            }
        })
        .is_err());
    drop(store);
    let mut store = f.open();
    assert_eq!(
        store.recover_with(synthetic_platform, true).unwrap(),
        Some(pin1.clone())
    );

    // 4. Atomic update activation to v2 (sequence 2).
    let v2_frozen = stage_dir
        .freeze_archive(&v2_path, org_release::MAX_PAYLOAD_BYTES)
        .unwrap();
    let mut v2_bundle = ArchiveBundle::with_policy(v2_frozen, &f.policy, &f.keys).unwrap();
    let pin2 = store
        .install_archive_with(&mut v2_bundle, synthetic_platform, |_| Ok(()))
        .unwrap();
    assert_eq!(pin2.release_id, "rel-2.0.0");
    assert_eq!(pin2.sequence, 2);
    assert_eq!(
        fs::read(f.store.join("releases/rel-2.0.0/ui/index.html")).unwrap(),
        b"v2-bin"
    );
    assert_eq!(
        fs::read(f.store.join("releases/rel-1.0.0/ui/index.html")).unwrap(),
        b"v1-bin"
    );

    // 5. Downgrade (sequence 1 after sequence 2) MUST be refused fail-closed.
    let downgrade_path = build_signed_tar_gz(&f, "rel-1.0.1-old", 1, b"v1-old", false, false, None);
    let dg_frozen = stage_dir
        .freeze_archive(&downgrade_path, org_release::MAX_PAYLOAD_BYTES)
        .unwrap();
    let mut dg_bundle = ArchiveBundle::with_policy(dg_frozen, &f.policy, &f.keys).unwrap();
    assert!(store
        .install_archive_with(&mut dg_bundle, synthetic_platform, |_| Ok(()))
        .is_err());
    assert_eq!(store.pin(), Some(&pin2));
}
