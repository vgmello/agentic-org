use super::*;
use std::os::unix::fs::{symlink, PermissionsExt};

#[test]
fn absent_build_trust_refuses_before_opening_input() {
    if RELEASE_REGISTRY.is_empty() {
        assert!(matches!(
            DirectoryBundle::open(Path::new("/does-not-exist")),
            Err(Error::MissingBuildTrust)
        ));
    }
}

#[test]
fn held_source_refuses_links_writable_files_and_traversal() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let file = root.join("payload");
    std::fs::write(&file, b"payload").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    let directory = files::Directory::open(&root).unwrap();
    assert!(directory.payload("payload").is_ok());
    symlink("payload", root.join("link")).unwrap();
    assert!(directory.payload("link").is_err());
    assert!(directory.payload("../payload").is_err());
    assert!(directory.payload("/payload").is_err());
    std::fs::hard_link(&file, root.join("hardlink")).unwrap();
    assert!(directory.payload("payload").is_err());
    std::fs::remove_file(root.join("hardlink")).unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o622)).unwrap();
    assert!(directory.payload("payload").is_err());
    symlink(&root, root.join("directory-link")).unwrap();
    assert!(files::Directory::open(&root.join("directory-link")).is_err());
}

struct SignedInput {
    _temp: tempfile::TempDir,
    root: std::path::PathBuf,
    keys: TrustedKeys,
    policy: TrustedPolicy,
    bytes: Vec<u8>,
}
impl SignedInput {
    fn new() -> Self {
        use ring::signature::{Ed25519KeyPair, KeyPair};
        use sha2::{Digest, Sha256};
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let encoded = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
        let key = Ed25519KeyPair::from_pkcs8(encoded.as_ref()).unwrap();
        let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let key_id = hex(&Sha256::digest(key.public_key().as_ref()));
        let registry = serde_json::to_vec(&serde_json::json!({"version":1,"keys":[{"key_id":key_id,"public_key_hex":hex(key.public_key().as_ref()),"min_sequence":1,"max_sequence":100}]})).unwrap();
        let keys = TrustedKeys::from_registry_bytes(&registry).unwrap();
        let mut files = Vec::new();
        std::fs::create_dir(root.join("ui")).unwrap();
        for path in org_release::EXECUTABLE_PATHS
            .into_iter()
            .chain(["ui/index.html"])
        {
            std::fs::write(root.join(path), b"abc").unwrap();
            files.push(serde_json::json!({"path":path,"size":3,"sha256":hex(&Sha256::digest(b"abc")),"role":if path.starts_with("ui/") {"data"} else {"executable"},"macos_cdhash":null}));
        }
        // An input-verifier fixture only: these are not executable/platform-
        // signed images and must never be treated as an activated release.
        let bytes = serde_json::to_vec_pretty(&serde_json::json!({"format":org_release::FORMAT,"release_id":"test-1","sequence":1,"platform":"linux","architecture":"x86_64","installer_contract":1,"key_id":key_id,"files":files})).unwrap();
        std::fs::write(root.join("release.v1.json"), &bytes).unwrap();
        std::fs::write(root.join("release.v1.sig"), key.sign(&bytes).as_ref()).unwrap();
        let policy = TrustedPolicy::new(Platform::Linux, Architecture::X86_64, 1, 1).unwrap();
        Self {
            _temp: temp,
            root,
            keys,
            policy,
            bytes,
        }
    }
    fn open(&self) -> Result<DirectoryBundle, Error> {
        DirectoryBundle::with_policy(&self.root, &self.policy, &self.keys)
    }
}
#[test]
fn signed_directory_is_exact_closed_and_repeatably_verified() {
    let input = SignedInput::new();
    let bundle = input.open().unwrap();
    assert_eq!(bundle.manifest().manifest_bytes(), input.bytes);
    bundle.root.verify_inventory(bundle.manifest()).unwrap();
    bundle.root.verify_inventory(bundle.manifest()).unwrap();
    let mut bytes = Vec::new();
    bundle.copy_payload("ui/index.html", &mut bytes).unwrap();
    assert_eq!(bytes, b"abc");
    assert!(bundle
        .copy_payload("release.v1.sig", &mut Vec::new())
        .is_err());
    std::fs::write(input.root.join("ui/extra.js"), b"extra").unwrap();
    assert!(input.open().is_err());
    std::fs::remove_file(input.root.join("ui/extra.js")).unwrap();
    std::fs::write(input.root.join("ui/index.html"), b"def").unwrap();
    assert!(bundle
        .copy_payload("ui/index.html", &mut Vec::new())
        .is_err());
    assert!(input.open().is_err());
}
#[test]
fn signed_directory_rejects_forgery_truncation_and_missing_payload() {
    let input = SignedInput::new();
    let mut changed = input.bytes.clone();
    changed.push(b' ');
    std::fs::write(input.root.join("release.v1.json"), changed).unwrap();
    assert!(input.open().is_err());
    std::fs::write(input.root.join("release.v1.json"), &input.bytes).unwrap();
    let signature = std::fs::read(input.root.join("release.v1.sig")).unwrap();
    std::fs::write(input.root.join("release.v1.sig"), &signature[..63]).unwrap();
    assert!(input.open().is_err());
    std::fs::write(input.root.join("release.v1.sig"), signature).unwrap();
    std::fs::remove_file(input.root.join("agent-os")).unwrap();
    assert!(input.open().is_err());
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    std::io::Write::write_all(&mut encoder, bytes).unwrap();
    encoder.finish().unwrap()
}
fn tar_input(input: &SignedInput, extra: Option<(&str, tar::EntryType)>) -> Vec<u8> {
    let mut archive = tar::Builder::new(Vec::new());
    for path in org_release::EXECUTABLE_PATHS.into_iter().chain([
        "ui/index.html",
        "release.v1.json",
        "release.v1.sig",
    ]) {
        let data = std::fs::read(input.root.join(path)).unwrap();
        let mut header = tar::Header::new_ustar();
        header.set_size(data.len() as u64);
        header.set_mode(0o600);
        header.set_cksum();
        archive
            .append_data(&mut header, path, data.as_slice())
            .unwrap();
    }
    if let Some((path, kind)) = extra {
        let mut header = tar::Header::new_ustar();
        header.set_size(3);
        header.set_mode(0o600);
        header.set_entry_type(kind);
        if kind.is_symlink() {
            header.set_link_name("agentic-org").unwrap();
        }
        header.set_cksum();
        archive.append_data(&mut header, path, &b"abc"[..]).unwrap();
    }
    archive.into_inner().unwrap()
}
fn open_compressed(input: &SignedInput, bytes: &[u8]) -> Result<ArchiveBundle, Error> {
    let source = input.root.join("input.gz");
    std::fs::write(&source, bytes).unwrap();
    let prepared = input.root.join("prepared");
    if !prepared.exists() {
        std::fs::create_dir(&prepared).unwrap();
        std::fs::set_permissions(&prepared, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let folder = files::Directory::open(&prepared).unwrap();
    let frozen = folder.freeze_archive(&source, org_release::MAX_PAYLOAD_BYTES)?;
    super::archive::ArchiveBundle::with_policy(frozen, &input.policy, &input.keys)
}
#[test]
fn signed_archive_verifies_complete_container_and_copies_only_exact_payloads() {
    let input = SignedInput::new();
    let tar = tar_input(&input, None);
    let mut archive = open_compressed(&input, &gzip(&tar)).unwrap();
    assert_eq!(archive.manifest().manifest_bytes(), input.bytes);
    assert_eq!(
        archive.signature().as_slice(),
        std::fs::read(input.root.join("release.v1.sig")).unwrap()
    );
    let mut copied = std::collections::BTreeMap::new();
    archive
        .copy_payloads(|entry, reader| {
            let mut data = Vec::new();
            reader.read_to_end(&mut data)?;
            copied.insert(entry.path().to_owned(), data);
            Ok(())
        })
        .unwrap();
    assert_eq!(copied.len(), 5);
    assert!(copied.values().all(|b| b == b"abc"));
    // Same held private input can be reverified, but an incomplete sink never
    // claims that all authenticated bytes were copied.
    assert!(archive.copy_payloads(|_, _| Ok(())).is_err());
    archive
        .copy_payloads(|_, reader| {
            io::copy(reader, &mut io::sink())?;
            Ok(())
        })
        .unwrap();
}
#[test]
fn archive_rejects_duplicate_extra_link_extension_and_changed_payload() {
    let input = SignedInput::new();
    for extra in [
        ("agentic-org", tar::EntryType::Regular),
        ("extra", tar::EntryType::Regular),
        ("ui/link", tar::EntryType::Symlink),
        ("extensions", tar::EntryType::new(b'x')),
    ] {
        assert!(open_compressed(&input, &gzip(&tar_input(&input, Some(extra)))).is_err());
    }
    let mut changed = tar_input(&input, None);
    changed[512] ^= 1;
    assert!(open_compressed(&input, &gzip(&changed)).is_err());
    let mut padding = tar_input(&input, None);
    padding[515] = b'x';
    assert!(open_compressed(&input, &gzip(&padding)).is_err());
}
#[test]
fn archive_rejects_prefix_acceptance_bad_crc_truncation_and_trailing_members() {
    let input = SignedInput::new();
    let tar = tar_input(&input, None);
    let compressed = gzip(&tar);
    let mut extra = compressed.clone();
    extra.extend_from_slice(&gzip(b"second member"));
    assert!(open_compressed(&input, &extra).is_err());
    let mut trailing = compressed.clone();
    trailing.push(0);
    assert!(open_compressed(&input, &trailing).is_err());
    let mut crc = compressed.clone();
    let at = crc.len() - 8;
    crc[at] ^= 1;
    assert!(open_compressed(&input, &crc).is_err());
    assert!(open_compressed(&input, &compressed[..compressed.len() - 3]).is_err());
    assert!(open_compressed(&input, &gzip(&tar[..tar.len() - 512])).is_err());
    let mut hidden = tar.clone();
    hidden.extend_from_slice(&tar);
    assert!(open_compressed(&input, &gzip(&hidden)).is_err());
    assert!(open_compressed(&input, &gzip(&tar[..tar.len() - 1024])).is_err());
}
#[test]
fn compressed_input_limit_rejects_before_private_copy_without_truncating_source() {
    let input = SignedInput::new();
    let source = input.root.join("oversize.gz");
    let file = std::fs::File::create(&source).unwrap();
    file.set_len(org_release::MAX_PAYLOAD_BYTES + 1).unwrap();
    let prepared = input.root.join("prepared");
    std::fs::create_dir(&prepared).unwrap();
    std::fs::set_permissions(&prepared, std::fs::Permissions::from_mode(0o700)).unwrap();
    let folder = files::Directory::open(&prepared).unwrap();
    assert!(folder
        .freeze_archive(&source, org_release::MAX_PAYLOAD_BYTES)
        .is_err());
    assert_eq!(std::fs::read_dir(&prepared).unwrap().count(), 0);
    assert_eq!(
        file.metadata().unwrap().len(),
        org_release::MAX_PAYLOAD_BYTES + 1
    );
}

/// A signed release input for `platform` and `architecture` whose executables all hold
/// `executable`, with `cdhash` as each executable's CodeDirectory hash where the platform
/// has one. Synthetic bytes: no test treats them as a release that may run.
fn signed_release(
    platform: &str,
    architecture: &str,
    executable: &[u8],
    cdhash: Option<&str>,
) -> (tempfile::TempDir, DirectoryBundle) {
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use sha2::{Digest, Sha256};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let encoded = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
    let key = Ed25519KeyPair::from_pkcs8(encoded.as_ref()).unwrap();
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let key_id = hex(&Sha256::digest(key.public_key().as_ref()));
    let registry = serde_json::to_vec(&serde_json::json!({"version":1,"keys":[{"key_id":key_id,"public_key_hex":hex(key.public_key().as_ref()),"min_sequence":1,"max_sequence":100}]})).unwrap();
    let keys = TrustedKeys::from_registry_bytes(&registry).unwrap();
    let mut files = Vec::new();
    std::fs::create_dir(root.join("ui")).unwrap();
    for path in org_release::EXECUTABLE_PATHS {
        std::fs::write(root.join(path), executable).unwrap();
        files.push(serde_json::json!({"path":path,"size":executable.len(),"sha256":hex(&Sha256::digest(executable)),"role":"executable","macos_cdhash":cdhash}));
    }
    std::fs::write(root.join("ui/index.html"), b"abc").unwrap();
    files.push(serde_json::json!({"path":"ui/index.html","size":3,"sha256":hex(&Sha256::digest(b"abc")),"role":"data","macos_cdhash":null}));
    let bytes = serde_json::to_vec_pretty(&serde_json::json!({"format":org_release::FORMAT,"release_id":"test-1","sequence":1,"platform":platform,"architecture":architecture,"installer_contract":1,"key_id":key_id,"files":files})).unwrap();
    std::fs::write(root.join("release.v1.json"), &bytes).unwrap();
    std::fs::write(root.join("release.v1.sig"), key.sign(&bytes).as_ref()).unwrap();
    let platform = match platform {
        "macos" => Platform::Macos,
        _ => Platform::Linux,
    };
    let architecture = match architecture {
        "aarch64" => Architecture::Aarch64,
        _ => Architecture::X86_64,
    };
    let policy = TrustedPolicy::new(platform, architecture, 1, 1).unwrap();
    let bundle = DirectoryBundle::with_policy(&root, &policy, &keys).unwrap();
    (temp, bundle)
}

/// An ELF header for `machine` (62: x86-64, 183: aarch64) followed by some bytes of program.
fn elf(machine: u16) -> Vec<u8> {
    let mut image = vec![0u8; 96];
    image[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    image[16..18].copy_from_slice(&3u16.to_le_bytes());
    image[18..20].copy_from_slice(&machine.to_le_bytes());
    image
}

/// The static rule of a release for Linux: an executable is accepted on its ELF header for
/// the release's architecture, and nothing else is asked of the file, because its bytes are
/// the ones whose SHA-256 the signed manifest names (the input was verified against it).
#[test]
fn a_linux_release_executable_is_accepted_on_its_header_and_its_signed_digest() {
    for (architecture, machine) in [("x86_64", 62u16), ("aarch64", 183)] {
        let (_temp, bundle) = signed_release("linux", architecture, &elf(machine), None);
        platform::verify(&bundle.root, bundle.manifest())
            .unwrap_or_else(|error| panic!("{architecture}: {error}"));
    }
    // Another machine's executable, a file that is not ELF, and one too short to be a
    // program are each refused, whatever the manifest says of their digest.
    let (_temp, bundle) = signed_release("linux", "x86_64", &elf(183), None);
    assert!(matches!(
        platform::verify(&bundle.root, bundle.manifest()),
        Err(Error::InvalidRelease)
    ));
    let (_temp, bundle) = signed_release("linux", "x86_64", &[b'#'; 96], None);
    assert!(matches!(
        platform::verify(&bundle.root, bundle.manifest()),
        Err(Error::InvalidRelease)
    ));
    let (_temp, bundle) = signed_release("linux", "x86_64", b"abc", None);
    assert!(matches!(
        platform::verify(&bundle.root, bundle.manifest()),
        Err(Error::Io(_))
    ));
}

/// A release for macOS is signed code: after the header check each executable's signature
/// must be established, which an unsigned synthetic file never passes, on any system (a
/// system without a static check is outside that release's profile).
#[test]
fn a_macos_release_executable_still_needs_its_code_signature() {
    let mut macho = vec![0u8; 96];
    macho[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
    macho[4..8].copy_from_slice(&0x0100_000c_u32.to_le_bytes());
    macho[12..16].copy_from_slice(&2u32.to_le_bytes());
    let cdhash = "ab".repeat(20);
    let (_temp, bundle) = signed_release("macos", "aarch64", &macho, Some(&cdhash));
    assert!(matches!(
        platform::verify(&bundle.root, bundle.manifest()),
        Err(Error::UnsupportedTarget | Error::PlatformAdmission | Error::UnsafeInput)
    ));
}

/// The pin of an executable is its manifest entry: the SHA-256 of its bytes always, and the
/// CodeDirectory hash exactly in a release for macOS. A data file and an unknown name have
/// no pin.
#[test]
fn a_release_pins_each_executable_with_the_identities_its_manifest_names() {
    use sha2::{Digest, Sha256};
    let image = elf(62);
    let digest: [u8; 32] = Sha256::digest(&image).into();
    let (_temp, linux) = signed_release("linux", "x86_64", &image, None);
    for name in org_release::EXECUTABLE_PATHS {
        assert_eq!(
            runtime::pin_of(linux.manifest(), name).unwrap(),
            crate::server_check::Pin {
                sha256: digest,
                macos_cdhash: None,
            },
            "{name}"
        );
    }
    for name in ["ui/index.html", "release.v1.json", "agentic-org-other"] {
        assert!(
            matches!(
                runtime::pin_of(linux.manifest(), name),
                Err(Error::PlatformAdmission)
            ),
            "{name}"
        );
    }
    let cdhash = "ab".repeat(20);
    let (_temp, macos) = signed_release("macos", "aarch64", &image, Some(&cdhash));
    assert_eq!(
        runtime::pin_of(macos.manifest(), "agentic-org-office").unwrap(),
        crate::server_check::Pin {
            sha256: digest,
            macos_cdhash: Some([0xab; 20]),
        }
    );
}
