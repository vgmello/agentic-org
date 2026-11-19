//! The office on its store (`docs/design/office-store.md` §4, ADR-0021): one folder that
//! holds the office's identity and its lock, the organization's data under its data roots,
//! written through one acceptance journal, and a projection that can be deleted and
//! rebuilt.
//!
//! ```text
//! <home>/offices/<office-id>/
//!   office.lock            the office lock: exclusive, taken before anything is read
//!   hq-creation.v1.json    the creation intent (version 2), frozen first
//!   office.json            written last: the office is finished
//! <data-root>/<org-id>/durable/   the journal and the organization's files
//! <data-root>/<org-id>/runtime/<office-id>/office.db   the projection
//! ```
//!
//! An office made by the old databases (`org.db`, `office.db`) is refused, never migrated.
//! Nothing here deletes anything under `durable/`.
//!
//! B1b builds this beside the office of the old databases ([`crate::Office`]); the worker
//! switches to it (block 6), and the old one is deleted after (block 4b).

mod creation;
mod intent;
#[cfg(any(test, feature = "testkit"))]
mod testkit;
#[cfg(test)]
mod tests;

use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};

use org_proto::{
    directory::{WhoParams, WhoResult},
    org::{CommandAccepted, OrgSnapshot},
    ApiError, ErrorCode,
};
use org_store::{Clock, Files, Id, OfficeIdentity, StoreError, SystemClock, WriterAuthority};

#[cfg(any(test, feature = "testkit"))]
pub use self::testkit::TestOffice;
use crate::{
    error::OfficeError,
    hq_creation::{LocalRecord, PublicRecord},
    key::Key,
    model::OfficeInfo,
    office::{check_named_by, check_not_a_link, exists, lock_folder, plain_folder, LOCK_WAIT},
    office_file,
    office_local::{read_ca, CaRecords},
    organization::{directory, operation_id, wire},
    rules::OrgCommand,
    store::open::{
        open_store, CreateTeamOp, DirectorySnapshot, EngineChoice, OfficeStanding, OfficeStore,
        StoreConfig,
    },
    time,
};

/// An organization's two data roots, both absolute: `<data-root>/<org-id>/durable` and
/// `<data-root>/<org-id>/runtime`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DataRoots {
    pub durable: PathBuf,
    pub runtime: PathBuf,
}

/// Who may write this office's journal: the identity the host reserved for it, and the
/// host's writer custody, which confirms every flush.
#[derive(Clone)]
pub struct Custody {
    pub identity: OfficeIdentity,
    pub authority: Arc<dyn WriterAuthority>,
}

/// What the office composes its store from besides the request: production uses SQLite
/// and the wall clock; tests choose.
#[derive(Clone)]
pub(crate) struct Options {
    pub engine: EngineChoice,
    pub clock: Arc<dyn Clock>,
    #[cfg(any(test, feature = "testkit"))]
    pub testing: crate::store::open::Testing,
}

impl Options {
    fn production() -> Self {
        Self {
            engine: EngineChoice::Sqlite,
            clock: Arc::new(SystemClock),
            #[cfg(any(test, feature = "testkit"))]
            testing: Default::default(),
        }
    }

    fn config(&self, roots: &DataRoots, custody: &Custody) -> StoreConfig {
        #[cfg_attr(not(any(test, feature = "testkit")), allow(unused_mut))]
        let mut config = StoreConfig::new(
            self.engine,
            roots.durable.clone(),
            roots.runtime.clone(),
            custody.identity.clone(),
            Arc::clone(&custody.authority),
            Arc::clone(&self.clock),
        );
        #[cfg(any(test, feature = "testkit"))]
        {
            config.testing = self.testing.clone();
        }
        config
    }
}

/// An HQ office, open on its store. Safe to share across threads.
pub struct Office {
    folder: PathBuf,
    info: OfficeInfo,
    durable: PathBuf,
    identity: OfficeIdentity,
    /// The member whose account owns the organization: the creator.
    owner: Id,
    /// The store operation that created the organization.
    creation: Id,
    secrets_key: Option<Key>,
    // The store closes before the lock is released: fields drop in order.
    store: OfficeStore,
    _lock: File,
    #[cfg(any(test, feature = "testkit"))]
    _test_root: Option<tempfile::TempDir>,
}

impl std::fmt::Debug for Office {
    /// What `office.json` records; never a key.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Office")
            .field("folder", &self.folder)
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

/// The one refusal of an HQ whose records disagree: it is never repaired by generating
/// anything again.
pub(crate) fn recovery() -> OfficeError {
    OfficeError::Invalid("HQ provisioning requires recovery")
}

/// A store failure, its path kept in the cause for the log and out of `Display`.
pub(crate) fn storage(error: StoreError) -> OfficeError {
    OfficeError::storage(error)
}

/// Refuses an office folder the old databases made.
pub(crate) fn refuse_older_build(folder: &Path) -> Result<(), OfficeError> {
    use crate::office::{OFFICE_DB, ORG_DB};
    for database in [ORG_DB, OFFICE_DB] {
        if exists(&folder.join(database))? {
            return Err(intent::older_build());
        }
    }
    Ok(())
}

/// Roots that are absolute and plain: no `.` or `..`, and two different folders.
pub(crate) fn check_roots(roots: &DataRoots) -> Result<(), OfficeError> {
    let plain = |path: &Path| {
        path.is_absolute()
            && path.to_str().is_some()
            && path.components().all(|c| {
                matches!(
                    c,
                    std::path::Component::RootDir | std::path::Component::Normal(_)
                )
            })
    };
    if !plain(&roots.durable) || !plain(&roots.runtime) || roots.durable == roots.runtime {
        return Err(OfficeError::Invalid(
            "the data roots are not two plain absolute folders",
        ));
    }
    // The host makes both, owner-only, when it reserves the office's custody.
    for root in [&roots.durable, &roots.runtime] {
        match std::fs::symlink_metadata(root) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Err(OfficeError::Invalid("a data root is not a folder")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(OfficeError::Invalid("a data root is not there"));
            }
            Err(e) => return Err(OfficeError::storage(e)),
        }
    }
    Ok(())
}

impl Office {
    /// Opens the finished HQ in `folder` on its store: takes the office lock, reads
    /// `office.json` and the creation intent, checks that the roots and the custody are the
    /// ones it was created with, opens the store (replaying or rebuilding the projection),
    /// and checks the organization, this office's role and the CA records. `key` is the
    /// host's office secrets key, held only to unwrap the office's private keys.
    pub fn open(
        folder: impl Into<PathBuf>,
        key: Option<Key>,
        instance: &str,
        roots: DataRoots,
        custody: Custody,
    ) -> Result<Office, OfficeError> {
        Self::open_with(folder, key, instance, roots, custody, Options::production())
    }

    pub(crate) fn open_with(
        folder: impl Into<PathBuf>,
        key: Option<Key>,
        instance: &str,
        roots: DataRoots,
        custody: Custody,
        options: Options,
    ) -> Result<Office, OfficeError> {
        if !org_proto::is_uuid_v7(instance) {
            return Err(OfficeError::Invalid(
                "instance id must be a canonical lowercase UUIDv7",
            ));
        }
        check_roots(&roots)?;
        let folder = plain_folder(folder.into())?;
        check_not_a_link(&folder)?;
        match std::fs::metadata(&folder) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Err(OfficeError::Invalid("the office folder is not a folder")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(OfficeError::NotFound);
            }
            Err(e) => return Err(OfficeError::storage(e)),
        }
        let lock = lock_folder(&folder, LOCK_WAIT)?;
        refuse_older_build(&folder)?;
        let info = office_file::read(&folder)?.ok_or(OfficeError::NotFound)?;
        check_named_by(&folder, &info.office_id)?;
        let intent = intent::read(&folder)?.ok_or_else(intent::older_build)?;
        let here = &custody.identity;
        let bound = intent.info() == info
            && intent.instance == instance
            && intent.roots() == roots
            && here.instance_id.as_str() == instance
            && here.organization_id.as_str() == info.org_id
            && here.office_id.as_str() == info.office_id;
        if !bound {
            return Err(OfficeError::Invalid(
                "the office was created with other roots or another identity",
            ));
        }
        let owner = Id::parse(&intent.owner).map_err(|_| recovery())?;
        let creation = operation_id::from_wire_ulid(&intent.operation).map_err(|_| recovery())?;

        let store = open_store(&options.config(&roots, &custody)).map_err(storage)?;
        let office = Office {
            folder,
            info,
            durable: roots.durable,
            identity: custody.identity,
            owner,
            creation,
            secrets_key: key,
            store,
            _lock: lock,
            #[cfg(any(test, feature = "testkit"))]
            _test_root: None,
        };
        office.check_standing()?;
        office.ca()?;
        Ok(office)
    }

    /// The projection holds this office's organization, with this office as its HQ.
    fn check_standing(&self) -> Result<(), OfficeError> {
        let read = OfficeStanding {
            office: self.identity.office_id.clone(),
        };
        let (standing, _) = self.store.read(read).map_err(storage)?;
        let ours = standing.organization.as_ref() == Some(&self.identity.organization_id);
        if !ours || !standing.hq {
            return Err(recovery());
        }
        Ok(())
    }

    /// The HQ's CA records, checked against the creation (M1): both files there, the
    /// package the key record names, the same public key, organization and operation.
    fn ca(&self) -> Result<CaRecords, OfficeError> {
        let files = Files::open(&self.durable).map_err(OfficeError::storage)?;
        let records = read_ca(&files, &self.identity.office_id).map_err(|_| recovery())?;
        let records = records.ok_or_else(recovery)?;
        let here = &self.identity;
        records
            .check(&here.organization_id, &here.office_id, &self.creation)
            .map_err(|_| recovery())?;
        Ok(records)
    }

    /// Removes an unfinished create: a folder with neither `office.json` nor a creation
    /// intent. One with either is `OfficeExists`; nothing under the data roots is ever
    /// touched. A folder that is not there is fine; one that holds anything an office does
    /// not make is `Invalid`.
    pub fn discard(folder: impl Into<PathBuf>, office_id: &str) -> Result<(), OfficeError> {
        creation::discard(folder.into(), office_id)
    }

    /// The orderly stop: the store closes (flushed, confirmed, written, its log folded in)
    /// and leaves the clean-stop marker, then the office lock is released.
    pub fn close(self) -> Result<(), OfficeError> {
        let Office { store, _lock, .. } = self;
        store.close().map_err(storage)
    }

    /// Why the store stopped accepting, if it has: a fixed text and the store's reason,
    /// which names the path. The office then serves nothing more and should exit.
    pub fn fatal(&self) -> Option<String> {
        let fatal = self.store.fatal()?;
        Some(format!("the office store stopped: {fatal}"))
    }

    /// Whether the stop before this opening was orderly.
    pub fn stopped_cleanly(&self) -> bool {
        self.store.stopped_cleanly()
    }

    /// The organization's structure from one consistent read of the projection.
    pub fn org_snapshot(&self) -> Result<OrgSnapshot, OfficeError> {
        let read = DirectorySnapshot {
            owner: self.owner.clone(),
        };
        Ok(self.store.read(read).map_err(storage)?.0)
    }

    /// `who`: the directory cards matching `params`. Nothing is live here: everyone reads
    /// as offline, with no status line, until status lines return (B6). `Err(ApiError)`
    /// inside the `Ok` is the caller's mistake; the outer error is the storage's.
    pub fn who(&self, params: &WhoParams) -> Result<Result<WhoResult, ApiError>, OfficeError> {
        let snapshot = self.org_snapshot()?;
        let office = directory::ThisOffice {
            id: &self.info.office_id,
            name: &self.info.office_name,
        };
        let now = time::now_rfc3339();
        Ok(directory::who(
            &snapshot,
            office,
            &now,
            &directory::NothingLive,
            params,
        ))
    }

    /// Runs one organization command from `member`, the member the caller's grant is bound
    /// to. `team.create` is the organization family's; every other command answers
    /// `unsupported` until B2. A retry with the same `command_id` gets the same answer.
    pub fn submit_org(
        &self,
        member: &str,
        command: OrgCommand,
    ) -> Result<Result<CommandAccepted, ApiError>, OfficeError> {
        let OrgCommand::TeamCreate(params) = command else {
            let refused = "this office does not serve this organization command yet";
            return Ok(Err(ApiError::new(ErrorCode::Unsupported, refused)));
        };
        let here = &self.identity;
        let answer = wire::team_create(member, &params, |request| {
            self.store.run(CreateTeamOp {
                operation_id: request.operation_id,
                here: here.clone(),
                by: request.by,
                command: request.command,
            })
        });
        answer.map_err(storage)
    }

    /// Whether this office is its organization's HQ, as the projection holds it.
    pub fn is_hq(&self) -> Result<bool, OfficeError> {
        let read = OfficeStanding {
            office: self.identity.office_id.clone(),
        };
        Ok(self.store.read(read).map_err(storage)?.0.hq)
    }

    /// The HQ's creation record with the wrapped CA key, read from its file and checked.
    pub fn hq_creation_record(&self) -> Result<Option<LocalRecord>, OfficeError> {
        let records = self.ca()?;
        let key = records.key;
        LocalRecord::new(
            key.organization_id.to_string(),
            key.operation_id.to_string(),
            key.public_key,
            records.package,
            key.wrapped_ca_key,
        )
        .map(Some)
    }

    /// The HQ's public creation record: the escrow package and what binds it. It carries
    /// no private key in any form.
    pub fn hq_creation_public_record(&self) -> Result<Option<PublicRecord>, OfficeError> {
        let records = self.ca()?;
        Ok(Some(PublicRecord {
            version: 1,
            organization_id: records.key.organization_id.to_string(),
            operation_id: records.key.operation_id.to_string(),
            public_key: records.key.public_key,
            package: records.package,
        }))
    }

    /// The office secrets key the host handed over, if any. Only for wrapping and
    /// unwrapping the office's private keys; never log, persist or send it.
    pub fn secrets_key(&self) -> Option<&Key> {
        self.secrets_key.as_ref()
    }

    /// What `office.json` records.
    pub fn info(&self) -> &OfficeInfo {
        &self.info
    }

    /// The office's folder.
    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// Every row of the projection, to compare with another of the same engine.
    #[cfg(any(test, feature = "testkit"))]
    pub fn dump(&self) -> Vec<String> {
        self.store.dump()
    }
}
