//! Unit tests for `org-host-tls`: certificate generation, contract check ordering, SAN coverage,
//! PEM normalization, and bounded regular-file PEM reading.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_host_control::listen::ListenOrigin;
use org_host_control::ops::TlsRefusal;
use rcgen::{CertificateParams, KeyPair};
use rustls::pki_types::pem::PemObject;
use x509_parser::prelude::{FromDer, X509Certificate};

use crate::testing::pair_for;
use crate::{check, make, now, read_pem_file, utc, Fingerprint, CHAIN_LIMIT, FILE_LIMIT};

const DAY: i64 = 86_400;

fn origin(text: &str) -> ListenOrigin {
    ListenOrigin::parse(text).unwrap()
}

fn wall() -> i64 {
    now().unwrap()
}

fn pem_begin(label: &str) -> String {
    format!("-----BEGIN {label}-----\n")
}

fn pem_block(label: &str, body: &str) -> String {
    format!("-----BEGIN {label}-----\n{body}\n-----END {label}-----\n")
}

fn pair(names: &[&str]) -> (String, String) {
    let now = wall();
    pair_for(names, now - DAY, now + 90 * DAY)
}

#[test]
fn the_fingerprint_is_uppercase_colon_hex() {
    let mut bytes = [0u8; 32];
    bytes[0] = 0xab;
    bytes[31] = 0x0f;
    let shown = Fingerprint(bytes).to_string();
    assert_eq!(shown.len(), 32 * 3 - 1);
    assert!(shown.starts_with("AB:00:"), "{shown}");
    assert!(shown.ends_with(":0F"), "{shown}");
}

#[test]
fn the_instance_makes_a_p256_certificate_naming_only_the_origins_host() {
    let now = wall();
    for (text, dns, ip) in [
        ("https://org.example:8443", Some("org.example"), None),
        ("https://127.0.0.1:8443", None, Some(vec![127, 0, 0, 1])),
        (
            "https://[::1]:8443",
            None,
            Some(std::net::Ipv6Addr::LOCALHOST.octets().to_vec()),
        ),
    ] {
        let made = make(&origin(text), now).unwrap();
        assert!(made.key().starts_with(&pem_begin("PRIVATE KEY")));
        assert_eq!(made.not_before(), now - 300);
        assert_eq!(made.not_after() - made.not_before(), 397 * DAY);
        assert!(!made.expiring(now));
        assert!(made.expiring(made.not_after() - 29 * DAY));
        let der =
            org_host_control::pem::first_block(made.chain(), org_host_control::pem::CERTIFICATE)
                .unwrap();
        let (_, cert) = X509Certificate::from_der(der.bytes()).unwrap();
        let names = &cert
            .subject_alternative_name()
            .unwrap()
            .unwrap()
            .value
            .general_names;
        assert_eq!(names.len(), 1, "{text}");
        match &names[0] {
            x509_parser::extensions::GeneralName::DNSName(name) => assert_eq!(Some(*name), dns),
            x509_parser::extensions::GeneralName::IPAddress(bytes) => {
                assert_eq!(Some(bytes.to_vec()), ip)
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            cert.public_key()
                .algorithm
                .parameters
                .as_ref()
                .unwrap()
                .as_oid()
                .unwrap(),
            x509_parser::oid_registry::OID_EC_P256
        );
        assert!(!cert.basic_constraints().unwrap().unwrap().value.ca);
        assert!(
            cert.extended_key_usage()
                .unwrap()
                .unwrap()
                .value
                .server_auth
        );
        assert_eq!(cert.tbs_certificate.raw_serial().len(), 16);
        let again = make(&origin(text), now).unwrap();
        assert_ne!(again.pair(), made.pair());
        assert_ne!(again.fingerprint(), made.fingerprint());
    }
}

#[test]
fn the_checks_run_in_the_contracts_order() {
    let now = wall();
    let here = origin("https://org.example:8443");
    let (cert, key) = pair(&["org.example"]);
    let (_, other_key) = pair(&["org.example"]);
    let (expired, expired_key) = pair_for(&["elsewhere.example"], now - 90 * DAY, now - DAY);
    assert!(check(&cert, &key, &[&here], Some(now)).is_ok());
    let bad_cert = pem_block("CERTIFICATE", "AAAA");
    let bad_enc_key = pem_block("ENCRYPTED PRIVATE KEY", "AAAA");
    let bad_key = pem_block("PRIVATE KEY", "AAAA");
    for (cert, key) in [
        ("", key.as_str()),
        ("not pem", key.as_str()),
        (cert.as_str(), ""),
        (bad_cert.as_str(), key.as_str()),
        (cert.as_str(), bad_enc_key.as_str()),
        (cert.as_str(), bad_key.as_str()),
    ] {
        assert_eq!(
            check(cert, key, &[&here], Some(now)).err(),
            Some(TlsRefusal::TlsInvalid)
        );
    }
    assert_eq!(
        check(&expired, &other_key, &[&here], Some(now)).err(),
        Some(TlsRefusal::TlsKeyMismatch)
    );
    match check(&expired, &expired_key, &[&here], Some(now)) {
        Err(TlsRefusal::TlsNotValid {
            not_before,
            not_after,
        }) => {
            assert_eq!(
                serde_json::to_value(&not_before).unwrap(),
                utc(now - 90 * DAY).unwrap()
            );
            assert_eq!(
                serde_json::to_value(&not_after).unwrap(),
                utc(now - DAY).unwrap()
            );
        }
        other => panic!("{:?}", other.map(drop)),
    }
    let (future, future_key) = pair_for(&["org.example"], now + DAY, now + 9 * DAY);
    assert!(matches!(
        check(&future, &future_key, &[&here], Some(now)),
        Err(TlsRefusal::TlsNotValid { .. })
    ));
    match check(&expired, &expired_key, &[&here], None) {
        Err(TlsRefusal::TlsNameMismatch { uncovered }) => {
            assert_eq!(uncovered.origins(), std::slice::from_ref(&here))
        }
        other => panic!("{:?}", other.map(drop)),
    }
}

#[test]
fn coverage_is_a_subject_alternative_name_of_the_origins_host() {
    let now = wall();
    let covered = |names: &[&str], text: &str| {
        let (cert, key) = pair(names);
        match check(&cert, &key, &[&origin(text)], Some(now)) {
            Ok(_) => true,
            Err(TlsRefusal::TlsNameMismatch { .. }) => false,
            Err(other) => panic!("{other:?}"),
        }
    };
    assert!(covered(&["org.example"], "https://org.example:8443"));
    assert!(covered(&["ORG.example"], "https://org.example"));
    assert!(covered(&["*.example.com"], "https://org.example.com"));
    assert!(!covered(&["*.example.com"], "https://a.org.example.com"));
    assert!(!covered(&["*.example.com"], "https://example.com"));
    assert!(!covered(&["*.com"], "https://example.com"));
    assert!(!covered(&["other.example"], "https://org.example"));
    assert!(covered(&["127.0.0.1"], "https://127.0.0.1:8443"));
    assert!(!covered(&["127.0.0.2"], "https://127.0.0.1:8443"));
    assert!(!covered(&["localhost"], "https://127.0.0.1:8443"));
    assert!(covered(&["::1"], "https://[::1]:8443"));
    assert!(!covered(&["127.0.0.1"], "https://[::1]:8443"));
    let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    let mut params = CertificateParams::default();
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "org.example");
    params.not_before = time::OffsetDateTime::from_unix_timestamp(now - DAY).unwrap();
    params.not_after = time::OffsetDateTime::from_unix_timestamp(now + DAY).unwrap();
    let cert = params.self_signed(&key).unwrap();
    assert!(matches!(
        check(
            &cert.pem(),
            &key.serialize_pem(),
            &[&origin("https://org.example")],
            Some(now)
        ),
        Err(TlsRefusal::TlsNameMismatch { .. })
    ));
    let (cert, key) = pair(&["org.example"]);
    let a = origin("https://a.example");
    let b = origin("https://b.example");
    match check(
        &cert,
        &key,
        &[&a, &origin("http://127.0.0.1:8080"), &b, &a],
        Some(now),
    ) {
        Err(TlsRefusal::TlsNameMismatch { uncovered }) => {
            assert_eq!(uncovered.origins(), [a.clone(), b.clone()])
        }
        other => panic!("{:?}", other.map(drop)),
    }
}

fn sec1_of(pkcs8: &[u8]) -> Vec<u8> {
    let (_, info) = x509_parser::der_parser::der::parse_der(pkcs8).unwrap();
    let fields = info.as_sequence().unwrap();
    fields[2].as_slice().unwrap().to_vec()
}

#[test]
fn a_pair_is_kept_in_its_normal_form() {
    let now = wall();
    let here = origin("https://org.example");
    let (cert, key) = pair(&["org.example"]);
    let combined = format!("subject=org.example\n{cert}{key}");
    let checked = check(&combined, &key, &[&here], Some(now)).unwrap();
    assert!(!checked.chain().contains("PRIVATE KEY"));
    assert!(checked.chain().starts_with("-----BEGIN CERTIFICATE-----\n"));
    assert_eq!(
        org_host_control::pem::all_blocks(checked.chain(), org_host_control::pem::CERTIFICATE)
            .unwrap()
            .len(),
        1
    );
    let crlf = check(
        &cert.replace('\n', "\r\n"),
        &key.replace('\n', "\r\n"),
        &[&here],
        Some(now),
    )
    .unwrap();
    assert_eq!(crlf.pair(), checked.pair());
    assert_eq!(crlf.chain(), checked.chain());
    assert_eq!(crlf.key(), checked.key());
    let rustls::pki_types::PrivateKeyDer::Pkcs8(pkcs8) =
        rustls::pki_types::PrivateKeyDer::from_pem_slice(key.as_bytes()).unwrap()
    else {
        panic!("rcgen writes PKCS #8")
    };
    let sec1 = STANDARD.encode(sec1_of(pkcs8.secret_pkcs8_der()));
    let sec1_pem = pem_block("EC PRIVATE KEY", &sec1);
    let from_sec1 = check(&cert, &sec1_pem, &[&here], Some(now)).unwrap();
    assert!(from_sec1.key().starts_with(&pem_begin("EC PRIVATE KEY")));
    assert_eq!(from_sec1.fingerprint(), checked.fingerprint());
    assert_ne!(from_sec1.pair(), checked.pair());
}

#[test]
fn a_supplied_file_is_a_bounded_regular_file_opened_without_following_a_link() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("cert.pem");
    std::fs::write(&file, "text").unwrap();
    assert_eq!(&**read_pem_file(&file).unwrap(), "text");
    let link = dir.path().join("link.pem");
    std::os::unix::fs::symlink(&file, &link).unwrap();
    let refused = read_pem_file(&link).unwrap_err();
    assert!(refused.contains("symbolic link"), "{refused}");
    assert!(
        refused.starts_with(&link.display().to_string()),
        "{refused}"
    );
    assert!(read_pem_file(dir.path())
        .unwrap_err()
        .contains("not a regular file"));
    let fifo = dir.path().join("fifo");
    let path = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
    org_platform::posix::make_fifo(&path, 0o600).unwrap();
    assert!(read_pem_file(&fifo)
        .unwrap_err()
        .contains("not a regular file"));
    let large = dir.path().join("large.pem");
    std::fs::write(&large, vec![b'a'; FILE_LIMIT + 1]).unwrap();
    assert!(read_pem_file(&large).unwrap_err().contains("64 KiB"));
    std::fs::write(&large, vec![b'a'; FILE_LIMIT]).unwrap();
    assert_eq!(read_pem_file(&large).unwrap().len(), FILE_LIMIT);
    let binary = dir.path().join("key.der");
    std::fs::write(&binary, [0x30, 0x82, 0xff, 0xfe]).unwrap();
    let refused = read_pem_file(&binary).unwrap_err();
    assert!(refused.contains("not PEM text"), "{refused}");
    assert!(read_pem_file(&dir.path().join("missing.pem")).is_err());
}

#[test]
fn a_chain_the_registry_cannot_keep_is_refused() {
    let now = wall();
    let here = origin("https://org.example");
    let (cert, key) = pair(&["org.example"]);
    let per = check(&cert, &key, &[&here], Some(now))
        .unwrap()
        .chain()
        .len();
    let fits = CHAIN_LIMIT / per;
    assert!(check(&cert.repeat(fits), &key, &[&here], Some(now)).is_ok());
    assert_eq!(
        check(&cert.repeat(fits + 1), &key, &[&here], Some(now)).err(),
        Some(TlsRefusal::TlsInvalid)
    );
    let (_, other_key) = pair(&["org.example"]);
    assert_eq!(
        check(&cert.repeat(fits + 1), &other_key, &[&here], Some(now)).err(),
        Some(TlsRefusal::TlsInvalid)
    );
}
