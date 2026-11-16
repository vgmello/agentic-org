use super::*;
use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{json, Value};

struct Fixture {
    key: Ed25519KeyPair,
    key_id: String,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn digest(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

impl Fixture {
    fn new() -> Self {
        let encoded = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        let key = Ed25519KeyPair::from_pkcs8(encoded.as_ref()).unwrap();
        let key_id = digest(key.public_key().as_ref());
        Self { key, key_id }
    }

    fn registry(&self, minimum: u64, maximum: Option<u64>) -> Vec<u8> {
        serde_json::to_vec(&json!({"version":1,"keys":[{
            "key_id":self.key_id,
            "public_key_hex":hex(self.key.public_key().as_ref()),
            "min_sequence":minimum,"max_sequence":maximum
        }]}))
        .unwrap()
    }

    fn keys(&self) -> TrustedKeys {
        TrustedKeys::from_registry_bytes(&self.registry(1, None)).unwrap()
    }

    fn manifest(&self) -> Value {
        let mut files: Vec<_> = EXECUTABLE_PATHS
            .iter()
            .map(|path| entry(path, 3, "executable", Value::Null))
            .collect();
        files.push(entry("ui/index.html", 3, "data", Value::Null));
        json!({"format":FORMAT,"release_id":"v1.0.0-test_1","sequence":10,
            "platform":"linux","architecture":"x86_64","installer_contract":1,
            "key_id":self.key_id,"files":files})
    }

    fn verify_bytes(&self, bytes: &[u8]) -> Result<VerifiedManifest, Error> {
        verify_manifest(
            bytes,
            self.key.sign(bytes).as_ref(),
            &policy(),
            &self.keys(),
        )
    }

    fn verify(&self, manifest: &Value) -> Result<VerifiedManifest, Error> {
        self.verify_bytes(&serde_json::to_vec(manifest).unwrap())
    }
}

fn policy() -> TrustedPolicy {
    TrustedPolicy::new(Platform::Linux, Architecture::X86_64, 1, 0).unwrap()
}

fn entry(path: &str, size: u64, role: &str, cdhash: Value) -> Value {
    json!({"path":path,"size":size,"sha256":digest(b"abc"),"role":role,
        "macos_cdhash":cdhash})
}

// Authored independently of the serializer above. Includes reordered keys,
// insignificant whitespace, an escaped path and known SHA256("abc") bytes.
fn literal(fixture: &Fixture) -> String {
    r#"{
 "files": [
  {"role":"executable","path":"agentic-org","size":3,"sha256":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad","macos_cdhash":null},
  {"role":"executable","path":"agentic-org-office","size":3,"sha256":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad","macos_cdhash":null},
  {"role":"executable","path":"agentic-org-web","size":3,"sha256":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad","macos_cdhash":null},
  {"role":"executable","path":"agent-os","size":3,"sha256":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad","macos_cdhash":null},
  {"role":"data","path":"ui\u002findex.html","size":3,"sha256":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad","macos_cdhash":null}
 ],
 "key_id":"@KEY@", "installer_contract":1, "architecture":"x86_64",
 "platform":"linux", "sequence":9007199254740993,
 "release_id":"release-1", "format":"agentic-org-release-v1"
}"#
    .replace("@KEY@", &fixture.key_id)
}

#[test]
fn hand_authored_exact_bytes_and_invalid_vectors() {
    let fixture = Fixture::new();
    let bytes = literal(&fixture);
    let verified = fixture.verify_bytes(bytes.as_bytes()).unwrap();
    assert_eq!(verified.manifest_bytes(), bytes.as_bytes());
    assert_eq!(
        verified.manifest_sha256(),
        &<[u8; 32]>::from(Sha256::digest(bytes.as_bytes()))
    );
    assert_eq!(verified.manifest().sequence(), 9_007_199_254_740_993);
    assert_eq!(verified.manifest().release_id(), "release-1");
    assert_eq!(verified.manifest().installer_contract(), 1);
    assert_eq!(verified.manifest().platform(), Platform::Linux);
    assert_eq!(verified.manifest().architecture(), Architecture::X86_64);
    assert_eq!(verified.manifest().key_id(), fixture.key_id);
    assert_eq!(verified.manifest().total_size(), 15);
    let file = verified.file("ui/index.html").unwrap();
    assert_eq!(file.path(), "ui/index.html");
    assert_eq!(file.size(), 3);
    assert_eq!(file.sha256(), &<[u8; 32]>::from(Sha256::digest(b"abc")));
    assert_eq!(file.role(), Role::Data);
    assert_eq!(file.macos_cdhash(), None);
    assert!(verified.file("UI/index.html").is_none());
    for bad in [
        bytes.replace("9007199254740993", "9007199254740993.0"),
        bytes.replace("9007199254740993", "18446744073709551616"),
        bytes.replacen("\"size\":3", "\"size\":3,\"s\\u0069ze\":3", 1),
        bytes.replacen(",\"macos_cdhash\":null", "", 1),
    ] {
        assert_eq!(
            fixture.verify_bytes(bad.as_bytes()).unwrap_err(),
            Error::InvalidManifest
        );
    }
}

#[test]
fn exact_signature_tamper_wrong_key_unknown_key_and_lengths() {
    let fixture = Fixture::new();
    let bytes = serde_json::to_vec(&fixture.manifest()).unwrap();
    let signature = fixture.key.sign(&bytes);
    verify_manifest(&bytes, signature.as_ref(), &policy(), &fixture.keys()).unwrap();
    let mut changed = bytes.clone();
    changed.push(b' ');
    assert_eq!(
        verify_manifest(&changed, signature.as_ref(), &policy(), &fixture.keys()).unwrap_err(),
        Error::InvalidSignature
    );
    let other = Fixture::new();
    assert_eq!(
        verify_manifest(
            &bytes,
            other.key.sign(&bytes).as_ref(),
            &policy(),
            &fixture.keys()
        )
        .unwrap_err(),
        Error::InvalidSignature
    );
    assert_eq!(
        verify_manifest(&bytes, signature.as_ref(), &policy(), &other.keys()).unwrap_err(),
        Error::UnknownSigningKey
    );
    let mut altered_signature = signature.as_ref().to_vec();
    altered_signature[7] ^= 0x80;
    assert_eq!(
        verify_manifest(&bytes, &altered_signature, &policy(), &fixture.keys()).unwrap_err(),
        Error::InvalidSignature
    );
    for length in [0, 1, 63, 65, 128] {
        assert_eq!(
            verify_manifest(&bytes, &vec![0; length], &policy(), &fixture.keys()).unwrap_err(),
            Error::InvalidSignatureLength
        );
    }
}

#[test]
fn strict_structural_fields_at_every_level() {
    let fixture = Fixture::new();
    let bytes = literal(&fixture);
    for (from, to) in [
        (
            "\"sequence\":9007199254740993",
            "\"sequence\":10,\"sequence\":11",
        ),
        (
            "\"sequence\":9007199254740993",
            "\"sequence\":10,\"sequ\\u0065nce\":11",
        ),
        (
            "\"sequence\":9007199254740993",
            "\"sequence\":10,\"unknown\":0",
        ),
        ("\"size\":3", "\"size\":3,\"size\":3"),
        ("\"size\":3", "\"size\":3,\"unknown\":{}"),
        ("\"size\":3,", ""),
        (
            "\"macos_cdhash\":null",
            "\"macos_cdhash\":null,\"macos_cdhash\":null",
        ),
        (
            "\"macos_cdhash\":null",
            "\"macos_cdhash\":null,\"macos_cd\\u0068ash\":null",
        ),
    ] {
        let bad = bytes.replacen(from, to, 1);
        assert_eq!(
            fixture.verify_bytes(bad.as_bytes()).unwrap_err(),
            Error::InvalidManifest,
            "{from} -> {to}"
        );
    }
    for field in [
        "format",
        "release_id",
        "sequence",
        "platform",
        "architecture",
        "installer_contract",
        "key_id",
        "files",
    ] {
        let mut manifest = fixture.manifest();
        manifest.as_object_mut().unwrap().remove(field);
        assert_eq!(
            fixture.verify(&manifest).unwrap_err(),
            Error::InvalidManifest,
            "missing {field}"
        );
        manifest[field] = Value::Null;
        assert_eq!(
            fixture.verify(&manifest).unwrap_err(),
            Error::InvalidManifest,
            "null {field}"
        );
    }
    for field in ["path", "size", "sha256", "role", "macos_cdhash"] {
        let mut manifest = fixture.manifest();
        manifest["files"][0].as_object_mut().unwrap().remove(field);
        assert_eq!(
            fixture.verify(&manifest).unwrap_err(),
            Error::InvalidManifest,
            "missing file {field}"
        );
        if field != "macos_cdhash" {
            manifest["files"][0][field] = Value::Null;
            assert_eq!(
                fixture.verify(&manifest).unwrap_err(),
                Error::InvalidManifest,
                "null file {field}"
            );
        }
    }
    for (field, value) in [
        ("platform", json!({"linux":null})),
        ("architecture", json!({"x86_64":null})),
    ] {
        let mut manifest = fixture.manifest();
        manifest[field] = value;
        assert_eq!(
            fixture.verify(&manifest).unwrap_err(),
            Error::InvalidManifest
        );
    }
    let mut manifest = fixture.manifest();
    manifest["files"][0]["role"] = json!({"executable":null});
    assert_eq!(
        fixture.verify(&manifest).unwrap_err(),
        Error::InvalidManifest
    );
    for bad in [
        b"null".as_slice(),
        b"[]",
        b"{} {}",
        b"\xef\xbb\xbf{}",
        b"{\xff}",
    ] {
        assert_eq!(
            fixture.verify_bytes(bad).unwrap_err(),
            Error::InvalidManifest
        );
    }
}

#[test]
fn integer_tokens_are_never_coerced_or_rounded() {
    let fixture = Fixture::new();
    let bytes = literal(&fixture);
    for token in [
        "null",
        "true",
        "\"1\"",
        "-0",
        "-1",
        "1.0",
        "1e0",
        "1E+0",
        "18446744073709551616",
        "1e300",
        "NaN",
    ] {
        for field in [
            "\"size\":3",
            "\"installer_contract\":1",
            "\"sequence\":9007199254740993",
        ] {
            let name = field.split(':').next().unwrap();
            let bad = bytes.replacen(field, &format!("{name}:{token}"), 1);
            assert_eq!(
                fixture.verify_bytes(bad.as_bytes()).unwrap_err(),
                Error::InvalidManifest,
                "{field}: {token}"
            );
        }
    }
    let largest = bytes.replace("9007199254740993", "18446744073709551615");
    assert_eq!(
        fixture
            .verify_bytes(largest.as_bytes())
            .unwrap()
            .manifest()
            .sequence(),
        u64::MAX
    );
}

#[test]
fn target_installer_floor_and_inclusive_key_intervals() {
    let fixture = Fixture::new();
    let mut manifest = fixture.manifest();
    for (field, bad, expected) in [
        ("platform", json!("macos"), Error::TargetMismatch),
        ("architecture", json!("aarch64"), Error::TargetMismatch),
        (
            "installer_contract",
            json!(2),
            Error::InstallerContractMismatch,
        ),
        ("sequence", json!(0), Error::InvalidSequence),
        (
            "format",
            json!("agentic-org-release-v2"),
            Error::InvalidManifest,
        ),
    ] {
        let old = manifest[field].clone();
        manifest[field] = bad;
        assert_eq!(fixture.verify(&manifest).unwrap_err(), expected);
        manifest[field] = old;
    }
    for (field, bad) in [("platform", "windows"), ("architecture", "arm64")] {
        let mut bad_manifest = manifest.clone();
        bad_manifest[field] = json!(bad);
        assert_eq!(
            fixture.verify(&bad_manifest).unwrap_err(),
            Error::InvalidManifest
        );
    }
    let bytes = serde_json::to_vec(&manifest).unwrap();
    let sig = fixture.key.sign(&bytes);
    let keys = TrustedKeys::from_registry_bytes(&fixture.registry(10, Some(10))).unwrap();
    let equal_floor = TrustedPolicy::new(Platform::Linux, Architecture::X86_64, 1, 10).unwrap();
    verify_manifest(&bytes, sig.as_ref(), &equal_floor, &keys).unwrap();
    let high_floor = TrustedPolicy::new(Platform::Linux, Architecture::X86_64, 1, 11).unwrap();
    assert_eq!(
        verify_manifest(&bytes, sig.as_ref(), &high_floor, &keys).unwrap_err(),
        Error::ReleaseBelowMinimum
    );
    for (minimum, maximum) in [(11, None), (1, Some(9))] {
        let keys = TrustedKeys::from_registry_bytes(&fixture.registry(minimum, maximum)).unwrap();
        assert_eq!(
            verify_manifest(&bytes, sig.as_ref(), &policy(), &keys).unwrap_err(),
            Error::KeySequenceOutOfRange
        );
    }
    assert_eq!(
        TrustedPolicy::new(Platform::Linux, Architecture::Aarch64, 0, 0).unwrap_err(),
        Error::InvalidPolicy
    );
    // Both allowed architectures also succeed when selected by the trusted policy.
    manifest["architecture"] = json!("aarch64");
    let bytes = serde_json::to_vec(&manifest).unwrap();
    verify_manifest(
        &bytes,
        fixture.key.sign(&bytes).as_ref(),
        &TrustedPolicy::new(Platform::Linux, Architecture::Aarch64, 1, 0).unwrap(),
        &fixture.keys(),
    )
    .unwrap();
}

#[test]
fn release_id_and_digest_validation() {
    let fixture = Fixture::new();
    for id in [
        "",
        ".",
        "..",
        ".a",
        "_a",
        "a/b",
        "é",
        "a b",
        "a\0",
        &"a".repeat(97),
    ] {
        let mut manifest = fixture.manifest();
        manifest["release_id"] = json!(id);
        assert_eq!(
            fixture.verify(&manifest).unwrap_err(),
            Error::InvalidReleaseId,
            "{id:?}"
        );
    }
    for id in ["a", "0", "a._-", &"Z".repeat(96)] {
        let mut manifest = fixture.manifest();
        manifest["release_id"] = json!(id);
        fixture.verify(&manifest).unwrap();
    }
    for bad in [
        "",
        &"A".repeat(64),
        &"g".repeat(64),
        &"0".repeat(63),
        &"0".repeat(65),
        &"é".repeat(32),
    ] {
        let mut manifest = fixture.manifest();
        manifest["files"][0]["sha256"] = json!(bad);
        assert_eq!(fixture.verify(&manifest).unwrap_err(), Error::InvalidDigest);
        manifest = fixture.manifest();
        manifest["key_id"] = json!(bad);
        assert_eq!(fixture.verify(&manifest).unwrap_err(), Error::InvalidKeyId);
    }
}

#[test]
fn registry_validation_and_exact_required_nullable_field() {
    let fixture = Fixture::new();
    assert_eq!(fixture.keys().len(), 1);
    assert!(!fixture.keys().is_empty());
    for raw in [
        b"".as_slice(),
        b"null",
        b"[]",
        b"{}",
        b"{\"version\":1,\"keys\":[]}",
        b"{\"version\":2,\"keys\":[]}",
    ] {
        assert_eq!(
            TrustedKeys::from_registry_bytes(raw).unwrap_err(),
            Error::InvalidRegistry
        );
    }
    let registry: Value = serde_json::from_slice(&fixture.registry(1, None)).unwrap();
    for field in ["key_id", "public_key_hex", "min_sequence", "max_sequence"] {
        let mut bad = registry.clone();
        bad["keys"][0].as_object_mut().unwrap().remove(field);
        assert_eq!(
            TrustedKeys::from_registry_bytes(&serde_json::to_vec(&bad).unwrap()).unwrap_err(),
            Error::InvalidRegistry,
            "missing {field}"
        );
        if field != "max_sequence" {
            bad["keys"][0][field] = Value::Null;
            assert_eq!(
                TrustedKeys::from_registry_bytes(&serde_json::to_vec(&bad).unwrap()).unwrap_err(),
                Error::InvalidRegistry,
                "null {field}"
            );
        }
    }
    for key in ["key_id", "public_key_hex"] {
        for bad in [
            "0".repeat(64),
            "A".repeat(64),
            "f".repeat(63),
            "g".repeat(64),
        ] {
            let mut raw = registry.clone();
            raw["keys"][0][key] = json!(bad);
            assert_eq!(
                TrustedKeys::from_registry_bytes(&serde_json::to_vec(&raw).unwrap()).unwrap_err(),
                Error::InvalidRegistry
            );
        }
    }
    for (minimum, maximum) in [(11, Some(10)), (u64::MAX, Some(0))] {
        assert_eq!(
            TrustedKeys::from_registry_bytes(&fixture.registry(minimum, maximum)).unwrap_err(),
            Error::InvalidRegistry
        );
    }
    TrustedKeys::from_registry_bytes(&fixture.registry(0, Some(0))).unwrap();
    TrustedKeys::from_registry_bytes(&fixture.registry(u64::MAX, Some(u64::MAX))).unwrap();
    let key = registry["keys"][0].clone();
    let duplicated = json!({"version":1,"keys":[key.clone(),key]});
    assert_eq!(
        TrustedKeys::from_registry_bytes(&serde_json::to_vec(&duplicated).unwrap()).unwrap_err(),
        Error::InvalidRegistry
    );
    let bytes = String::from_utf8(fixture.registry(1, None)).unwrap();
    for (from, to) in [
        ("\"version\":1", "\"version\":1,\"version\":1"),
        ("\"version\":1", "\"version\":1,\"ver\\u0073ion\":1"),
        ("\"version\":1", "\"version\":1,\"unknown\":0"),
        (
            "\"min_sequence\":1",
            "\"min_sequence\":1,\"min_sequence\":1",
        ),
        (
            "\"min_sequence\":1",
            "\"min_sequence\":1,\"min_sequen\\u0063e\":1",
        ),
        ("\"min_sequence\":1", "\"min_sequence\":1,\"unknown\":0"),
        (
            "\"max_sequence\":null",
            "\"max_sequence\":null,\"max_sequence\":null",
        ),
    ] {
        assert_eq!(
            TrustedKeys::from_registry_bytes(bytes.replacen(from, to, 1).as_bytes()).unwrap_err(),
            Error::InvalidRegistry,
            "{from} -> {to}"
        );
    }
    for token in [
        "-0",
        "-1",
        "1.0",
        "1e0",
        "18446744073709551616",
        "\"1\"",
        "true",
        "{}",
        "[]",
    ] {
        for field in [
            "\"min_sequence\":1",
            "\"max_sequence\":null",
            "\"version\":1",
        ] {
            let name = field.split(':').next().unwrap();
            let raw = bytes.replacen(field, &format!("{name}:{token}"), 1);
            assert_eq!(
                TrustedKeys::from_registry_bytes(raw.as_bytes()).unwrap_err(),
                Error::InvalidRegistry,
                "{field}:{token}"
            );
        }
    }
    let mut keys = Vec::new();
    for _ in 0..=MAX_TRUSTED_KEYS {
        keys.push(
            serde_json::from_slice::<Value>(&Fixture::new().registry(0, None)).unwrap()["keys"][0]
                .clone(),
        );
    }
    let raw = serde_json::to_vec(&json!({"version":1,"keys":&keys[..MAX_TRUSTED_KEYS]})).unwrap();
    assert_eq!(
        TrustedKeys::from_registry_bytes(&raw).unwrap().len(),
        MAX_TRUSTED_KEYS
    );
    let raw = serde_json::to_vec(&json!({"version":1,"keys":keys})).unwrap();
    assert_eq!(
        TrustedKeys::from_registry_bytes(&raw).unwrap_err(),
        Error::InvalidRegistry
    );
}

#[test]
fn inventory_requires_exact_executables_and_ui_index_with_correct_roles() {
    let fixture = Fixture::new();
    let base = fixture.manifest();
    for index in 0..5 {
        let mut bad = base.clone();
        bad["files"].as_array_mut().unwrap().remove(index);
        assert_eq!(fixture.verify(&bad).unwrap_err(), Error::InvalidInventory);
    }
    for path in [
        "extra",
        "agentic-org2",
        "release.v1.json",
        "release.v1.sig",
        "index.html",
        "UI/extra",
        "ui",
    ] {
        let mut bad = base.clone();
        bad["files"]
            .as_array_mut()
            .unwrap()
            .push(entry(path, 0, "data", Value::Null));
        assert_eq!(
            fixture.verify(&bad).unwrap_err(),
            Error::InvalidInventory,
            "{path}"
        );
    }
    for index in 0..5 {
        let mut bad = base.clone();
        bad["files"][index]["role"] = json!(if index == 4 { "executable" } else { "data" });
        assert_eq!(fixture.verify(&bad).unwrap_err(), Error::InvalidRole);
        bad["files"][index]["role"] = json!("symlink");
        assert_eq!(fixture.verify(&bad).unwrap_err(), Error::InvalidManifest);
    }
    let mut no_index = base.clone();
    no_index["files"][4]["path"] = json!("ui/a.js");
    assert_eq!(
        fixture.verify(&no_index).unwrap_err(),
        Error::InvalidInventory
    );
    let mut extra = base;
    extra["files"]
        .as_array_mut()
        .unwrap()
        .push(entry("ui/nested/main.js", 0, "data", Value::Null));
    let verified = fixture.verify(&extra).unwrap();
    let paths: Vec<_> = verified
        .manifest()
        .files()
        .iter()
        .map(FileEntry::path)
        .collect();
    verified.verify_inventory(paths.iter().copied()).unwrap();
    assert_eq!(
        verified
            .verify_inventory(paths[..5].iter().copied())
            .unwrap_err(),
        Error::MissingPayload
    );
    assert_eq!(
        verified
            .verify_inventory(paths.iter().copied().chain(["ui/extra"]))
            .unwrap_err(),
        Error::UnexpectedPayload
    );
    assert_eq!(
        verified
            .verify_inventory(paths.iter().copied().chain([paths[0]]))
            .unwrap_err(),
        Error::DuplicatePayload
    );
    assert_eq!(
        verified
            .verify_inventory(paths.iter().copied().chain(["UI/index.html"]))
            .unwrap_err(),
        Error::UnexpectedPayload
    );
}

#[test]
fn signed_cdhash_is_required_exactly_for_macos_executables() {
    let fixture = Fixture::new();
    let mut manifest = fixture.manifest();
    manifest["platform"] = json!("macos");
    for index in 0..4 {
        manifest["files"][index]["macos_cdhash"] =
            json!("0123456789abcdef0123456789abcdef01234567");
    }
    let mac_policy = TrustedPolicy::new(Platform::Macos, Architecture::X86_64, 1, 0).unwrap();
    let verify = |value: &Value| {
        let bytes = serde_json::to_vec(value).unwrap();
        verify_manifest(
            &bytes,
            fixture.key.sign(&bytes).as_ref(),
            &mac_policy,
            &fixture.keys(),
        )
    };
    let verified = verify(&manifest).unwrap();
    assert_eq!(
        verified.file("agentic-org").unwrap().macos_cdhash(),
        Some(&decode_hex::<20>("0123456789abcdef0123456789abcdef01234567").unwrap())
    );
    assert!(verified
        .file("ui/index.html")
        .unwrap()
        .macos_cdhash()
        .is_none());
    for bad in [
        Value::Null,
        json!(""),
        json!("f".repeat(39)),
        json!("a".repeat(41)),
        json!("A".repeat(40)),
        json!("g".repeat(40)),
    ] {
        let mut value = manifest.clone();
        value["files"][0]["macos_cdhash"] = bad;
        assert_eq!(verify(&value).unwrap_err(), Error::InvalidCdHash);
    }
    manifest["files"][4]["macos_cdhash"] = json!("0".repeat(40));
    assert_eq!(verify(&manifest).unwrap_err(), Error::InvalidCdHash);
    let mut linux = fixture.manifest();
    linux["files"][0]["macos_cdhash"] = json!("0".repeat(40));
    assert_eq!(fixture.verify(&linux).unwrap_err(), Error::InvalidCdHash);
}

#[test]
fn paths_reject_aliases_traversal_controls_and_utf8_byte_overrun() {
    let fixture = Fixture::new();
    for path in [
        "",
        "/ui/x",
        "ui/x/",
        "ui//x",
        "ui/.",
        "ui/../x",
        "ui/./x",
        "ui/a/../x",
        "ui/\\x",
        "ui/\0x",
        "ui/\nx",
        "ui/\u{7f}x",
        "ui/\u{85}x",
        "C:/ui/x",
        "c:foo",
        "//server/ui/x",
    ] {
        let mut bad = fixture.manifest();
        bad["files"][4]["path"] = json!(path);
        assert_eq!(
            fixture.verify(&bad).unwrap_err(),
            Error::InvalidPath,
            "{path:?}"
        );
    }
    let valid = format!("ui/{}", "a".repeat(MAX_PATH_BYTES - 3));
    let multibyte = format!("ui/{}a", "é".repeat((MAX_PATH_BYTES - 4) / 2));
    assert_eq!(multibyte.len(), MAX_PATH_BYTES);
    for path in [&valid, &multibyte] {
        let mut value = fixture.manifest();
        value["files"]
            .as_array_mut()
            .unwrap()
            .push(entry(path, 0, "data", Value::Null));
        fixture.verify(&value).unwrap();
        value["files"][5]["path"] = json!(format!("{path}x"));
        assert_eq!(fixture.verify(&value).unwrap_err(), Error::InvalidPath);
    }
    for path in ["ui/日本語/☃.js", "ui/e\u{301}.js", "ui/ß/hello world.js"] {
        let mut value = fixture.manifest();
        value["files"]
            .as_array_mut()
            .unwrap()
            .push(entry(path, 0, "data", Value::Null));
        let verified = fixture.verify(&value).unwrap();
        assert_eq!(
            verified.file(path).unwrap().path().as_bytes(),
            path.as_bytes()
        );
    }
}

#[test]
fn case_unicode_normalization_duplicate_and_prefix_collisions() {
    let fixture = Fixture::new();
    for (left, right) in [
        ("ui/index.html", "ui/index.html"),
        ("ui/A.js", "ui/a.js"),
        ("ui/é.js", "ui/e\u{301}.js"),
        ("ui/Straße.js", "ui/STRASSE.js"),
        ("ui/Σ.js", "ui/ς.js"),
        ("ui/K.js", "ui/k.js"),
        ("ui/a", "ui/a/b"),
        ("ui/A", "ui/a/b"),
        ("ui/é", "ui/e\u{301}/b"),
    ] {
        for reverse in [false, true] {
            let mut value = fixture.manifest();
            let additions = if reverse {
                [right, left]
            } else {
                [left, right]
            };
            for path in additions {
                value["files"]
                    .as_array_mut()
                    .unwrap()
                    .push(entry(path, 0, "data", Value::Null));
            }
            assert_eq!(
                fixture.verify(&value).unwrap_err(),
                Error::PathCollision,
                "{left:?} {right:?}"
            );
        }
    }
    // A naive adjacent-pair check misses the prefix when a- sits between a and a/b.
    let mut value = fixture.manifest();
    for path in ["ui/a", "ui/a-", "ui/a/b"] {
        value["files"]
            .as_array_mut()
            .unwrap()
            .push(entry(path, 0, "data", Value::Null));
    }
    assert_eq!(fixture.verify(&value).unwrap_err(), Error::PathCollision);
}

#[test]
fn manifest_registry_byte_limits_are_inclusive() {
    let fixture = Fixture::new();
    let mut bytes = serde_json::to_vec(&fixture.manifest()).unwrap();
    bytes.resize(MAX_MANIFEST_BYTES, b' ');
    fixture.verify_bytes(&bytes).unwrap();
    bytes.push(b' ');
    assert_eq!(
        fixture.verify_bytes(&bytes).unwrap_err(),
        Error::ManifestTooLarge
    );
    let mut registry = fixture.registry(1, None);
    registry.resize(MAX_REGISTRY_BYTES, b' ');
    TrustedKeys::from_registry_bytes(&registry).unwrap();
    registry.push(b' ');
    assert_eq!(
        TrustedKeys::from_registry_bytes(&registry).unwrap_err(),
        Error::RegistryTooLarge
    );
}

#[test]
fn file_count_limit_inclusive_and_aggregate_checked_sizes() {
    let fixture = Fixture::new();
    let mut value = fixture.manifest();
    for n in 5..MAX_FILES {
        value["files"].as_array_mut().unwrap().push(entry(
            &format!("ui/f{n}"),
            0,
            "data",
            Value::Null,
        ));
    }
    // These independent limits overlap: this valid-inventory count fixture
    // exceeds the stricter 2MiB signed-byte cap. Exercise the bounded decoder
    // directly at its count boundary and the public byte cap independently.
    let bytes = serde_json::to_vec(&value).unwrap();
    assert!(bytes.len() > MAX_MANIFEST_BYTES);
    assert_eq!(
        fixture.verify_bytes(&bytes).unwrap_err(),
        Error::ManifestTooLarge
    );
    assert_eq!(
        serde_json::from_slice::<decode::Manifest>(&bytes)
            .unwrap()
            .files
            .len(),
        MAX_FILES
    );
    value["files"]
        .as_array_mut()
        .unwrap()
        .push(entry("ui/extra", 0, "data", Value::Null));
    assert!(
        serde_json::from_slice::<decode::Manifest>(&serde_json::to_vec(&value).unwrap()).is_err()
    );
    let mut value = fixture.manifest();
    for n in 0..4 {
        value["files"][n]["size"] = json!(MAX_EXECUTABLE_BYTES);
    }
    value["files"][4]["size"] = json!(0);
    assert_eq!(
        fixture.verify(&value).unwrap().manifest().total_size(),
        MAX_PAYLOAD_BYTES
    );
    value["files"][4]["size"] = json!(1);
    assert_eq!(fixture.verify(&value).unwrap_err(), Error::PayloadSizeLimit);
    for (index, limit) in [(0, MAX_EXECUTABLE_BYTES), (4, MAX_UI_ASSET_BYTES)] {
        let mut value = fixture.manifest();
        value["files"][index]["size"] = json!(limit);
        fixture.verify(&value).unwrap();
        value["files"][index]["size"] = json!(limit + 1);
        assert_eq!(fixture.verify(&value).unwrap_err(), Error::FileSizeLimit);
        value["files"][index]["size"] = json!(u64::MAX);
        assert_eq!(fixture.verify(&value).unwrap_err(), Error::FileSizeLimit);
    }
}

#[test]
fn payload_chunked_exact_overrun_underrun_wrong_hash_and_empty() {
    let fixture = Fixture::new();
    let verified = fixture.verify(&fixture.manifest()).unwrap();
    let entry = verified.file("ui/index.html").unwrap();
    let mut stream = entry.payload_verifier();
    stream.update(b"").unwrap();
    stream.update(b"a").unwrap();
    stream.update(b"bc").unwrap();
    assert_eq!(stream.bytes_received(), 3);
    stream.finish().unwrap();
    let mut stream = entry.payload_verifier();
    stream.update(b"ab").unwrap();
    assert_eq!(stream.finish().unwrap_err(), Error::PayloadUnderrun);
    let mut stream = entry.payload_verifier();
    assert_eq!(stream.update(b"abcd").unwrap_err(), Error::PayloadOverrun);
    assert_eq!(stream.bytes_received(), 0);
    assert_eq!(stream.update(b"abc").unwrap_err(), Error::PayloadOverrun);
    assert_eq!(stream.finish().unwrap_err(), Error::PayloadOverrun);
    let mut stream = entry.payload_verifier();
    stream.update(b"abc").unwrap();
    assert_eq!(stream.update(b"d").unwrap_err(), Error::PayloadOverrun);
    assert_eq!(stream.finish().unwrap_err(), Error::PayloadOverrun);
    let mut stream = entry.payload_verifier();
    stream.update(b"abd").unwrap();
    assert_eq!(stream.finish().unwrap_err(), Error::PayloadDigestMismatch);
    let mut value = fixture.manifest();
    value["files"][4]["size"] = json!(0);
    value["files"][4]["sha256"] = json!(digest(b""));
    fixture
        .verify(&value)
        .unwrap()
        .file("ui/index.html")
        .unwrap()
        .payload_verifier()
        .finish()
        .unwrap();
}

#[test]
fn ui_aggregate_byte_limit_is_inclusive_and_excludes_executables() {
    assert_eq!(MAX_UI_BYTES, 67_108_864);
    assert_eq!(MAX_UI_ENTRIES, 4096);
    let fixture = Fixture::new();
    let mut value = fixture.manifest();
    // Signed sizes exercise the boundary without allocating payload bytes.
    value["files"][4]["size"] = json!(MAX_UI_BYTES / 2);
    value["files"].as_array_mut().unwrap().push(entry(
        "ui/_app/a.js",
        MAX_UI_BYTES / 2,
        "data",
        Value::Null,
    ));
    let verified = fixture.verify(&value).unwrap();
    assert_eq!(verified.manifest().total_size(), MAX_UI_BYTES + 12);
    assert!(verified.manifest().total_size() < MAX_PAYLOAD_BYTES);
    value["files"][5]["size"] = json!(MAX_UI_BYTES / 2 + 1);
    assert_eq!(fixture.verify(&value).unwrap_err(), Error::PayloadSizeLimit);
}

fn flat_ui_manifest(fixture: &Fixture, ui_files: usize) -> Value {
    let mut value = fixture.manifest();
    for n in 1..ui_files {
        value["files"].as_array_mut().unwrap().push(entry(
            &format!("ui/f{n}"),
            0,
            "data",
            Value::Null,
        ));
    }
    value
}

#[test]
fn ui_flat_file_limit_excludes_root_and_rejects_one_over() {
    let fixture = Fixture::new();
    let mut value = flat_ui_manifest(&fixture, MAX_UI_ENTRIES);
    let bytes = serde_json::to_vec(&value).unwrap();
    assert!(bytes.len() <= MAX_MANIFEST_BYTES);
    assert!(value["files"].as_array().unwrap().len() < MAX_FILES);
    assert_eq!(
        fixture
            .verify_bytes(&bytes)
            .unwrap()
            .manifest()
            .files()
            .len(),
        EXECUTABLE_PATHS.len() + MAX_UI_ENTRIES
    );
    value["files"]
        .as_array_mut()
        .unwrap()
        .push(entry("ui/one-over", 0, "data", Value::Null));
    assert_eq!(fixture.verify(&value).unwrap_err(), Error::InvalidInventory);
}

#[test]
fn ui_4096_regular_files_with_an_inferred_directory_exceed_loader_limit() {
    let fixture = Fixture::new();
    let mut value = flat_ui_manifest(&fixture, MAX_UI_ENTRIES);
    // Exactly 4096 UI files remain below the existing manifest and release-file
    // caps. One inferred directory raises their loader-entry count to 4097.
    value["files"][4]["path"] = json!("ui/_app/index.html");
    value["files"][5]["path"] = json!("ui/index.html");
    let bytes = serde_json::to_vec(&value).unwrap();
    assert!(bytes.len() <= MAX_MANIFEST_BYTES);
    assert!(value["files"].as_array().unwrap().len() < MAX_FILES);
    assert_eq!(
        fixture.verify_bytes(&bytes).unwrap_err(),
        Error::InvalidInventory
    );
}

#[test]
fn ui_shared_directory_example_counts_three_loader_entries() {
    let fixture = Fixture::new();
    // ui/index.html plus ui/_app/a.js count as three entries (two files and
    // _app). Fill the remaining slots with root-level files, then add a file
    // under the shared _app prefix to cross the boundary by exactly one.
    let mut value = flat_ui_manifest(&fixture, MAX_UI_ENTRIES - 2);
    value["files"]
        .as_array_mut()
        .unwrap()
        .push(entry("ui/_app/a.js", 0, "data", Value::Null));
    fixture.verify(&value).unwrap();
    value["files"]
        .as_array_mut()
        .unwrap()
        .push(entry("ui/_app/b.js", 0, "data", Value::Null));
    assert_eq!(fixture.verify(&value).unwrap_err(), Error::InvalidInventory);
}

#[test]
fn ui_nested_shared_multidepth_directories_count_once_at_exact_limit() {
    let fixture = Fixture::new();
    // Five inferred directories: _app, _app/immutable,
    // _app/immutable/chunks, _app/immutable/assets, fonts. Together with the
    // four added files and index.html, these consume ten loader entries.
    let mut value = flat_ui_manifest(&fixture, MAX_UI_ENTRIES - 9);
    for path in [
        "ui/_app/immutable/chunks/a.js",
        "ui/_app/immutable/chunks/b.js",
        "ui/_app/immutable/assets/c.css",
        "ui/fonts/inter.woff2",
    ] {
        value["files"]
            .as_array_mut()
            .unwrap()
            .push(entry(path, 0, "data", Value::Null));
    }
    let bytes = serde_json::to_vec(&value).unwrap();
    assert!(bytes.len() <= MAX_MANIFEST_BYTES);
    fixture.verify_bytes(&bytes).unwrap();
    // The file count is unchanged; one additional distinct directory alone
    // crosses the loader-entry cap. It is not enough to count regular files.
    let mut directory_over = value.clone();
    directory_over["files"][5]["path"] = json!("ui/new-directory/f1");
    assert_eq!(
        fixture.verify(&directory_over).unwrap_err(),
        Error::InvalidInventory
    );
    // Count every depth, not only immediate parents or direct children of ui.
    let last = value["files"].as_array().unwrap().len() - 1;
    directory_over = value.clone();
    directory_over["files"][last]["path"] = json!("ui/fonts/sub/inter.woff2");
    assert_eq!(
        fixture.verify(&directory_over).unwrap_err(),
        Error::InvalidInventory
    );
    // An extra file in an already counted directory is also exactly one over.
    value["files"].as_array_mut().unwrap().push(entry(
        "ui/_app/immutable/chunks/c.js",
        0,
        "data",
        Value::Null,
    ));
    assert_eq!(fixture.verify(&value).unwrap_err(), Error::InvalidInventory);
}
