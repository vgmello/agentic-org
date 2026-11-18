//! Tests of the contract's own code that run on every target. Those that need a real
//! adapter are in `adapter_tests.rs`.

use std::io;
use std::path::Path;

use super::{
    credentials, protected_store, refusal, CredentialError, CredentialLocation, DirectoryStore,
    ItemName, KeychainItems, Secret, Sharing, WriteError, MAX_ITEM_BYTES, MAX_NAME_BYTES,
};

#[test]
fn an_item_name_is_what_a_file_name_and_a_keychain_account_both_hold() {
    for valid in [
        "a".to_owned(),
        "registry".to_owned(),
        "writer-device".to_owned(),
        "recovery.Abc123".to_owned(),
        "01AAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
        "0197c9f0-7c1e-7b7a-9f5e-3b1d2c4a5e6f".to_owned(),
        format!("tls.{}", "0123456789abcdef".repeat(4)),
        "a_b-c.d".to_owned(),
        "x".repeat(MAX_NAME_BYTES),
    ] {
        assert_eq!(ItemName::new(&valid).unwrap().as_str(), valid);
    }
    for invalid in [
        "".to_owned(),
        ".".to_owned(),
        "..".to_owned(),
        ".lock".to_owned(),
        ".put.0123".to_owned(),
        "a/b".to_owned(),
        "../x".to_owned(),
        "a b".to_owned(),
        "a\0".to_owned(),
        "a\n".to_owned(),
        "caf\u{e9}".to_owned(),
        "a:b".to_owned(),
        "x".repeat(MAX_NAME_BYTES + 1),
    ] {
        assert_eq!(
            ItemName::new(&invalid),
            Err(CredentialError::InvalidName),
            "{invalid:?}"
        );
    }
    let mut names = [
        ItemName::new("b").unwrap(),
        ItemName::new("B").unwrap(),
        ItemName::new("a").unwrap(),
    ];
    names.sort();
    assert_eq!(names.map(|name| name.as_str().to_owned()), ["B", "a", "b"]);
}

#[test]
fn a_secret_is_redacted_and_hands_over_its_own_buffer() {
    let secret = Secret::from(b"do-not-log-this".to_vec());
    assert_eq!(format!("{secret:?}"), "<redacted>");
    assert_eq!(format!("{secret}"), "<redacted>");
    assert_eq!(secret.expose(), b"do-not-log-this");
    let buffer = secret.expose().as_ptr();
    let bytes = secret.into_bytes();
    assert_eq!(bytes, b"do-not-log-this");
    assert_eq!(bytes.as_ptr(), buffer, "the bytes were copied");
    let mut bytes = [0xa5u8; 32];
    super::zero(&mut bytes);
    assert_eq!(bytes, [0; 32]);
}

#[test]
fn errors_show_kinds_and_never_a_path_or_a_message_from_the_system() {
    let detailed = io::Error::new(
        io::ErrorKind::PermissionDenied,
        "/Users/me/keys/01AAAA.key: SECRET",
    );
    let error = CredentialError::from(detailed);
    assert_eq!(error, CredentialError::Io(io::ErrorKind::PermissionDenied));
    let shown = [
        CredentialError::Refused(refusal()),
        CredentialError::InvalidName,
        CredentialError::TooLarge,
        CredentialError::Exists,
        CredentialError::Corrupt,
        CredentialError::Malformed,
        CredentialError::Unsafe,
        CredentialError::InUse,
        CredentialError::CaseAlias,
        CredentialError::Os(-25308),
        error,
    ]
    .map(|error| (error.to_string(), format!("{error:?}")));
    assert!(shown[9].0.contains("-25308"), "{}", shown[9].0);
    for (display, debug) in shown {
        for text in [display, debug] {
            assert!(!text.is_empty());
            assert!(!text.contains('/') && !text.contains("SECRET"), "{text}");
            assert!(!text.bytes().any(|b| b == b'\n'), "{text}");
        }
    }
    let failure = WriteError::from(CredentialError::Exists);
    assert!(!failure.temporary_left);
    assert_eq!(failure.to_string(), CredentialError::Exists.to_string());
    assert_eq!(MAX_ITEM_BYTES, 16 * 1024);
}

/// A refusing build is one that refuses a directory store. There every location is
/// refused before anything is made, and there is no protected store.
#[test]
fn on_a_refusing_build_every_location_refuses_first() {
    let fixture = tempfile::tempdir().unwrap();
    let anchor = fixture.path().canonicalize().unwrap();
    std::fs::set_permissions(&anchor, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let directory = CredentialLocation::Directory(DirectoryStore {
        // Not a private directory: a real adapter refuses it for that, and makes nothing.
        anchor: &anchor,
        below: &["store"],
        create: true,
        sharing: Sharing::Exclusive,
        suffix: "",
    });
    let refused = CredentialError::Refused(refusal());
    // No real adapter accepts a directory that is open to others.
    let error = credentials(directory).err();
    let error = error.expect("a directory that is open to others was accepted");
    assert_eq!(std::fs::read_dir(&anchor).unwrap().count(), 0);
    if error != refused {
        return;
    }
    assert_eq!(protected_store(), Err(refusal()));
    for location in [
        CredentialLocation::Keychain(KeychainItems {
            access_group: "TEAMID1234.org.agentic.host",
            service: "org.agentic.office-key",
        }),
        CredentialLocation::Directory(DirectoryStore {
            anchor: Path::new("relative"),
            below: &[],
            create: false,
            sharing: Sharing::Shared,
            suffix: ".key",
        }),
    ] {
        assert_eq!(credentials(location).unwrap_err(), refused);
    }
}

#[test]
fn a_location_prints_its_kind_and_no_path() {
    let location = CredentialLocation::Directory(DirectoryStore {
        anchor: std::path::Path::new("/secret/place"),
        below: &["host-secrets", "an-instance"],
        create: false,
        sharing: Sharing::Exclusive,
        suffix: ".key",
    });
    let printed = format!("{location:?}");
    assert!(printed.starts_with("Directory(DirectoryStore"), "{printed}");
    for hidden in ["/secret/place", "host-secrets", "an-instance"] {
        assert!(!printed.contains(hidden), "{printed}");
    }
}
