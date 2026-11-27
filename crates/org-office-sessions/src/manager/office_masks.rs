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
//! Every mask goes through one place ([`OfficeMasks::push_mask`]): a path is looked at without
//! following it (`lstat`, after dropping a trailing `/` or `/.`), a link is masked as the link
//! itself and never through or as what it points to, and nothing that holds the office's own
//! data roots, as given or as resolved (`/var` and `/private/var` on macOS), is ever masked.
//!
//! What a running session can't see: the masks are computed at launch, so an organization or a
//! co-hosted satellite office created afterwards in a folder that isn't masked whole is masked
//! only from that session's next launch. The approval bridge keeps the launch-time globs too.
//!
//! [`SessionManager::denied_office_globs_for`]: super::SessionManager::denied_office_globs_for

use std::path::{Component, Path, PathBuf};

use org_agent_runtime::PathPattern;
use org_office::backup::RESTORE_BACKUPS_DIR_NAME;
use org_office::companion_paths::{
    office_local_entries, CompanionKind, AGENTS_DIR_NAME, OFFICE_LOCAL_DIR,
};
use org_office::{Office, OfficeRole};

use super::rust_toolchain::canonicalize_with_tail;
use super::sandbox_rules::push_raw_pattern;

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
    /// The office's own data roots, as given and as resolved ([`own_root_forms`]): nothing
    /// that holds one is ever masked.
    own_roots: Vec<PathBuf>,
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
            own_roots: own_root_forms([durable, office.runtime_root()]),
        }
    }

    /// Every mask as deny patterns: each folder whole, each file exactly, each neighbour
    /// whole, and each entry on disk that nothing expects (beside `agents/`, beside the data
    /// roots, another office's or organization's).
    pub(super) fn push_denies(&self, out: &mut Vec<PathPattern>) {
        for folder in &self.folders {
            self.push_mask(out, &folder.path, Shape::Below);
        }
        for file in &self.files {
            self.push_mask(out, file, Shape::Exact);
        }
        for neighbour in &self.neighbours {
            self.push_mask(out, neighbour, Shape::Whole);
        }
        for sweep in &self.sweeps {
            self.push_swept_entries(out, sweep);
        }
    }

    /// The folders a Linux `bwrap` sandbox makes before launch: never one holding the office's
    /// own roots, which is never masked.
    pub(super) fn pre_created(&self) -> impl Iterator<Item = &Path> {
        self.folders
            .iter()
            .filter(|folder| folder.pre_create && !self.holds_own_root(&folder.path))
            .map(|folder| folder.path.as_path())
    }

    /// Whether masking `path` would mask the office's own data roots (a layout this module
    /// doesn't expect): such a path is never masked, so a session never loses its own home.
    /// `path` is compared as given and with its parent resolved, never following `path` itself
    /// (a link is masked as the link), against each root as given and as resolved, so a root
    /// reached through a linked parent (`/var` and `/private/var` on macOS) is found either way.
    fn holds_own_root(&self, path: &Path) -> bool {
        let path = normalized(path);
        let beside = resolved_beside(&path);
        self.own_roots
            .iter()
            .any(|root| root.starts_with(&path) || root.starts_with(&beside))
    }

    /// Whether `link`'s resolved target overlaps any of the office's own roots (`root` is a
    /// prefix of `target` or `target` is a prefix of `root`).
    ///
    /// Live Claude Code (`bwrap` and macOS Seatbelt) and Codex (`Seatbelt`/`bwrap`) dereference
    /// leaf symlinks in deny lists via `realpath`/`canonicalize`, so emitting a bare symlink
    /// pointing to `/`, `<data-root>`, `<own_org>`, `<own_durable>`, `<own_runtime>`, or
    /// `<own_home>` breaks the agent's own session. Omitting such a link is safe: every
    /// sensitive tree inside `own_roots` is already masked directly at its canonical path,
    /// which `bwrap`, Seatbelt, and `resolve_path_for_deny` enforce regardless of traversal.
    fn link_overlaps_own_root(&self, link: &Path) -> bool {
        let target = match std::fs::canonicalize(link) {
            Ok(real) => Some(real),
            Err(_) => resolve_dangling_target(link),
        };
        let Some(target) = target else {
            return false;
        };
        self.own_roots
            .iter()
            .any(|root| root.starts_with(&target) || target.starts_with(root))
    }

    /// The one place a mask is made. `path` is looked at without following it (after dropping
    /// a trailing `/` or `/.`, as `lstat("link/")` follows the link): a link whose target
    /// overlaps the office's own roots is omitted (so sandboxes that dereference deny entries
    /// never wipe the session's own home or worktree); any other link is masked as the link
    /// itself ([`push_symlink_pattern`]), never through and never as what it points to.
    /// Anything else is masked in its `shape`. A path holding the office's own roots is never
    /// masked.
    fn push_mask(&self, out: &mut Vec<PathPattern>, path: &Path, shape: Shape) {
        let path = normalized(path);
        if self.holds_own_root(&path) {
            return;
        }
        let is_link =
            std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink());
        if is_link && self.link_overlaps_own_root(&path) {
            return;
        }
        match shape {
            Shape::Below if !is_link => {
                push_raw_pattern(out, &format!("{}/**", resolved_beside(&path).display()))
            }
            Shape::Whole if !is_link => push_whole_folder(out, &path),
            _ => push_symlink_pattern(out, &path),
        }
    }

    /// Masks every entry of `sweep.dir` the sweep doesn't keep, in name order (a stable spec
    /// hash): a folder whole, anything else (a file, a socket, a link) exactly.
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
            // Seen without following it; `push_mask` looks again, so an entry swapped for a
            // link since is masked as the link.
            let shape = if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                Shape::Whole
            } else {
                Shape::Exact
            };
            self.push_mask(out, &entry.path(), shape);
        }
    }
}

/// How a path that isn't a link is masked.
#[derive(Clone, Copy)]
enum Shape {
    /// Everything below it (a durable family, a companion store folder).
    Below,
    /// It and everything below it (a neighbour, a swept folder).
    Whole,
    /// It alone (a one-file store, a swept file).
    Exact,
}

/// Each of the office's own data roots as given and as resolved through every link, its own
/// last component included (what the session really uses), or, when it doesn't exist yet,
/// through its nearest existing ancestor.
fn own_root_forms(roots: [&Path; 2]) -> Vec<PathBuf> {
    let mut forms = Vec::with_capacity(4);
    for root in roots {
        let root = normalized(root);
        let resolved = canonicalize_with_tail(&root);
        for form in [root, resolved] {
            if !forms.contains(&form) {
                forms.push(form);
            }
        }
    }
    forms
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
    /// The satellite layout is checked first, so roots that read as both (an office id equal
    /// to its organization's) are a satellite's, the narrower of the two.
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
        let satellite_org_root = base
            .parent()
            .filter(|offices| is_named(base, office_id) && is_named(offices, OFFICES_DIR_NAME))
            .and_then(Path::parent)
            .filter(|org_root| is_named(org_root, org_id));
        let (org_root, satellite) = match satellite_org_root {
            Some(org_root) => (org_root, Some(office_id.to_owned())),
            None if is_named(base, org_id) => (base, None),
            None => return None,
        };
        Some(Self {
            data_root: org_root.parent()?.to_path_buf(),
            org_root: org_root.to_path_buf(),
            org_id: org_id.to_owned(),
            satellite,
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

/// Masks `path` (not a link) and everything below it, by its path as given and with its
/// parent resolved ([`resolved_beside`]).
fn push_whole_folder(out: &mut Vec<PathPattern>, path: &Path) {
    let beside = resolved_beside(path);
    push_raw_pattern(out, &path.display().to_string());
    push_raw_pattern(out, &beside.display().to_string());
    push_raw_pattern(out, &format!("{}/**", beside.display()));
    push_raw_pattern(out, &format!("{}/**", path.display()));
}

/// Masks `path` itself, exactly: as given and with its parent resolved ([`resolved_beside`]).
/// A link is masked as the link, never through (`link/**`) and never as what it points to.
fn push_symlink_pattern(out: &mut Vec<PathPattern>, path: &Path) {
    push_raw_pattern(out, &path.display().to_string());
    push_raw_pattern(out, &resolved_beside(path).display().to_string());
}

/// `path` with its parent resolved through links and its last component kept as it is, so a
/// link `path` ends in is never followed (`/var/x/link` is `/private/var/x/link` on macOS).
/// A path that doesn't end in a name (`/`, `x/..`) has nothing to keep and is resolved whole.
fn resolved_beside(path: &Path) -> PathBuf {
    match (path.parent(), path.components().next_back()) {
        (Some(parent), Some(Component::Normal(name))) => canonicalize_with_tail(parent).join(name),
        _ => canonicalize_with_tail(path),
    }
}

/// `path` without a trailing `/` or `/.` (or a `.` inside), so looking at it with `lstat`
/// never follows a link it ends in.
fn normalized(path: &Path) -> PathBuf {
    path.components().collect()
}

/// Collapses `.` and `..` lexically without touching the filesystem.
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                } else if !matches!(out.components().next_back(), Some(Component::RootDir)) {
                    out.push(comp);
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// Resolves a dangling symlink chain (bounded, cycle-safe) against existing ancestors so a
/// dangling link targeting `<own_home>/future` is detected before it becomes live, while a
/// symlink loop returns `None` and remains masked as a link.
fn resolve_dangling_target(link: &Path) -> Option<PathBuf> {
    let mut cur = link.to_path_buf();
    let mut seen = Vec::with_capacity(8);
    for _ in 0..8 {
        let Ok(target) = std::fs::read_link(&cur) else {
            return (cur != link).then(|| canonicalize_with_tail(&cur));
        };
        seen.push(cur.clone());
        let next = if target.is_absolute() {
            target
        } else {
            let base = cur
                .parent()
                .map_or_else(PathBuf::new, canonicalize_with_tail);
            base.join(target)
        };
        cur = lexical_normalize(&next);
        if seen.contains(&cur) {
            return None;
        }
    }
    None
}

#[cfg(test)]
#[path = "office_masks_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "office_masks_link_tests.rs"]
mod link_tests;
