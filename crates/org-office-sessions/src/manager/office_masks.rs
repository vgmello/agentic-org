//! The office's own trees every agent sandbox masks (`TASK-203`): each durable family, each
//! office-local companion store ([`org_office::companion_paths`], the attachment staging area
//! and blob outbox included, `TASK-98.1`, `TASK-98.6`), the restore backups beside
//! the two data roots ([`RESTORE_BACKUPS_DIR_NAME`]), and anything else found beside the
//! office's `agents/` folder or beside the data roots (a legacy `durable.before-restore-*`, a
//! store added later), so none of them is readable through `$HOME/..`.
//!
//! And the neighbours an office has on a host instance that serves several (`TASK-204`):
//! every other office's local folder under the same durable root (`local/offices/<id>/`) and
//! every office folder in the office home (`<office-home>/offices/`), whole; and, when the data
//! roots sit where the host puts them ([`Placement`]), every other organization's folder in the
//! data root and every other office of the organization: its HQ's data roots for a co-hosted
//! satellite, every co-hosted satellite (`<org>/offices/`, made before launch so a Linux `bwrap`
//! mask hides the ones that join later) for the HQ. Another office's runtime folder in the same
//! runtime root is masked with the runtime denies (`SessionManager::append_runtime_office_denies`).
//!
//! One list feeds both the deny globs every runtime gets
//! ([`SessionManager::denied_office_globs_for`]: Claude's `permissions.deny` and sandbox
//! `denyRead`, Codex's profile, the approval bridge that confines Agy) and the folders a Linux
//! `bwrap` sandbox pre-creates to mask atomically
//! ([`super::bwrap_isolation::ensure_bwrap_deny_dirs`]).
//!
//! What a running session can't see: the masks are computed at launch, so an organization or a
//! co-hosted satellite office created afterwards in a folder that isn't masked whole is masked
//! only from that session's next launch. The approval bridge keeps the launch-time globs too.
//!
//! [`SessionManager::denied_office_globs_for`]: super::SessionManager::denied_office_globs_for

use std::path::{Path, PathBuf};

use org_agent_runtime::PathPattern;
use org_office::backup::RESTORE_BACKUPS_DIR_NAME;
use org_office::companion_paths::{
    office_local_entries, CompanionKind, AGENTS_DIR_NAME, OFFICE_LOCAL_DIR,
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

/// The folder that holds one folder per office: below an organization's folder, its co-hosted
/// satellites' data roots (`<data-root>/<org-id>/offices/<office-id>/`, `org-host`'s
/// `satellite_data_roots`); below the office home, every office folder
/// (`org_office_launch::OFFICES_DIR_NAME`).
const OFFICES_DIR_NAME: &str = "offices";

/// The length of a UUID in text form.
const UUID_LEN: usize = 36;

/// A folder a sandbox masks whole.
pub(super) struct MaskedFolder {
    pub(super) path: PathBuf,
    /// Whether a Linux `bwrap` sandbox makes it (owner-only) before launch when it is
    /// missing, so its mask is a mount and never a hole or a stray file.
    pub(super) pre_create: bool,
}

/// What a sandbox masks of `office`'s durable side and of its neighbours.
pub(super) struct OfficeMasks {
    pub(super) folders: Vec<MaskedFolder>,
    /// Single files masked exactly (a companion store that is one file).
    pub(super) files: Vec<PathBuf>,
    /// Folders masked whole, raw and resolved, whether they exist yet or not.
    neighbours: Vec<PathBuf>,
    /// Folders whose entries are masked, each but the ones kept.
    sweeps: Vec<Sweep>,
    /// The office's own data roots: nothing that holds one is ever masked.
    own_roots: [PathBuf; 2],
}

impl OfficeMasks {
    pub(super) fn of(office: &Office) -> Self {
        let durable = office.durable_root();
        let info = office.info();
        let is_hq = info.role == OfficeRole::Hq;
        let offices_local = durable.join(OFFICE_LOCAL_DIR);
        let office_local = offices_local.join(&info.office_id);
        let mut folders: Vec<MaskedFolder> = DURABLE_FAMILIES
            .iter()
            .map(|family| MaskedFolder {
                path: durable.join(family),
                pre_create: is_hq || *family != HQ_ONLY_FAMILY,
            })
            .collect();
        let mut sweeps = vec![
            Sweep::all_but(&office_local, [AGENTS_DIR_NAME]),
            // Every other office's local folder under this durable root, whole: never made.
            Sweep::all_but(&offices_local, [info.office_id.as_str()]),
        ];
        if let Some(base) = durable.parent() {
            folders.push(MaskedFolder {
                path: base.join(RESTORE_BACKUPS_DIR_NAME),
                pre_create: true,
            });
            sweeps.push(Sweep::all_but(base, DATA_ROOT_NAMES));
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
        let mut neighbours: Vec<PathBuf> = office_folders(office.folder(), &info.office_id)
            .into_iter()
            .collect();
        if let Some(placement) = Placement::locate(
            durable,
            office.runtime_root(),
            &info.org_id,
            &info.office_id,
        ) {
            placement.add_masks(&mut folders, &mut neighbours, &mut sweeps);
        }
        Self {
            folders,
            files,
            neighbours,
            sweeps,
            own_roots: [durable.to_path_buf(), office.runtime_root().to_path_buf()],
        }
    }

    /// Every mask as deny patterns: each folder whole, each file exactly, each neighbour
    /// whole, and each entry on disk that nothing expects (beside `agents/`, beside the data
    /// roots, another office's or organization's).
    pub(super) fn push_denies(&self, out: &mut Vec<PathPattern>) {
        for folder in &self.folders {
            push_glob_pattern(out, &folder.path);
        }
        for file in &self.files {
            push_exact_pattern(out, file);
        }
        for neighbour in &self.neighbours {
            if !self.holds_own_root(neighbour) {
                push_whole_folder(out, neighbour);
            }
        }
        for sweep in &self.sweeps {
            self.push_swept_entries(out, sweep);
        }
    }

    /// The folders a Linux `bwrap` sandbox makes before launch.
    pub(super) fn pre_created(&self) -> impl Iterator<Item = &Path> {
        self.folders
            .iter()
            .filter(|folder| folder.pre_create)
            .map(|folder| folder.path.as_path())
    }

    /// Whether masking `path` would mask the office's own data roots (a layout this module
    /// doesn't expect): such a path is never masked, so a session never loses its own home.
    fn holds_own_root(&self, path: &Path) -> bool {
        self.own_roots.iter().any(|root| root.starts_with(path))
    }

    /// Masks every entry of `sweep.dir` the sweep doesn't keep, in name order (a stable spec
    /// hash): a folder whole, a file exactly, and a link as the link itself (never what it
    /// points to, which could be any folder a session needs).
    fn push_swept_entries(&self, out: &mut Vec<PathPattern>, sweep: &Sweep) {
        let Ok(entries) = std::fs::read_dir(&sweep.dir) else {
            return;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            if !sweep.keep.masks(entry.file_name().to_str()) {
                continue;
            }
            let path = entry.path();
            if self.holds_own_root(&path) {
                continue;
            }
            match entry.file_type() {
                Ok(kind) if kind.is_symlink() => push_raw_pattern(out, &path.display().to_string()),
                Ok(kind) if kind.is_dir() => push_whole_folder(out, &path),
                _ => push_exact_pattern(out, &path),
            }
        }
    }
}

/// A folder whose entries are masked, all but some.
struct Sweep {
    dir: PathBuf,
    keep: Keep,
}

impl Sweep {
    fn all_but<'a>(dir: &Path, names: impl IntoIterator<Item = &'a str>) -> Self {
        Self {
            dir: dir.to_path_buf(),
            keep: Keep::Named(names.into_iter().map(str::to_owned).collect()),
        }
    }
}

/// Which entries of a swept folder stay unmasked.
enum Keep {
    /// The entries with these names; every other entry is masked, one whose name isn't UTF-8
    /// included.
    Named(Vec<String>),
    /// Every entry but another organization's folder: one named by a UUID (any case, with or
    /// without a suffix such as a leftover's) other than `own`. The data root also holds the
    /// host's own folders (`office-state`, which holds the office sockets), which stay.
    AllButOtherOrganizations { own: String },
}

impl Keep {
    fn masks(&self, name: Option<&str>) -> bool {
        match self {
            Self::Named(kept) => !name.is_some_and(|name| kept.iter().any(|kept| kept == name)),
            Self::AllButOtherOrganizations { own } => {
                name.is_some_and(|name| name != own && names_an_organization(name))
            }
        }
    }
}

/// Where an office's data roots sit among the organizations and offices co-hosted on one host
/// instance (`TASK-204`), as `org-host` lays them out: `<data-root>/<org-id>/{durable,runtime}`
/// for the organization's HQ, `<data-root>/<org-id>/offices/<office-id>/{durable,runtime}` for
/// a satellite. Derived from the roots and the office's own ids alone, so roots laid out any
/// other way (a test's bare temporary folder, a development root) have no neighbours here and
/// nothing beside them is swept.
#[derive(Debug, PartialEq, Eq)]
struct Placement {
    /// The folder that holds every organization's folder on this host instance.
    data_root: PathBuf,
    /// `<data-root>/<org-id>`.
    org_root: PathBuf,
    org_id: String,
    /// The office's id when it is a co-hosted satellite (data roots in
    /// `<org-root>/offices/<office-id>/`); `None` for the office at the organization's folder.
    satellite: Option<String>,
}

impl Placement {
    fn locate(durable: &Path, runtime: &Path, org_id: &str, office_id: &str) -> Option<Self> {
        if !org_proto::is_uuid_v7(org_id) || !org_proto::is_uuid_v7(office_id) {
            return None;
        }
        let base = durable.parent()?;
        if !is_named(durable, DATA_ROOT_NAMES[0])
            || !is_named(runtime, DATA_ROOT_NAMES[1])
            || runtime.parent() != Some(base)
        {
            return None;
        }
        if is_named(base, org_id) {
            return Some(Self {
                data_root: base.parent()?.to_path_buf(),
                org_root: base.to_path_buf(),
                org_id: org_id.to_owned(),
                satellite: None,
            });
        }
        let offices = base.parent()?;
        let org_root = offices.parent()?;
        if !is_named(base, office_id)
            || !is_named(offices, OFFICES_DIR_NAME)
            || !is_named(org_root, org_id)
        {
            return None;
        }
        Some(Self {
            data_root: org_root.parent()?.to_path_buf(),
            org_root: org_root.to_path_buf(),
            org_id: org_id.to_owned(),
            satellite: Some(office_id.to_owned()),
        })
    }

    /// Every other organization's folder, whole; for a satellite its HQ's data roots (masked
    /// even before they exist) and every other entry of the organization's folder and of its
    /// `offices/`; for the HQ every co-hosted satellite, made before a Linux `bwrap` launch so
    /// a satellite that joins later is masked too.
    fn add_masks(
        self,
        folders: &mut Vec<MaskedFolder>,
        neighbours: &mut Vec<PathBuf>,
        sweeps: &mut Vec<Sweep>,
    ) {
        sweeps.push(Sweep {
            dir: self.data_root,
            keep: Keep::AllButOtherOrganizations { own: self.org_id },
        });
        let satellites = self.org_root.join(OFFICES_DIR_NAME);
        match self.satellite {
            None => folders.push(MaskedFolder {
                path: satellites,
                pre_create: true,
            }),
            Some(own) => {
                neighbours.extend(DATA_ROOT_NAMES.map(|root| self.org_root.join(root)));
                sweeps.push(Sweep::all_but(&self.org_root, [OFFICES_DIR_NAME]));
                sweeps.push(Sweep::all_but(&satellites, [own.as_str()]));
            }
        }
    }
}

/// `<office-home>/offices/` when `folder` is the host's `<office-home>/offices/<office-id>`:
/// every office's folder on this host (its `git-credentials/` included), none of which a
/// session needs, its own being masked whole too.
fn office_folders(folder: &Path, office_id: &str) -> Option<PathBuf> {
    let offices = folder.parent()?;
    (is_named(folder, office_id) && is_named(offices, OFFICES_DIR_NAME))
        .then(|| offices.to_path_buf())
}

/// Whether `path`'s last component is `name`.
fn is_named(path: &Path, name: &str) -> bool {
    path.file_name().is_some_and(|last| last == name)
}

/// Whether `name` starts with a UUID in text form (hex digits of any case), as an
/// organization's folder in the data root is named.
fn names_an_organization(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() >= UUID_LEN
        && bytes[..UUID_LEN]
            .iter()
            .enumerate()
            .all(|(at, byte)| match at {
                8 | 13 | 18 | 23 => *byte == b'-',
                _ => byte.is_ascii_hexdigit(),
            })
}

/// Masks `path` and everything below it, by its path as given and as resolved.
fn push_whole_folder(out: &mut Vec<PathPattern>, path: &Path) {
    push_exact_pattern(out, path);
    push_glob_pattern(out, path);
    push_raw_pattern(out, &format!("{}/**", path.display()));
}

#[cfg(test)]
#[path = "office_masks_tests.rs"]
mod tests;
