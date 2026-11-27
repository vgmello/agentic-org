//! The certificate the instance makes, the verifier's order and the stored pair's lifecycle,
//! against a real temp registry and an in-memory secrets store.
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Arc, Mutex};

use org_host_control::listen::ListenOrigin;
use org_host_control::ops::{TlsRefusal, TlsSetParams};
use org_host_control::{HostOpResult, SessionHandle, TlsPrivateKey};

use super::*;
use crate::host_secrets::{Error as SecretError, HostSecrets, Name, Value};
use crate::hostdb::memory::MemoryHostStore;
use crate::hostdb::WebTlsStore;

const INSTANCE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071b0";
const DAY: i64 = 86_400;

fn origin(text: &str) -> ListenOrigin {
    ListenOrigin::parse(text).unwrap()
}

fn wall() -> i64 {
    now().unwrap()
}

pub(crate) use org_host_tls::testing::pair_for;

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
    db: Arc<MemoryHostStore>,
    secrets: Arc<Secrets>,
}

fn fixture() -> Fixture {
    let home = tempfile::tempdir().unwrap();
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let secrets = Arc::new(Secrets::default());
    let db = Arc::new(MemoryHostStore::new(INSTANCE, 1));
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
    ) -> Result<Arc<WebTls>, store::StartRefusal> {
        WebTls::start(
            Arc::clone(&self.db) as Arc<dyn crate::hostdb::HostStore>,
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
    assert!(!line.contains("-----BEGIN"), "{line}");
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
    assert_eq!(tls.status(later), org_proto::web::TlsStatus::Missing);
    // The administrator's replacement is served.
    let (cert, key) = pair_for(&["org.example"], later - DAY, later + 90 * DAY);
    let id = crate::ids::uuid_v7().unwrap();
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
    assert!(!refused.0.contains("-----BEGIN"), "{refused}");
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
    let id = crate::ids::uuid_v7().unwrap();
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
    let mismatch = crate::ids::uuid_v7().unwrap();
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
    assert_eq!(tls.status(now), org_proto::web::TlsStatus::Ok);
    assert_eq!(
        tls.status(now + 370 * DAY),
        org_proto::web::TlsStatus::Expiring
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
    assert_eq!(tls.status(now), org_proto::web::TlsStatus::Missing);
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
    let id = crate::ids::uuid_v7().unwrap();
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
    assert_eq!(tls.status(now), org_proto::web::TlsStatus::Missing);
}

/// A stored pair that has expired is judged from its remembered bounds after one full read:
/// the gate, asked every few seconds, does not read its key again until it is replaced.
#[test]
fn an_expired_pair_is_read_once_by_the_gate() {
    let f = fixture();
    let here = origin("https://org.example:8443");
    let now = wall();
    let tls = f.start(&here, None, now).unwrap();
    let later = now + 398 * DAY;
    assert!(matches!(
        tls.servable(later),
        Err(store::Unservable::NotValid)
    ));
    f.secrets.1.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(matches!(
        tls.servable(later),
        Err(store::Unservable::NotValid)
    ));
    assert_eq!(tls.status(later), org_proto::web::TlsStatus::Missing);
}
