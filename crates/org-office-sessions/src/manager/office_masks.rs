//! The office's own trees every agent sandbox masks (`TASK-203`): each durable family, each
//! office-local companion store ([`org_office::companion_paths`], the attachment staging area
//! and blob outbox included, and every sibling office's staging area and blob outbox,
//! `TASK-98.1`, `TASK-98.6`), the restore backups beside
//! the two data roots ([`RESTORE_BACKUPS_DIR_NAME`]), and anything else found beside the
//! office's `agents/` folder or beside the data roots (a legacy `durable.before-restore-*`, a
//! store added later), so none of them is readable through `$HOME/..`. One list feeds both
//! the deny globs every runtime gets ([`SessionManager::denied_office_globs_for`]) and the
//! folders a Linux `bwrap` sandbox pre-creates to mask atomically
//! ([`super::bwrap_isolation::ensure_bwrap_deny_dirs`]).
//!
//! [`SessionManager::denied_office_globs_for`]: super::SessionManager::denied_office_globs_for

use std::path::{Path, PathBuf};

use org_agent_runtime::PathPattern;
use org_office::backup::RESTORE_BACKUPS_DIR_NAME;
use org_office::companion_paths::{
    office_local_entries, CompanionKind, AGENTS_DIR_NAME, ATTACHMENT_STAGING_DIR_NAME,
    BLOB_OUTBOX_DIR_NAME, OFFICE_LOCAL_DIR,
};
use org_office::{Office, OfficeRole};

use super::sandbox_rules::{push_exact_pattern, push_glob_pattern, push_raw_pattern};

/// The durable families every session masks whole.
const DURABLE_FAMILIES: [&str; 10] = [
    "state",
    "journal",
    "authority",
    "private",
    "messaging",
    "budgets",
    "accounting",
    "recovery",
    "blobs",
    "checkpoints",
];

/// The one durable family a satellite never holds and never gets made (`ARCH-02`).
const HQ_ONLY_FAMILY: &str = "private";

/// The names of the two data roots in the folder that holds both.
const DATA_ROOT_NAMES: [&str; 2] = ["durable", "runtime"];

/// A folder a sandbox masks whole.
pub(super) struct MaskedFolder {
    pub(super) path: PathBuf,
    /// Whether a Linux `bwrap` sandbox makes it (owner-only) before launch when it is
    /// missing, so its mask is a mount and never a hole or a stray file.
    pub(super) pre_create: bool,
}

/// What a sandbox masks of `office`'s durable side.
pub(super) struct OfficeMasks {
    pub(super) folders: Vec<MaskedFolder>,
    /// Single files masked exactly (a companion store that is one file).
    pub(super) files: Vec<PathBuf>,
    /// `durable/local/offices/<office-id>/`.
    office_local: PathBuf,
    /// The folder that holds both data roots.
    base: Option<PathBuf>,
}

impl OfficeMasks {
    pub(super) fn of(office: &Office) -> Self {
        let durable = office.durable_root();
        let is_hq = office.info().role == OfficeRole::Hq;
        let office_local = durable
            .join(OFFICE_LOCAL_DIR)
            .join(&office.info().office_id);
        let base = durable.parent().map(Path::to_path_buf);
        let mut folders: Vec<MaskedFolder> = DURABLE_FAMILIES
            .iter()
            .map(|family| MaskedFolder {
                path: durable.join(family),
                pre_create: is_hq || *family != HQ_ONLY_FAMILY,
            })
            .collect();
        if let Some(base) = &base {
            folders.push(MaskedFolder {
                path: base.join(RESTORE_BACKUPS_DIR_NAME),
                pre_create: true,
            });
        }
        let mut files = Vec::new();
        for (entry, kind) in office_local_entries() {
            if entry == AGENTS_DIR_NAME {
                continue;
            }
            match kind {
                CompanionKind::Folder => folders.push(MaskedFolder {
                    path: office_local.join(entry),
                    pre_create: true,
                }),
                CompanionKind::File => files.push(office_local.join(entry)),
            }
        }
        push_sibling_file_stores(
            &mut folders,
            &durable.join(OFFICE_LOCAL_DIR),
            &office.info().office_id,
        );
        Self {
            folders,
            files,
            office_local,
            base,
        }
    }

    /// Every mask as deny patterns: each folder whole, each file exactly, and each entry on
    /// disk that nothing expects (beside `agents/`, or beside the data roots).
    pub(super) fn push_denies(&self, out: &mut Vec<PathPattern>) {
        for folder in &self.folders {
            push_glob_pattern(out, &folder.path);
        }
        for file in &self.files {
            push_exact_pattern(out, file);
        }
        push_unexpected_entries(out, &self.office_local, &[AGENTS_DIR_NAME]);
        if let Some(base) = &self.base {
            push_unexpected_entries(out, base, &DATA_ROOT_NAMES);
        }
    }

    /// The folders a Linux `bwrap` sandbox makes before launch.
    pub(super) fn pre_created(&self) -> impl Iterator<Item = &Path> {
        self.folders
            .iter()
            .filter(|folder| folder.pre_create)
            .map(|folder| folder.path.as_path())
    }
}

/// The file stores of every office that hold uploaded bytes: staged uploads (`TASK-98.1`) and
/// shared files waiting for HQ (`TASK-98.6`).
const FILE_STORES: [&str; 2] = [ATTACHMENT_STAGING_DIR_NAME, BLOB_OUTBOX_DIR_NAME];

/// Every other office's file stores under the same durable root
/// (`local/offices/*/attachment-staging/` and `local/offices/*/blob-outbox/`), masked whole.
/// Never pre-created: a sandbox makes nothing in another office's folder.
fn push_sibling_file_stores(folders: &mut Vec<MaskedFolder>, offices: &Path, own: &str) {
    let Ok(entries) = std::fs::read_dir(offices) else {
        return;
    };
    for entry in entries.flatten() {
        let is_own = entry.file_name().to_str() == Some(own);
        let is_folder = entry.file_type().is_ok_and(|kind| kind.is_dir());
        if !is_own && is_folder {
            for store in FILE_STORES {
                folders.push(MaskedFolder {
                    path: entry.path().join(store),
                    pre_create: false,
                });
            }
        }
    }
}

/// Masks every entry of `dir` not named in `expected`: a folder whole and exactly, a file
/// exactly, and a link as the link itself (never what it points to, which could be any
/// folder a session needs).
fn push_unexpected_entries(out: &mut Vec<PathPattern>, dir: &Path, expected: &[&str]) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| expected.contains(&name))
        {
            continue;
        }
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_symlink() => push_raw_pattern(out, &path.display().to_string()),
            Ok(kind) if kind.is_dir() => {
                push_exact_pattern(out, &path);
                push_glob_pattern(out, &path);
            }
            _ => push_exact_pattern(out, &path),
        }
    }
}
