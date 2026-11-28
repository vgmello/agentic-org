use super::*;
use ring::signature::{Ed25519KeyPair, KeyPair};

struct PointerFixture {
    key: Ed25519KeyPair,
    key_id: String,
    keys: TrustedKeys,
    policy: TrustedPolicy,
}

impl PointerFixture {
    fn new(platform: Platform, architecture: Architecture, min_seq: u64) -> Self {
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
        let key = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let pub_bytes = key.public_key().as_ref();
        let key_id = hex(Sha256::digest(pub_bytes).as_slice());
        let registry = serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "keys": [{
                "key_id": key_id,
                "public_key_hex": hex(pub_bytes),
                "min_sequence": 1,
                "max_sequence": 100
            }]
        }))
        .unwrap();
        let keys = TrustedKeys::from_registry_bytes(&registry).unwrap();
        let policy = TrustedPolicy::new(platform, architecture, 1, min_seq).unwrap();
        Self {
            key,
            key_id,
            keys,
            policy,
        }
    }

    fn sign_json(&self, value: &serde_json::Value) -> (Vec<u8>, Vec<u8>) {
        let bytes = serde_json::to_vec_pretty(value).unwrap();
        let sig = self.key.sign(&bytes).as_ref().to_vec();
        (bytes, sig)
    }

    fn valid_pointer_value(&self, sequence: u64) -> serde_json::Value {
        serde_json::json!({
            "format": POINTER_FORMAT,
            "channel": "stable",
            "release_id": format!("v0.2.{sequence}"),
            "sequence": sequence,
            "installer_contract": 1,
            "key_id": self.key_id,
            "published_at": "2026-11-27T23:00:00Z",
            "targets": [
                {
                    "platform": "linux",
                    "architecture": "x86_64",
                    "url": "https://github.com/vgmello/agentic-org/releases/download/v0.2.0/agentic-org-v0.2.0-linux-x86_64.tar.gz",
                    "sha256": "a".repeat(64),
                    "size": 1024
                },
                {
                    "platform": "macos",
                    "architecture": "aarch64",
                    "url": "https://github.com/vgmello/agentic-org/releases/download/v0.2.0/agentic-org-v0.2.0-macos-aarch64.tar.gz",
                    "sha256": "b".repeat(64),
                    "size": 2048
                }
            ]
        })
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn verifies_signed_multi_target_pointer_and_selects_matching_target() {
    let fx = PointerFixture::new(Platform::Linux, Architecture::X86_64, 1);
    let (bytes, sig) = fx.sign_json(&fx.valid_pointer_value(5));
    let verified =
        verify_pointer(&bytes, &sig, &fx.policy, &fx.keys, UrlPolicy::HttpsOnly).unwrap();
    assert_eq!(verified.pointer().channel(), "stable");
    assert_eq!(verified.pointer().release_id(), "v0.2.5");
    assert_eq!(verified.pointer().sequence(), 5);
    assert_eq!(verified.pointer().target().platform(), Platform::Linux);
    assert_eq!(
        verified.pointer().target().architecture(),
        Architecture::X86_64
    );
    assert_eq!(verified.pointer().target().size(), 1024);
    assert_eq!(verified.pointer().target().sha256_hex(), "a".repeat(64));

    let mac_policy = TrustedPolicy::new(Platform::Macos, Architecture::Aarch64, 1, 1).unwrap();
    let mac = verify_pointer(&bytes, &sig, &mac_policy, &fx.keys, UrlPolicy::HttpsOnly).unwrap();
    assert_eq!(mac.pointer().target().platform(), Platform::Macos);
    assert_eq!(mac.pointer().target().size(), 2048);
}

#[test]
fn rejects_forged_signature_unknown_key_and_wrong_length() {
    let fx = PointerFixture::new(Platform::Linux, Architecture::X86_64, 1);
    let (bytes, mut sig) = fx.sign_json(&fx.valid_pointer_value(5));

    assert_eq!(
        verify_pointer(
            &bytes,
            &sig[..63],
            &fx.policy,
            &fx.keys,
            UrlPolicy::HttpsOnly
        )
        .unwrap_err(),
        Error::InvalidSignatureLength
    );

    sig[0] ^= 0xff;
    assert_eq!(
        verify_pointer(&bytes, &sig, &fx.policy, &fx.keys, UrlPolicy::HttpsOnly).unwrap_err(),
        Error::InvalidSignature
    );

    let other = PointerFixture::new(Platform::Linux, Architecture::X86_64, 1);
    let (other_bytes, other_sig) = other.sign_json(&other.valid_pointer_value(5));
    assert_eq!(
        verify_pointer(
            &other_bytes,
            &other_sig,
            &fx.policy,
            &fx.keys,
            UrlPolicy::HttpsOnly
        )
        .unwrap_err(),
        Error::UnknownSigningKey
    );
}

#[test]
fn rejects_sequence_downgrade_zero_and_key_window_violation() {
    let fx = PointerFixture::new(Platform::Linux, Architecture::X86_64, 10);
    let (below, sig) = fx.sign_json(&fx.valid_pointer_value(9));
    assert_eq!(
        verify_pointer(&below, &sig, &fx.policy, &fx.keys, UrlPolicy::HttpsOnly).unwrap_err(),
        Error::ReleaseBelowMinimum
    );

    let (zero, sig) = fx.sign_json(&fx.valid_pointer_value(0));
    assert_eq!(
        verify_pointer(&zero, &sig, &fx.policy, &fx.keys, UrlPolicy::HttpsOnly).unwrap_err(),
        Error::InvalidSequence
    );

    let (over_max, sig) = fx.sign_json(&fx.valid_pointer_value(101));
    assert_eq!(
        verify_pointer(&over_max, &sig, &fx.policy, &fx.keys, UrlPolicy::HttpsOnly).unwrap_err(),
        Error::KeySequenceOutOfRange
    );
}

#[test]
fn rejects_duplicate_unknown_and_escaped_json_keys() {
    let fx = PointerFixture::new(Platform::Linux, Architecture::X86_64, 1);
    let mut val = fx.valid_pointer_value(5);
    val.as_object_mut()
        .unwrap()
        .insert("trusted_keys".into(), serde_json::json!([]));
    let (unknown_bytes, unknown_sig) = fx.sign_json(&val);
    assert_eq!(
        verify_pointer(
            &unknown_bytes,
            &unknown_sig,
            &fx.policy,
            &fx.keys,
            UrlPolicy::HttpsOnly
        )
        .unwrap_err(),
        Error::InvalidPointer
    );

    // Top-level duplicate key and Unicode-escaped duplicate key.
    for dup in [
        format!(
            r#"{{"format":"{POINTER_FORMAT}","channel":"stable","release_id":"v0.2.5","sequence":5,"sequence":6,"installer_contract":1,"key_id":"{}","published_at":"2026-11-27T23:00:00Z","targets":[{{"platform":"linux","architecture":"x86_64","url":"https://example.com/a.tar.gz","sha256":"{}","size":10}}]}}"#,
            fx.key_id,
            "a".repeat(64)
        ),
        format!(
            r#"{{"format":"{POINTER_FORMAT}","channel":"stable","release_id":"v0.2.5","sequence":5,"\u0073equence":5,"installer_contract":1,"key_id":"{}","published_at":"2026-11-27T23:00:00Z","targets":[{{"platform":"linux","architecture":"x86_64","url":"https://example.com/a.tar.gz","sha256":"{}","size":10}}]}}"#,
            fx.key_id,
            "a".repeat(64)
        ),
        format!(
            r#"{{"format":"{POINTER_FORMAT}","channel":"stable","release_id":"v0.2.5","sequence":5,"installer_contract":1,"key_id":"{}","published_at":"2026-11-27T23:00:00Z","targets":[{{"platform":"linux","architecture":"x86_64","url":"https://example.com/a.tar.gz","url":"https://evil.example/b.tar.gz","sha256":"{}","size":10}}]}}"#,
            fx.key_id,
            "a".repeat(64)
        ),
    ] {
        let sig = fx.key.sign(dup.as_bytes()).as_ref().to_vec();
        assert_eq!(
            verify_pointer(
                dup.as_bytes(),
                &sig,
                &fx.policy,
                &fx.keys,
                UrlPolicy::HttpsOnly
            )
            .unwrap_err(),
            Error::InvalidPointer,
            "{dup}"
        );
    }
}

#[test]
fn enforces_strict_https_in_production_and_rejects_malformed_urls() {
    let fx = PointerFixture::new(Platform::Linux, Architecture::X86_64, 1);
    for bad_url in [
        "http://github.com/vgmello/agentic-org/releases/download/v1/a.tar.gz",
        "http://127.0.0.1:8080/a.tar.gz",
        "file:///tmp/a.tar.gz",
        "ftp://example.com/a.tar.gz",
        "https://user:pass@example.com/a.tar.gz",
        "https://example.com/releases/../secret.tar.gz",
        "https://example.com/releases/%2e%2e/secret.tar.gz",
        "https://example.com/a.tar.gz#frag",
        "https://example.com/a.tar.gz\n--output=/tmp/pwn",
        "-https://example.com/a.tar.gz",
        "https://example.com/",
        "https:///missing-host.tar.gz",
    ] {
        assert_eq!(
            validate_pointer_url(bad_url, UrlPolicy::HttpsOnly),
            Err(Error::InvalidUrl),
            "HttpsOnly must reject `{bad_url}`"
        );
        let mut val = fx.valid_pointer_value(5);
        val["targets"][0]["url"] = serde_json::json!(bad_url);
        let (bytes, sig) = fx.sign_json(&val);
        assert_eq!(
            verify_pointer(&bytes, &sig, &fx.policy, &fx.keys, UrlPolicy::HttpsOnly).unwrap_err(),
            Error::InvalidUrl,
            "verify_pointer HttpsOnly must reject `{bad_url}`"
        );
    }

    // Explicit test harness policy permits loopback HTTP and file:///, while still refusing
    // non-loopback HTTP, userinfo, and traversal.
    assert!(validate_pointer_url(
        "http://127.0.0.1:8080/a.tar.gz",
        UrlPolicy::AllowLocalForTesting
    )
    .is_ok());
    assert!(validate_pointer_url(
        "http://[::1]:8080/a.tar.gz",
        UrlPolicy::AllowLocalForTesting
    )
    .is_ok());
    assert!(validate_pointer_url(
        "file:///tmp/releases/a.tar.gz",
        UrlPolicy::AllowLocalForTesting
    )
    .is_ok());
    assert_eq!(
        validate_pointer_url(
            "http://example.com/a.tar.gz",
            UrlPolicy::AllowLocalForTesting
        ),
        Err(Error::InvalidUrl)
    );
    assert_eq!(
        validate_pointer_url("file:///tmp/../etc/passwd", UrlPolicy::AllowLocalForTesting),
        Err(Error::InvalidUrl)
    );
}

#[test]
fn rejects_missing_platform_duplicate_target_and_oversized_bundle() {
    let fx = PointerFixture::new(Platform::Linux, Architecture::Aarch64, 1);
    let (bytes, sig) = fx.sign_json(&fx.valid_pointer_value(5));
    assert_eq!(
        verify_pointer(&bytes, &sig, &fx.policy, &fx.keys, UrlPolicy::HttpsOnly).unwrap_err(),
        Error::TargetMismatch
    );

    let fx_x86 = PointerFixture::new(Platform::Linux, Architecture::X86_64, 1);
    let mut dup_target = fx_x86.valid_pointer_value(5);
    let first = dup_target["targets"][0].clone();
    dup_target["targets"].as_array_mut().unwrap().push(first);
    let (bytes, sig) = fx_x86.sign_json(&dup_target);
    assert_eq!(
        verify_pointer(
            &bytes,
            &sig,
            &fx_x86.policy,
            &fx_x86.keys,
            UrlPolicy::HttpsOnly
        )
        .unwrap_err(),
        Error::DuplicateTarget
    );

    let mut oversized = fx_x86.valid_pointer_value(5);
    oversized["targets"][0]["size"] = serde_json::json!(MAX_PAYLOAD_BYTES + 1);
    let (bytes, sig) = fx_x86.sign_json(&oversized);
    assert_eq!(
        verify_pointer(
            &bytes,
            &sig,
            &fx_x86.policy,
            &fx_x86.keys,
            UrlPolicy::HttpsOnly
        )
        .unwrap_err(),
        Error::BundleSizeLimit
    );
}
