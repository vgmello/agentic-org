//! Office durable and co-hosted neighbour sandbox masks (`TASK-203`, `TASK-204`, `TASK-245`,
//! `TASK-246`): durable families, companion stores (`org_office::companion_paths`), restore
//! backups, unexpected entries beside `agents/` or the data roots, co-hosted sibling orgs and
//! satellites (`Placement`), and `<office-home>` / `<data-root>/office-state/host-registry.*`
//! (keeping `<office-home>/run/` reachable for sockets). Every mask goes through
//! [`OfficeMasks::push_mask`] (`lstat`, never following leaf symlinks, never masking own roots).
//! Warm sessions re-check [`OfficeMasks::topology_digest`] on reuse (`TASK-245`) and recycle idle
//! handles with `"sandbox_topology_changed"` (failing closed while busy).

use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use org_agent_runtime::PathPattern;
use org_office::backup::RESTORE_BACKUPS_DIR_NAME;
use org_office::companion_paths::{
    office_local_entries, CompanionKind, AGENTS_DIR_NAME, OFFICE_LOCAL_DIR,
};
use org_office::{Office, OfficeRole};
use ring::digest::{Context, SHA256};

use super::rust_toolchain::canonicalize_with_tail;
use super::sandbox_rules::push_raw_pattern;

/// Durable families masked whole; `private` is HQ-only (`ARCH-02`).
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
const HQ_ONLY_FAMILY: &str = "private";
const DATA_ROOT_NAMES: [&str; 2] = ["durable", "runtime"];
const OFFICES_DIR_NAME: &str = "offices";
const UUID_LEN: usize = 36;
const OFFICE_STATE_DIR_NAME: &str = "office-state";
const HOST_REGISTRY_PREFIX: &str = "host-registry.";
/// Host-registry DB, SQLite sidecars/temps, and sentinel masked under `<office-home>` (`TASK-246`).
const HOST_REGISTRY_FILES: [&str; 9] = [
    "host-registry.db",
    "host-registry.db-wal",
    "host-registry.db-shm",
    "host-registry.db-journal",
    "host-registry.db.tmp",
    "host-registry.db.tmp-wal",
    "host-registry.db.tmp-shm",
    "host-registry.db.tmp-journal",
    "host-registry.created",
];

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
        if let Some(home) = office_home(office.folder(), &info.office_id) {
            push_host_registry_masks(&home, &mut files, &mut sweeps);
        }
        if let Some(placement) = Placement::locate(
            durable,
            office.runtime_root(),
            &info.org_id,
            &info.office_id,
        ) {
            placement.add_masks(&mut folders, &mut files, &mut neighbours, &mut sweeps);
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

    /// SHA-256 digest of dynamic swept neighbours (`TASK-245`) plus, when
    /// `track_absent_static_masks` is true (Linux `bwrap`), presence bits for static masks.
    /// Returns `None` on unexpected I/O errors so callers fail closed.
    pub(super) fn topology_digest(&self, track_absent_static_masks: bool) -> Option<String> {
        let static_paths = self.static_mask_paths();
        let mut ctx = Context::new(&SHA256);
        for sweep in &self.sweeps {
            hash_path(&mut ctx, b"sweep:", &sweep.dir);
            self.hash_sweep_entries(&mut ctx, sweep, &static_paths)?;
        }
        if track_absent_static_masks {
            self.hash_static_presence(&mut ctx)?;
        }
        let mut out = String::with_capacity(64);
        for byte in ctx.finish().as_ref() {
            use std::fmt::Write as _;
            let _ = write!(&mut out, "{byte:02x}");
        }
        Some(out)
    }

    fn hash_sweep_entries(
        &self,
        ctx: &mut Context,
        sweep: &Sweep,
        static_paths: &HashSet<PathBuf>,
    ) -> Option<()> {
        let entries = match std::fs::read_dir(&sweep.dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == ErrorKind::NotFound => return Some(()),
            Err(_) => return None,
        };
        let mut sorted: Vec<_> = entries.collect::<Result<_, _>>().ok()?;
        sorted.sort_by_key(std::fs::DirEntry::file_name);
        let mut patterns = Vec::new();
        for entry in sorted {
            if !sweep.keep.masks(entry.file_name().to_str()) {
                continue;
            }
            let entry_path = normalized(&entry.path());
            if static_paths.contains(&entry_path) {
                continue;
            }
            let shape = match entry.file_type().is_ok_and(|k| k.is_dir()) {
                true => Shape::Whole,
                false => Shape::Exact,
            };
            patterns.clear();
            self.push_mask(&mut patterns, &entry_path, shape);
            for pat in &patterns {
                ctx.update(b"pat:");
                ctx.update(pat.as_str().as_bytes());
                ctx.update(b"\0");
            }
        }
        Some(())
    }

    fn static_mask_paths(&self) -> HashSet<PathBuf> {
        let mut set = HashSet::new();
        let all = self
            .folders
            .iter()
            .map(|f| f.path.as_path())
            .chain(self.files.iter().map(PathBuf::as_path))
            .chain(self.neighbours.iter().map(PathBuf::as_path));
        for path in all {
            let norm = normalized(path);
            set.insert(resolved_beside(&norm));
            set.insert(norm);
        }
        set
    }

    fn hash_static_presence(&self, ctx: &mut Context) -> Option<()> {
        let entries = self
            .folders
            .iter()
            .filter(|f| !f.pre_create)
            .map(|f| (b"folder:".as_slice(), f.path.as_path()))
            .chain(
                self.files
                    .iter()
                    .map(|p| (b"file:".as_slice(), p.as_path())),
            )
            .chain(
                self.neighbours
                    .iter()
                    .map(|p| (b"neighbour:".as_slice(), p.as_path())),
            );
        for (tag, path) in entries {
            let norm = normalized(path);
            hash_path(ctx, tag, &norm);
            let marker: &[u8] = match std::fs::symlink_metadata(&norm) {
                Ok(_) => b"1\0",
                Err(err) if err.kind() == ErrorKind::NotFound => b"0\0",
                Err(_) => return None,
            };
            ctx.update(marker);
        }
        Some(())
    }

    /// True if masking `path` (as given or parent-resolved) would cover the office's own roots.
    fn holds_own_root(&self, path: &Path) -> bool {
        let path = normalized(path);
        let beside = resolved_beside(&path);
        self.own_roots
            .iter()
            .any(|root| root.starts_with(&path) || root.starts_with(&beside))
    }

    /// True if `link`'s resolved target overlaps any own root (omitted so `bwrap`/Seatbelt
    /// leaf-symlink dereference never wipes own roots; canonical sensitive paths stay masked).
    fn link_overlaps_own_root(&self, link: &Path) -> bool {
        let target = std::fs::canonicalize(link)
            .ok()
            .or_else(|| resolve_dangling_target(link));
        target.is_some_and(|t| {
            self.own_roots
                .iter()
                .any(|root| root.starts_with(&t) || t.starts_with(root))
        })
    }

    /// Single choke-point for emitting a mask without following leaf symlinks (`lstat`).
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

    /// Masks unkept entries of `sweep.dir` in sorted name order (folders whole, else exact).
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
            let shape = if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                Shape::Whole
            } else {
                Shape::Exact
            };
            self.push_mask(out, &entry.path(), shape);
        }
    }
}

#[derive(Clone, Copy)]
enum Shape {
    Below,
    Whole,
    Exact,
}

/// Own data roots as given and resolved through links (or nearest existing ancestor).
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

enum Keep {
    Named(Vec<String>),
    AllButOtherOrganizations { own: String },
    OnlyHostRegistry,
}

impl Keep {
    fn masks(&self, name: Option<&str>) -> bool {
        match self {
            Self::Named(kept) => !name.is_some_and(|name| kept.iter().any(|kept| kept == name)),
            Self::AllButOtherOrganizations { own } => {
                name.is_some_and(|name| name != own && names_an_organization(name))
            }
            Self::OnlyHostRegistry => name.is_some_and(names_host_registry_file),
        }
    }
}

/// Host co-hosted data-root placement (`TASK-204`): HQ at `<data-root>/<org-id>/{durable,runtime}`
/// or satellite at `<data-root>/<org-id>/offices/<office-id>/{durable,runtime}`.
#[derive(Debug, PartialEq, Eq)]
struct Placement {
    data_root: PathBuf,
    org_root: PathBuf,
    org_id: String,
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

    fn add_masks(
        self,
        folders: &mut Vec<MaskedFolder>,
        files: &mut Vec<PathBuf>,
        neighbours: &mut Vec<PathBuf>,
        sweeps: &mut Vec<Sweep>,
    ) {
        let office_state = self.data_root.join(OFFICE_STATE_DIR_NAME);
        push_host_registry_masks(&office_state, files, sweeps);
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

/// `<office-home>/offices/` when `folder` is `<office-home>/offices/<office-id>`.
fn office_folders(folder: &Path, office_id: &str) -> Option<PathBuf> {
    let offices = folder.parent()?;
    (is_named(folder, office_id) && is_named(offices, OFFICES_DIR_NAME))
        .then(|| offices.to_path_buf())
}

fn office_home(folder: &Path, office_id: &str) -> Option<PathBuf> {
    office_folders(folder, office_id)?
        .parent()
        .map(Path::to_path_buf)
}

fn push_host_registry_masks(office_home: &Path, files: &mut Vec<PathBuf>, sweeps: &mut Vec<Sweep>) {
    let norm = normalized(office_home);
    let resolved = canonicalize_with_tail(&norm);
    if sweeps.iter().any(|s| {
        matches!(s.keep, Keep::OnlyHostRegistry)
            && (normalized(&s.dir) == norm
                || canonicalize_with_tail(&normalized(&s.dir)) == resolved)
    }) {
        return;
    }
    for name in HOST_REGISTRY_FILES {
        let candidate = office_home.join(name);
        if !files.contains(&candidate) {
            files.push(candidate);
        }
    }
    sweeps.push(Sweep {
        dir: office_home.to_path_buf(),
        keep: Keep::OnlyHostRegistry,
    });
}

fn names_host_registry_file(name: &str) -> bool {
    name.len() > HOST_REGISTRY_PREFIX.len()
        && name.as_bytes()[..HOST_REGISTRY_PREFIX.len()]
            .eq_ignore_ascii_case(HOST_REGISTRY_PREFIX.as_bytes())
}

fn hash_path(ctx: &mut Context, tag: &[u8], path: &Path) {
    ctx.update(tag);
    ctx.update(normalized(path).to_string_lossy().as_bytes());
    ctx.update(b"\0");
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
