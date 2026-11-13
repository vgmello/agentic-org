use super::*;

// Opaque, structurally valid PHC fixture; the registry never hashes a password.
pub(super) const PHC: &str = "$argon2id$v=19$m=65536,t=3,p=1$MDEyMzQ1Njc4OWFiY2RlZg$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

pub(super) fn assert_first_account(db: &HostDb) {
    let c = db.connection.lock().unwrap();
    let row = c
        .query_row(
            "SELECT account_id,login_key,login,password_phc,admin,created_at_ms,password_changed_at_ms,revoked_at_ms FROM accounts",
            [],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, i64>(4)?, r.get::<_, i64>(5)?, r.get::<_, i64>(6)?, r.get::<_, Option<i64>>(7)?)),
        )
        .unwrap();
    assert_eq!(
        row,
        (
            init(false).bootstrap.account,
            "sage".into(),
            "Sage".into(),
            PHC.into(),
            1,
            123,
            123,
            None
        )
    );
    assert_eq!(
        c.query_row("SELECT count(*) FROM accounts", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn created_registry_publishes_its_first_administrator_and_reopens() {
    let (home, secrets, db) = fixture();
    assert_first_account(&db);
    drop(db);
    assert_first_account(&HostDb::open(home.path(), &secrets).unwrap());
}

#[test]
fn schema_has_pinned_columns_storage_flags_link_index_and_foreign_key() {
    let (_home, _secrets, db) = fixture();
    let c = db.connection.lock().unwrap();
    for (table, columns) in [
        (
            "accounts",
            vec![
                ("account_id", "TEXT"),
                ("login_key", "TEXT"),
                ("login", "TEXT"),
                ("password_phc", "TEXT"),
                ("admin", "INTEGER"),
                ("created_at_ms", "INTEGER"),
                ("password_changed_at_ms", "INTEGER"),
                ("revoked_at_ms", "INTEGER"),
            ],
        ),
        (
            "account_links",
            vec![
                ("account_id", "TEXT"),
                ("org_id", "TEXT"),
                ("member_id", "TEXT"),
            ],
        ),
        (
            "login_lockout",
            vec![
                ("socket", "TEXT"),
                ("login_key", "TEXT"),
                ("failures", "INTEGER"),
                ("locked_until_ms", "INTEGER"),
                ("last_failure_ms", "INTEGER"),
            ],
        ),
    ] {
        let flags = c
            .query_row(
                "SELECT strict,wr FROM pragma_table_list WHERE name=?1",
                [table],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
            )
            .unwrap();
        assert_eq!(flags, (1, 1), "{table}");
        let actual: Vec<_> = c
            .prepare("SELECT name,type FROM pragma_table_info(?1) ORDER BY cid")
            .unwrap()
            .query_map([table], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            actual,
            columns
                .into_iter()
                .map(|(n, t)| (n.to_string(), t.to_string()))
                .collect::<Vec<_>>(),
            "{table}"
        );
    }
    for (table, keys) in [
        ("accounts", vec!["account_id"]),
        ("account_links", vec!["account_id", "org_id"]),
        ("login_lockout", vec!["socket", "login_key"]),
    ] {
        let actual: Vec<String> = c
            .prepare("SELECT name FROM pragma_table_info(?1) WHERE pk>0 ORDER BY pk")
            .unwrap()
            .query_map([table], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            actual,
            keys.into_iter().map(String::from).collect::<Vec<_>>(),
            "{table}"
        );
    }
    assert_eq!(
        c.query_row(
            "SELECT name FROM pragma_index_info('account_links_org')",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "org_id"
    );
    let fk = c.query_row("SELECT \"table\",\"from\",\"to\",on_delete FROM pragma_foreign_key_list('account_links')", [], |r| Ok((r.get::<_, String>(0)?,r.get::<_, String>(1)?,r.get::<_, String>(2)?,r.get::<_, String>(3)?))).unwrap();
    assert_eq!(
        fk,
        (
            "accounts".into(),
            "account_id".into(),
            "account_id".into(),
            "NO ACTION".into()
        )
    );
    assert_eq!(scalar(&c, "user_version"), 1);
}

fn constraint(c: &Connection, sql: &str) {
    let error = Error::from(c.execute_batch(sql).unwrap_err());
    assert_eq!(
        error,
        Error::Sql(rusqlite::ErrorCode::ConstraintViolation),
        "{sql}"
    );
    assert!(!format!("{error:?}").contains(PHC));
}

#[test]
fn account_constraints_accept_boundaries_and_reject_invalid_or_duplicate_rows() {
    let (_home, _secrets, db) = fixture();
    let c = db.connection.lock().unwrap();
    constraint(&c, "INSERT INTO accounts SELECT account_id,'different',login,password_phc,admin,created_at_ms,password_changed_at_ms,NULL FROM accounts");
    for n in [25, 27] {
        let invalid_id = "A".repeat(n);
        let error = c
            .execute("UPDATE accounts SET account_id=?1", [invalid_id])
            .unwrap_err();
        assert_eq!(
            Error::from(error),
            Error::Sql(rusqlite::ErrorCode::ConstraintViolation)
        );
    }
    // A different 26-byte identity is valid before it has any links.
    c.execute("UPDATE accounts SET account_id=?1", ["B".repeat(26)])
        .unwrap();
    c.execute(
        "UPDATE accounts SET account_id=?1",
        [init(false).bootstrap.account],
    )
    .unwrap();
    for value in [-1, 2] {
        constraint(&c, &format!("UPDATE accounts SET admin={value}"));
    }
    for value in [0, 1] {
        assert_eq!(
            c.execute("UPDATE accounts SET admin=?1", [value]).unwrap(),
            1
        );
    }
    constraint(&c, "UPDATE accounts SET created_at_ms='not an integer'");
    for column in [
        "account_id",
        "login_key",
        "login",
        "password_phc",
        "admin",
        "created_at_ms",
        "password_changed_at_ms",
    ] {
        constraint(&c, &format!("UPDATE accounts SET {column}=NULL"));
    }
    constraint(&c, "INSERT INTO accounts SELECT '01J00000000000000000000004',login_key,login,password_phc,admin,created_at_ms,password_changed_at_ms,NULL FROM accounts");
    c.execute("UPDATE accounts SET revoked_at_ms=456", [])
        .unwrap();
    c.execute("UPDATE accounts SET revoked_at_ms=NULL", [])
        .unwrap();
    drop(c);
    assert_first_account(&db);
}

#[test]
fn link_and_lockout_keys_constraints_and_socket_isolation_are_enforced() {
    let (_home, _secrets, db) = fixture();
    let c = db.connection.lock().unwrap();
    let account = init(false).bootstrap.account;
    c.execute(
        "INSERT INTO account_links VALUES (?1,'ORG','MEMBER')",
        [&account],
    )
    .unwrap();
    constraint(
        &c,
        "INSERT INTO account_links SELECT account_id,org_id,'different' FROM account_links",
    );
    constraint(
        &c,
        "INSERT INTO account_links VALUES ('01J00000000000000000000004','OTHER','MEMBER')",
    );
    constraint(&c, "DELETE FROM accounts");
    constraint(
        &c,
        "UPDATE accounts SET account_id='01J00000000000000000000004'",
    );
    constraint(&c, "UPDATE account_links SET member_id=x'01'");
    for column in ["account_id", "org_id", "member_id"] {
        constraint(&c, &format!("UPDATE account_links SET {column}=NULL"));
    }
    c.execute(
        "INSERT INTO account_links VALUES (?1,'OTHER','MEMBER2')",
        [&account],
    )
    .unwrap();
    c.execute_batch("INSERT INTO accounts SELECT '01J00000000000000000000004','other','Other',password_phc,0,124,124,NULL FROM accounts LIMIT 1; INSERT INTO account_links VALUES ('01J00000000000000000000004','ORG','MEMBER3');").unwrap();
    assert_eq!(
        c.query_row("SELECT count(*) FROM account_links", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        3
    );
    for socket in ["control", "local"] {
        c.execute(
            "INSERT INTO login_lockout VALUES (?1,'sage',0,0,123)",
            [socket],
        )
        .unwrap();
    }
    assert_eq!(
        c.query_row("SELECT count(*) FROM login_lockout", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
    c.execute(
        "INSERT INTO login_lockout VALUES ('control','other',0,0,123)",
        [],
    )
    .unwrap();
    constraint(
        &c,
        "INSERT INTO login_lockout VALUES ('control','sage',1,1,124)",
    );
    for socket in ["", "CONTROL", "other"] {
        let error = c
            .execute(
                "UPDATE login_lockout SET socket=?1 WHERE socket='control'",
                [socket],
            )
            .unwrap_err();
        assert_eq!(
            Error::from(error),
            Error::Sql(rusqlite::ErrorCode::ConstraintViolation)
        );
    }
    constraint(&c, "UPDATE login_lockout SET failures='not an integer'");
    for column in [
        "socket",
        "login_key",
        "failures",
        "locked_until_ms",
        "last_failure_ms",
    ] {
        constraint(&c, &format!("UPDATE login_lockout SET {column}=NULL"));
    }
}

#[test]
fn new_tables_commit_rollback_update_delete_replace_and_reopen() {
    let (home, secrets, db) = fixture();
    {
        let mut c = db.connection.lock().unwrap();
        let insert_account = "INSERT INTO accounts SELECT '01J00000000000000000000004','other','Other',password_phc,0,124,124,NULL FROM accounts LIMIT 1";
        let insert_link =
            "INSERT INTO account_links VALUES ('01J00000000000000000000004','ORG','MEMBER')";
        let insert_lockout = "INSERT INTO login_lockout VALUES ('control','other',5,0,124)";
        {
            let tx = c
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            tx.execute_batch(insert_account).unwrap();
            tx.execute_batch(insert_link).unwrap();
            tx.execute_batch(insert_lockout).unwrap();
            tx.rollback().unwrap();
        }
        for (table, count) in [("accounts", 1), ("account_links", 0), ("login_lockout", 0)] {
            assert_eq!(
                c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                count
            );
        }
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        tx.execute_batch(insert_account).unwrap();
        tx.execute_batch(insert_link).unwrap();
        tx.execute_batch(insert_lockout).unwrap();
        assert_eq!(
            tx.execute(
                "UPDATE accounts SET login='Changed',revoked_at_ms=125 WHERE login_key='other'",
                []
            )
            .unwrap(),
            1
        );
        assert_eq!(
            tx.execute(
                "UPDATE account_links SET member_id='MEMBER2' WHERE org_id='ORG'",
                []
            )
            .unwrap(),
            1
        );
        tx.execute_batch("INSERT OR REPLACE INTO login_lockout VALUES ('control','other',6,1000,126); INSERT INTO login_lockout VALUES ('local','other',1,0,127);").unwrap();
        assert_eq!(
            tx.execute("DELETE FROM account_links WHERE org_id='ORG'", [])
                .unwrap(),
            1
        );
        assert_eq!(
            tx.execute("DELETE FROM accounts WHERE login_key='other'", [])
                .unwrap(),
            1
        );
        assert_eq!(
            tx.execute("DELETE FROM login_lockout WHERE socket='local'", [])
                .unwrap(),
            1
        );
        tx.commit().unwrap();
    }
    drop(db);
    let db = HostDb::open(home.path(), &secrets).unwrap();
    assert_first_account(&db);
    let c = db.connection.lock().unwrap();
    assert_eq!(
        c.query_row("SELECT count(*) FROM account_links", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        c.query_row(
            "SELECT socket,login_key,failures,locked_until_ms,last_failure_ms FROM login_lockout",
            [],
            |r| Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?
            ))
        )
        .unwrap(),
        ("control".into(), "other".into(), 6, 1000, 126)
    );
}

#[test]
fn mismatched_first_account_refuses_before_any_registry_or_secret_write() {
    for change_bootstrap in [false, true] {
        let home = home();
        let secrets = Secrets::default();
        let mut value = init(true);
        if change_bootstrap {
            value.bootstrap.account.push('x');
        } else {
            value.first_account.account_id.push('x');
        }
        assert!(matches!(
            HostDb::create(home.path(), &secrets, &value),
            Err(Error::Unsafe)
        ));
        assert_eq!(fs::read_dir(home.path()).unwrap().count(), 0);
        assert!(secrets.0.lock().unwrap().is_empty());
    }
}

#[test]
fn failed_first_account_insert_rolls_back_schema_metadata_and_tls_and_retries() {
    for length in [25, 27] {
        let home = home();
        let secrets = Secrets::default();
        let mut value = init(true);
        value.first_account.account_id = "A".repeat(length);
        value.bootstrap.account = value.first_account.account_id.clone();
        let error = HostDb::create(home.path(), &secrets, &value).err().unwrap();
        assert_eq!(error, Error::Sql(rusqlite::ErrorCode::ConstraintViolation));
        assert!(!format!("{error:?}").contains(PHC));
        assert!(!home.path().join(DB).exists());
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
        drop(c);
        let db = HostDb::create(home.path(), &secrets, &init(false)).unwrap();
        assert_first_account(&db);
    }
}

#[test]
fn repeat_create_never_replaces_the_original_account_or_registry() {
    let (home, secrets, db) = fixture();
    drop(db);
    let before = bytes(home.path());
    let mut value = init(false);
    value.first_account.login_key = "replacement".into();
    value.first_account.login = "Replacement".into();
    value.first_account.verifier = journal::Verifier::new("private replacement PHC".into());
    assert!(matches!(
        HostDb::create(home.path(), &secrets, &value),
        Err(Error::Exists)
    ));
    assert_eq!(bytes(home.path()), before);
    assert_first_account(&HostDb::open(home.path(), &secrets).unwrap());
    for path in [DB, "host-registry.db-wal", "host-registry.db-shm"] {
        if let Ok(body) = fs::read(home.path().join(path)) {
            assert!(!body.windows(PHC.len()).any(|w| w == PHC.as_bytes()));
        }
    }
}
