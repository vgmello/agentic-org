//! Finding an organization's office in an office home, and holding the host's and the
//! office's locks for as long as a backup or a restore runs (`doc-91`: a live office is never
//! copied or overwritten).

use std::fs::{File, OpenOptions, TryLockError};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};

use org_office::backup::recorded_office;
use org_office_launch::{office_dir, HOST_LOCK_FILE_NAME, OFFICES_DIR_NAME, OFFICE_LOCK_FILE_NAME};
use org_store::OfficeIdentity;

use crate::error::BackupError;
use crate::paths::shown;

/// What a stopped office must be for a backup or a restore.
const STOP_FIRST: &str = "stop it first (`agentic-org service stop`, or end the foreground \
`agentic-org serve`), then run this command again";

/// One office of the organization, as its folder records it.
#[derive(Clone, Debug)]
pub struct OfficeTarget {
    pub office_home: PathBuf,
    pub folder: PathBuf,
    pub office_id: String,
    pub org_id: String,
    pub org_name: String,
    pub is_hq: bool,
    /// `<base>/durable`.
    pub durable: PathBuf,
    /// `<base>/runtime`.
    pub runtime: PathBuf,
    /// The journal identity the host reserved for the office.
    pub identity: OfficeIdentity,
}

impl OfficeTarget {
    /// The folder that holds both data roots.
    pub fn base(&self) -> &Path {
        self.durable.parent().unwrap_or(&self.durable)
    }

    pub(crate) fn role(&self) -> &'static str {
        if self.is_hq {
            "hq"
        } else {
            "satellite"
        }
    }
}

/// A root a backup may read and a restore may replace: absolute, plain, named `durable` or
/// `runtime`, both in one folder.
fn check_roots(durable: &Path, runtime: &Path) -> Result<(), BackupError> {
    let plain = |p: &Path| {
        p.is_absolute()
            && p.components()
                .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
    };
    let named = |p: &Path, name: &str| p.file_name().is_some_and(|n| n == name);
    if !plain(durable)
        || !plain(runtime)
        || !named(durable, "durable")
        || !named(runtime, "runtime")
        || durable.parent() != runtime.parent()
    {
        return Err(BackupError::Refused(format!(
            "the office records data roots this command does not handle ({} and {})",
            durable.display(),
            runtime.display()
        )));
    }
    Ok(())
}

/// The office of organization `org` (a UUIDv7) in `office_home`; `office` picks one when
/// the organization has several offices here.
pub fn locate(
    office_home: &Path,
    org: &str,
    office: Option<&str>,
) -> Result<OfficeTarget, BackupError> {
    if !org_office_launch::is_selected_entity_id(org) {
        return Err(BackupError::Usage(format!(
            "`{}` is not an organization ID (a lowercase UUIDv7); find it with `agentic-org org list`",
            shown(org)
        )));
    }
    if let Some(office) = office.filter(|o| !org_office_launch::is_storage_id(o)) {
        return Err(BackupError::Usage(format!(
            "`{}` is not an office ID",
            shown(office)
        )));
    }
    let offices = office_home.join(OFFICES_DIR_NAME);
    let listing = std::fs::read_dir(&offices)
        .map_err(BackupError::io(format!("listing {}", offices.display())))?;
    let mut names = Vec::new();
    for entry in listing {
        let entry = entry.map_err(BackupError::io(format!("listing {}", offices.display())))?;
        if let Some(name) = entry.file_name().to_str() {
            if org_office_launch::is_storage_id(name) && office.is_none_or(|o| o == name) {
                names.push(name.to_owned());
            }
        }
    }
    names.sort();
    let mut found = Vec::new();
    for name in names {
        let Some(folder) = office_dir(office_home, &name) else {
            continue;
        };
        // A folder that is not a finished office of this organization is not a candidate.
        let Ok(Some(recorded)) = recorded_office(&folder) else {
            continue;
        };
        if recorded.info.org_id == org && recorded.info.office_id == name {
            found.push((folder, recorded));
        }
    }
    let (folder, recorded) = match found.len() {
        1 => found.remove(0),
        0 => {
            return Err(BackupError::NotFound(format!(
                "organization {org} has no {} in {}",
                office.map_or("office".to_owned(), |o| format!("office {o}")),
                office_home.display()
            )))
        }
        n => {
            return Err(BackupError::Usage(format!(
                "organization {org} has {n} offices here; choose one with --office <office-id>"
            )))
        }
    };
    let roots = recorded.roots;
    check_roots(&roots.durable, &roots.runtime)?;
    Ok(OfficeTarget {
        office_home: office_home.to_path_buf(),
        folder,
        office_id: recorded.info.office_id,
        org_id: recorded.info.org_id,
        org_name: recorded.info.org_name,
        is_hq: recorded.info.role == org_office::OfficeRole::Hq,
        durable: roots.durable,
        runtime: roots.runtime,
        identity: recorded.identity,
    })
}

/// The host's and the office's locks, held until dropped.
#[derive(Debug)]
pub(crate) struct HeldLocks {
    _files: Vec<File>,
}

impl Drop for HeldLocks {
    fn drop(&mut self) {
        for file in &self._files {
            let _ = file.unlock();
        }
    }
}

fn take_lock(path: &Path, what: String) -> Result<File, BackupError> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(org_platform::fs::O_NOFOLLOW | org_platform::fs::O_CLOEXEC)
        .open(path)
        .map_err(BackupError::io(format!("opening {}", path.display())))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => Err(BackupError::Live {
            what,
            lock: path.to_path_buf(),
            help: STOP_FIRST,
        }),
        Err(TryLockError::Error(e)) => {
            Err(BackupError::io(format!("locking {}", path.display()))(e))
        }
    }
}

/// Takes `host.lock` in the office home and `office.lock` in the office's folder, without
/// waiting: either held means the host or the office runs, and nothing is read or written.
/// Held, they keep a host or office from starting until the work is done.
pub(crate) fn take_locks(target: &OfficeTarget) -> Result<HeldLocks, BackupError> {
    let host = take_lock(
        &target.office_home.join(HOST_LOCK_FILE_NAME),
        "the Agentic Org host".into(),
    )?;
    let office = take_lock(
        &target.folder.join(OFFICE_LOCK_FILE_NAME),
        format!("office {}", target.office_id),
    )?;
    Ok(HeldLocks {
        _files: vec![host, office],
    })
}
