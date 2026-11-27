//! Integration tests verifying `org-host-sqlite` behind `org_host::ports`:
//! - `SqliteHostStoreOpener` registered host lifecycle (create, reopen, missing DB refusal,
//!   marker repair),
//! - writer custody and pending recovery protection against missing-registry cleanup/recreation,
//! - `HostStore` trait-object conformance backed by `HostDb`.

#![cfg(debug_assertions)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;

use org_host::instance::registry::Registry;
use org_host::instance::setup::{Input, Journal, Paths, Record, Service};
use org_host::ports::accounts::{
    AccountId, AccountStore, NewAccount, Registration, Username, Verifier,
};
use org_host::ports::host_secrets::{HostSecrets, Name, Value};
use org_host::ports::host_store::{
    lockout, Error as HostDbError, HostAccountsStore, HostDbInit, HostStore, LoginLockoutStore,
    OperationJournalStore, ProvisioningStore, RecoveryStore, WebTlsStore,
};
use org_host::ports::links::{LinkKind, OneTimeLinks, Subject};
use org_host::ports::{HostPorts, HostStoreOpener};
use org_host::release::ReleasePin;
use org_host::test_support::process::{self, REGISTRY_MISSING};
use org_host::Config;
use org_host_control::listen::ListenConfig;
use org_host_sqlite::{HostDb, SqliteHostStoreOpener};

fn listen(port: u16) -> ListenConfig {
    serde_json::from_value(serde_json::json!({
        "address": "127.0.0.1",
        "port": port,
        "scheme": "http_loopback",
        "origin": format!("http://127.0.0.1:{port}"),
        "hsts": "off"
    }))
    .unwrap()
}

struct RegisteredFixture {
    paths: Paths,
    base: PathBuf,
    _temp: tempfile::TempDir,
}

impl RegisteredFixture {
    fn new() -> Self {
        let temp =
            tempfile::tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap()).unwrap();
        let base = temp.path().canonicalize().unwrap();
        fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).unwrap();
        for name in ["temp", "keys"] {
            fs::create_dir(base.join(name)).unwrap();
            fs::set_permissions(base.join(name), fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self {
            paths: Paths {
                base: base.clone(),
                locator: base.join("locator"),
                user_temp: base.join("temp"),
            },
            base,
            _temp: temp,
        }
    }

    fn record(&self, listen: ListenConfig, instance_id: Option<String>) -> Record {
        let journal = Journal::open(&self.base, None).unwrap();
        let input = Input {
            version: 1,
            service: Service::Registered,
            root: self.base.join("instance"),
            credentials: None,
            listen,
            federation: None,
        };
        let pin = ReleasePin {
            release_id: "sqlite-fixture-1".into(),
            sequence: 1,
            manifest_sha256: "b".repeat(64),
        };
        let record = journal.freeze(input, true, pin).unwrap();
        if instance_id.is_none() {
            return record;
        }
        let next = Record {
            instance_id,
            ..record
        };
        journal.write(&next).unwrap();
        next
    }

    fn reserve(&self, browser: Option<&ListenConfig>) -> String {
        let registry = Registry::open(&self.paths.locator, &self.paths.user_temp).unwrap();
        let instance = registry.reserve(None, &self.base.join("instance")).unwrap();
        if let Some(browser) = browser {
            instance.set_browser(browser).unwrap();
        }
        instance.id().to_owned()
    }

    fn config(&self) -> Config {
        let mut config = Config::new(
            self.base.join("unused-home"),
            self.base.join("no-such-worker"),
        );
        config.dev_key_dir = Some(self.base.join("keys"));
        config
    }

    fn ports(&self) -> HostPorts {
        HostPorts {
            store_opener: Some(Arc::new(SqliteHostStoreOpener)),
            ..HostPorts::default()
        }
    }

    fn office_state(&self) -> PathBuf {
        self.base.join("instance/office-state")
    }

    fn registry_db(&self) -> PathBuf {
        self.office_state().join("host-registry.db")
    }

    fn marker(&self) -> PathBuf {
        self.base.join("instance/host-registry.created")
    }
}

fn custody_snapshot(custody: &dyn HostSecrets) -> Vec<(Name, Vec<u8>)> {
    custody
        .list()
        .unwrap()
        .into_iter()
        .map(|name| {
            let bytes = custody.load(&name).unwrap().unwrap().expose().to_vec();
            (name, bytes)
        })
        .collect()
}

fn private_dir(path: &std::path::Path) {
    fs::create_dir_all(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn sqlite_opener_registered_host_lifecycle() {
    let f = RegisteredFixture::new();
    let cfg = listen(7428);
    let id = f.reserve(Some(&cfg));
    let _record = f.record(cfg, Some(id.clone()));

    // First start creates `host-registry.db` and the creation marker.
    let host = process::start_registered_at_with_ports(f.config(), &f.paths, None, f.ports())
        .expect("registered host creates SQLite registry on first start");
    assert!(f.registry_db().is_file(), "SQLite registry must exist");
    assert!(f.marker().is_file(), "registry-created marker must exist");
    assert_eq!(process::verify_host_instance(&host, &id), Ok(()));
    assert!(SqliteHostStoreOpener.exists(&f.office_state()).unwrap());
    host.shutdown();

    // Second start reopens the existing SQLite registry.
    let host = process::start_registered_at_with_ports(f.config(), &f.paths, None, f.ports())
        .expect("registered host reopens existing SQLite registry");
    assert_eq!(process::verify_host_instance(&host, &id), Ok(()));
    host.shutdown();

    // Missing marker while DB exists is repaired on reopen.
    fs::remove_file(f.marker()).unwrap();
    let host = process::start_registered_at_with_ports(f.config(), &f.paths, None, f.ports())
        .expect("missing marker with existing DB repairs cleanly");
    assert!(f.marker().is_file(), "marker repaired on reopen");
    host.shutdown();

    // Missing DB while marker exists refuses fail-closed with REGISTRY_MISSING.
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let p = f.office_state().join(format!("host-registry.db{suffix}"));
        let _ = fs::remove_file(p);
    }
    let err = process::start_registered_at_with_ports(f.config(), &f.paths, None, f.ports())
        .err()
        .expect("missing SQLite DB after marker must fail closed");
    assert!(
        err.to_string().contains(REGISTRY_MISSING),
        "expected REGISTRY_MISSING, got: {err}"
    );
    assert!(
        !f.registry_db().exists(),
        "refused restart must not recreate missing registry"
    );

    // Direct registry_state_with check confirms missing registry refuses fail-closed.
    let registry = Registry::open(&f.paths.locator, &f.paths.user_temp).unwrap();
    let instance = registry.reserve(None, &f.base.join("instance")).unwrap();
    let state_err =
        process::registry_state_with(&instance, &f.office_state(), &SqliteHostStoreOpener)
            .expect_err("missing SQLite DB after marker must error");
    assert!(
        state_err.to_string().contains(REGISTRY_MISSING),
        "expected REGISTRY_MISSING, got: {state_err}"
    );
}

#[test]
fn missing_registry_cannot_sweep_retained_writer_custody() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let instance_root = root.join("instance");
    let home = instance_root.join("office-state");
    let dev_keys = root.join("dev-keys");
    private_dir(&instance_root);
    private_dir(&home);
    private_dir(&dev_keys);

    let instance_id = org_host::ids::uuid_v7().unwrap();
    let custody = process::open_dev_secrets_raw(&dev_keys, &home, &instance_id).unwrap();
    let db = Arc::new(
        HostDb::create(
            &home,
            &*custody,
            &HostDbInit {
                instance_id: instance_id.clone(),
                clock_high_ms: 1_000,
            },
        )
        .unwrap(),
    );

    let admin_id = org_host::ids::uuid_v7().unwrap();
    let verifier = Verifier::new(
        "$argon2id$v=19$m=65536,t=3,p=1$AAAAAAAAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
            .into(),
    )
    .unwrap();
    db.seed_account(&admin_id, "owner", &verifier, true, 1_000)
        .unwrap();
    let account = db
        .account_authority(&AccountId::parse(&admin_id).unwrap())
        .unwrap()
        .unwrap();
    let org_id = org_host::ids::uuid_v7().unwrap();
    let member_id = org_host::ids::uuid_v7().unwrap();
    let office_id = org_host::ids::uuid_v7().unwrap();
    db.establish_organization_owner(&account, &org_id, &member_id)
        .unwrap();
    private_dir(&home.join("offices").join(&office_id));

    process::reserve_writer_office(
        Arc::clone(&custody),
        db.clone(),
        &instance_id,
        &instance_root,
        process::WriterBinding {
            organization_id: &org_id,
            office_id: &office_id,
            owner_account: &admin_id,
            owner_member: &member_id,
        },
    )
    .unwrap();
    drop(db);

    let before = custody_snapshot(&*custody);
    assert!(
        before.iter().any(|(n, _)| n.as_str() == "writer-device")
            && before
                .iter()
                .any(|(n, _)| n.as_str().starts_with("writer.")),
        "writer custody must exist before registry loss"
    );

    for suffix in ["", "-wal", "-shm", "-journal"] {
        let _ = fs::remove_file(home.join(format!("host-registry.db{suffix}")));
    }

    let err = HostDb::create(
        &home,
        &*custody,
        &HostDbInit {
            instance_id,
            clock_high_ms: 2_000,
        },
    )
    .err()
    .expect("HostDb::create must refuse when writer custody exists");
    assert_eq!(err, HostDbError::Unsafe);
    assert_eq!(
        custody_snapshot(&*custody),
        before,
        "retained writer custody must never be swept on registry loss"
    );
}

#[test]
fn pending_recovery_survives_registry_loss_before_writer_reservation() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let home = root.join("home");
    let dev_keys = root.join("dev-keys");
    private_dir(&home);
    private_dir(&dev_keys);

    let instance_id = org_host::ids::uuid_v7().unwrap();
    let custody = process::open_dev_secrets_raw(&dev_keys, &home, &instance_id).unwrap();
    let db = HostDb::create(
        &home,
        &*custody,
        &HostDbInit {
            instance_id: instance_id.clone(),
            clock_high_ms: 1_000,
        },
    )
    .unwrap();
    drop(db);

    let op = org_host::ids::uuid_v7().unwrap();
    let recovery_name = Name::parse(&format!("recovery.{op}")).unwrap();
    let staged = Value::new(vec![0x5a; 32]).unwrap();
    custody.put(&recovery_name, &staged).unwrap();
    let before = custody_snapshot(&*custody);

    for suffix in ["", "-wal", "-shm", "-journal"] {
        let _ = fs::remove_file(home.join(format!("host-registry.db{suffix}")));
    }

    let err = HostDb::create(
        &home,
        &*custody,
        &HostDbInit {
            instance_id,
            clock_high_ms: 2_000,
        },
    )
    .err()
    .expect("HostDb::create must refuse when pending recovery custody exists");
    assert_eq!(err, HostDbError::Unsafe);
    assert_eq!(
        custody_snapshot(&*custody),
        before,
        "pending recovery custody must survive registry loss"
    );
}

#[test]
fn sqlite_host_store_trait_object_conformance() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let home = root.join("home");
    let dev_keys = root.join("dev-keys");
    private_dir(&home);
    private_dir(&dev_keys);

    let instance_id = org_host::ids::uuid_v7().unwrap();
    let custody = process::open_dev_secrets_raw(&dev_keys, &home, &instance_id).unwrap();
    let store: Arc<dyn HostStore> = org_host_sqlite::create_host_store(
        &home,
        &*custody,
        &HostDbInit {
            instance_id: instance_id.clone(),
            clock_high_ms: 10_000,
        },
    )
    .unwrap();

    // Instance verification
    assert_eq!(store.verify_instance(&instance_id), Ok(()));
    assert!(store
        .verify_instance(&org_host::ids::uuid_v7().unwrap())
        .is_err());
    assert!(!store.has_administrator().unwrap());

    // OneTimeLinks + AccountStore administrator registration through trait object
    let subject = Subject::new("admin-setup").unwrap();
    let issued =
        OneTimeLinks::issue(store.as_ref(), LinkKind::Registration, &subject, 10_000).unwrap();
    let account_id = AccountId::parse(&org_host::ids::uuid_v7().unwrap()).unwrap();
    let verifier = Verifier::new(
        "$argon2id$v=19$m=65536,t=3,p=1$AAAAAAAAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
            .into(),
    )
    .unwrap();
    let new_admin = NewAccount {
        account_id: account_id.clone(),
        username: Username::new("Ada").unwrap(),
        verifier,
    };
    let registered =
        AccountStore::register_administrator(store.as_ref(), &issued.token, &new_admin, 10_001)
            .unwrap();
    assert_eq!(
        registered,
        Registration::Registered {
            account_id: account_id.clone()
        }
    );
    assert!(store.has_administrator().unwrap());

    // HostAccountsStore preferences
    let (tz, last_org) = HostAccountsStore::set_account_preferences(
        store.as_ref(),
        &account_id,
        Some(Some("UTC")),
        None,
    )
    .unwrap();
    assert_eq!(tz.as_deref(), Some("UTC"));
    assert_eq!(last_org, None);

    // LoginLockoutStore
    let state = LoginLockoutStore::record_login(
        store.as_ref(),
        lockout::Socket::Control,
        "ada",
        10_010,
        lockout::Outcome::Failure,
    )
    .unwrap();
    assert!(!state.is_locked());
    assert_eq!(state.retry_after_ms(), 0);

    // ProvisioningStore, RecoveryStore, WebTlsStore, OperationJournalStore empty-state reads
    assert!(ProvisioningStore::provisioning_reserved(store.as_ref())
        .unwrap()
        .is_empty());
    assert!(ProvisioningStore::provisioning_offices(store.as_ref())
        .unwrap()
        .is_empty());
    assert!(
        RecoveryStore::recovery_state(store.as_ref(), &org_host::ids::uuid_v7().unwrap())
            .unwrap()
            .is_none()
    );
    assert!(WebTlsStore::web_tls_pair(store.as_ref()).unwrap().is_none());
    let pruned = OperationJournalStore::journal_prune_expired(store.as_ref(), 20_000).unwrap();
    assert_eq!(pruned.removed, 0);
}

#[test]
fn the_registry_file_holds_no_token_bytes() {
    use org_host::ports::links::LinkDigest;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let home = root.join("home");
    let dev_keys = root.join("dev-keys");
    private_dir(&home);
    private_dir(&dev_keys);

    let instance_id = org_host::ids::uuid_v7().unwrap();
    let custody = process::open_dev_secrets_raw(&dev_keys, &home, &instance_id).unwrap();
    let db = HostDb::create(
        &home,
        &*custody,
        &HostDbInit {
            instance_id: instance_id.clone(),
            clock_high_ms: 10_000,
        },
    )
    .unwrap();
    let s = Subject::new("initial-administrator").unwrap();
    let issued: Vec<_> = (0..4)
        .map(|i| OneTimeLinks::issue(&db, LinkKind::Registration, &s, 10_000 + i).unwrap())
        .collect();
    let used = &issued[3];
    OneTimeLinks::consume(&db, LinkKind::Registration, &used.token.encode(), 10_010)
        .unwrap()
        .unwrap();
    drop(db);

    let mut bytes = Vec::new();
    for entry in fs::read_dir(&home).unwrap() {
        bytes.extend(fs::read(entry.unwrap().path()).unwrap());
    }
    assert!(!bytes.is_empty());
    for link in &issued {
        let text = link.token.encode();
        let raw = decode_base64url(&text);
        assert_eq!(raw.len(), 32);
        assert!(
            !bytes.windows(32).any(|w| w == raw.as_slice()),
            "raw token stored"
        );
        assert!(
            !bytes.windows(text.len()).any(|w| w == text.as_bytes()),
            "encoded token stored"
        );
        let digest = LinkDigest::of(&instance_id, LinkKind::Registration, &link.token).0;
        assert!(bytes.windows(32).any(|w| w == digest), "digest is stored");
    }
}

fn decode_base64url(text: &str) -> Vec<u8> {
    let val = |b: u8| -> u8 {
        match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            other => panic!("unexpected base64url byte {other}"),
        }
    };
    let mut out = Vec::with_capacity((text.len() * 3) / 4);
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for &b in text.as_bytes() {
        acc = (acc << 6) | u32::from(val(b));
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    out
}
