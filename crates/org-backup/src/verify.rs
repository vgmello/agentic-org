//! `restore --verify`: proving an archive restores. It is read and extracted into a
//! disposable folder (every member checked against the manifest), the private task notes
//! copy passes SQLite's integrity check, the journal verifies to the head the manifest
//! records, and the office's projections are rebuilt from it on a disposable runtime root.
//! With `--to`, the archive is then extracted once more, untouched by the rebuild, into a
//! new folder of the caller's.

use std::path::{Path, PathBuf};

use org_office::backup::{check_task_notes, rebuild_disposable, DisposableRebuild};

use crate::envelope::{ArchiveKind, Passphrase};
use crate::error::BackupError;
use crate::journal::check_restored;
use crate::manifest::{Limits, Manifest};
use crate::output::sync_dir;
use crate::paths::{RelPath, NOTES_DB};
use crate::read::{archive_kind, extract, Destination};

/// What to verify.
#[derive(Debug)]
pub struct VerifyRequest<'a> {
    pub archive: &'a Path,
    pub passphrase: Option<&'a Passphrase>,
    /// A new folder to extract the verified archive into (`<to>/durable`, `<to>/manifest.json`).
    pub to: Option<&'a Path>,
    /// Where the disposable copy is made [default: the system's temporary folder].
    pub work_dir: Option<&'a Path>,
    pub limits: Limits,
}

/// What the projections rebuilt from the archive answered.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Rebuilt {
    pub head_ordinal: u64,
    pub stopped_cleanly: bool,
    pub offices: usize,
    pub employees: usize,
    pub projects: usize,
}

/// What a verification found.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct VerifyReport {
    pub encrypted: bool,
    pub manifest: Manifest,
    pub private_notes_checked: bool,
    pub rebuilt: Rebuilt,
    pub extracted_to: Option<PathBuf>,
}

/// Verifies `request.archive`.
pub fn verify(request: &VerifyRequest<'_>) -> Result<VerifyReport, BackupError> {
    let encrypted = archive_kind(request.archive)? == ArchiveKind::Encrypted;
    if encrypted && request.passphrase.is_none() {
        return Err(BackupError::PassphraseRequired);
    }
    // `--to` is checked first and made only once the archive has verified, so a refused
    // archive leaves nothing behind.
    if let Some(to) = request.to {
        check_new_folder(to)?;
    }
    let work = match request.work_dir {
        Some(dir) => tempfile::Builder::new().prefix(".verify-").tempdir_in(dir),
        None => tempfile::Builder::new()
            .prefix("agentic-org-verify-")
            .tempdir(),
    }
    .map_err(BackupError::io("making a disposable folder"))?;
    let (manifest, notes, rebuilt) = verify_in(
        request.archive,
        request.passphrase,
        &request.limits,
        work.path(),
    )?;
    drop(work);
    let to = match request.to {
        Some(to) => {
            let (to, made) = new_folder(to)?;
            if let Err(error) = extract_to(request, &to, &manifest) {
                discard_output(&to, made);
                return Err(error);
            }
            Some(to)
        }
        None => None,
    };
    Ok(VerifyReport {
        encrypted,
        manifest,
        private_notes_checked: notes,
        rebuilt,
        extracted_to: to,
    })
}

/// Extracts and checks the archive below `work` (a folder of the caller's that it discards),
/// and rebuilds every projection from the copy.
pub(crate) fn verify_in(
    archive: &Path,
    passphrase: Option<&Passphrase>,
    limits: &Limits,
    work: &Path,
) -> Result<(Manifest, bool, Rebuilt), BackupError> {
    let durable = work.join("durable");
    let runtime = work.join("runtime");
    let manifest = extract(
        archive,
        passphrase,
        limits,
        &Destination {
            durable: &durable,
            flush: false,
        },
    )?;
    let notes = RelPath::parse(NOTES_DB).map_err(BackupError::Refused)?;
    let has_notes = manifest.files.iter().any(|f| f.path == notes);
    if has_notes {
        check_task_notes(&notes.under(&durable))
            .map_err(|e| BackupError::invalid(format!("its private task notes database: {e}")))?;
    }
    let (identity, head) = check_restored(&durable, &manifest)?;
    org_platform::fs::create_owner_only_dir(&runtime)
        .map_err(BackupError::io(format!("making {}", runtime.display())))?;
    let spec = DisposableRebuild {
        durable_root: durable,
        runtime_root: runtime,
        identity,
        pinned: Some(head),
        is_hq: manifest.office_role == "hq",
    };
    let (report, ()) = rebuild_disposable(&spec, |_| Ok(()))
        .map_err(|e| BackupError::invalid(format!("its office does not open from it: {e}")))?;
    let rebuilt = Rebuilt {
        head_ordinal: report.head_ordinal,
        stopped_cleanly: report.stopped_cleanly,
        offices: report.offices,
        employees: report.employees,
        projects: report.projects,
    };
    Ok((manifest, has_notes, rebuilt))
}

/// Refuses a `--to` that is not a new or empty folder.
fn check_new_folder(path: &Path) -> Result<(), BackupError> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(meta) if meta.is_dir() => {
            let mut listing = std::fs::read_dir(path)
                .map_err(BackupError::io(format!("listing {}", path.display())))?;
            match listing.next() {
                None => Ok(()),
                Some(_) => Err(BackupError::Usage(format!(
                    "{} is not empty; --to needs a new or empty folder",
                    path.display()
                ))),
            }
        }
        Ok(_) => Err(BackupError::Usage(format!(
            "{} is not a folder",
            path.display()
        ))),
        Err(e) => Err(BackupError::io(format!("checking {}", path.display()))(e)),
    }
}

/// `path` as a new owner-only folder: it must not exist, or be an empty folder. Returns it
/// with whether it was made here.
fn new_folder(path: &Path) -> Result<(PathBuf, bool), BackupError> {
    let path = std::path::absolute(path)
        .map_err(BackupError::io(format!("resolving {}", path.display())))?;
    match std::fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_dir() => {
            let mut listing = std::fs::read_dir(&path)
                .map_err(BackupError::io(format!("listing {}", path.display())))?;
            if listing.next().is_some() {
                return Err(BackupError::Usage(format!(
                    "{} is not empty; --to needs a new or empty folder",
                    path.display()
                )));
            }
            org_platform::fs::set_owner_only_dir(&path)
                .map_err(BackupError::io(format!("protecting {}", path.display())))?;
            Ok((path, false))
        }
        Ok(_) => Err(BackupError::Usage(format!(
            "{} is not a folder",
            path.display()
        ))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            org_platform::fs::create_owner_only_dir(&path)
                .map_err(BackupError::io(format!("making {}", path.display())))?;
            Ok((path, true))
        }
        Err(e) => Err(BackupError::io(format!("checking {}", path.display()))(e)),
    }
}

/// After a failed extraction into `to`: what it wrote is removed, and `to` too when it was
/// made for it, so no partial copy is left to be mistaken for a verified one.
fn discard_output(to: &Path, made: bool) {
    if made {
        let _ = std::fs::remove_dir_all(to);
        return;
    }
    let _ = std::fs::remove_dir_all(to.join("durable"));
    let _ = std::fs::remove_file(to.join("manifest.json"));
}

/// The second, pristine extraction into `to`, with the manifest beside it.
fn extract_to(
    request: &VerifyRequest<'_>,
    to: &Path,
    verified: &Manifest,
) -> Result<(), BackupError> {
    let manifest = extract(
        request.archive,
        request.passphrase,
        &request.limits,
        &Destination {
            durable: &to.join("durable"),
            flush: true,
        },
    )?;
    if &manifest != verified {
        return Err(BackupError::invalid("it changed while it was verified"));
    }
    let mut bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| BackupError::Refused(format!("encoding the manifest: {e}")))?;
    bytes.push(b'\n');
    org_platform::fs::write_owner_only_file(&to.join("manifest.json"), &bytes).map_err(
        BackupError::io(format!("writing {}", to.join("manifest.json").display())),
    )?;
    sync_dir(to)
}
