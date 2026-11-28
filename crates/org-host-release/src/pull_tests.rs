use super::*;
use crate::fetcher::LocalDirectoryFetcher;
use ring::signature::{Ed25519KeyPair, KeyPair};
use sha2::{Digest, Sha256};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

struct PullFixture {
    _temp: tempfile::TempDir,
    mirror: PathBuf,
    staging: PathBuf,
    key: Ed25519KeyPair,
    key_id: String,
    keys: TrustedKeys,
    policy: TrustedPolicy,
}

impl PullFixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let mirror = root.join("mirror");
        let staging = root.join("staging");
        org_platform::fs::create_owner_only_dir(&mirror).unwrap();
        org_platform::fs::create_owner_only_dir(&staging).unwrap();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
        let key = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let pub_bytes = key.public_key().as_ref();
        let key_id = hex(Sha256::digest(pub_bytes).as_slice());
        let reg = serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "keys": [{
                "key_id": key_id,
                "public_key_hex": hex(pub_bytes),
                "min_sequence": 1,
                "max_sequence": 100
            }]
        }))
        .unwrap();
        let keys = TrustedKeys::from_registry_bytes(&reg).unwrap();
        let policy = TrustedPolicy::new(
            org_release::Platform::Linux,
            org_release::Architecture::X86_64,
            1,
            1,
        )
        .unwrap();
        Self {
            _temp: temp,
            mirror,
            staging,
            key,
            key_id,
            keys,
            policy,
        }
    }

    fn write_signed_bundle(
        &self,
        release_id: &str,
        sequence: u64,
        file_name: &str,
    ) -> (PathBuf, String, u64) {
        let payload = b"abc";
        let files = org_release::EXECUTABLE_PATHS
            .into_iter()
            .chain(["ui/index.html"])
            .map(|path| {
                serde_json::json!({
                    "path": path,
                    "size": payload.len(),
                    "sha256": hex(Sha256::digest(payload).as_slice()),
                    "role": if path.starts_with("ui/") { "data" } else { "executable" },
                    "macos_cdhash": null
                })
            })
            .collect::<Vec<_>>();
        let manifest = serde_json::to_vec_pretty(&serde_json::json!({
            "format": org_release::FORMAT,
            "release_id": release_id,
            "sequence": sequence,
            "platform": "linux",
            "architecture": "x86_64",
            "installer_contract": 1,
            "key_id": self.key_id,
            "files": files
        }))
        .unwrap();
        let sig = self.key.sign(&manifest).as_ref().to_vec();
        let mut tar = tar::Builder::new(Vec::new());
        for path in org_release::EXECUTABLE_PATHS
            .into_iter()
            .chain(["ui/index.html"])
        {
            let mut h = tar::Header::new_ustar();
            h.set_size(payload.len() as u64);
            h.set_mode(0o600);
            h.set_cksum();
            tar.append_data(&mut h, path, &payload[..]).unwrap();
        }
        for (name, bytes) in [
            ("release.v1.json", manifest.as_slice()),
            ("release.v1.sig", sig.as_slice()),
        ] {
            let mut h = tar::Header::new_ustar();
            h.set_size(bytes.len() as u64);
            h.set_mode(0o600);
            h.set_cksum();
            tar.append_data(&mut h, name, bytes).unwrap();
        }
        let tar_bytes = tar.into_inner().unwrap();
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(&tar_bytes).unwrap();
        let gz = enc.finish().unwrap();
        let dest = self.mirror.join(file_name);
        fs::write(&dest, &gz).unwrap();
        let digest = hex(Sha256::digest(&gz).as_slice());
        (dest, digest, gz.len() as u64)
    }

    fn write_signed_pointer(
        &self,
        release_id: &str,
        sequence: u64,
        bundle_path: &Path,
        bundle_sha: &str,
        bundle_size: u64,
    ) -> PullConfig {
        let ptr_bytes = serde_json::to_vec_pretty(&serde_json::json!({
            "format": org_release::POINTER_FORMAT,
            "channel": "stable",
            "release_id": release_id,
            "sequence": sequence,
            "installer_contract": 1,
            "key_id": self.key_id,
            "published_at": "2026-11-27T23:00:00Z",
            "targets": [{
                "platform": "linux",
                "architecture": "x86_64",
                "url": format!("file://{}", bundle_path.display()),
                "sha256": bundle_sha,
                "size": bundle_size
            }]
        }))
        .unwrap();
        let sig = self.key.sign(&ptr_bytes).as_ref().to_vec();
        let ptr_path = self.mirror.join("release-pointer.v1.json");
        let sig_path = self.mirror.join("release-pointer.v1.sig");
        fs::write(&ptr_path, ptr_bytes).unwrap();
        fs::write(&sig_path, sig).unwrap();
        PullConfig::with_url_policy(
            format!("file://{}", ptr_path.display()),
            None,
            UrlPolicy::AllowLocalForTesting,
        )
    }
}

#[test]
fn stages_valid_signed_pointer_and_bundle_and_cleans_up_on_drop() {
    let fx = PullFixture::new();
    let (bundle_path, sha, size) = fx.write_signed_bundle("v0.2.0", 2, "v0.2.0.tar.gz");
    let cfg = fx.write_signed_pointer("v0.2.0", 2, &bundle_path, &sha, size);
    let fetcher = LocalDirectoryFetcher::new(&fx.mirror).unwrap();
    let state = UpdateState::default();
    let check_state = PointerCheckState {
        current_pin: None,
        highest_sequence: 0,
        update_state: &state,
        allow_quarantined: false,
    };
    let check =
        check_pointer_with_policy(&fetcher, &cfg, &fx.policy, &fx.keys, &check_state).unwrap();
    assert_eq!(check.decision, PointerDecision::UpdateAvailable);

    let staged = stage_verified_pointer_with_policy(
        &fetcher,
        &cfg,
        check.verified,
        &fx.staging,
        &fx.policy,
        &fx.keys,
    )
    .unwrap();
    assert_eq!(staged.bundle().manifest().manifest().release_id(), "v0.2.0");
    assert_eq!(fs::read_dir(&fx.staging).unwrap().count(), 1);
    drop(staged);
    assert_eq!(fs::read_dir(&fx.staging).unwrap().count(), 0);
}

#[test]
fn rejects_tampered_bundle_hash_and_cross_binding_mismatch() {
    let fx = PullFixture::new();
    let (bundle_v1, sha_v1, size_v1) = fx.write_signed_bundle("v0.1.0", 1, "v0.1.0.tar.gz");
    let fetcher = LocalDirectoryFetcher::new(&fx.mirror).unwrap();

    // 1. Cross-binding attack: signed pointer says v0.2.0 seq 2, but points to legitimately signed v0.1.0 seq 1 bundle.
    let state = UpdateState::default();
    let check_state = PointerCheckState {
        current_pin: None,
        highest_sequence: 0,
        update_state: &state,
        allow_quarantined: false,
    };
    let cfg_mismatch = fx.write_signed_pointer("v0.2.0", 2, &bundle_v1, &sha_v1, size_v1);
    let check =
        check_pointer_with_policy(&fetcher, &cfg_mismatch, &fx.policy, &fx.keys, &check_state)
            .unwrap();
    let err = stage_verified_pointer_with_policy(
        &fetcher,
        &cfg_mismatch,
        check.verified,
        &fx.staging,
        &fx.policy,
        &fx.keys,
    )
    .err()
    .expect("cross-binding mismatch must fail");
    assert!(matches!(err, PullError::CrossBindingMismatch), "{err}");

    // 2. Tampered bundle byte fails streaming SHA-256 verification before unpacking.
    let mut tampered = fs::read(&bundle_v1).unwrap();
    tampered[0] ^= 0xff;
    let tampered_path = fx.mirror.join("tampered.tar.gz");
    fs::write(&tampered_path, &tampered).unwrap();
    let cfg_tampered = fx.write_signed_pointer("v0.1.0", 1, &tampered_path, &sha_v1, size_v1);
    let check =
        check_pointer_with_policy(&fetcher, &cfg_tampered, &fx.policy, &fx.keys, &check_state)
            .unwrap();
    let err = stage_verified_pointer_with_policy(
        &fetcher,
        &cfg_tampered,
        check.verified,
        &fx.staging,
        &fx.policy,
        &fx.keys,
    )
    .err()
    .expect("tampered bundle hash must fail");
    assert!(
        matches!(
            err,
            PullError::Verify(org_release::Error::PayloadDigestMismatch)
        ),
        "{err}"
    );
}

#[test]
fn p2_2_quarantined_unconfirmed_active_pin_reports_quarantined_not_up_to_date() {
    let fx = PullFixture::new();
    let (v1_bundle, v1_sha, v1_size) = fx.write_signed_bundle("v0.1.0", 1, "v0.1.0.tar.gz");
    let cfg_v1 = fx.write_signed_pointer("v0.1.0", 1, &v1_bundle, &v1_sha, v1_size);
    let fetcher = LocalDirectoryFetcher::new(&fx.mirror).unwrap();

    let v1_pin = ReleasePin {
        release_id: "v0.1.0".into(),
        manifest_sha256: "1".repeat(64),
        sequence: 1,
    };
    let v2_pin = ReleasePin {
        release_id: "v0.2.0".into(),
        manifest_sha256: "2".repeat(64),
        sequence: 2,
    };
    let quarantined_v1 = crate::update_state::QuarantinedRelease {
        release_id: "v0.1.0".into(),
        sequence: 1,
        manifest_sha256: v1_pin.manifest_sha256.clone(),
        reason: "post-restart health check failed".into(),
        quarantined_at_ms: 1_700_000_000_000,
    };

    // 1. First-ever install (`confirmed == None`) failed health & quarantined: NEVER UpToDate.
    let unconfirmed_state = UpdateState {
        version: 1,
        confirmed: None,
        pending_health_check: None,
        quarantined: vec![quarantined_v1.clone()],
    };
    let check = check_pointer_with_policy(
        &fetcher,
        &cfg_v1,
        &fx.policy,
        &fx.keys,
        &PointerCheckState {
            current_pin: Some(&v1_pin),
            highest_sequence: 1,
            update_state: &unconfirmed_state,
            allow_quarantined: false,
        },
    )
    .unwrap();
    assert_eq!(
        check.decision,
        PointerDecision::Quarantined {
            reason: "post-restart health check failed".into(),
        }
    );

    // 2. Explicit `--allow-quarantined` opt-in sees the active pin as UpToDate.
    let allowed = check_pointer_with_policy(
        &fetcher,
        &cfg_v1,
        &fx.policy,
        &fx.keys,
        &PointerCheckState {
            current_pin: Some(&v1_pin),
            highest_sequence: 1,
            update_state: &unconfirmed_state,
            allow_quarantined: true,
        },
    )
    .unwrap();
    assert_eq!(allowed.decision, PointerDecision::UpToDate);

    // 3. Once the active pin IS confirmed healthy, it reports UpToDate even if formerly quarantined.
    let confirmed_state = UpdateState {
        version: 1,
        confirmed: Some(v1_pin.clone()),
        pending_health_check: None,
        quarantined: vec![quarantined_v1],
    };
    let confirmed = check_pointer_with_policy(
        &fetcher,
        &cfg_v1,
        &fx.policy,
        &fx.keys,
        &PointerCheckState {
            current_pin: Some(&v1_pin),
            highest_sequence: 1,
            update_state: &confirmed_state,
            allow_quarantined: false,
        },
    )
    .unwrap();
    assert_eq!(confirmed.decision, PointerDecision::UpToDate);

    // 4. When confirmed is a different pin (e.g. v1 confirmed, v2 active & quarantined), NEVER UpToDate.
    let (v2_bundle, v2_sha, v2_size) = fx.write_signed_bundle("v0.2.0", 2, "v0.2.0.tar.gz");
    let cfg_v2 = fx.write_signed_pointer("v0.2.0", 2, &v2_bundle, &v2_sha, v2_size);
    let failed_rb_state = UpdateState {
        version: 1,
        confirmed: Some(v1_pin),
        pending_health_check: None,
        quarantined: vec![crate::update_state::QuarantinedRelease {
            release_id: "v0.2.0".into(),
            sequence: 2,
            manifest_sha256: v2_pin.manifest_sha256.clone(),
            reason: "v2 health check failed".into(),
            quarantined_at_ms: 1_700_000_000_001,
        }],
    };
    let check_v2 = check_pointer_with_policy(
        &fetcher,
        &cfg_v2,
        &fx.policy,
        &fx.keys,
        &PointerCheckState {
            current_pin: Some(&v2_pin),
            highest_sequence: 2,
            update_state: &failed_rb_state,
            allow_quarantined: false,
        },
    )
    .unwrap();
    assert_eq!(
        check_v2.decision,
        PointerDecision::Quarantined {
            reason: "v2 health check failed".into(),
        }
    );

    // 5. A newer non-quarantined pointer v2 over quarantined first-ever v1 is UpdateAvailable.
    let upgrade = check_pointer_with_policy(
        &fetcher,
        &cfg_v2,
        &fx.policy,
        &fx.keys,
        &PointerCheckState {
            current_pin: Some(&ReleasePin {
                release_id: "v0.1.0".into(),
                manifest_sha256: "1".repeat(64),
                sequence: 1,
            }),
            highest_sequence: 1,
            update_state: &unconfirmed_state,
            allow_quarantined: false,
        },
    )
    .unwrap();
    assert_eq!(upgrade.decision, PointerDecision::UpdateAvailable);
}
