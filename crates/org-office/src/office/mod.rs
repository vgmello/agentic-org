//! The office on its store (`docs/design/office-store.md` §4, ADR-0021): one folder that
//! holds the office's identity and its lock, the organization's data under its data roots,
//! written through one acceptance journal, and a projection that can be deleted and
//! rebuilt.
//!
//! ```text
//! <home>/offices/<office-id>/
//!   office.lock            the office lock: exclusive, taken before anything is read
//!   selected-writer.required   the host's writer-custody marker, there before a create
//!   hq-creation.v1.json    the creation intent (version 3), frozen first
//!   office.json            written last: the office is finished
//! <data-root>/<org-id>/durable/   the journal and the organization's files
//! <data-root>/<org-id>/runtime/<office-id>/office.db   the projection
//! ```
//!
//! An office made by the old databases (`org.db`, `office.db`) is refused, never migrated.
//! Nothing here deletes anything under `durable/`.

mod creation;
mod folder;
mod git;
mod hiring;
mod intent;
mod messaging;
mod private_notes;
#[cfg(test)]
mod private_notes_tests;
mod tasks;
#[cfg(any(test, feature = "testkit"))]
mod testkit;
#[cfg(test)]
mod tests;

use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use org_proto::{
    directory::{WhoParams, WhoResult},
    org::{CommandAccepted, OrgSnapshot},
    ApiError, ErrorCode, OperationOutcome,
};
use org_store::{
    Actor, Clock, Files, Id, OfficeIdentity, StoreError, SystemClock, WriterAuthority,
};

#[cfg(any(test, feature = "testkit"))]
pub use self::testkit::TestOffice;
use crate::{
    error::OfficeError,
    hq_creation::{LocalRecord, PublicRecord},
    key::Key,
    model::OfficeInfo,
    office_file,
    office_local::{read_ca, CaRecords},
    organization::{directory, records, wire},
    rules::OrgCommand,
    store::open::{
        open_store, AddPersonOp, AddProjectTeamOp, ArchiveTeamOp, CheckOp, CreateProjectOp,
        CreateTeamOp, DeactivatePersonOp, DirectorySnapshot, EngineChoice, MovePersonOp,
        MoveTeamOp, OfficeStanding, OfficeStore, ProjectCheckOp, ProjectInspectByRef, ProjectsList,
        ReceiptById, RemoveProjectTeamOp, StoreConfig, TopTeamId, UpdatePersonOp, UpdateTeamOp,
    },
    time,
};
pub use folder::LOCK_WAIT;
use folder::{check_named_by, check_not_a_link, lock_folder, plain_folder, refuse_older_build};

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
    /// Runs once a creation is durable and its files are written, before the final checks
    /// and `office.json`: a test spoils the disk there.
    #[cfg(test)]
    pub before_final_checks: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl Options {
    fn production() -> Self {
        Self {
            engine: EngineChoice::Sqlite,
            clock: Arc::new(SystemClock),
            #[cfg(any(test, feature = "testkit"))]
            testing: Default::default(),
            #[cfg(test)]
            before_final_checks: None,
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
    runtime_root: PathBuf,
    identity: OfficeIdentity,
    /// The member whose account owns the organization: the creator.
    owner: Id,
    /// The store operation that created the organization.
    creation: Id,
    /// The creation request's canonical lowercase UUIDv7 command ID as the host sent it.
    creation_command: String,
    secrets_key: Option<Key>,
    clock: Arc<dyn Clock>,
    /// Monotonic high-water mark of `clock` (ms), so wall-clock rollbacks never reopen expired windows.
    clock_high_ms: AtomicU64,
    /// In-memory runtime presence and self-reported status lines (`status.set`, `who`).
    runtime: crate::messaging::runtime::OfficeRuntime,
    /// Isolated per-author private task notes store (`runtime/<office-id>/private/task-notes.db`).
    private_notes: Arc<dyn crate::private_notes::PrivateNotesStore>,
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
        Self::open_with_owner(
            folder,
            key,
            instance,
            roots,
            custody,
            None,
            Options::production(),
        )
    }

    /// Opens the finished HQ in `folder` with an explicit host-authoritative owner member ID
    /// (supports organization ownership handover, `TASK-38` / `doc-78`, while keeping the
    /// immutable `creation-intent.json` intact).
    pub fn open_as_owner(
        folder: impl Into<PathBuf>,
        key: Option<Key>,
        instance: &str,
        roots: DataRoots,
        custody: Custody,
        active_owner: &str,
    ) -> Result<Office, OfficeError> {
        Self::open_with_owner(
            folder,
            key,
            instance,
            roots,
            custody,
            Some(active_owner),
            Options::production(),
        )
    }

    #[cfg(test)]
    pub(crate) fn open_with(
        folder: impl Into<PathBuf>,
        key: Option<Key>,
        instance: &str,
        roots: DataRoots,
        custody: Custody,
        options: Options,
    ) -> Result<Office, OfficeError> {
        Self::open_with_owner(folder, key, instance, roots, custody, None, options)
    }

    fn open_with_owner(
        folder: impl Into<PathBuf>,
        key: Option<Key>,
        instance: &str,
        roots: DataRoots,
        custody: Custody,
        active_owner: Option<&str>,
        options: Options,
    ) -> Result<Office, OfficeError> {
        if !org_proto::is_uuid_v7(instance) {
            return Err(OfficeError::Invalid(
                "instance id must be a canonical lowercase UUIDv7",
            ));
        }
        check_roots(&roots)?;
        let folder = plain_folder(folder.into())?;
        // A link or a file where the folder goes is refused here; a missing folder is
        // `NotFound` from `lock_folder`, which creates nothing.
        check_not_a_link(&folder)?;
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
        if !org_proto::is_uuid_v7(&intent.owner) || !org_proto::is_uuid_v7(&intent.operation) {
            return Err(recovery());
        }
        let effective_owner = active_owner.unwrap_or(&intent.owner);
        if !org_proto::is_valid_member_id(effective_owner) {
            return Err(OfficeError::Invalid("active owner id is invalid"));
        }
        let owner = Id::parse(effective_owner).map_err(|_| recovery())?;
        let creation = Id::parse(&intent.operation).map_err(|_| recovery())?;
        let creation_command = intent.operation.clone();

        let store = open_store(&options.config(&roots, &custody)).map_err(storage)?;
        let private_notes = crate::store::open::open_private_notes(
            options.engine,
            &roots.runtime,
            &info.office_id,
        )?;
        let clock_high_ms = AtomicU64::new(options.clock.now().0);
        let office = Office {
            folder,
            info,
            durable: roots.durable,
            runtime_root: roots.runtime,
            identity: custody.identity,
            owner,
            creation,
            creation_command,
            secrets_key: key,
            clock: options.clock,
            clock_high_ms,
            runtime: crate::messaging::runtime::OfficeRuntime::new(),
            private_notes,
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
    /// intent, nor the host's custody marker or a file it stages one in. One with any of
    /// them is `OfficeExists`; nothing under the data roots is ever touched. A folder that
    /// is not there is fine; one that holds anything an office does not make is `Invalid`.
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

    /// Returns the office's live runtime presence/status record for `member_id`, if any (`AVAIL-5`).
    pub fn member_runtime(&self, member_id: &str) -> Option<directory::MemberRuntime> {
        use directory::RuntimeView;
        self.runtime.runtime_of(member_id)
    }

    /// `who`: the directory cards matching `params`, including live runtime presence and
    /// unexpired self-reported status lines (`status.set`).
    pub fn who(&self, params: &WhoParams) -> Result<Result<WhoResult, ApiError>, OfficeError> {
        self.who_with_runtime(params, &self.runtime)
    }

    /// Like [`Self::who`], overlaying caller-provided live session presence (such as
    /// `WaitingForAnswer`, `Busy`, or `UnableToRun` from `org-office-sessions`) on top of
    /// stored `status.set` lines (`ASK-4`, `AVAIL-4`).
    pub fn who_with_runtime(
        &self,
        params: &WhoParams,
        runtime: &dyn directory::RuntimeView,
    ) -> Result<Result<WhoResult, ApiError>, OfficeError> {
        let snapshot = self.org_snapshot()?;
        let office = directory::ThisOffice {
            id: &self.info.office_id,
            name: &self.info.office_name,
        };
        let now = time::format_rfc3339_ms(self.monotonic_now_ms());
        let merged = OverlayRuntime {
            primary: runtime,
            fallback: &self.runtime,
        };
        Ok(directory::who(&snapshot, office, &now, &merged, params))
    }

    fn monotonic_now_ms(&self) -> u64 {
        let now = self.clock.now().0;
        self.clock_high_ms
            .fetch_max(now, Ordering::Relaxed)
            .max(now)
    }

    /// Runs one organization command from `member`, the member the caller's grant is bound
    /// to, dispatching all implemented organization-family operations on `org-store`. A
    /// retry with the same `command_id` gets the same answer, bypassing age/skew checks.
    pub fn submit_org(
        &self,
        member: &str,
        command: OrgCommand,
    ) -> Result<Result<CommandAccepted, ApiError>, OfficeError> {
        if member != self.owner.as_str() {
            return Ok(Err(ApiError::new(
                ErrorCode::NotAuthorized,
                crate::organization::features::NOT_AUTHORIZED,
            )));
        }
        let op = match wire::parse_command(member, &command, || {
            self.store.read(TopTeamId).map(|(team, _)| team)
        })
        .map_err(storage)?
        {
            Ok(op) => op,
            Err(refusal) => return Ok(Err(refusal)),
        };
        // Stored answers always replay even after the 29-day window; only unknown command IDs
        // are subject to `expired` and `clock_skew` admission rules (unrecorded).
        let op_id = op.operation_id().clone();
        let (existing, _) = self
            .store
            .read(ReceiptById(op_id.clone()))
            .map_err(storage)?;
        if existing.is_none() {
            if let Err(refusal) =
                wire::check_command_window(op_id.as_str(), self.monotonic_now_ms())
            {
                return Ok(Err(refusal));
            }
        }
        let here = &self.identity;
        let owner = &self.owner;
        let run = |op: wire::WireOp| -> Result<Result<CommandAccepted, ApiError>, StoreError> {
            match op {
                wire::WireOp::CreateTeam(r) => wire::team_create_answer(
                    self.store
                        .run(CreateTeamOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::UpdateTeam(r) => wire::changed_answer(
                    "team.update",
                    self.store
                        .run(UpdateTeamOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::MoveTeam(r) => wire::changed_answer(
                    "team.move",
                    self.store
                        .run(MoveTeamOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::ArchiveTeam(r) => wire::changed_answer(
                    "team.delete",
                    self.store
                        .run(ArchiveTeamOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::AddPerson(r) => wire::changed_answer(
                    "member.add_person",
                    self.store
                        .run(AddPersonOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::UpdatePerson(r) => wire::changed_answer(
                    "member.update_person",
                    self.store
                        .run(UpdatePersonOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::MovePerson(r) => wire::changed_answer(
                    "member.move",
                    self.store
                        .run(MovePersonOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::DeactivatePerson(r) => wire::changed_answer(
                    "member.remove",
                    self.store.run(DeactivatePersonOp::from_request(
                        r,
                        here.clone(),
                        owner.clone(),
                    )),
                ),
                wire::WireOp::CreateProject(r) => wire::project_create_answer(self.store.run(
                    CreateProjectOp::from_request(r, here.clone(), owner.clone()),
                )),
                wire::WireOp::AddProjectTeam(r) => wire::changed_answer(
                    "project.add_team",
                    self.store.run(AddProjectTeamOp::from_request(
                        r,
                        here.clone(),
                        owner.clone(),
                    )),
                ),
                wire::WireOp::RemoveProjectTeam(r) => wire::changed_answer(
                    "project.remove_team",
                    self.store.run(RemoveProjectTeamOp::from_request(
                        r,
                        here.clone(),
                        owner.clone(),
                    )),
                ),
            }
        };
        run(op).map_err(storage)
    }

    /// Runs a dry-run check of one organization command from `member` through `Store::check`:
    /// decides, frames, applies, validates, and unconditionally undoes with zero journal
    /// entries, files, receipts, or sequence advancement.
    pub fn check_org(
        &self,
        member: &str,
        command: OrgCommand,
    ) -> Result<Result<CommandAccepted, ApiError>, OfficeError> {
        if member != self.owner.as_str() {
            return Ok(Err(ApiError::new(
                ErrorCode::NotAuthorized,
                crate::organization::features::NOT_AUTHORIZED,
            )));
        }
        let op = match wire::parse_check_command(member, &command, || {
            self.store.read(TopTeamId).map(|(team, _)| team)
        })
        .map_err(storage)?
        {
            Ok(op) => op,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let here = &self.identity;
        let owner = &self.owner;
        let run = |op: wire::WireOp| -> Result<Result<CommandAccepted, ApiError>, StoreError> {
            match op {
                wire::WireOp::CreateTeam(r) => wire::checked_answer(
                    "team.create",
                    self.store
                        .run(CheckOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::UpdateTeam(r) => wire::checked_answer(
                    "team.update",
                    self.store
                        .run(CheckOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::MoveTeam(r) => wire::checked_answer(
                    "team.move",
                    self.store
                        .run(CheckOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::ArchiveTeam(r) => wire::checked_answer(
                    "team.delete",
                    self.store
                        .run(CheckOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::AddPerson(r) => wire::checked_answer(
                    "member.add_person",
                    self.store
                        .run(CheckOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::UpdatePerson(r) => wire::checked_answer(
                    "member.update_person",
                    self.store
                        .run(CheckOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::MovePerson(r) => wire::checked_answer(
                    "member.move",
                    self.store
                        .run(CheckOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::DeactivatePerson(r) => wire::checked_answer(
                    "member.remove",
                    self.store
                        .run(CheckOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::CreateProject(r) => wire::checked_answer(
                    "project.create",
                    self.store
                        .run(ProjectCheckOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::AddProjectTeam(r) => wire::checked_answer(
                    "project.add_team",
                    self.store
                        .run(ProjectCheckOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::RemoveProjectTeam(r) => wire::checked_answer(
                    "project.remove_team",
                    self.store
                        .run(ProjectCheckOp::from_request(r, here.clone(), owner.clone())),
                ),
            }
        };
        run(op).map_err(storage)
    }

    /// Lists projects from a consistent snapshot of the office store (B3).
    pub fn list_projects(
        &self,
        include_archived: bool,
    ) -> Result<org_proto::projects::ProjectListResult, OfficeError> {
        let (projects, _) = self
            .store
            .read(ProjectsList { include_archived })
            .map_err(storage)?;
        Ok(org_proto::projects::ProjectListResult {
            projects: projects.iter().map(|p| p.to_wire()).collect(),
        })
    }

    /// Inspects one project by UUIDv7 ID, `P-<number>`, or canonical slug (B3).
    pub fn inspect_project(
        &self,
        reference: &str,
    ) -> Result<Result<org_proto::projects::ProjectRecord, ApiError>, OfficeError> {
        let (found, _) = self
            .store
            .read(ProjectInspectByRef(reference.to_owned()))
            .map_err(storage)?;
        Ok(found
            .map(|p| p.to_wire())
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "project not found")))
    }

    /// Reads the stored outcome of an organization command by its UUIDv7 `command_id` for
    /// `member`. Refuses another actor's receipt and non-organization receipts as `not_found`.
    pub fn operation(
        &self,
        member: &str,
        command_id: &str,
    ) -> Result<Result<OperationOutcome, ApiError>, OfficeError> {
        let not_found = || ApiError::new(ErrorCode::NotFound, "operation not found");
        if !records::is_stable_id(command_id) {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "command_id is not a UUIDv7 in lowercase text",
            )));
        }
        let Ok(op_id) = Id::parse(command_id) else {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "command_id is not a UUIDv7 in lowercase text",
            )));
        };
        if !records::is_stable_id(member) {
            return Ok(Err(not_found()));
        }
        let Ok(actor_id) = Id::parse(member) else {
            return Ok(Err(not_found()));
        };
        let (receipt, _) = self.store.read(ReceiptById(op_id)).map_err(storage)?;
        let Some(receipt) = receipt else {
            return Ok(Err(not_found()));
        };
        if !receipt.kind.starts_with("organization.")
            || receipt.kind == "organization.create"
            || receipt.actor != Actor::Employee(actor_id)
        {
            return Ok(Err(not_found()));
        }
        let outcome = wire::operation_outcome_from_receipt(&receipt).map_err(storage)?;
        Ok(Ok(outcome))
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

    /// The canonical lowercase UUIDv7 command ID of the request that created this HQ,
    /// kept in the creation intent and matching the store operation ID.
    pub fn creation_command_id(&self) -> &str {
        &self.creation_command
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

    /// The organization's durable data root (`<data-root>/<org-id>/durable`).
    pub fn durable_root(&self) -> &Path {
        &self.durable
    }

    /// The canonical UUIDv7 member ID of the organization's current owner.
    pub fn owner_member_id(&self) -> &str {
        self.owner.as_str()
    }

    /// Every row of the projection, to compare with another of the same engine.
    #[cfg(any(test, feature = "testkit"))]
    pub fn dump(&self) -> Vec<String> {
        self.store.dump()
    }
}

struct OverlayRuntime<'a> {
    primary: &'a dyn directory::RuntimeView,
    fallback: &'a dyn directory::RuntimeView,
}

impl directory::RuntimeView for OverlayRuntime<'_> {
    fn runtime_of(&self, member_id: &str) -> Option<directory::MemberRuntime> {
        match (
            self.primary.runtime_of(member_id),
            self.fallback.runtime_of(member_id),
        ) {
            (Some(mut prim), Some(base)) => {
                if prim.status.is_none() {
                    prim.status = base.status;
                    prim.status_expires_at = base.status_expires_at;
                }
                if prim.current_task_id.is_none() {
                    prim.current_task_id = base.current_task_id;
                }
                if prim.last_seen_at.is_none() {
                    prim.last_seen_at = base.last_seen_at;
                }
                Some(prim)
            }
            (Some(prim), None) => Some(prim),
            (None, base) => base,
        }
    }
}
