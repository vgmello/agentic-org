//! Local-only CA persistence. Excluded from replication and acceptance capture.
use super::Store;
use crate::{hq_creation::LocalRecord, OfficeError};
use rusqlite::{params, OptionalExtension};
use zeroize::Zeroizing;
fn invalid() -> OfficeError {
    OfficeError::Invalid("HQ provisioning requires recovery")
}
pub(crate) fn create_table(tx: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch("CREATE TABLE local_hq_creation (
        singleton INTEGER PRIMARY KEY CHECK(singleton=1),
        version INTEGER NOT NULL CHECK(version=1),
        organization_id TEXT NOT NULL, operation_id TEXT NOT NULL,
        profile INTEGER NOT NULL CHECK(profile=1),
        public_key BLOB NOT NULL CHECK(length(public_key)=65),
        escrow_package BLOB NOT NULL CHECK(length(escrow_package) BETWEEN 1 AND 4475),
        private_pkcs8 BLOB NOT NULL CHECK(length(private_pkcs8) BETWEEN 1 AND 4096),
        complete INTEGER NOT NULL CHECK(complete=1)
    ) STRICT;
    CREATE TRIGGER local_hq_creation_update BEFORE UPDATE ON local_hq_creation BEGIN SELECT RAISE(ABORT,'HQ creation material is immutable'); END;
    CREATE TRIGGER local_hq_creation_delete BEFORE DELETE ON local_hq_creation BEGIN SELECT RAISE(ABORT,'HQ creation material is retained'); END;
    CREATE TRIGGER local_hq_creation_replace BEFORE INSERT ON local_hq_creation WHEN EXISTS(SELECT 1 FROM local_hq_creation) BEGIN SELECT RAISE(ABORT,'HQ creation material already exists'); END;")
}
pub(crate) fn read(db: &rusqlite::Connection) -> Result<Option<LocalRecord>, OfficeError> {
    let raw=db.query_row("SELECT version,organization_id,operation_id,profile,public_key,escrow_package,private_pkcs8,complete FROM local_hq_creation WHERE singleton=1",[],|r| {
        let key=r.get_ref(4)?.as_blob()?;let package=r.get_ref(5)?.as_blob()?;let private=r.get_ref(6)?.as_blob()?;
        if key.len()!=65||package.is_empty()||package.len()>4475||private.is_empty()||private.len()>4096 {return Err(rusqlite::Error::InvalidQuery);}
        Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,i64>(3)?,key.try_into().map_err(|_|rusqlite::Error::InvalidQuery)?,package.to_vec(),Zeroizing::new(private.to_vec()),r.get::<_,i64>(7)?))
    }).optional().map_err(OfficeError::storage)?;
    let Some((version, org, operation, profile, key, package, private, complete)) = raw else {
        return Ok(None);
    };
    if version != 1 || profile != 1 || complete != 1 {
        return Err(invalid());
    }
    LocalRecord::new(org, operation, key, package, private).map(Some)
}
fn same(a: &LocalRecord, b: &LocalRecord) -> bool {
    a.organization_id == b.organization_id
        && a.operation_id == b.operation_id
        && a.public_key == b.public_key
        && a.package == b.package
        && a.private_der == b.private_der
}
pub(crate) fn read_public(
    db: &rusqlite::Connection,
) -> Result<Option<crate::hq_creation::PublicRecord>, OfficeError> {
    let record = db.query_row("SELECT version,organization_id,operation_id,profile,public_key,escrow_package,complete FROM local_hq_creation WHERE singleton=1", [], |r| {
        let key = r.get_ref(4)?.as_blob()?;
        let package = r.get_ref(5)?.as_blob()?;
        if r.get::<_,i64>(0)? != 1 || r.get::<_,i64>(3)? != 1 || r.get::<_,i64>(6)? != 1
            || key.len() != 65 || key[0] != 4 || package.is_empty() || package.len() > 4475 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        Ok(crate::hq_creation::PublicRecord {
            version: 1,
            organization_id: r.get(1)?,
            operation_id: r.get(2)?,
            public_key: key.try_into().map_err(|_| rusqlite::Error::InvalidQuery)?,
            package: package.to_vec(),
        })
    }).optional().map_err(OfficeError::storage)?;
    Ok(record)
}
pub(crate) fn initialize(store: &Store, record: LocalRecord) -> Result<(), OfficeError> {
    // Generation/sealing occurred before this retryable callback. Every rerun
    // observes the exact same frozen material and original operation binding.
    store.write(move |db| {
        let binding: Option<String> = db
            .query_row(
                "SELECT value FROM meta WHERE key='hq_creation_operation'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(OfficeError::storage)?;
        if binding.as_deref() != Some(&record.operation_id) {
            return Err(invalid());
        }
        if let Some(prior) = read(db)? {
            return if same(&prior, &record) {
                Ok(((), None))
            } else {
                Err(invalid())
            };
        }
        let journaled: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM selected_journal_cursor)",
                [],
                |r| r.get(0),
            )
            .map_err(OfficeError::storage)?;
        if journaled {
            return Err(invalid());
        }
        db.execute(
            "INSERT INTO local_hq_creation VALUES(1,1,?1,?2,1,?3,?4,?5,1)",
            params![
                record.organization_id,
                record.operation_id,
                record.public_key.as_slice(),
                record.package,
                record.local_der()
            ],
        )
        .map_err(OfficeError::storage)?;
        Ok(((), None))
    })
}

/// Initial creation is authorized by its immutable intent and protected caller.
/// A pre-schema crash may leave an empty encrypted database. Only an entirely
/// empty version-0 database is finishable; any foreign schema is retained/refused.
fn open_preparing_store(
    path: &std::path::Path,
    key: &crate::Key,
    schema: &super::Schema,
    instance: Option<&str>,
    office: &str,
    org: Option<&str>,
) -> Result<Store, OfficeError> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            if !meta.is_file() || meta.file_type().is_symlink() {
                return Err(invalid());
            }
            Store::inspect(path, key, |db| {
                let version: i64 = db
                    .pragma_query_value(None, "user_version", |r| r.get(0))
                    .map_err(OfficeError::storage)?;
                if version == 0 {
                    let count: i64 = db
                        .query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))
                        .map_err(OfficeError::storage)?;
                    return if count == 0 { Ok(()) } else { Err(invalid()) };
                }
                if version != schema.version {
                    return Err(invalid());
                }
                for (name, expected) in [
                    ("office_id", Some(office)),
                    ("organization_id", org),
                    ("instance_id", instance),
                ] {
                    let value: Option<String> = db
                        .query_row("SELECT value FROM meta WHERE key=?1", [name], |r| r.get(0))
                        .optional()
                        .map_err(OfficeError::storage)?;
                    if value
                        .as_deref()
                        .is_some_and(|value| Some(value) != expected)
                    {
                        return Err(invalid());
                    }
                    if name == "instance_id" && expected.is_some() && value.is_none() {
                        return Err(invalid());
                    }
                }
                Ok(())
            })?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(OfficeError::storage(e)),
    }
    Store::open_bound(path, key, super::Opening::New, schema, instance)
}
pub(crate) fn open_preparing(
    folder: &std::path::Path,
    key: &crate::Key,
    instance: &str,
    office: &str,
    org: &str,
) -> Result<(Store, Store), OfficeError> {
    // The initial local CA transaction happens before organization genesis.
    let local = open_preparing_store(
        &folder.join("office.db"),
        key,
        &crate::office::OFFICE_SCHEMA,
        Some(instance),
        office,
        None,
    )?;
    let organization = open_preparing_store(
        &folder.join("org.db"),
        key,
        &super::org::SCHEMA,
        None,
        office,
        Some(org),
    )?;
    Ok((organization, local))
}
pub(crate) fn initialize_bound(
    store: &Store,
    binding: &crate::hq_creation::Binding,
    office: &str,
    record: LocalRecord,
) -> Result<(), OfficeError> {
    let binding = binding.clone();
    let office = office.to_owned();
    store.write(move|db| {
        let current=read(db)?;
        for (name,expected) in [("office_id",office.as_str()),("hq_creation_operation",binding.operation_id.as_str()),("hq_creation_owner",binding.owner_member_id.as_str())] {
            let value:Option<String>=db.query_row("SELECT value FROM meta WHERE key=?1",[name],|r|r.get(0)).optional().map_err(OfficeError::storage)?;
            match value {
                Some(value) if value!=expected=>return Err(invalid()),
                None if current.is_some()=>return Err(invalid()),
                None=> {db.execute("INSERT INTO meta VALUES(?1,?2)",(name,expected)).map_err(OfficeError::storage)?;}
                _=>{}
            }
        }
        if let Some(prior)=current {return if same(&prior,&record) {Ok(((),None))} else {Err(invalid())};}
        let occupied:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM selected_journal_cursor) OR EXISTS(SELECT 1 FROM work_feed)",[],|r|r.get(0)).map_err(OfficeError::storage)?;
        if occupied {return Err(invalid());}
        db.execute("INSERT INTO local_hq_creation VALUES(1,1,?1,?2,1,?3,?4,?5,1)",params![record.organization_id,record.operation_id,record.public_key.as_slice(),record.package,record.local_der()]).map_err(OfficeError::storage)?;
        Ok(((),None))
    })
}

pub(crate) fn verify_binding(
    store: &Store,
    binding: &crate::hq_creation::Binding,
) -> Result<(), OfficeError> {
    store.read(|db| {
        for (name, expected) in [
            ("hq_creation_operation", &binding.operation_id),
            ("hq_creation_owner", &binding.owner_member_id),
        ] {
            let value: Option<String> = db
                .query_row("SELECT value FROM meta WHERE key=?1", [name], |r| r.get(0))
                .optional()
                .map_err(OfficeError::storage)?;
            if value.as_ref() != Some(expected) {
                return Err(invalid());
            }
        }
        if read(db)?.is_none() {
            return Err(invalid());
        }
        Ok(())
    })
}

#[cfg(test)]
mod owner_tests {
    use super::*;
    use crate::{AcceptanceConfig, AcceptanceIdentity, OwnerAccount, OwnerAuthority};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    struct Owner {
        live: AtomicBool,
        account: &'static str,
    }
    impl OwnerAuthority for Owner {
        fn verify_live(&self, _: &AcceptanceIdentity) -> Result<OwnerAccount, OfficeError> {
            if !self.live.load(Ordering::SeqCst) {
                return Err(invalid());
            }
            Ok(OwnerAccount {
                account_id: self.account.into(),
                member_id: "security-member".into(),
            })
        }
    }
    fn retained_files(
        root: &std::path::Path,
    ) -> std::collections::BTreeMap<std::path::PathBuf, (Vec<u8>, std::time::SystemTime)> {
        let mut files = std::collections::BTreeMap::new();
        for entry in std::fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                files.extend(retained_files(&path));
            } else {
                files.insert(
                    path.clone(),
                    (
                        std::fs::read(&path).unwrap(),
                        std::fs::metadata(path).unwrap().modified().unwrap(),
                    ),
                );
            }
        }
        files
    }
    #[test]
    fn original_owner_binding_is_readonly_at_max_and_live_account_checks_survive() {
        let root = tempfile::tempdir().unwrap();
        let key = crate::Key::from_bytes([19; 32]);
        let store = Store::open(
            &root.path().join("office.db"),
            &key,
            super::super::Opening::New,
            &crate::office::OFFICE_SCHEMA,
        )
        .unwrap();
        store
            .write(|db| {
                db.execute_batch(
                    "INSERT INTO meta VALUES('office_id','office'),('instance_id','instance')",
                )
                .map_err(OfficeError::storage)?;
                Ok(((), None))
            })
            .unwrap();
        store
            .install_acceptance(AcceptanceConfig {
                authority: None,
                identity: AcceptanceIdentity {
                    instance_id: "instance".into(),
                    organization_id: "org".into(),
                    office_id: "office".into(),
                    machine_id: "machine".into(),
                    writer_epoch: 1,
                    journal_incarnation: "journal".into(),
                },
                public_root: root.path().join("public"),
                protected_root: root.path().join("protected"),
            })
            .unwrap();
        let owner = Arc::new(Owner {
            live: AtomicBool::new(true),
            account: "original",
        });
        crate::organization::admission::bind_owner(&store, owner.clone()).unwrap();
        store
            .selected_transaction(|db, _| {
                db.execute(
                    "INSERT INTO selected_artifacts VALUES('protected','original-receipt',?1,1)",
                    [b"original".as_slice()],
                )
                .map_err(OfficeError::storage)?;
                Ok(())
            })
            .unwrap();
        let read = || {
            store
                .selected_read(|db, _| {
                    db.query_row("SELECT ordinal FROM selected_journal_cursor", [], |r| {
                        r.get::<_, i64>(0)
                    })
                    .map_err(OfficeError::storage)
                })
                .unwrap()
        };
        let before = read();
        let immutable_before = retained_files(&root.path().join("protected"));
        crate::organization::admission::bind_owner(&store, owner.clone()).unwrap();
        assert_eq!(read(), before);
        // Inject a coherent exhausted in-memory/cursor position using the same
        // existing storage fixture as native replay tests; no new production API.
        store
            .control(|state| {
                super::super::acceptance::exhaust_for_test(
                    &state.db,
                    state.acceptance.as_mut().unwrap(),
                )
            })
            .unwrap();
        crate::organization::admission::bind_owner(&store, owner.clone()).unwrap();
        assert_eq!(read(), i64::MAX);
        let saved = store
            .selected_read(|db, _| {
                db.query_row(
                    "SELECT content FROM selected_artifacts WHERE path='original-receipt'",
                    [],
                    |r| r.get::<_, Vec<u8>>(0),
                )
                .map_err(OfficeError::storage)
            })
            .unwrap();
        assert_eq!(saved, b"original");
        assert!(store
            .selected_transaction::<()>(|_, _| panic!("exhaustion must precede decision"))
            .is_err());
        assert!(crate::organization::admission::bind_owner(
            &store,
            Arc::new(Owner {
                live: AtomicBool::new(true),
                account: "other"
            })
        )
        .is_err());
        owner.live.store(false, Ordering::SeqCst);
        assert!(crate::organization::admission::bind_owner(&store, owner).is_err());
        assert_eq!(read(), i64::MAX);
        assert_eq!(
            retained_files(&root.path().join("protected")),
            immutable_before
        );
    }
}
