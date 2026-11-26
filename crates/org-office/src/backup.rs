//! The office's side of an organization backup and restore (`TASK-104`, `doc-91` loss
//! boundaries): where an office folder says its data lives ([`recorded_office`]), a consistent
//! copy and integrity check of the private task notes database ([`snapshot_task_notes`],
//! [`check_task_notes`]), and the proof that a restored durable root opens
//! ([`rebuild_disposable`]).
//!
//! A disposable rebuild opens the office's store, its `L2` accounting and `L3` budgets
//! companions and its provider pool over a restored durable root on a disposable runtime
//! root, so every projection is rebuilt from the journal and the files, read back and closed. Writer custody is an
//! in-memory stand-in pinned to the head the backup recorded: an opening that would need less
//! (a journal behind it, a fork) refuses as it would on the host. Nothing here touches a host,
//! a key store or the host's custody.
//!
//! Opening finishes what a stop left behind in the durable root it is given (a torn tail
//! above the pinned head is cut, files are materialized, the clean-stop marker is consumed
//! and written again), so callers run it on a disposable copy, never on a live office.

pub use crate::office::recorded::{recorded_office, RecordedOffice};
pub use crate::store::sqlite::private_notes_copy::{check_task_notes, snapshot_task_notes};

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use org_store::{CustodyError, JournalHead, OfficeIdentity, SystemClock, WriterAuthority};

use crate::error::OfficeError;
use crate::store::open::{
    open_accounting, open_budgets, open_providers, open_store, EmployeesList, EngineChoice,
    OfficeStore, OfficesCount, ProjectsList, StoreConfig,
};

/// What a disposable rebuild opens.
#[derive(Clone, Debug)]
pub struct DisposableRebuild {
    /// The restored copy's durable root (`.../durable`), absolute.
    pub durable_root: PathBuf,
    /// A fresh, disposable runtime root, absolute and distinct from the durable root.
    pub runtime_root: PathBuf,
    /// The journal identity the restored anchor binds.
    pub identity: OfficeIdentity,
    /// The head the backup recorded; the restored journal must hold it.
    pub pinned: Option<JournalHead>,
    /// Whether the office is its organization's HQ (budgets are HQ-owned).
    pub is_hq: bool,
}

/// What the rebuilt projections answered.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RebuildReport {
    /// The flushed head the store opened at.
    pub head_ordinal: u64,
    /// Whether the stop before the backup was orderly.
    pub stopped_cleanly: bool,
    pub offices: usize,
    pub employees: usize,
    pub projects: usize,
}

/// Rebuilds every projection of `spec`'s office on its disposable runtime root, runs `read`
/// on the open store (a caller's own queries), and closes everything.
pub fn rebuild_disposable<R>(
    spec: &DisposableRebuild,
    read: impl FnOnce(&OfficeStore) -> Result<R, OfficeError>,
) -> Result<(RebuildReport, R), OfficeError> {
    if spec.durable_root == spec.runtime_root
        || !spec.durable_root.is_absolute()
        || !spec.runtime_root.is_absolute()
    {
        return Err(OfficeError::Invalid(
            "a disposable rebuild needs two distinct absolute roots",
        ));
    }
    let custody = Arc::new(PinnedCustody {
        head: Mutex::new(spec.pinned.clone()),
    });
    let config = StoreConfig::new(
        EngineChoice::Sqlite,
        spec.durable_root.clone(),
        spec.runtime_root.clone(),
        spec.identity.clone(),
        custody as Arc<dyn WriterAuthority>,
        Arc::new(SystemClock),
    );
    let store = open_store(&config).map_err(OfficeError::storage)?;
    let report = report(&store)?;
    let answer = read(&store)?;
    store.close().map_err(OfficeError::storage)?;
    open_companions(spec)?;
    Ok((report, answer))
}

fn report(store: &OfficeStore) -> Result<RebuildReport, OfficeError> {
    let status = store.status();
    if let Some(stopped) = status.stopped {
        return Err(OfficeError::storage(stopped));
    }
    let (offices, _) = store.read(OfficesCount).map_err(OfficeError::storage)?;
    let (employees, _) = store.read(EmployeesList).map_err(OfficeError::storage)?;
    let (projects, _) = store
        .read(ProjectsList {
            include_archived: true,
        })
        .map_err(OfficeError::storage)?;
    Ok(RebuildReport {
        head_ordinal: status.flushed,
        stopped_cleanly: store.stopped_cleanly(),
        offices,
        employees: employees.len(),
        projects: projects.len(),
    })
}

/// The companion stores rebuild their own projections under the runtime root when opened:
/// accounting, budgets, and the provider pool, which reads the restored provider connection
/// bindings and execution-authority files.
fn open_companions(spec: &DisposableRebuild) -> Result<(), OfficeError> {
    let office = spec.identity.office_id.as_str();
    let org = spec.identity.organization_id.as_str();
    let durable: &Path = &spec.durable_root;
    let runtime: &Path = &spec.runtime_root;
    drop(open_accounting(
        EngineChoice::Sqlite,
        durable,
        runtime,
        office,
        org,
    )?);
    drop(open_budgets(
        EngineChoice::Sqlite,
        durable,
        runtime,
        office,
        org,
        spec.is_hq,
    )?);
    drop(open_providers(
        EngineChoice::Sqlite,
        durable,
        runtime,
        office,
        org,
        spec.is_hq,
    )?);
    Ok(())
}

/// The host's custody rule, in memory, starting from the pinned head: the restored journal
/// must hold it, and any confirmation must continue from the head last confirmed.
struct PinnedCustody {
    head: Mutex<Option<JournalHead>>,
}

impl WriterAuthority for PinnedCustody {
    fn checkpoint(&self, _identity: &OfficeIdentity) -> Result<Option<JournalHead>, CustodyError> {
        Ok(self.head.lock().unwrap_or_else(|p| p.into_inner()).clone())
    }

    fn confirm(
        &self,
        _identity: &OfficeIdentity,
        before: Option<&JournalHead>,
        after: &JournalHead,
    ) -> Result<(), CustodyError> {
        let mut head = self.head.lock().unwrap_or_else(|p| p.into_inner());
        let continues = match (head.as_ref(), before) {
            (None, _) => true,
            (Some(stored), Some(before)) => {
                stored.ordinal == before.ordinal && stored.sha256 == before.sha256
            }
            (Some(_), None) => false,
        };
        let forward = head.as_ref().is_none_or(|stored| {
            after.ordinal > stored.ordinal
                || (after.ordinal == stored.ordinal && after.sha256 == stored.sha256)
        });
        if !continues || !forward {
            return Err(CustodyError(
                "a disposable rebuild may only continue from the pinned head".into(),
            ));
        }
        *head = Some(after.clone());
        Ok(())
    }
}

/// Removes the office's rebuildable projections under `runtime_root` (the store's `office.db`
/// with its sidecars, and the companion stores' `protected/` databases) so the next opening
/// rebuilds them from a restored durable root. A companion projection rebuilds itself only
/// when it is missing or broken, never when it is ahead of its files, so after an in-place
/// restore it must go. Nothing else in the runtime root (repository caches and worktrees) is
/// touched; a link where a projection belongs is refused.
pub fn discard_projections(runtime_root: &Path, office_id: &str) -> Result<(), OfficeError> {
    let office = org_store::Id::parse(office_id)
        .map_err(|_| OfficeError::Invalid("an office id must be one plain component"))?;
    let folder = runtime_root.join(office.as_str());
    match std::fs::symlink_metadata(&folder) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => {
            return Err(OfficeError::Invalid(
                "a runtime office folder is not a folder",
            ))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(OfficeError::storage(e)),
    }
    let protected = folder.join("protected");
    match std::fs::symlink_metadata(&protected) {
        Ok(meta) if meta.is_dir() => {
            std::fs::remove_dir_all(&protected).map_err(OfficeError::storage)?;
        }
        Ok(_) => return Err(OfficeError::Invalid("a runtime projection is not a folder")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(OfficeError::storage(e)),
    }
    crate::private_fs::remove_sqlite_files(&folder.join(crate::store::open::PROJECTION_FILE))
        .map_err(OfficeError::storage)?;
    crate::private_fs::sync_dir(&folder).map_err(OfficeError::storage)
}
