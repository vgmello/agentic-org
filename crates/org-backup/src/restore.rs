//! `restore <archive> --org <org>`: putting a verified archive back as a stopped office's
//! durable root. The archive is verified in full first (a disposable rebuild included), the
//! custody guard refuses anything the host's writer custody would have to be reset for, and
//! only then is a pristine copy staged and swapped in. The previous durable root is kept,
//! renamed, never deleted; the office's rebuildable projections are discarded before the
//! swap, so whatever root a crash leaves in place, they are rebuilt from it. A lost
//! `<organization-id>/` folder (both roots gone) is made again once the archive has verified.
//!
//! Everything a restore writes besides the restored root itself (the disposable check, the
//! staged copy and the replaced root, `before-<suffix>`) lives in one owner-only folder beside
//! the two roots, [`RESTORE_BACKUPS_DIR_NAME`], which agent sandboxes mask: each is a full
//! copy of the office's secrets. Nothing goes to the shared temporary folder.

use std::path::{Path, PathBuf};

use crate::envelope::Passphrase;
use crate::error::BackupError;
use crate::journal::{guard_custody, LiveJournal};
use crate::locate::{locate, take_locks};
use crate::manifest::{Limits, Manifest};
use crate::output::{private_tempdir_in, random_suffix, sync_dir};
use crate::read::{extract, Destination};
use crate::verify::{verify_in, Rebuilt};

pub use org_office::backup::RESTORE_BACKUPS_DIR_NAME;

/// What a restore covers and what it never does (`doc-91`: office relocation and host
/// custody are out of scope; copying files confers no authority).
pub const SCOPE_NOTICE: &str = "Scope: only this office's durable organization files were \
restored. Host-local custody is not in a backup and was not changed: the host's writer custody \
(the journal head it last confirmed), its organization registry and accounts, and the office \
secrets key in the Keychain or the system's credential store. Moving an office to another \
machine is not supported in v1. If the host confirmed a later journal head than this archive \
holds, the office refuses to start and needs explicit recovery; nothing was reset.";

/// What to restore.
#[derive(Debug)]
pub struct RestoreRequest<'a> {
    pub archive: &'a Path,
    pub passphrase: Option<&'a Passphrase>,
    pub office_home: &'a Path,
    pub org: &'a str,
    pub office: Option<&'a str>,
    pub limits: Limits,
}

/// What a restore did.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RestoreReport {
    pub manifest: Manifest,
    pub rebuilt: Rebuilt,
    pub durable: PathBuf,
    /// Where the durable root that was replaced now is
    /// (`<base>/restore-backups/before-<suffix>`).
    pub previous_durable: Option<PathBuf>,
    pub live_journal: LiveJournal,
}

/// Restores `request.archive` over its organization's stopped office.
pub fn restore_in_place(request: &RestoreRequest<'_>) -> Result<RestoreReport, BackupError> {
    let target = locate(request.office_home, request.org, request.office)?;
    let _locks = take_locks(&target)?;
    let base = target.base().to_path_buf();
    let backups = base.join(RESTORE_BACKUPS_DIR_NAME);
    let suffix = random_suffix()?;
    let work = check_folder(&target.office_home, &base, &backups)?;
    let (manifest, _, rebuilt) = verify_in(
        request.archive,
        request.passphrase,
        &request.limits,
        work.path(),
    )?;
    drop(work);
    if manifest.org_id != target.org_id
        || manifest.office_id != target.office_id
        || manifest.office_role != target.role()
    {
        return Err(BackupError::Refused(format!(
            "the archive is of office {} of organization {}, not office {} of organization {}; \
             nothing was changed",
            manifest.office_id, manifest.org_id, target.office_id, target.org_id
        )));
    }
    ensure_folder(&base)?;
    ensure_folder(&target.runtime)?;
    ensure_private_folder(&backups)?;
    let staging = Staging::new(backups.join(format!(".staging-{suffix}")))?;
    let staged = staging.path.join("durable");
    let again = extract(
        request.archive,
        request.passphrase,
        &request.limits,
        &Destination {
            durable: &staged,
            flush: true,
        },
    )?;
    if again != manifest {
        return Err(BackupError::invalid("it changed while it was restored"));
    }
    let live_journal = guard_custody(&target.durable, &target.identity, &staged, &manifest)?;
    // Discarded first: projections are rebuilt from whichever root is in place when the
    // office next opens, so a crash on either side of the swap leaves nothing stale.
    org_office::backup::discard_projections(&target.runtime, &target.office_id).map_err(
        BackupError::office("discarding the office's rebuildable projections"),
    )?;
    sync_dir(&staging.path)?;
    let previous_durable = swap_in(&target.durable, &staged, &backups, &suffix)?;
    Ok(RestoreReport {
        manifest,
        rebuilt,
        durable: target.durable.clone(),
        previous_durable,
        live_journal,
    })
}

/// Makes `folder` (owner-only, with any missing parents) when it is missing, flushing the
/// folder that now names it.
fn ensure_folder(folder: &Path) -> Result<(), BackupError> {
    match std::fs::symlink_metadata(folder) {
        Ok(meta) if meta.is_dir() => return Ok(()),
        Ok(_) => {
            return Err(BackupError::Refused(format!(
                "{} is not a folder; nothing was changed",
                folder.display()
            )))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(BackupError::io(format!("checking {}", folder.display()))(e)),
    }
    org_platform::fs::create_owner_only_dir_all(folder)
        .map_err(BackupError::io(format!("making {}", folder.display())))?;
    match folder.parent() {
        Some(parent) => sync_dir(parent),
        None => Ok(()),
    }
}

/// The owner-only disposable folder the archive is checked in, removed when dropped: in the
/// office's [`RESTORE_BACKUPS_DIR_NAME`] folder, or, when the folder holding both roots is
/// the very thing lost, in the office home (an archive that does not verify must leave no
/// `<organization-id>/` folder behind).
fn check_folder(
    office_home: &Path,
    base: &Path,
    backups: &Path,
) -> Result<tempfile::TempDir, BackupError> {
    let (parent, prefix) = match std::fs::symlink_metadata(base) {
        Ok(meta) if meta.is_dir() => {
            ensure_private_folder(backups)?;
            (backups, ".verify-")
        }
        Ok(_) => {
            return Err(BackupError::Refused(format!(
                "{} is not a folder; nothing was changed",
                base.display()
            )))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (office_home, ".restore-verify-"),
        Err(e) => return Err(BackupError::io(format!("checking {}", base.display()))(e)),
    };
    private_tempdir_in(parent, prefix).map_err(BackupError::io(format!(
        "making a disposable folder in {}",
        parent.display()
    )))
}

/// [`ensure_folder`], then owner-only whether it was made now or found.
fn ensure_private_folder(folder: &Path) -> Result<(), BackupError> {
    ensure_folder(folder)?;
    org_platform::fs::set_owner_only_dir(folder)
        .map_err(BackupError::io(format!("protecting {}", folder.display())))
}

/// A staging folder in the restore-backups folder, removed with what is left in it when
/// dropped.
struct Staging {
    path: PathBuf,
}

impl Staging {
    fn new(path: PathBuf) -> Result<Self, BackupError> {
        org_platform::fs::create_owner_only_dir(&path)
            .map_err(BackupError::io(format!("making {}", path.display())))?;
        Ok(Self { path })
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Renames the live root aside into `backups` (kept), renames the staged root into place,
/// and flushes both folders.
fn swap_in(
    live: &Path,
    staged: &Path,
    backups: &Path,
    suffix: &str,
) -> Result<Option<PathBuf>, BackupError> {
    let base = live.parent().unwrap_or(live);
    let previous = match std::fs::symlink_metadata(live) {
        Ok(meta) if meta.is_dir() => {
            let aside = backups.join(format!("before-{suffix}"));
            std::fs::rename(live, &aside)
                .map_err(BackupError::io(format!("moving {} aside", live.display())))?;
            Some(aside)
        }
        Ok(_) => {
            return Err(BackupError::Refused(format!(
                "{} is not a folder; nothing was changed",
                live.display()
            )))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(BackupError::io(format!("checking {}", live.display()))(e)),
    };
    if let Err(error) = std::fs::rename(staged, live) {
        // Put the live root back before reporting: the office is left as it was.
        if let Some(aside) = &previous {
            let _ = std::fs::rename(aside, live);
        }
        return Err(BackupError::io(format!(
            "moving the restored root to {}",
            live.display()
        ))(error));
    }
    sync_dir(base)?;
    sync_dir(backups)?;
    Ok(previous)
}
