//! `restore <archive> --org <org>`: putting a verified archive back as a stopped office's
//! durable root. The archive is verified in full first (a disposable rebuild included), the
//! custody guard refuses anything the host's writer custody would have to be reset for, and
//! only then is a pristine copy staged beside the live root and swapped in. The previous
//! durable root is kept, renamed, never deleted; the office's rebuildable projections are
//! discarded before the swap, so whatever root a crash leaves in place, they are rebuilt from
//! it. A lost `<organization-id>/` folder (both roots gone) is made again once the archive
//! has verified.

use std::path::{Path, PathBuf};

use crate::envelope::Passphrase;
use crate::error::BackupError;
use crate::journal::{guard_custody, LiveJournal};
use crate::locate::{locate, take_locks};
use crate::manifest::{Limits, Manifest};
use crate::output::{random_suffix, sync_dir};
use crate::read::{extract, Destination};
use crate::verify::{verify_in, Rebuilt};

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
    /// Where the durable root that was replaced now is.
    pub previous_durable: Option<PathBuf>,
    pub live_journal: LiveJournal,
}

/// Restores `request.archive` over its organization's stopped office.
pub fn restore_in_place(request: &RestoreRequest<'_>) -> Result<RestoreReport, BackupError> {
    let target = locate(request.office_home, request.org, request.office)?;
    let _locks = take_locks(&target)?;
    let base = target.base().to_path_buf();
    let suffix = random_suffix()?;
    // The check runs apart from the office's folders, which may be the very thing lost.
    let work = tempfile::Builder::new()
        .prefix("agentic-org-restore-check-")
        .tempdir()
        .map_err(BackupError::io("making a disposable folder"))?;
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
    let staging = Staging::new(base.join(format!(".restore-{suffix}")))?;
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
    let previous_durable = swap_in(&target.durable, &staged, &base, &suffix)?;
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

/// A staging folder beside the live root, removed with what is left in it when dropped.
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

/// Renames the live root aside (kept), renames the staged root into place, and flushes the
/// folder that holds both.
fn swap_in(
    live: &Path,
    staged: &Path,
    base: &Path,
    suffix: &str,
) -> Result<Option<PathBuf>, BackupError> {
    let previous = match std::fs::symlink_metadata(live) {
        Ok(meta) if meta.is_dir() => {
            let aside = base.join(format!("durable.before-restore-{suffix}"));
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
    Ok(previous)
}
