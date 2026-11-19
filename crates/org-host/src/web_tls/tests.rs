//! The certificate the instance makes, the verifier's order and the stored pair's lifecycle,
//! against a real temp registry and an in-memory secrets store.
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Arc, Mutex};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_host_control::listen::ListenOrigin;
use org_host_control::ops::{TlsRefusal, TlsSetParams};
use org_host_control::{HostOpResult, SessionHandle, TlsPrivateKey};
use rcgen::{CertificateParams, KeyPair};
use rustls::pki_types::pem::PemObject;
use x509_parser::prelude::{FromDer, X509Certificate};

use super::*;
use crate::host_secrets::{Error as SecretError, HostSecrets, Name, Value};
use crate::hostdb::{HostDb, HostDbInit};

const INSTANCE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071b0";
const DAY: i64 = 86_400;

fn origin(text: &str) -> ListenOrigin {
    ListenOrigin::parse(text).unwrap()
}

fn wall() -> i64 {
    now().unwrap()
}

/// A pair made by rcgen, outside the host: `names` as its subject alternative names (an IP
/// literal is an IP address), valid from `from` to `to` (Unix seconds).
pub(crate) fn pair_for(names: &[&str], from: i64, to: i64) -> (String, String) {
    let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    let mut params =
        CertificateParams::new(names.iter().map(|n| n.to_string()).collect::<Vec<_>>()).unwrap();
    params.not_before = time::OffsetDateTime::from_unix_timestamp(from).unwrap();
    params.not_after = time::OffsetDateTime::from_unix_timestamp(to).unwrap();
    let cert = params.self_signed(&key).unwrap();
    (cert.pem(), key.serialize_pem())
}

fn pair(names: &[&str]) -> (String, String) {
    let now = wall();
    pair_for(names, now - DAY, now + 90 * DAY)
}

/// The host's secrets, in memory, with the store's add-only rule for `tls.` names.
#[derive(Default)]
struct Secrets(
    Mutex<BTreeMap<String, Vec<u8>>>,
    /// Every load fails while set: a store that cannot be read.
    FailLoads,
);
#[derive(Default)]
struct FailLoads(std::sync::atomic::AtomicBool);
impl std::ops::Deref for FailLoads {
    type Target = std::sync::atomic::AtomicBool;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl HostSecrets for Secrets {
    fn put(&self, name: &Name, value: &Value) -> Result<(), SecretError> {
        let mut items = self.0.lock().unwrap();
        if name.as_str().starts_with("tls.") && items.contains_key(name.as_str()) {
            return Ok(());
        }
        items.insert(name.as_str().into(), value.expose().to_vec());
        Ok(())
    }
    fn load(&self, name: &Name) -> Result<Option<Value>, SecretError> {
        if self.1.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(SecretError::Io(std::io::ErrorKind::Other));
        }
        let items = self.0.lock().unwrap();
        Ok(items
            .get(name.as_str())
            .map(|v| Value::new(v.clone()).unwrap()))
    }
    fn delete(&self, name: &Name) -> Result<(), SecretError> {
        self.0.lock().unwrap().remove(name.as_str());
        Ok(())
    }
    fn list(&self) -> Result<Vec<Name>, SecretError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .keys()
            .map(|n| Name::parse(n).unwrap())
            .collect())
    }
}
impl Secrets {
    fn names(&self) -> Vec<String> {
        self.0.lock().unwrap().keys().cloned().collect()
    }
    fn holds(&self, text: &str) -> bool {
        self.0
            .lock()
            .unwrap()
            .values()
            .any(|v| v.windows(text.len()).any(|w| w == text.as_bytes()))
    }
}

pub(crate) struct Fixture {
    _home: tempfile::TempDir,
    db: Arc<HostDb>,
    secrets: Arc<Secrets>,
}

fn fixture() -> Fixture {
    let home = tempfile::tempdir().unwrap();
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let secrets = Arc::new(Secrets::default());
    let init = HostDbInit {
        instance_id: INSTANCE.into(),
        clock_high_ms: 1,
    };
    let db = Arc::new(HostDb::create(home.path(), &*secrets, &init).unwrap());
    Fixture {
        _home: home,
        db,
        secrets,
    }
}

impl Fixture {
    fn start(
        &self,
        origin: &ListenOrigin,
        supplied: Option<(&Path, &Path)>,
        now: i64,
    ) -> Result<Arc<WebTls>, StartRefusal> {
        WebTls::start(
            Arc::clone(&self.db),
            Arc::clone(&self.secrets) as Arc<dyn HostSecrets>,
            origin.clone(),
            supplied,
            now,
        )
    }
    fn stored(&self) -> crate::hostdb::web_tls::StoredPair {
        self.db.web_tls_pair().unwrap().unwrap()
    }
    fn leaf(&self) -> Fingerprint {
        use sha2::Digest;
        let chain = self.stored().cert_pem;
        let der =
            org_host_control::pem::first_block(&chain, org_host_control::pem::CERTIFICATE).unwrap();
        Fingerprint(sha2::Sha256::digest(der.bytes()).into())
    }
}

/// A started `https` pair for `origin` over a temp registry, for tests of other modules; the
/// fixture must outlive it.
pub(crate) fn started(origin: &str) -> (Fixture, Arc<WebTls>) {
    let f = fixture();
    let tls = f.start(&self::origin(origin), None, wall()).unwrap();
    (f, tls)
}

/// Stores, behind the host's back, a pair made for `other` in `f`'s registry and secrets.
pub(crate) fn store_pair_of(f: &Fixture, _tls: &WebTls, other: &str) {
    let made = make(&origin(other), wall()).unwrap();
    let name = Name::parse(&format!("tls.{}", hex(&made.pair()))).unwrap();
    f.secrets
        .put(&name, &Value::new(made.key().as_bytes().to_vec()).unwrap())
        .unwrap();
    f.db.set_web_tls_pair(&crate::hostdb::web_tls::StoredPair {
        source: Source::Supplied,
        cert_pem: made.chain().into(),
        pair_fp: made.pair(),
        set_at_ms: 1,
    })
    .unwrap();
}

/// Empties `f`'s secrets: every key is gone.
pub(crate) fn lose_keys(f: &Fixture) {
    f.secrets.0.lock().unwrap().clear();
}

fn captured<T>(f: impl FnOnce() -> T) -> (T, Vec<String>) {
    crate::host_log::capture(f)
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

/// The certificate the instance makes: a P-256 key, the origin's host as its only subject
/// alternative name, 397 days, not a CA, for a server; it verifies as any pair does.
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
        assert!(made.key().starts_with("-----BEGIN PRIVATE KEY-----\n"));
        assert_eq!(made.not_before_for_test(), now - 300);
        assert_eq!(made.not_after() - made.not_before_for_test(), 397 * DAY);
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
        // Two certificates are two keys.
        let again = make(&origin(text), now).unwrap();
        assert_ne!(again.pair(), made.pair());
        assert_ne!(again.fingerprint(), made.fingerprint());
    }
}

/// `tls.set`'s order from its third step: a pair that fails several checks gets the first.
#[test]
fn the_checks_run_in_the_contracts_order() {
    let now = wall();
    let here = origin("https://org.example:8443");
    let (cert, key) = pair(&["org.example"]);
    let (_, other_key) = pair(&["org.example"]);
    let (expired, expired_key) = pair_for(&["elsewhere.example"], now - 90 * DAY, now - DAY);
    assert!(check(&cert, &key, &[&here], Some(now)).is_ok());
    for (cert, key) in [
        ("", key.as_str()),
        ("not pem", key.as_str()),
        (cert.as_str(), ""),
        (
            "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n",
            key.as_str(),
        ),
        (
            cert.as_str(),
            "-----BEGIN ENCRYPTED PRIVATE KEY-----\nAAAA\n-----END ENCRYPTED PRIVATE KEY-----\n",
        ),
        (
            cert.as_str(),
            "-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----\n",
        ),
    ] {
        assert_eq!(
            check(cert, key, &[&here], Some(now)).err(),
            Some(TlsRefusal::TlsInvalid)
        );
    }
    // Expired, of another name and not its key: the key comes first.
    assert_eq!(
        check(&expired, &other_key, &[&here], Some(now)).err(),
        Some(TlsRefusal::TlsKeyMismatch)
    );
    // Expired and of another name: validity comes first, with its bounds.
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
    // Not valid yet.
    let (future, future_key) = pair_for(&["org.example"], now + DAY, now + 9 * DAY);
    assert!(matches!(
        check(&future, &future_key, &[&here], Some(now)),
        Err(TlsRefusal::TlsNotValid { .. })
    ));
    // Validity skipped (the re-check of `tls.fetch`): the name is judged.
    match check(&expired, &expired_key, &[&here], None) {
        Err(TlsRefusal::TlsNameMismatch { uncovered }) => {
            assert_eq!(uncovered.origins(), std::slice::from_ref(&here))
        }
        other => panic!("{:?}", other.map(drop)),
    }
}

/// Coverage is by subject alternative name, as browsers match: DNS names (and one wildcard
/// label), IP addresses by their bytes, never the common name, never an IP by a name.
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
    // A certificate whose only name is its common name covers nothing.
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
    // Only `https` origins are judged; each uncovered one is listed once, in order.
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

/// The privateKey OCTET STRING of a PKCS #8 EC key: the SEC 1 `ECPrivateKey`.
fn sec1_of(pkcs8: &[u8]) -> Vec<u8> {
    let (_, info) = x509_parser::der_parser::der::parse_der(pkcs8).unwrap();
    let fields = info.as_sequence().unwrap();
    fields[2].as_slice().unwrap().to_vec()
}

/// What is kept is the pair's normal form: its certificates and its key, re-encoded, never
/// other text of the files (a key in the certificate file, say). A SEC 1 key keeps its label;
/// the pair's name does not depend on the text's line ends.
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
    // An EC key in SEC 1 form: the same key, its own label.
    let rustls::pki_types::PrivateKeyDer::Pkcs8(pkcs8) =
        rustls::pki_types::PrivateKeyDer::from_pem_slice(key.as_bytes()).unwrap()
    else {
        panic!("rcgen writes PKCS #8")
    };
    let sec1 = STANDARD.encode(sec1_of(pkcs8.secret_pkcs8_der()));
    let sec1_pem =
        format!("-----BEGIN EC PRIVATE KEY-----\n{sec1}\n-----END EC PRIVATE KEY-----\n");
    let from_sec1 = check(&cert, &sec1_pem, &[&here], Some(now)).unwrap();
    assert!(from_sec1
        .key()
        .starts_with("-----BEGIN EC PRIVATE KEY-----\n"));
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
    // SAFETY: a valid NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
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

/// The first start of an `https` instance makes its own pair: the key only in the host's
/// secrets, under `tls.<pair>`, the chain in the registry; the log names the fingerprint and
/// no key. A later start keeps it.
#[test]
fn the_first_start_makes_a_pair_and_later_starts_keep_it() {
    let f = fixture();
    let here = origin("https://127.0.0.1:8443");
    let now = wall();
    let (tls, lines) = captured(|| f.start(&here, None, now).unwrap());
    let stored = f.stored();
    assert_eq!(stored.source, Source::Made);
    assert_eq!(f.secrets.names(), [format!("tls.{}", hex(&stored.pair_fp))]);
    assert!(!stored.cert_pem.contains("PRIVATE KEY"));
    let fingerprint = f.leaf();
    let line = lines.join("\n");
    assert!(line.contains(&fingerprint.to_string()), "{line}");
    assert!(line.contains("made by this instance"), "{line}");
    assert!(!line.contains("BEGIN"), "{line}");
    let (chain, key) = tls.fetch().unwrap();
    assert_eq!(chain, stored.cert_pem);
    assert!(f.secrets.holds(key.expose()));
    assert!(!stored.cert_pem.contains(key.expose()));
    drop(tls);
    let (_, lines) = captured(|| f.start(&here, None, now + DAY).unwrap());
    assert_eq!(f.stored(), stored);
    assert_eq!(f.leaf(), fingerprint);
    assert!(lines.join("\n").contains(&fingerprint.to_string()));
}

/// A lost key of the instance's own pair is made again, with a new fingerprint, said so; one
/// that has fewer than 30 days left is renewed. Keys no row names are deleted at a start.
#[test]
fn the_instances_own_pair_is_made_again_when_lost_and_renewed_before_it_expires() {
    let f = fixture();
    let here = origin("https://org.example:8443");
    let now = wall();
    f.start(&here, None, now).unwrap();
    let first = f.stored();
    f.secrets.0.lock().unwrap().clear();
    let (_, lines) = captured(|| f.start(&here, None, now).unwrap());
    let second = f.stored();
    assert_ne!(second.pair_fp, first.pair_fp);
    assert!(lines.join("\n").contains("missing"), "{lines:?}");
    assert_eq!(f.secrets.names(), [format!("tls.{}", hex(&second.pair_fp))]);
    // 368 days on, 29 remain.
    let (_, lines) = captured(|| f.start(&here, None, now + 368 * DAY).unwrap());
    let third = f.stored();
    assert_ne!(third.pair_fp, second.pair_fp);
    assert!(lines.join("\n").contains("renewed"), "{lines:?}");
    assert_eq!(f.secrets.names(), [format!("tls.{}", hex(&third.pair_fp))]);
    // A key a stop left behind is deleted.
    let orphan = Name::parse(&format!("tls.{}", "ab".repeat(32))).unwrap();
    f.secrets
        .put(&orphan, &Value::new(b"x".to_vec()).unwrap())
        .unwrap();
    f.start(&here, None, now + 368 * DAY).unwrap();
    assert_eq!(f.stored(), third);
    assert_eq!(f.secrets.names(), [format!("tls.{}", hex(&third.pair_fp))]);
}

/// A supplied pair is imported at the first start, verified again. One that no longer
/// verifies (expired) does not stop the host: it is kept, logged, and not served (the web
/// gate) until the administrator replaces it with `tls.set`. A supplied key that is gone
/// refuses the start; it is never made again.
#[test]
fn a_supplied_pair_is_imported_and_never_made_again() {
    let f = fixture();
    let here = origin("https://org.example:8443");
    let now = wall();
    let dir = tempfile::tempdir().unwrap();
    let (cert, key) = pair_for(&["org.example"], now - DAY, now + 10 * DAY);
    let (cert_path, key_path) = (dir.path().join("cert.pem"), dir.path().join("key.pem"));
    std::fs::write(&cert_path, &cert).unwrap();
    std::fs::write(&key_path, &key).unwrap();
    let (_, lines) = captured(|| f.start(&here, Some((&cert_path, &key_path)), now).unwrap());
    let stored = f.stored();
    assert_eq!(stored.source, Source::Supplied);
    assert!(
        lines.join("\n").contains("supplied by the operator"),
        "{lines:?}"
    );
    // Its files are not read again.
    std::fs::remove_file(&cert_path).unwrap();
    f.start(&here, Some((&cert_path, &key_path)), now).unwrap();
    // Close to its end it is not renewed (it is the operator's), and the log says so.
    let (_, lines) = captured(|| f.start(&here, None, now + 5 * DAY).unwrap());
    assert_eq!(f.stored(), stored);
    assert!(
        lines.join("\n").contains("ends within 30 days"),
        "{lines:?}"
    );
    // Expired: the host starts, nothing is replaced, and the pair is not served.
    let later = now + 11 * DAY;
    let (tls, lines) = captured(|| f.start(&here, None, later).unwrap());
    assert!(
        lines.join("\n").contains("tls_not_valid") && lines.join("\n").contains("tls.set"),
        "{lines:?}"
    );
    assert_eq!(f.stored(), stored);
    assert_eq!(f.secrets.names(), [format!("tls.{}", hex(&stored.pair_fp))]);
    assert!(matches!(
        tls.servable(later),
        Err(store::Unservable::NotValid)
    ));
    assert_eq!(tls.status(later), org_proto::gateway::TlsStatus::Missing);
    // The administrator's replacement is served.
    let (cert, key) = pair_for(&["org.example"], later - DAY, later + 90 * DAY);
    let id = crate::ids::generate().unwrap();
    let (answer, renew) = tls
        .set(&set_params(&id, &cert, &key), "account", later)
        .unwrap();
    assert!(matches!(answer, HostOpResult::Completed { .. }) && renew);
    tls.servable(later).unwrap();
    let stored = f.stored();
    drop(tls);
    // Gone: refused, never made again.
    f.secrets.0.lock().unwrap().clear();
    let refused = f.start(&here, None, now).err().unwrap();
    assert!(refused.0.contains("never made again"), "{refused}");
    assert_eq!(f.stored(), stored);
    assert!(f.secrets.names().is_empty());
}

#[test]
fn a_supplied_pair_that_does_not_verify_refuses_the_first_start_and_stores_nothing() {
    let now = wall();
    let here = origin("https://org.example:8443");
    let dir = tempfile::tempdir().unwrap();
    let (cert, _) = pair(&["org.example"]);
    let (_, other_key) = pair(&["org.example"]);
    let (cert_path, key_path) = (dir.path().join("cert.pem"), dir.path().join("key.pem"));
    std::fs::write(&cert_path, &cert).unwrap();
    std::fs::write(&key_path, &other_key).unwrap();
    let f = fixture();
    let refused = f
        .start(&here, Some((&cert_path, &key_path)), now)
        .err()
        .unwrap();
    assert!(refused.0.contains("tls_key_mismatch"), "{refused}");
    assert!(!refused.0.contains("BEGIN"), "{refused}");
    assert!(f.db.web_tls_pair().unwrap().is_none());
    assert!(f.secrets.names().is_empty());
    std::fs::remove_file(&key_path).unwrap();
    let refused = f
        .start(&here, Some((&cert_path, &key_path)), now)
        .err()
        .unwrap();
    assert!(
        refused.0.contains(&key_path.display().to_string()),
        "{refused}"
    );
}

fn set_params(operation_id: &str, cert: &str, key: &str) -> TlsSetParams {
    TlsSetParams {
        session: SessionHandle::new("s".repeat(43)).unwrap(),
        operation_id: operation_id.into(),
        cert: cert.into(),
        key: TlsPrivateKey::new(key.into()).unwrap(),
    }
}

/// `tls.set`: the journal first, then the checks; a success installs the pair as supplied and
/// deletes the previous key; `tls_key_mismatch` is stored and replayed, other refusals are
/// not stored; another pair under a used operation is `operation_reused`.
#[test]
fn tls_set_is_journaled_checked_and_installs_a_supplied_pair() {
    let f = fixture();
    let here = origin("https://org.example:8443");
    let now = wall();
    let tls = f.start(&here, None, now).unwrap();
    let made = f.stored();
    let (cert, key) = pair(&["org.example"]);
    let (_, other_key) = pair(&["org.example"]);
    let completed = HostOpResult::Completed {
        result: org_proto::Empty {},
    };

    // Unparseable: refused at the fingerprint, nothing stored, even under the same ID twice.
    let id = crate::ids::generate().unwrap();
    for _ in 0..2 {
        let (answer, renew) = tls
            .set(&set_params(&id, "x", &key), "account", now)
            .unwrap();
        assert_eq!(
            answer,
            HostOpResult::Rejected {
                refusal: TlsRefusal::TlsInvalid
            }
        );
        assert!(!renew);
    }
    // Another name: refused, not stored, so the same ID with a good pair goes through.
    let (wrong, wrong_key) = pair(&["other.example"]);
    let (answer, _) = tls
        .set(&set_params(&id, &wrong, &wrong_key), "account", now)
        .unwrap();
    assert!(matches!(
        answer,
        HostOpResult::Rejected {
            refusal: TlsRefusal::TlsNameMismatch { .. }
        }
    ));
    let (answer, renew) = tls
        .set(&set_params(&id, &cert, &key), "account", now)
        .unwrap();
    assert_eq!(answer, completed);
    assert!(renew);
    let supplied = f.stored();
    assert_eq!(supplied.source, Source::Supplied);
    assert_eq!(
        f.secrets.names(),
        [format!("tls.{}", hex(&supplied.pair_fp))]
    );
    assert_ne!(supplied.pair_fp, made.pair_fp);
    // A replay answers the stored result and installs nothing again.
    let (answer, renew) = tls
        .set(&set_params(&id, &cert, &key), "account", now)
        .unwrap();
    assert_eq!(answer, completed);
    assert!(!renew);
    // The same ID with another pair, or from another account, is another operation.
    let (other, other_cert_key) = pair(&["org.example"]);
    let reused = tls
        .set(&set_params(&id, &other, &other_cert_key), "account", now)
        .unwrap_err();
    assert_eq!(reused.code, org_proto::ErrorCode::OperationReused);
    let reused = tls
        .set(&set_params(&id, &cert, &key), "another", now)
        .unwrap_err();
    assert_eq!(reused.code, org_proto::ErrorCode::OperationReused);
    // A key that is not the certificate's: stored and replayed; nothing installed.
    let mismatch = crate::ids::generate().unwrap();
    for _ in 0..2 {
        let (answer, renew) = tls
            .set(&set_params(&mismatch, &cert, &other_key), "account", now)
            .unwrap();
        assert_eq!(
            answer,
            HostOpResult::Rejected {
                refusal: TlsRefusal::TlsKeyMismatch
            }
        );
        assert!(!renew);
    }
    assert_eq!(f.stored(), supplied);
    // The served pair is the supplied one.
    let (chain, served_key) = tls.fetch().unwrap();
    assert_eq!(chain, supplied.cert_pem);
    assert_eq!(
        check(&chain, served_key.expose(), &[&here], Some(now))
            .unwrap()
            .pair(),
        supplied.pair_fp
    );
}

/// `tls.fetch`'s re-check: a pair that does not cover the origin is `tls_name_mismatch`; a
/// lost key or no pair cannot be served.
#[test]
fn a_pair_that_cannot_be_served_is_never_fetched() {
    let f = fixture();
    let here = origin("https://org.example:8443");
    let now = wall();
    let tls = f.start(&here, None, now).unwrap();
    assert!(tls.servable(now).is_ok());
    assert_eq!(tls.status(now), org_proto::gateway::TlsStatus::Ok);
    assert_eq!(
        tls.status(now + 370 * DAY),
        org_proto::gateway::TlsStatus::Expiring
    );
    // A row naming another origin's pair (written behind the host's back).
    let other = make(&origin("https://other.example"), now).unwrap();
    let name = Name::parse(&format!("tls.{}", hex(&other.pair()))).unwrap();
    f.secrets
        .put(&name, &Value::new(other.key().as_bytes().to_vec()).unwrap())
        .unwrap();
    f.db.set_web_tls_pair(&crate::hostdb::web_tls::StoredPair {
        source: Source::Supplied,
        cert_pem: other.chain().into(),
        pair_fp: other.pair(),
        set_at_ms: 1,
    })
    .unwrap();
    match tls.fetch() {
        Err(store::Unservable::Failure(
            org_host_control::listen::ListenFailure::TlsNameMismatch { uncovered },
        )) => assert_eq!(uncovered.origins(), std::slice::from_ref(&here)),
        other => panic!("{:?}", other.map(drop)),
    }
    f.secrets.0.lock().unwrap().clear();
    assert!(matches!(tls.fetch(), Err(store::Unservable::Storage)));
    assert_eq!(tls.status(now), org_proto::gateway::TlsStatus::Missing);
    f.db.set_web_tls_pair(&crate::hostdb::web_tls::StoredPair {
        source: Source::Made,
        cert_pem: other.chain().into(),
        pair_fp: [9; 32],
        set_at_ms: 1,
    })
    .unwrap();
    assert!(matches!(tls.servable(now), Err(store::Unservable::Storage)));
}

/// A registry or secrets error during the running renewal changes nothing: the pair, its key
/// and its fingerprint stay, and the error is reported for the next look.
#[test]
fn a_failed_read_during_the_renewal_never_replaces_the_pair() {
    let f = fixture();
    let here = origin("https://org.example:8443");
    let now = wall();
    let tls = f.start(&here, None, now).unwrap();
    let made = f.stored();
    f.secrets.1.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(tls.renew_if_expiring(now + 368 * DAY).is_err());
    assert_eq!(f.stored(), made);
    f.secrets
        .1
        .store(false, std::sync::atomic::Ordering::SeqCst);
    // A key that is gone is made again.
    lose_keys(&f);
    assert!(tls.renew_if_expiring(now).unwrap());
    assert_ne!(f.stored().pair_fp, made.pair_fp);
}

/// The renewal while the host runs: only the instance's own pair, only within 30 days.
#[test]
fn only_the_instances_own_pair_is_renewed_while_running() {
    let f = fixture();
    let here = origin("https://org.example:8443");
    let now = wall();
    let tls = f.start(&here, None, now).unwrap();
    let made = f.stored();
    assert!(!tls.renew_if_expiring(now + 300 * DAY).unwrap());
    assert_eq!(f.stored(), made);
    assert!(tls.renew_if_expiring(now + 368 * DAY).unwrap());
    let renewed = f.stored();
    assert_ne!(renewed.pair_fp, made.pair_fp);
    assert_eq!(
        f.secrets.names(),
        [format!("tls.{}", hex(&renewed.pair_fp))]
    );
    let (cert, key) = pair_for(&["org.example"], now - DAY, now + 10 * DAY);
    let id = crate::ids::generate().unwrap();
    tls.set(&set_params(&id, &cert, &key), "account", now)
        .unwrap();
    let supplied = f.stored();
    assert!(!tls.renew_if_expiring(now + 9 * DAY).unwrap());
    assert_eq!(f.stored(), supplied);
}

/// The web start's gate, asked every few seconds, re-reads the key only for a pair it has
/// not verified: while the row names the verified pair it judges only the validity; a new
/// row is verified whole.
#[test]
fn the_gate_verifies_each_stored_pair_once_and_then_judges_its_validity() {
    let f = fixture();
    let here = origin("https://org.example:8443");
    let now = wall();
    let tls = f.start(&here, None, now).unwrap();
    tls.servable(now).unwrap();
    f.secrets.1.store(true, std::sync::atomic::Ordering::SeqCst);
    tls.servable(now).unwrap();
    assert!(matches!(
        tls.servable(now + 398 * DAY),
        Err(store::Unservable::NotValid)
    ));
    f.secrets
        .1
        .store(false, std::sync::atomic::Ordering::SeqCst);
    store_pair_of(&f, &tls, "https://elsewhere.example");
    assert!(matches!(
        tls.servable(now),
        Err(store::Unservable::Failure(_))
    ));
}

/// A chain longer than the registry keeps, in its normal form, is `tls_invalid`, so setup
/// refuses it before anything is saved rather than the host at its first start.
#[test]
fn a_chain_the_registry_cannot_keep_is_refused() {
    let now = wall();
    let here = origin("https://org.example");
    let (cert, key) = pair(&["org.example"]);
    let per = check(&cert, &key, &[&here], Some(now))
        .unwrap()
        .chain()
        .len();
    let fits = verify::CHAIN_LIMIT / per;
    assert!(check(&cert.repeat(fits), &key, &[&here], Some(now)).is_ok());
    assert_eq!(
        check(&cert.repeat(fits + 1), &key, &[&here], Some(now)).err(),
        Some(TlsRefusal::TlsInvalid)
    );
}

/// A key lost while the host runs: `tls.fetch` cannot serve it, and the gate, which had
/// verified the pair, verifies it whole again and keeps the web process waiting.
#[test]
fn a_key_lost_while_running_closes_the_gate_after_a_failed_fetch() {
    let f = fixture();
    let here = origin("https://org.example:8443");
    let now = wall();
    let tls = f.start(&here, None, now).unwrap();
    tls.servable(now).unwrap();
    lose_keys(&f);
    assert!(
        tls.servable(now).is_ok(),
        "the verified pair is not read again"
    );
    assert!(matches!(tls.fetch(), Err(store::Unservable::Storage)));
    assert!(matches!(tls.servable(now), Err(store::Unservable::Storage)));
    assert_eq!(tls.status(now), org_proto::gateway::TlsStatus::Missing);
}
