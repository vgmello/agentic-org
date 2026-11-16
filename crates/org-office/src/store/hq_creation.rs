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
