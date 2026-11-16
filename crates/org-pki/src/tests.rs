use super::*;

#[test]
fn generated_code_has_the_exact_160_bit_display_profile_and_normalized_bytes() {
    let code = Code::generate().unwrap();
    let formatted = code.formatted();
    assert_eq!(formatted.len(), 49);
    assert_eq!(formatted.split('-').count(), 10);
    assert!(formatted.split('-').all(|p| p.len() == 4
        && p.bytes()
            .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b))));
    let parsed = Code::parse(&formatted).unwrap();
    assert_eq!(&code.0[..], &parsed.0[..]);
    let normalized = Code::parse(&format!(
        " \t{}\r\n",
        formatted.to_ascii_lowercase().replace('-', " \t-")
    ))
    .unwrap();
    assert_eq!(&code.0[..], &normalized.0[..]);
    assert_eq!(format!("{code:?}"), "Code(<redacted>)");
    for input in [
        "0".repeat(39),
        "0".repeat(41),
        "x".repeat(40),
        format!("{}é", "0".repeat(40)),
        format!("{}\u{a0}", "0".repeat(40)),
        format!("{}{}", "0".repeat(40), " ".repeat(89)),
    ] {
        assert!(matches!(Code::parse(&input), Err(Error::InvalidCode)));
    }
}

#[test]
fn package_roundtrip_preserves_exact_signing_identity_and_original_local_der() {
    let key = SigningKey::generate().unwrap();
    let code = Code::generate().unwrap();
    let package = key.seal("org/é", &code).unwrap();
    assert_eq!(&package[..8], b"AGORGCA1");
    assert_eq!(&package[8..10], &(6u16).to_be_bytes());
    assert_eq!(&package[10..16], "org/é".as_bytes());
    assert_eq!(package[16], 1);
    let envelope = Envelope::parse(&package, "org/é").unwrap();
    assert_eq!(envelope.organization(), "org/é");
    assert_eq!(envelope.public_key(), &key.public_key());
    let reopened = recover(
        &package,
        "org/é",
        &Code::parse(&code.formatted().to_ascii_lowercase()).unwrap(),
    )
    .unwrap();
    assert_eq!(reopened.local_der(), key.local_der());
    let message = b"disposable escrow signature continuity";
    ring::signature::UnparsedPublicKey::new(
        &ring::signature::ECDSA_P256_SHA256_ASN1,
        key.public_key(),
    )
    .verify(message, &reopened.sign_message_der(message).unwrap())
    .unwrap();
    assert_eq!(format!("{reopened:?}"), "SigningKey(<redacted>)");
}

fn structural(organization: &str, ciphertext: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"AGORGCA1");
    bytes.extend_from_slice(&(organization.len() as u16).to_be_bytes());
    bytes.extend_from_slice(organization.as_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&[0; 16 + 12]);
    bytes.push(4);
    bytes.extend_from_slice(&[0; 64]);
    bytes.extend_from_slice(&(ciphertext as u32).to_be_bytes());
    bytes.resize(bytes.len() + ciphertext, 0);
    bytes
}
#[test]
fn all_framing_identity_profile_and_allocation_bounds_are_checked_before_kdf() {
    let organization = "x".repeat(255);
    let package = structural(&organization, 4112);
    assert_eq!(package.len(), MAX_PACKAGE_BYTES);
    assert!(Envelope::parse(&package, &organization).is_ok());
    let before = envelope::KDFS.with(|n| n.get());
    let code = Code::generate().unwrap();
    let mut malformed = Vec::new();
    malformed.push([package.as_slice(), &[0]].concat());
    let package = structural("org", 17);
    for length in 0..package.len() {
        malformed.push(package[..length].to_vec());
    }
    for (offset, value) in [(0, b'B'), (9, 0), (13, 2), (14 + 16 + 12, 3)] {
        let mut bytes = package.clone();
        bytes[offset] = value;
        malformed.push(bytes);
    }
    let mut bytes = package.clone();
    bytes[10] = 0xff;
    malformed.push(bytes);
    for count in [0u32, 16, 4113, u32::MAX] {
        let mut bytes = package.clone();
        let header = 111;
        bytes[header - 4..header].copy_from_slice(&count.to_be_bytes());
        malformed.push(bytes);
    }
    for bytes in malformed {
        assert!(matches!(
            recover(&bytes, "org", &code),
            Err(Error::InvalidPackage)
        ));
    }
    assert!(matches!(
        recover(&package, "other", &code),
        Err(Error::InvalidPackage)
    ));
    assert_eq!(envelope::KDFS.with(|n| n.get()), before);
}
#[test]
fn wrong_code_changed_aad_and_ciphertext_have_one_sanitized_recovery_failure() {
    let key = SigningKey::generate().unwrap();
    let code = Code::generate().unwrap();
    let package = key.seal("org", &code).unwrap();
    assert!(matches!(
        recover(&package, "org", &Code::generate().unwrap()),
        Err(Error::RecoveryFailed)
    ));
    for offset in [10, 14, 30, 43, package.len() - 1] {
        let mut changed = package.clone();
        changed[offset] ^= 1;
        let org = if offset == 10 { "nrg" } else { "org" };
        assert!(matches!(
            recover(&changed, org, &code),
            Err(Error::RecoveryFailed)
        ));
    }
    assert!(matches!(
        SigningKey::from_local_der(b"not PKCS8"),
        Err(Error::RecoveryFailed)
    ));
}

// Public test-only P-256 scalar 1 and its standard generator point. Never use
// this deliberately known signing key or code for any organization.
fn vector_der() -> Vec<u8> {
    hex_bytes(concat!(
        "308187020100301306072a8648ce3d020106082a8648ce3d030107046d306b0201010420",
        "0000000000000000000000000000000000000000000000000000000000000001",
        "a14403420004",
        "6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296",
        "4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5"
    ))
}
fn hex_bytes(input: &str) -> Vec<u8> {
    input
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| {
            let digit = |b: u8| match b {
                b'0'..=b'9' => b - b'0',
                b'a'..=b'f' => b - b'a' + 10,
                _ => panic!("invalid public test fixture"),
            };
            digit(p[0]) * 16 + digit(p[1])
        })
        .collect()
}
#[test]
fn rec_esc_1_exact_public_vector() {
    let key = SigningKey::from_local_der(&vector_der()).unwrap();
    let code = Code::parse("000102030405060708090a0b0c0d0e0f10111213").unwrap();
    let package = envelope::seal_with(
        &key,
        "org/é",
        &code,
        std::array::from_fn(|i| i as u8),
        std::array::from_fn(|i| i as u8 + 16),
    )
    .unwrap();
    assert_eq!(package,hex_bytes("41474f524743413100066f72672fc3a901000102030405060708090a0b0c0d0e0f101112131415161718191a1b046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f50000009a5d6a20fe56cf999a703d644b99d76424ebf724a4edd541ccfdaa2c01fbf2330d8ebb3f2c2b7e8885b9fd6c7dc0a93f0136474a31d85ea60f009db18c948b8a8a51652e6c6c26f935ecca80ce9679211be680ddd060ebc232c2925f7b2a3d09e3b2d5a5f4731c2a165d61a4db271c96006887a0b3b3465b39fa3f5e9bad16c1d062959c4880877fd3bba8a0d5a1cd91c801d0d0a834a9263d0e0f"));
    assert_eq!(
        recover(&package, "org/é", &code).unwrap().public_key(),
        key.public_key()
    );
}
