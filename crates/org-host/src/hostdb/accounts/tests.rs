use super::*;
use crate::hostdb::{
    tests::{admin_fixture, fixture},
    *,
};

thread_local! {
    static ROW_HOOK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}
pub(in crate::hostdb) fn after_row() {
    let hook = ROW_HOOK.with(|cell| cell.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

fn first() -> Account {
    Account {
        account_id: AccountId::parse(admin_fixture::ACCOUNT).unwrap(),
        login: "Sage".into(),
        admin: true,
        revoked_at_ms: None,
        password_changed_at_ms: 123,
        links: vec![],
    }
}

#[test]
fn credentials_and_authority_read_exact_account_and_opaque_phc() {
    let (_home, _secrets, db) = fixture();
    let credentials = db
        .account_credentials(&LoginKey::new("sage").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(credentials.account, first());
    assert_eq!(credentials.verifier.expose(), admin_fixture::PHC);
    assert_eq!(
        db.account_authority(&first().account_id).unwrap(),
        Some(first())
    );
    assert!(db
        .account_credentials(&LoginKey::new("Sage").unwrap())
        .unwrap()
        .is_some());
    // The checked value normalizes input; SQLite key matching remains BINARY.
    assert_eq!(
        db.connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM accounts WHERE login_key=?1",
                ["Sage"],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    assert!(db
        .account_credentials(&LoginKey::new("missing").unwrap())
        .unwrap()
        .is_none());
    assert!(db
        .account_authority(&AccountId::parse("01J00000000000000000000009").unwrap())
        .unwrap()
        .is_none());
}

#[test]
fn reads_bind_keys_and_ids_and_keep_each_accounts_links_separate() {
    let (_home, _secrets, db) = fixture();
    let second = "01J00000000000000000000004";
    let key = "sage' or 1=1--"; // Stored keys are already normalized.
    {
        let c = db.connection.lock().unwrap();
        c.execute(
            "INSERT INTO accounts VALUES (?1,?2,'Other',?3,0,124,124,NULL)",
            (
                second,
                key,
                &admin_fixture::PHC.replace("$AAAAAAAA", "$BAAAAAAA"),
            ),
        )
        .unwrap();
        for (account, org, member) in [
            (first().account_id.as_str(), "ORG-B", "MEMBER-B"),
            (first().account_id.as_str(), "ORG-A", "MEMBER-A"),
            (second, "ORG-C", "MEMBER-C"),
        ] {
            c.execute(
                "INSERT INTO account_links VALUES (?1,?2,?3)",
                (account, org, member),
            )
            .unwrap();
        }
    }
    let expected = Account {
        account_id: AccountId::parse(second).unwrap(),
        login: "Other".into(),
        admin: false,
        revoked_at_ms: None,
        password_changed_at_ms: 124,
        links: vec![Link {
            org_id: "ORG-C".into(),
            member_id: "MEMBER-C".into(),
        }],
    };
    let credentials = db
        .account_credentials(&LoginKey::new(key).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(credentials.account, expected);
    assert_eq!(
        credentials.verifier.expose(),
        &admin_fixture::PHC.replace("$AAAAAAAA", "$BAAAAAAA")
    );
    assert_eq!(
        db.account_authority(&AccountId::parse(second).unwrap())
            .unwrap(),
        Some(expected)
    );
    assert!(AccountId::parse("' OR 1=1--").is_err());
    assert!(db
        .account_credentials(&LoginKey::new("' OR 1=1--").unwrap())
        .unwrap()
        .is_none());
    for links in [
        db.account_credentials(&LoginKey::new("sage").unwrap())
            .unwrap()
            .unwrap()
            .account
            .links,
        db.account_authority(&first().account_id)
            .unwrap()
            .unwrap()
            .links,
    ] {
        assert_eq!(
            links,
            vec![
                Link {
                    org_id: "ORG-A".into(),
                    member_id: "MEMBER-A".into()
                },
                Link {
                    org_id: "ORG-B".into(),
                    member_id: "MEMBER-B".into()
                },
            ]
        );
    }
}

#[test]
fn every_read_observes_demotion_revocation_display_and_link_changes() {
    let (_home, _secrets, db) = fixture();
    assert_eq!(
        db.account_authority(&first().account_id).unwrap(),
        Some(first())
    );
    {
        let c = db.connection.lock().unwrap();
        c.execute_batch("UPDATE accounts SET login='Changed',admin=0,revoked_at_ms=456; INSERT INTO account_links SELECT account_id,'ORG','MEMBER' FROM accounts").unwrap();
    }
    let expected = Account {
        login: "Changed".into(),
        admin: false,
        revoked_at_ms: Some(456),
        links: vec![Link {
            org_id: "ORG".into(),
            member_id: "MEMBER".into(),
        }],
        ..first()
    };
    assert_eq!(
        db.account_authority(&first().account_id).unwrap(),
        Some(expected)
    );
    let credentials = db
        .account_credentials(&LoginKey::new("sage").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(credentials.account.login, "Changed");
    assert!(!credentials.account.admin);
    assert_eq!(credentials.account.revoked_at_ms, Some(456));
    assert_eq!(credentials.account.links[0].member_id, "MEMBER");
    assert_eq!(credentials.verifier.expose(), admin_fixture::PHC);
    db.connection.lock().unwrap().execute_batch("UPDATE accounts SET login='Sage',admin=1,revoked_at_ms=NULL; DELETE FROM account_links").unwrap();
    assert_eq!(
        db.account_authority(&first().account_id).unwrap(),
        Some(first())
    );
    assert_eq!(
        db.account_credentials(&LoginKey::new("sage").unwrap())
            .unwrap()
            .unwrap()
            .account,
        first()
    );
}

#[test]
fn row_and_links_share_one_snapshot_even_if_another_writer_commits_between_them() {
    for credentials in [false, true] {
        let (home, _secrets, db) = fixture();
        let writer = connect(&home.path().canonicalize().unwrap().join(DB)).unwrap();
        ROW_HOOK.with(|cell| *cell.borrow_mut()=Some(Box::new(move || {
            writer.execute_batch("BEGIN IMMEDIATE; UPDATE accounts SET login='After',admin=0,revoked_at_ms=789; INSERT INTO account_links SELECT account_id,'ORG','MEMBER' FROM accounts; COMMIT;").unwrap();
        })));
        let before = if credentials {
            db.account_credentials(&LoginKey::new("sage").unwrap())
                .unwrap()
                .unwrap()
                .account
        } else {
            db.account_authority(&first().account_id).unwrap().unwrap()
        };
        assert_eq!(before, first());
        let after = db.account_authority(&first().account_id).unwrap().unwrap();
        assert_eq!(after.login, "After");
        assert!(!after.admin);
        assert_eq!(after.revoked_at_ms, Some(789));
        assert_eq!(
            after.links,
            vec![Link {
                org_id: "ORG".into(),
                member_id: "MEMBER".into()
            }]
        );
    }
}

#[test]
fn read_errors_propagate_without_sql_or_secret_payloads() {
    for table in ["account_links", "accounts", "unreadable_link"] {
        let (_home, _secrets, db) = fixture();
        let c = db.connection.lock().unwrap();
        if table == "unreadable_link" {
            c.execute_batch("DROP TABLE account_links; CREATE TABLE account_links (account_id TEXT, org_id BLOB, member_id BLOB); INSERT INTO account_links SELECT account_id,x'00',x'01' FROM accounts;").unwrap();
        } else {
            c.execute_batch(&format!("DROP TABLE {table}")).unwrap();
        }
        drop(c);
        let errors = [
            db.account_credentials(&LoginKey::new("sage").unwrap())
                .err()
                .unwrap(),
            db.account_authority(&first().account_id).err().unwrap(),
        ];
        for error in errors {
            assert!(matches!(error, Error::Sql(_)));
            let shown = format!("{error:?}");
            assert!(!shown.contains(admin_fixture::PHC));
            assert!(!shown.contains(table));
            assert!(!shown.contains("sage"));
        }
    }
}

#[test]
fn reads_do_not_write_accounts_links_lockout_or_clock_and_survive_reopen() {
    let (home, secrets, db) = fixture();
    let before = db.connection.lock().unwrap().total_changes();
    for _ in 0..2 {
        assert_eq!(
            db.account_credentials(&LoginKey::new("sage").unwrap())
                .unwrap()
                .unwrap()
                .account,
            first()
        );
        assert_eq!(
            db.account_authority(&first().account_id).unwrap(),
            Some(first())
        );
        assert!(db
            .account_credentials(&LoginKey::new("missing").unwrap())
            .unwrap()
            .is_none());
        assert!(db
            .account_authority(&AccountId::parse("01J00000000000000000000009").unwrap())
            .unwrap()
            .is_none());
    }
    assert_eq!(db.connection.lock().unwrap().total_changes(), before);
    drop(db);
    let db = HostDb::open(home.path(), &secrets).unwrap();
    assert_eq!(
        db.account_credentials(&LoginKey::new("sage").unwrap())
            .unwrap()
            .unwrap()
            .account,
        first()
    );
    let c = db.connection.lock().unwrap();
    assert_eq!(
        c.query_row(
            "SELECT value FROM meta WHERE key='clock_high_ms'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "123"
    );
    for table in ["account_links", "login_lockout"] {
        assert_eq!(
            c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}

#[test]
fn poisoned_registry_mutex_refuses_both_read_paths() {
    let (_home, _secrets, db) = fixture();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = db.connection.lock().unwrap();
        panic!("poison registry mutex");
    }))
    .is_err());
    assert!(matches!(
        db.account_credentials(&LoginKey::new("sage").unwrap()),
        Err(Error::Unsafe)
    ));
    assert!(matches!(
        db.account_authority(&first().account_id),
        Err(Error::Unsafe)
    ));
}

#[test]
fn read_only_accessors_expose_exact_snapshot_fields() {
    let (_home, _secrets, db) = fixture();
    db.connection.lock().unwrap().execute_batch("UPDATE accounts SET revoked_at_ms=456; INSERT INTO account_links SELECT account_id,'ORG','MEMBER' FROM accounts;").unwrap();
    let credentials = db
        .account_credentials(&LoginKey::new("SAGE").unwrap())
        .unwrap()
        .unwrap();
    let account = credentials.account();
    assert_eq!(account.account_id(), admin_fixture::ACCOUNT.to_string());
    assert_eq!(account.login(), "Sage");
    assert!(account.admin());
    assert_eq!(account.revoked_at_ms(), Some(456));
    assert_eq!(account.links().len(), 1);
    assert_eq!(account.links()[0].org_id(), "ORG");
    assert_eq!(account.links()[0].member_id(), "MEMBER");
    assert_eq!(credentials.verifier().expose(), admin_fixture::PHC);
}

#[test]
fn malformed_stored_identity_and_phc_refuse_without_repair_through_reopen() {
    for (column, bad) in [
        ("account_id", "I".repeat(26)),
        ("password_phc", "sensitive invalid PHC".into()),
    ] {
        let (home, secrets, db) = fixture();
        db.connection
            .lock()
            .unwrap()
            .execute(&format!("UPDATE accounts SET {column}=?1"), [&bad])
            .unwrap();
        let before = db.connection.lock().unwrap().total_changes();
        assert!(matches!(
            db.account_credentials(&LoginKey::new("sage").unwrap()),
            Err(Error::Unsafe)
        ));
        assert_eq!(db.connection.lock().unwrap().total_changes(), before);
        drop(db);
        let db = HostDb::open(home.path(), &secrets).unwrap();
        assert!(matches!(
            db.account_credentials(&LoginKey::new("sage").unwrap()),
            Err(Error::Unsafe)
        ));
        assert_eq!(
            db.connection
                .lock()
                .unwrap()
                .query_row(&format!("SELECT {column} FROM accounts"), [], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            bad
        );
    }
}
