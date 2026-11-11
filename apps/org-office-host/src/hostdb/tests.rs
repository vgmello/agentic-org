use super::*;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
use std::sync::Mutex;

thread_local! {
    static FAIL: RefCell<Option<u8>> = const { RefCell::new(None) };
    static CRASH: std::cell::Cell<Option<u8>> = const { std::cell::Cell::new(None) };
    static SWAP: RefCell<Option<(PathBuf, PathBuf)>> = const { RefCell::new(None) };
    static RESIDUAL: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}
pub(super) fn checkpoint(step: u8) -> Result<(), Error> {
    if step == 4 {
        if let Some(path) = RESIDUAL.with(|r| r.borrow_mut().take()) {
            fs::write(path, b"uncheckpointed temp").unwrap();
        }
    }
    if step == 8 {
        if let Some((path, target)) = SWAP.with(|s| s.borrow_mut().take()) {
            fs::rename(&path, path.with_extension("held")).unwrap();
            symlink(target, path).unwrap();
        }
    }
    if CRASH.with(|c| c.get() == Some(step)) {
        // SAFETY: deliberate process death; the parent checks persisted recovery without Drop.
        unsafe { libc::_exit(150 + i32::from(step)) };
    }
    if FAIL.with(|f| *f.borrow() == Some(step)) {
        return Err(Error::Io(io::ErrorKind::Interrupted));
    }
    Ok(())
}
#[derive(Default)]
struct Secrets(Mutex<BTreeMap<Name, Vec<u8>>>);
impl HostSecrets for Secrets {
    fn put(&self, name: &Name, value: &Value) -> Result<(), crate::host_secrets::Error> {
        self.0
            .lock()
            .unwrap()
            .insert(name.clone(), value.expose().to_vec());
        Ok(())
    }
    fn load(&self, name: &Name) -> Result<Option<Value>, crate::host_secrets::Error> {
        self.0
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .map(Value::new)
            .transpose()
    }
    fn delete(&self, name: &Name) -> Result<(), crate::host_secrets::Error> {
        self.0.lock().unwrap().remove(name);
        Ok(())
    }
    fn list(&self) -> Result<Vec<Name>, crate::host_secrets::Error> {
        Ok(self.0.lock().unwrap().keys().cloned().collect())
    }
}
fn home() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    fs::set_permissions(home.path(), fs::Permissions::from_mode(0o700)).unwrap();
    home
}
fn init(tls: bool) -> HostDbInit {
    HostDbInit {
        instance_id: "01J00000000000000000000000".into(),
        clock_high_ms: 123,
        bootstrap: BootstrapRecord {
            operation_id: "01J00000000000000000000001".into(),
            hq_operation_id: "01J00000000000000000000002".into(),
            fingerprint: "ab".repeat(32),
            account: "admin".into(),
            finished: false,
        },
        tls_pair: tls.then(|| TlsPair {
            cert_pem: "test certificate".into(),
            pair_fp: [0xab; 32],
            set_at_ms: 123,
            key: Value::new(b"test private key".to_vec()).unwrap(),
        }),
    }
}
fn fixture() -> (tempfile::TempDir, Secrets, HostDb) {
    let home = home();
    let secrets = Secrets::default();
    let db = HostDb::create(home.path(), &secrets, &init(true)).unwrap();
    (home, secrets, db)
}
fn bytes(home: &Path) -> BTreeMap<String, Vec<u8>> {
    [DB, "host-registry.db-wal", "host-registry.db-shm"]
        .into_iter()
        .filter_map(|n| fs::read(home.join(n)).ok().map(|b| (n.into(), b)))
        .collect()
}
fn scalar(db: &Connection, pragma: &str) -> i64 {
    db.pragma_query_value(None, pragma, |r| r.get(0)).unwrap()
}

#[test]
fn encrypted_schema_metadata_tls_and_pragmas_survive_reopen() {
    let (home, secrets, db) = fixture();
    let c = db.connection.lock().unwrap();
    assert_eq!(scalar(&c, "user_version"), 1);
    assert_eq!(scalar(&c, "synchronous"), 2);
    assert_eq!(scalar(&c, "fullfsync"), 1);
    assert_eq!(scalar(&c, "checkpoint_fullfsync"), 1);
    assert_eq!(scalar(&c, "foreign_keys"), 1);
    assert_eq!(scalar(&c, "busy_timeout"), 5000);
    assert_eq!(
        c.pragma_query_value(None, "journal_mode", |r| r.get::<_, String>(0))
            .unwrap(),
        "wal"
    );
    let strict: i64 = c.query_row("SELECT count(*) FROM pragma_table_list WHERE name IN ('meta','op_journal','op_verifiers','org_create_staging','tls_pair','org_owners') AND strict=1", [], |r| r.get(0)).unwrap();
    assert_eq!(strict, 6);
    assert_eq!(c.query_row("SELECT count(*) FROM pragma_table_list WHERE name IN ('meta','op_journal','op_verifiers','org_create_staging','tls_pair','org_owners') AND wr=1",[],|r|r.get::<_,i64>(0)).unwrap(),5);
    assert_eq!(c.query_row("SELECT count(*) FROM sqlite_schema WHERE type='index' AND name IN ('op_journal_expiry','op_verifiers_account')",[],|r|r.get::<_,i64>(0)).unwrap(),2);
    for (name, value) in [
        ("instance_id", init(false).instance_id),
        ("clock_high_ms", "123".into()),
        (
            "bootstrap",
            serde_json::to_string(&init(false).bootstrap).unwrap(),
        ),
    ] {
        assert_eq!(
            c.query_row("SELECT value FROM meta WHERE key=?1", [name], |r| r
                .get::<_, String>(0))
                .unwrap(),
            value
        );
    }
    assert_eq!(
        c.query_row("SELECT cert_pem,pair_fp,set_at_ms FROM tls_pair", [], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Vec<u8>>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })
        .unwrap(),
        ("test certificate".into(), vec![0xab; 32], 123)
    );
    for path in [DB, "host-registry.db-wal", "host-registry.db-shm"] {
        assert_eq!(
            fs::metadata(home.path().join(path)).unwrap().mode() & 0o7777,
            0o600
        );
    }
    assert!(!fs::read(home.path().join(DB))
        .unwrap()
        .starts_with(b"SQLite format 3"));
    let key = secrets
        .load(&Name::parse("registry").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(key.expose().len(), 32);
    assert_eq!(
        secrets
            .load(&Name::parse(&format!("tls.{}", "ab".repeat(32))).unwrap())
            .unwrap()
            .unwrap()
            .expose(),
        b"test private key"
    );
    drop(c);
    drop(db);
    let db = HostDb::open(home.path(), &secrets).unwrap();
    assert_eq!(
        db.connection
            .lock()
            .unwrap()
            .query_row("SELECT count(*) FROM meta", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
    // Independently opens with a raw hex key, not SQLCipher's passphrase KDF.
    let c = Connection::open(home.path().join(DB)).unwrap();
    c.execute_batch(&format!("PRAGMA key=\"x'{}'\";", hex(key.expose())))
        .unwrap();
    assert_eq!(scalar(&c, "user_version"), 1);
}
#[test]
fn create_replaces_leftover_secrets_and_temps_without_tls() {
    let home = home();
    let secrets = Secrets::default();
    for n in [
        "registry",
        &format!("tls.{}", "cd".repeat(32)),
        "recovery.Abc123",
    ] {
        secrets
            .put(&Name::parse(n).unwrap(), &Value::new(vec![7; 32]).unwrap())
            .unwrap();
    }
    for n in [TMP, "host-registry.db.tmp-journal"] {
        fs::write(home.path().join(n), b"stale").unwrap();
    }
    let db = HostDb::create(home.path(), &secrets, &init(false)).unwrap();
    assert_eq!(secrets.list().unwrap(), [Name::parse("registry").unwrap()]);
    assert_ne!(
        secrets
            .load(&Name::parse("registry").unwrap())
            .unwrap()
            .unwrap()
            .expose(),
        &[7; 32]
    );
    assert!(!home.path().join(TMP).exists());
    assert!(!home.path().join("host-registry.db.tmp-journal").exists());
    assert_eq!(
        db.connection
            .lock()
            .unwrap()
            .query_row("SELECT count(*) FROM tls_pair", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn existing_file_and_dangling_symlink_create_have_no_effect() {
    for dangling in [false, true] {
        let home = home();
        let secrets = Secrets::default();
        secrets
            .put(
                &Name::parse("registry").unwrap(),
                &Value::new(vec![9; 32]).unwrap(),
            )
            .unwrap();
        if dangling {
            symlink(home.path().join("absent"), home.path().join(DB)).unwrap();
        } else {
            fs::write(home.path().join(DB), b"existing").unwrap();
        }
        fs::write(home.path().join(TMP), b"preserve tmp").unwrap();
        assert!(matches!(
            HostDb::create(home.path(), &secrets, &init(true)),
            Err(Error::Exists)
        ));
        assert_eq!(fs::read(home.path().join(TMP)).unwrap(), b"preserve tmp");
        assert_eq!(
            secrets
                .load(&Name::parse("registry").unwrap())
                .unwrap()
                .unwrap()
                .expose(),
            &[9; 32]
        );
        if dangling {
            assert!(!home.path().join("absent").exists());
        } else {
            assert_eq!(fs::read(home.path().join(DB)).unwrap(), b"existing");
        }
    }
}
#[test]
fn refusals_preserve_existing_database_and_wal_bytes() {
    let (home, secrets, db) = fixture();
    for key in [
        None,
        Some(vec![8; 31]),
        Some(vec![8; 32]),
        Some(vec![8; 33]),
    ] {
        let other = Secrets::default();
        if let Some(key) = key {
            other
                .put(&Name::parse("registry").unwrap(), &Value::new(key).unwrap())
                .unwrap();
        }
        let before = bytes(home.path());
        let result = HostDb::open(home.path(), &other);
        let stored = other.load(&Name::parse("registry").unwrap()).unwrap();
        if stored.as_ref().is_some_and(|k| k.expose().len() != 32) {
            assert!(matches!(result, Err(Error::KeyLength)));
        } else {
            assert!(result.is_err());
        }
        let after = bytes(home.path());
        // Opening a WAL reader may update its shared-memory read marks; the durable files stay exact.
        for name in [DB, "host-registry.db-wal"] {
            assert_eq!(before.get(name), after.get(name), "{name}");
        }
    }
    db.connection
        .lock()
        .unwrap()
        .pragma_update(None, "user_version", 2)
        .unwrap();
    let before = bytes(home.path());
    assert!(matches!(
        HostDb::open(home.path(), &secrets),
        Err(Error::Version)
    ));
    let after = bytes(home.path());
    for name in [DB, "host-registry.db-wal"] {
        assert_eq!(before.get(name), after.get(name), "{name}");
    }
}
#[test]
fn missing_empty_nonregular_symlink_plaintext_and_sidecar_aliases_refuse() {
    let home = home();
    let secrets = Secrets::default();
    secrets
        .put(
            &Name::parse("registry").unwrap(),
            &Value::new(vec![1; 32]).unwrap(),
        )
        .unwrap();
    assert!(HostDb::open(home.path(), &secrets).is_err());
    assert!(!home.path().join(DB).exists());
    fs::write(home.path().join(DB), []).unwrap();
    fs::write(home.path().join("host-registry.db-wal"), b"precious wal").unwrap();
    assert!(matches!(
        HostDb::open(home.path(), &secrets),
        Err(Error::Empty)
    ));
    assert_eq!(
        fs::read(home.path().join("host-registry.db-wal")).unwrap(),
        b"precious wal"
    );
    fs::remove_file(home.path().join(DB)).unwrap();
    fs::create_dir(home.path().join(DB)).unwrap();
    assert!(matches!(
        HostDb::open(home.path(), &secrets),
        Err(Error::Unsafe)
    ));
    fs::remove_dir(home.path().join(DB)).unwrap();
    let target = home.path().join("target");
    fs::write(&target, b"outside").unwrap();
    symlink(&target, home.path().join(DB)).unwrap();
    assert!(HostDb::open(home.path(), &secrets).is_err());
    assert_eq!(fs::read(target).unwrap(), b"outside");
    fs::remove_file(home.path().join(DB)).unwrap();
    fs::remove_file(home.path().join("host-registry.db-wal")).unwrap();
    let plain = Connection::open(home.path().join(DB)).unwrap();
    plain
        .execute_batch("CREATE TABLE plain(x); PRAGMA user_version=1")
        .unwrap();
    drop(plain);
    let before = fs::read(home.path().join(DB)).unwrap();
    assert!(HostDb::open(home.path(), &secrets).is_err());
    assert_eq!(fs::read(home.path().join(DB)).unwrap(), before);
    let (home, secrets, db) = fixture();
    drop(db);
    let outside = home.path().join("outside");
    fs::write(&outside, b"outside").unwrap();
    for suffix in ["-wal", "-shm"] {
        let sidecar = home.path().join(format!("{DB}{suffix}"));
        symlink(&outside, &sidecar).unwrap();
        assert!(HostDb::open(home.path(), &secrets).is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"outside");
        fs::remove_file(sidecar).unwrap();
    }
}
#[test]
fn unsafe_homes_hardlinks_and_loose_modes() {
    let (home, secrets, db) = fixture();
    drop(db);
    let alias = home.path().join("alias");
    symlink(home.path(), &alias).unwrap();
    assert!(matches!(HostDb::open(&alias, &secrets), Err(Error::Unsafe)));
    assert!(matches!(
        HostDb::open(Path::new("relative"), &secrets),
        Err(Error::Unsafe)
    ));
    fs::set_permissions(home.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(
        HostDb::open(home.path(), &secrets),
        Err(Error::Unsafe)
    ));
    fs::set_permissions(home.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let link = home.path().join("hardlink");
    fs::hard_link(home.path().join(DB), &link).unwrap();
    assert!(matches!(
        HostDb::open(home.path(), &secrets),
        Err(Error::Unsafe)
    ));
    fs::remove_file(link).unwrap();
    fs::set_permissions(home.path().join(DB), fs::Permissions::from_mode(0o644)).unwrap();
    let db = HostDb::open(home.path(), &secrets).unwrap();
    assert_eq!(
        fs::metadata(home.path().join(DB)).unwrap().mode() & 0o7777,
        0o600
    );
    drop(db);
}
#[test]
fn failures_at_every_checkpoint_are_recoverable_and_precommit_is_atomic() {
    for step in 1..=7 {
        let home = home();
        let secrets = Secrets::default();
        FAIL.with(|f| *f.borrow_mut() = Some(step));
        assert!(matches!(
            HostDb::create(home.path(), &secrets, &init(true)),
            Err(Error::Io(io::ErrorKind::Interrupted))
        ));
        FAIL.with(|f| *f.borrow_mut() = None);
        if step == 7 {
            let key = secrets
                .load(&Name::parse("registry").unwrap())
                .unwrap()
                .unwrap();
            let c = connect(&home.path().canonicalize().unwrap().join(TMP), &key).unwrap();
            assert_eq!(scalar(&c, "user_version"), 0);
            assert_eq!(
                c.query_row("SELECT count(*) FROM sqlite_schema", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
        if home.path().join(DB).exists() {
            HostDb::open(home.path(), &secrets).unwrap();
        } else {
            HostDb::create(home.path(), &secrets, &init(false)).unwrap();
        }
    }
}
#[test]
fn exclusive_install_never_overwrites_even_a_late_symlink() {
    let home = home();
    let from = home.path().join(TMP);
    let to = home.path().join(DB);
    for symlinked in [false, true] {
        fs::write(&from, b"new").unwrap();
        if symlinked {
            symlink(home.path().join("missing"), &to).unwrap();
        } else {
            fs::write(&to, b"old").unwrap();
        }
        assert!(install(&from, &to).is_err());
        assert_eq!(fs::read(&from).unwrap(), b"new");
        if symlinked {
            assert!(!home.path().join("missing").exists());
        } else {
            assert_eq!(fs::read(&to).unwrap(), b"old");
        }
        fs::remove_file(&to).unwrap();
        fs::remove_file(&from).unwrap();
    }
    fs::write(&from, b"new").unwrap();
    install(&from, &to).unwrap();
    assert_eq!(fs::read(&to).unwrap(), b"new");
    assert!(!from.exists());
}
#[test]
fn bootstrap_json_is_strict_and_errors_do_not_expose_secrets() {
    let mut record = serde_json::to_value(&init(false).bootstrap).unwrap();
    record["extra"] = serde_json::json!(1);
    assert!(serde_json::from_value::<BootstrapRecord>(record).is_err());
    let (home, _, db) = fixture();
    drop(db);
    let secrets = Secrets::default();
    secrets
        .put(
            &Name::parse("registry").unwrap(),
            &Value::new(vec![0xab; 32]).unwrap(),
        )
        .unwrap();
    let error = HostDb::open(home.path(), &secrets).err().unwrap();
    let text = format!("{error:?}");
    assert!(!text.contains(&"ab".repeat(32)));
    assert!(!text.contains("PRAGMA"));
    assert!(!text.contains(home.path().to_str().unwrap()));
}

#[test]
#[cfg(debug_assertions)]
fn real_process_death_at_every_create_checkpoint_recovers() {
    const CHILD: &str = "SAGE_HOSTDB_CRASH_STEP";
    if let Ok(step) = std::env::var(CHILD) {
        let home = std::env::var_os("SAGE_HOSTDB_CRASH_HOME").unwrap();
        let base = std::env::var_os("SAGE_HOSTDB_CRASH_BASE").unwrap();
        let store = crate::host_secrets::dev::DevSecrets::open(
            Path::new(&base),
            Path::new(&home),
            &init(false).instance_id,
        )
        .unwrap();
        CRASH.with(|c| c.set(Some(step.parse().unwrap())));
        let _ = HostDb::create(Path::new(&home), &store, &init(true));
        panic!("crash point was not reached");
    }
    for step in 1..=7u8 {
        let home = home();
        let base = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "hostdb::tests::real_process_death_at_every_create_checkpoint_recovers",
                "--nocapture",
            ])
            .env(CHILD, step.to_string())
            .env("SAGE_HOSTDB_CRASH_HOME", home.path())
            .env("SAGE_HOSTDB_CRASH_BASE", base.path())
            .output()
            .unwrap();
        assert_eq!(
            result.status.code(),
            Some(150 + i32::from(step)),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let secrets = crate::host_secrets::dev::DevSecrets::open(
            base.path(),
            home.path(),
            &init(false).instance_id,
        )
        .unwrap();
        assert_eq!(home.path().join(DB).exists(), step == 5 || step == 6);
        assert_eq!(
            secrets
                .load(&Name::parse("registry").unwrap())
                .unwrap()
                .is_some(),
            step >= 3
        );
        if step == 5 || step == 6 {
            HostDb::open(home.path(), &secrets).unwrap();
        } else {
            HostDb::create(home.path(), &secrets, &init(false)).unwrap();
        }
    }
}

#[test]
fn schema_checks_foreign_keys_update_delete_conflict_rollback_and_reopen() {
    let (home, secrets, db) = fixture();
    {
        let mut c = db.connection.lock().unwrap();
        let insert = "INSERT INTO op_journal VALUES (?1,?2,?3,'actor',?4,?5,?6)";
        let id = "01J00000000000000000000003";
        for (kind, state, result, allowed) in [
            ("account.create", "completed", Some("{}"), true),
            ("account.create", "rejected", Some("{}"), true),
            ("account.create", "reserved", None, false),
            ("account.create", "linked", Some("{}"), false),
            ("account.create", "abandoned", Some("{}"), false),
            ("organization.create", "reserved", None, true),
            ("organization.create", "linked", Some("{}"), true),
            ("organization.create", "abandoned", Some("{}"), true),
            ("organization.create", "completed", Some("{}"), false),
            ("organization.create", "unknown", Some("{}"), false),
            ("organization.create", "reserved", Some("{}"), false),
            ("organization.create", "linked", None, false),
        ] {
            let tx = c
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            assert_eq!(
                tx.execute(insert, (id, kind, &[1u8; 32][..], state, result, 0))
                    .is_ok(),
                allowed,
                "{kind}/{state}/{result:?}"
            );
        }
        for (id_len, fp_len, expiry, allowed) in [
            (25, 32, 0, false),
            (26, 31, 0, false),
            (26, 33, 0, false),
            (27, 32, 0, false),
            (26, 32, -1, false),
            (26, 32, 0, true),
        ] {
            let tx = c
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            assert_eq!(
                tx.execute(
                    insert,
                    (
                        "A".repeat(id_len),
                        "account.create",
                        vec![1; fp_len],
                        "completed",
                        "{}",
                        expiry
                    )
                )
                .is_ok(),
                allowed
            );
        }
        c.execute(
            insert,
            (id, "account.create", &[1u8; 32][..], "completed", "{}", 0),
        )
        .unwrap();
        assert!(c
            .execute(
                insert,
                (id, "account.create", &[1u8; 32][..], "completed", "{}", 0)
            )
            .is_err());
        c.execute(
            "INSERT INTO op_verifiers VALUES (?1,'actor','opaque')",
            [id],
        )
        .unwrap();
        assert!(c
            .execute(
                "INSERT INTO op_verifiers VALUES ('missing','actor','opaque')",
                []
            )
            .is_err());
        c.execute("DELETE FROM op_journal WHERE operation_id=?1", [id])
            .unwrap();
        assert_eq!(
            c.query_row("SELECT count(*) FROM op_verifiers", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        c.execute(
            insert,
            (
                id,
                "organization.create",
                &[1u8; 32][..],
                "reserved",
                None::<&str>,
                0,
            ),
        )
        .unwrap();
        c.execute(
            "INSERT INTO org_create_staging VALUES (?1,'name','org','office','owner')",
            [id],
        )
        .unwrap();
        assert!(c
            .execute("DELETE FROM op_journal WHERE operation_id=?1", [id])
            .is_err());
        for (name, org, office) in [
            ("name", "org2", "office2"),
            ("name2", "org", "office2"),
            ("name2", "org2", "office"),
        ] {
            let tx = c
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            let second = "01J00000000000000000000004";
            tx.execute(
                insert,
                (
                    second,
                    "organization.create",
                    &[1u8; 32][..],
                    "reserved",
                    None::<&str>,
                    0,
                ),
            )
            .unwrap();
            assert!(tx
                .execute(
                    "INSERT INTO org_create_staging VALUES (?1,?2,?3,?4,'owner2')",
                    (second, name, org, office)
                )
                .is_err());
        }
        c.execute(
            "UPDATE op_journal SET state='linked',result='{}' WHERE operation_id=?1",
            [id],
        )
        .unwrap();
        assert!(c
            .execute(
                "UPDATE op_journal SET result=NULL WHERE operation_id=?1",
                [id]
            )
            .is_err());
        c.execute("DELETE FROM org_create_staging WHERE operation_id=?1", [id])
            .unwrap();
        c.execute(
            "INSERT OR REPLACE INTO org_owners VALUES ('org','actor','owner')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT OR REPLACE INTO org_owners VALUES ('org','actor2','owner2')",
            [],
        )
        .unwrap();
        assert!(c
            .execute(
                "INSERT INTO tls_pair VALUES (2,'cert',?1,0)",
                [&[1u8; 32][..]]
            )
            .is_err());
        assert!(c
            .execute("UPDATE tls_pair SET pair_fp=?1", [&[1u8; 31][..]])
            .is_err());
        assert!(c.execute("UPDATE meta SET value=1.5", []).is_ok()); // STRICT TEXT accepts lossless text coercion.
        assert!(c
            .execute("UPDATE op_journal SET expires_at_ms='not-an-integer'", [])
            .is_err());
        {
            let tx = c
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            tx.execute("UPDATE org_owners SET account='rolled-back'", [])
                .unwrap();
        }
        assert_eq!(
            c.query_row("SELECT account FROM org_owners", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "actor2"
        );
    }
    drop(db);
    let db = HostDb::open(home.path(), &secrets).unwrap();
    let c = db.connection.lock().unwrap();
    assert_eq!(
        c.query_row("SELECT state FROM op_journal", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "linked"
    );
    assert_eq!(
        c.query_row("SELECT account FROM org_owners", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "actor2"
    );
}

#[test]
fn sqlite_file_descriptors_are_close_on_exec() {
    use std::os::fd::AsRawFd;
    let (home, _, db) = fixture();
    let held = File::open(home.path().join(DB)).unwrap();
    let inode = held.metadata().unwrap();
    let mut found = 0;
    for fd in 3..4096 {
        if fd == held.as_raw_fd() {
            continue;
        }
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: the output is initialized by a successful fstat; failed descriptors are skipped.
        if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } != 0 {
            continue;
        }
        // SAFETY: fstat succeeded.
        let stat = unsafe { stat.assume_init() };
        if stat.st_ino as u128 == u128::from(inode.ino())
            && stat.st_dev as u128 == u128::from(inode.dev())
        {
            // SAFETY: F_GETFD has no variadic argument and examines a live fd.
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            assert_ne!(flags & libc::FD_CLOEXEC, 0);
            found += 1;
        }
    }
    assert!(found >= 1, "the SQLite connection owns a main-file fd");
    drop(db);
}

#[test]
fn sqlite_drop_releases_locks_while_a_forked_child_retains_its_fds() {
    use std::os::fd::AsRawFd;
    let (home, secrets, db) = fixture();
    let (mut release, wait) = std::os::unix::net::UnixStream::pair().unwrap();
    let (release_fd, wait_fd) = (release.as_raw_fd(), wait.as_raw_fd());
    // SAFETY: the child calls only async-signal-safe close/read/_exit; it never uses SQLite.
    let pid = unsafe { libc::fork() };
    if pid == 0 {
        // SAFETY: retain inherited SQLite fds until the parent's close/reopen/write attempt.
        unsafe {
            libc::close(release_fd);
            let mut byte = 0u8;
            let read = libc::read(wait_fd, (&mut byte as *mut u8).cast(), 1);
            libc::_exit(if read == 1 && byte == 1 { 0 } else { 42 });
        }
    }
    assert!(pid > 0, "fork failed");
    drop(wait);
    drop(db);
    let reopened = HostDb::open(home.path(), &secrets);
    let written = reopened.as_ref().ok().map(|db| {
        db.connection
            .lock()
            .unwrap()
            .execute("INSERT INTO org_owners VALUES ('fork','actor','owner')", [])
    });
    let released = std::io::Write::write_all(&mut release, &[1]);
    drop(release);
    let mut status = 0;
    // SAFETY: reap exactly the child this test created, before asserting operation results.
    let waited = unsafe { libc::waitpid(pid, &mut status, 0) };
    released.unwrap();
    assert_eq!(waited, pid);
    assert_eq!(status, 0);
    assert!(reopened.is_ok(), "{:?}", reopened.err());
    assert_eq!(written.unwrap().unwrap(), 1);
}

#[test]
fn sqlcipher_build_encrypts_and_a_plain_sqlite_build_refuses_creation() {
    let probe = Connection::open_in_memory().unwrap();
    let cipher: Option<String> = probe
        .query_row("PRAGMA cipher_version", [], |r| r.get(0))
        .ok();
    let home = home();
    let secrets = Secrets::default();
    let result = HostDb::create(home.path(), &secrets, &init(false));
    if cipher.is_some_and(|s| !s.is_empty()) {
        assert!(result.is_ok());
        assert!(!fs::read(home.path().join(DB))
            .unwrap()
            .starts_with(b"SQLite format 3"));
    } else {
        assert!(matches!(result, Err(Error::NoCipher)));
        assert!(!home.path().join(DB).exists());
        let c = Connection::open(home.path().join(DB)).unwrap();
        c.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)
            .unwrap();
        c.pragma_update(None, "journal_mode", "WAL").unwrap();
        c.execute_batch("CREATE TABLE preserved(value); INSERT INTO preserved VALUES ('valuable'); PRAGMA user_version=1;").unwrap();
        drop(c);
        let before = bytes(home.path());
        assert!(matches!(
            HostDb::open(home.path(), &secrets),
            Err(Error::NoCipher)
        ));
        let after = bytes(home.path());
        for name in [DB, "host-registry.db-wal"] {
            assert_eq!(before.get(name), after.get(name), "{name}");
        }
    }
}

#[test]
fn every_secret_provider_error_propagates_and_remains_recoverable() {
    struct Faults {
        inner: Secrets,
        at: usize,
        calls: std::sync::atomic::AtomicUsize,
    }
    impl Faults {
        fn check(&self) -> Result<(), crate::host_secrets::Error> {
            if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1 == self.at {
                Err(crate::host_secrets::Error::Io(io::ErrorKind::Interrupted))
            } else {
                Ok(())
            }
        }
    }
    impl HostSecrets for Faults {
        fn put(&self, n: &Name, v: &Value) -> Result<(), crate::host_secrets::Error> {
            self.check()?;
            self.inner.put(n, v)
        }
        fn load(&self, n: &Name) -> Result<Option<Value>, crate::host_secrets::Error> {
            self.check()?;
            self.inner.load(n)
        }
        fn delete(&self, n: &Name) -> Result<(), crate::host_secrets::Error> {
            self.check()?;
            self.inner.delete(n)
        }
        fn list(&self) -> Result<Vec<Name>, crate::host_secrets::Error> {
            self.check()?;
            self.inner.list()
        }
    }
    // list, delete old registry, put TLS, put registry, reopen load.
    for at in 1..=5 {
        let home = home();
        let inner = Secrets::default();
        inner
            .put(
                &Name::parse("registry").unwrap(),
                &Value::new(vec![7; 32]).unwrap(),
            )
            .unwrap();
        let secrets = Faults {
            inner,
            at,
            calls: std::sync::atomic::AtomicUsize::new(0),
        };
        assert!(matches!(
            HostDb::create(home.path(), &secrets, &init(true)),
            Err(Error::Secret(crate::host_secrets::Error::Io(
                io::ErrorKind::Interrupted
            )))
        ));
        assert_eq!(home.path().join(DB).exists(), at == 5);
        if at == 5 {
            HostDb::open(home.path(), &secrets).unwrap();
        } else {
            HostDb::create(home.path(), &secrets, &init(false)).unwrap();
        }
    }
}

#[test]
fn ownership_guards_accept_ours_and_refuse_foreign_linux_root_fixtures() {
    let home = home();
    let file = home.path().join("file");
    fs::write(&file, b"x").unwrap();
    assert!(checked_home(home.path()).is_ok());
    assert!(checked_file(&file, false).unwrap().is_some());
    #[cfg(target_os = "linux")]
    if crate::home::euid() == 0 {
        std::os::unix::fs::chown(&file, Some(1), None).unwrap();
        assert!(matches!(checked_file(&file, false), Err(Error::Unsafe)));
        std::os::unix::fs::chown(&file, Some(0), None).unwrap();
        std::os::unix::fs::chown(home.path(), Some(1), None).unwrap();
        assert!(matches!(checked_home(home.path()), Err(Error::Unsafe)));
        std::os::unix::fs::chown(home.path(), Some(0), None).unwrap();
    }
}

#[test]
fn schema_refusal_is_read_only_even_when_it_is_the_last_wal_connection() {
    for version in [0, 2, -1] {
        let (home, secrets, db) = fixture();
        {
            let c = db.connection.lock().unwrap();
            c.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)
                .unwrap();
            c.pragma_update(None, "user_version", version).unwrap();
        }
        drop(db);
        let before = bytes(home.path());
        assert!(!before["host-registry.db-wal"].is_empty());
        assert!(matches!(
            HostDb::open(home.path(), &secrets),
            Err(Error::Version)
        ));
        let after = bytes(home.path());
        for name in [DB, "host-registry.db-wal"] {
            assert_eq!(before.get(name), after.get(name), "{version}/{name}");
        }
    }
}

#[test]
fn hot_rollback_journal_refusal_preserves_the_database_and_journal() {
    const CHILD: &str = "SAGE_HOSTDB_HOT_JOURNAL";
    const TEST: &str =
        "hostdb::tests::hot_rollback_journal_refusal_preserves_the_database_and_journal";
    if let Some(path) = std::env::var_os(CHILD) {
        let c = Connection::open(Path::new(&path)).unwrap();
        c.execute_batch(&format!("PRAGMA key = \"x'{}'\";", "12".repeat(32)))
            .unwrap();
        let version: i64 = std::env::var("SAGE_HOSTDB_HOT_VERSION")
            .unwrap()
            .parse()
            .unwrap();
        c.pragma_update(None, "user_version", version).unwrap();
        c.execute_batch("PRAGMA journal_mode=DELETE; CREATE TABLE t(x); WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM c WHERE x<300) INSERT INTO t SELECT zeroblob(4000) FROM c;").unwrap();
        c.execute_batch("PRAGMA cache_size=1; BEGIN IMMEDIATE; UPDATE t SET x=randomblob(4000);")
            .unwrap();
        // SAFETY: isolated child dies with a live transaction, without SQLite rollback/Drop.
        unsafe { libc::_exit(172) };
    }
    for version in [1, 0, 2, -1] {
        let home = home();
        let path = home.path().canonicalize().unwrap().join(DB);
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST, "--nocapture"])
            .env(CHILD, &path)
            .env("SAGE_HOSTDB_HOT_VERSION", version.to_string())
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(172), "{result:?}");
        let journal = path.with_extension("db-journal");
        let before = fs::read(&path).unwrap();
        let journal_before = fs::read(&journal).unwrap();
        assert!(journal_before.len() > 512, "real spilled rollback journal");
        let secrets = Secrets::default();
        secrets
            .put(
                &Name::parse("registry").unwrap(),
                &Value::new(vec![0x12; 32]).unwrap(),
            )
            .unwrap();
        assert!(matches!(
            HostDb::open(home.path(), &secrets),
            Err(Error::Unsafe)
        ));
        assert!(
            fs::read(&path).unwrap() == before,
            "main changed on refusal"
        );
        assert!(
            fs::read(&journal).unwrap() == journal_before,
            "journal changed on refusal"
        );
    }
}

#[test]
fn creation_publishes_wal_before_first_open_and_cleans_temp_sidecars() {
    let home = home();
    let secrets = Secrets::default();
    FAIL.with(|f| *f.borrow_mut() = Some(5));
    let result = HostDb::create(home.path(), &secrets, &init(false));
    FAIL.with(|f| *f.borrow_mut() = None);
    assert!(matches!(result, Err(Error::Io(io::ErrorKind::Interrupted))));
    for name in ["host-registry.db.tmp-wal", "host-registry.db.tmp-shm"] {
        assert!(!home.path().join(name).exists());
    }
    let key = secrets
        .load(&Name::parse("registry").unwrap())
        .unwrap()
        .unwrap();
    let path = home.path().canonicalize().unwrap().join(DB);
    let before = fs::read(&path).unwrap();
    assert!(!before.starts_with(b"SQLite format 3")); // Header bytes are encrypted too.
    let c = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .unwrap();
    c.execute_batch(&format!("PRAGMA key=\"x'{}'\";", hex(key.expose())))
        .unwrap();
    assert_eq!(
        c.pragma_query_value(None, "journal_mode", |r| r.get::<_, String>(0))
            .unwrap(),
        "wal"
    );
    drop(c);
    assert!(fs::read(path).unwrap() == before);
}

#[test]
fn retry_removes_both_temporary_wal_sidecars_before_sqlite() {
    let home = home();
    let names = ["host-registry.db.tmp-wal", "host-registry.db.tmp-shm"];
    for name in names {
        fs::write(home.path().join(name), b"crashed create").unwrap();
    }
    FAIL.with(|f| *f.borrow_mut() = Some(1));
    let result = HostDb::create(home.path(), &Secrets::default(), &init(false));
    FAIL.with(|f| *f.borrow_mut() = None);
    assert!(matches!(result, Err(Error::Io(io::ErrorKind::Interrupted))));
    for name in names {
        assert!(!home.path().join(name).exists(), "{name}");
    }
}

#[test]
fn any_rollback_journal_is_refused_before_key_load_and_left_untouched() {
    for kind in ["empty", "nonempty", "directory", "symlink", "dangling"] {
        let (home, _, db) = fixture();
        drop(db);
        let path = home.path().join("host-registry.db-journal");
        let target = home.path().join("precious");
        fs::write(&target, b"untouched target").unwrap();
        match kind {
            "empty" => fs::write(&path, []).unwrap(),
            "nonempty" => fs::write(&path, b"not even a valid journal").unwrap(),
            "directory" => fs::create_dir(&path).unwrap(),
            "symlink" => symlink(&target, &path).unwrap(),
            "dangling" => symlink(home.path().join("absent"), &path).unwrap(),
            _ => unreachable!(),
        }
        let before = fs::read(home.path().join(DB)).unwrap();
        let metadata = fs::symlink_metadata(&path).unwrap();
        // No registry key: loading it before the journal guard would return KeyMissing.
        assert!(
            matches!(
                HostDb::open(home.path(), &Secrets::default()),
                Err(Error::Unsafe)
            ),
            "{kind}"
        );
        assert_eq!(fs::symlink_metadata(&path).unwrap().ino(), metadata.ino());
        assert!(fs::read(home.path().join(DB)).unwrap() == before);
        assert_eq!(fs::read(&target).unwrap(), b"untouched target");
        if kind == "empty" || kind == "nonempty" {
            assert_eq!(
                fs::read(&path).unwrap(),
                if kind == "empty" {
                    b"".as_slice()
                } else {
                    b"not even a valid journal".as_slice()
                }
            );
        }
    }
}

#[test]
fn absent_guard_distinguishes_missing_present_dangling_and_io_error() {
    let home = home();
    let path = home.path().join("file");
    assert_eq!(absent(&path, Error::Unsafe), Ok(()));
    fs::write(&path, b"present").unwrap();
    assert_eq!(absent(&path, Error::Exists), Err(Error::Exists));
    assert_eq!(absent(&path, Error::Unsafe), Err(Error::Unsafe));
    let alias = home.path().join("dangling");
    symlink(home.path().join("missing"), &alias).unwrap();
    assert_eq!(absent(&alias, Error::Unsafe), Err(Error::Unsafe));
    assert_eq!(
        absent(&path.join("child"), Error::Unsafe),
        Err(Error::Io(io::ErrorKind::NotADirectory))
    );
}

#[test]
fn creation_refuses_to_publish_if_either_temp_sidecar_survives_close() {
    for name in ["host-registry.db.tmp-wal", "host-registry.db.tmp-shm"] {
        let home = home();
        let secrets = Secrets::default();
        RESIDUAL.with(|r| *r.borrow_mut() = Some(home.path().join(name)));
        assert!(
            matches!(
                HostDb::create(home.path(), &secrets, &init(false)),
                Err(Error::Unsafe)
            ),
            "{name}"
        );
        assert!(!home.path().join(DB).exists());
        assert_eq!(
            fs::read(home.path().join(name)).unwrap(),
            b"uncheckpointed temp"
        );
        HostDb::create(home.path(), &secrets, &init(false)).unwrap();
        assert!(!home.path().join(name).exists());
    }
}

#[test]
fn committed_wal_recovers_after_real_process_death() {
    const CHILD: &str = "SAGE_HOSTDB_WAL_CRASH";
    if let Some(path) = std::env::var_os(CHILD) {
        let c = Connection::open(Path::new(&path)).unwrap();
        c.execute_batch(&format!("PRAGMA key=\"x'{}'\";", "12".repeat(32)))
            .unwrap();
        c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
            .unwrap();
        c.execute_batch(schema::DDL).unwrap();
        c.pragma_update(None, "user_version", 1).unwrap();
        c.execute(
            "INSERT INTO org_owners VALUES ('org','committed','owner')",
            [],
        )
        .unwrap();
        // SAFETY: leave a committed WAL and no live connection, without a close checkpoint.
        unsafe { libc::_exit(173) };
    }
    let home = home();
    let path = home.path().canonicalize().unwrap().join(DB);
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "hostdb::tests::committed_wal_recovers_after_real_process_death",
            "--nocapture",
        ])
        .env(CHILD, &path)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(173), "{result:?}");
    assert!(!home.path().join("host-registry.db-journal").exists());
    assert!(
        fs::metadata(home.path().join("host-registry.db-wal"))
            .unwrap()
            .len()
            > 0
    );
    let secrets = Secrets::default();
    secrets
        .put(
            &Name::parse("registry").unwrap(),
            &Value::new(vec![0x12; 32]).unwrap(),
        )
        .unwrap();
    let db = HostDb::open(home.path(), &secrets).unwrap();
    assert_eq!(
        db.connection
            .lock()
            .unwrap()
            .query_row("SELECT account FROM org_owners", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "committed"
    );
    drop(db);
    assert!(!home.path().join("host-registry.db-wal").exists());
}

#[test]
fn non_directory_home_and_fifo_database_are_refused_before_sqlite() {
    let home = home();
    let file = home.path().join("not-a-directory");
    fs::write(&file, b"x").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(matches!(checked_home(&file), Err(Error::Unsafe)));
    use std::os::unix::ffi::OsStrExt;
    let path = home.path().join(DB);
    let cpath = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: live NUL-terminated path; owner-only FIFO in an isolated temporary directory.
    assert_eq!(unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) }, 0);
    assert!(matches!(
        HostDb::open(home.path(), &Secrets::default()),
        Err(Error::Unsafe)
    ));
}

#[test]
fn existing_empty_sidecars_and_successful_close_checkpoint_are_supported() {
    let (home, secrets, db) = fixture();
    drop(db);
    for suffix in ["-wal", "-shm"] {
        fs::write(home.path().join(format!("{DB}{suffix}")), []).unwrap();
    }
    let db = HostDb::open(home.path(), &secrets).unwrap();
    db.connection
        .lock()
        .unwrap()
        .execute("INSERT INTO org_owners VALUES ('org','actor','owner')", [])
        .unwrap();
    assert!(
        fs::metadata(home.path().join("host-registry.db-wal"))
            .unwrap()
            .len()
            > 0
    );
    drop(db);
    assert!(!home.path().join("host-registry.db-wal").exists());
    let db = HostDb::open(home.path(), &secrets).unwrap();
    assert_eq!(
        db.connection
            .lock()
            .unwrap()
            .query_row("SELECT count(*) FROM org_owners", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn validation_descriptors_are_close_on_exec_and_sql_errors_discard_payloads() {
    use std::os::fd::AsRawFd;
    let home = home();
    let path = home.path().join("owned");
    fs::write(&path, b"x").unwrap();
    let file = checked_file(&path, false).unwrap().unwrap();
    // SAFETY: this retained File owns the fd and F_GETFD has no variadic argument.
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) };
    assert!(flags >= 0);
    assert_ne!(flags & libc::FD_CLOEXEC, 0);
    let input = rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
        Some("private-key-and-path".into()),
    );
    let error = Error::from(input);
    assert!(!format!("{error:?}").contains("private-key-and-path"));
}

#[test]
fn sqlite_refuses_a_valid_encrypted_symlink_swapped_after_validation() {
    let (home, secrets, db) = fixture();
    drop(db);
    let root = home.path().canonicalize().unwrap();
    let target = root.join("outside");
    fs::copy(root.join(DB), &target).unwrap();
    let before = fs::read(&target).unwrap();
    SWAP.with(|s| *s.borrow_mut() = Some((root.join(DB), target.clone())));
    assert!(HostDb::open(&root, &secrets).is_err());
    assert_eq!(fs::read(&target).unwrap(), before);
    assert!(checked_file(&root.join(DB), false).is_err());
}

#[test]
fn restrictive_umask_still_creates_private_usable_database_and_sidecars() {
    const CHILD: &str = "SAGE_HOSTDB_UMASK_PROBE";
    if std::env::var_os(CHILD).is_none() {
        let result=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","hostdb::tests::restrictive_umask_still_creates_private_usable_database_and_sidecars","--nocapture"]).env(CHILD,"1").output().unwrap();
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }
    let home = home();
    let secrets = Secrets::default();
    // SAFETY: isolated child test process; restore before assertions and temporary cleanup.
    let previous = unsafe { libc::umask(0o777) };
    let result = HostDb::create(home.path(), &secrets, &init(false));
    // SAFETY: restores the child's original process mask.
    unsafe { libc::umask(previous) };
    let db = result.unwrap();
    db.connection
        .lock()
        .unwrap()
        .query_row("SELECT count(*) FROM meta", [], |r| r.get::<_, i64>(0))
        .unwrap();
    for path in [DB, "host-registry.db-wal", "host-registry.db-shm"] {
        assert_eq!(
            fs::metadata(home.path().join(path)).unwrap().mode() & 0o7777,
            0o600
        );
    }
}

#[test]
fn wal_capability_refusal_covers_non_wal_vfs_results() {
    for mode in ["wal", "WAL", "WaL"] {
        assert!(require_wal(mode).is_ok());
    }
    for mode in ["", "delete", "memory", "wal "] {
        assert_eq!(require_wal(mode), Err(Error::Unsafe));
    }
}
