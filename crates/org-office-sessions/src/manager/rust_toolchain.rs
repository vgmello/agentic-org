//! Host Rust toolchain environment, read-only shared Cargo caches, and isolated per-agent
//! `CARGO_HOME` / `CARGO_TARGET_DIR` (`TASK-126`, `TASK-159`, `TASK-135`).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// `true` when `path` lies inside a forbidden root or contains one (`TASK-190`, rules §3.1): a
/// host cache at `/home` would expose every agent home below it, as `sandbox_rules` checks.
fn overlaps_any_root(path: &Path, roots: &[PathBuf]) -> bool {
    roots
        .iter()
        .any(|root| path.starts_with(root) || root.starts_with(path))
}

/// Points `<parent>/<name>` at a host read-only cache directory, relative to an already-verified
/// `OwnerOnlyDir` descriptor. Refuses a target under or containing any forbidden root.
pub(super) fn link_readonly_cache(
    parent: &org_platform::fs::OwnerOnlyDir,
    name: &str,
    target: &Path,
    forbidden_canon: &[PathBuf],
) -> bool {
    if !target.is_dir() {
        return false;
    }
    let Ok(resolved_target) = target.canonicalize() else {
        return false;
    };
    if overlaps_any_root(&resolved_target, forbidden_canon) {
        return false;
    }
    // An existing real directory is the agent's own cache and is kept; a regular file is refused.
    parent
        .ensure_symlink(OsStr::new(name), &resolved_target)
        .is_ok()
}

/// Prepares `<agent_home>/.cargo/registry` using a split read-only/writable layout (`TASK-135`):
/// - `registry/index` -> read-only symlink to `<host_cargo>/registry/index` (when present)
/// - `registry/cache` -> read-only symlink to `<host_cargo>/registry/cache` (when present)
/// - `registry/src`   -> real per-agent writable `0700` dir (so Cargo unpacks `.crate` tarballs
///   offline without writing into host `~/.cargo` or contending across concurrent agents; falls
///   back to a read-only symlink to `<host_cargo>/registry/src` only when `cache` is absent and
///   host `src` exists).
fn prepare_split_registry(
    agent_cargo: &org_platform::fs::OwnerOnlyDir,
    host_registry: &Path,
    office_uid: u32,
    forbidden_canon: &[PathBuf],
) -> bool {
    let Ok(resolved) = host_registry.canonicalize() else {
        return false;
    };
    if overlaps_any_root(&resolved, forbidden_canon) {
        return false;
    }
    // Migrate any pre-TASK-135 `<agent_home>/.cargo/registry` symlink to a real `0700` dir
    // without touching a regular file (so a tampered regular file fails closed below).
    let _ = agent_cargo.remove_symlink(OsStr::new("registry"));
    let Ok(agent_reg) = agent_cargo.open_or_create_child(OsStr::new("registry"), office_uid) else {
        return false;
    };
    let linked_index = link_readonly_cache(
        &agent_reg,
        "index",
        &resolved.join("index"),
        forbidden_canon,
    );
    let linked_cache = link_readonly_cache(
        &agent_reg,
        "cache",
        &resolved.join("cache"),
        forbidden_canon,
    );
    if linked_cache || !resolved.join("src").is_dir() {
        let _ = agent_reg.remove_symlink(OsStr::new("src"));
        let has_writable_src = agent_reg
            .open_or_create_child(OsStr::new("src"), office_uid)
            .is_ok();
        linked_index || linked_cache || has_writable_src
    } else {
        let linked_src =
            link_readonly_cache(&agent_reg, "src", &resolved.join("src"), forbidden_canon);
        linked_index || linked_src
    }
}

/// Prepares `<agent_home>/.cargo/git` with a read-only symlink to `<host_cargo>/git/db` (when
/// present) and a real per-agent writable `0700` `git/checkouts` directory (`TASK-135`).
fn prepare_split_git(
    agent_cargo: &org_platform::fs::OwnerOnlyDir,
    host_git: &Path,
    office_uid: u32,
    forbidden_canon: &[PathBuf],
) -> bool {
    let Ok(resolved) = host_git.canonicalize() else {
        return false;
    };
    if overlaps_any_root(&resolved, forbidden_canon) {
        return false;
    }
    let _ = agent_cargo.remove_symlink(OsStr::new("git"));
    let Ok(agent_git) = agent_cargo.open_or_create_child(OsStr::new("git"), office_uid) else {
        return false;
    };
    let linked_db = link_readonly_cache(&agent_git, "db", &resolved.join("db"), forbidden_canon);
    let _ = agent_git.remove_symlink(OsStr::new("checkouts"));
    let has_checkouts = agent_git
        .open_or_create_child(OsStr::new("checkouts"), office_uid)
        .is_ok();
    linked_db || has_checkouts
}

/// Ensures an isolated owner-only `<agent_home>/.cargo` exists without linking any host cache
/// (used when a sandboxed host has no pre-warmed `~/.cargo` cache yet).
pub(crate) fn prepare_isolated_cargo_home(agent_home: &Path) -> Option<PathBuf> {
    let office_uid = org_platform::fs::effective_uid().ok()?;
    let home = org_platform::fs::OwnerOnlyDir::open(agent_home, office_uid).ok()?;
    let _ = home.remove_symlink(OsStr::new(".rustup"));
    let _agent_cargo = home
        .open_or_create_child(OsStr::new(".cargo"), office_uid)
        .ok()?;
    Some(canonicalize_with_tail(&agent_home.join(".cargo")))
}

/// Ensures an isolated owner-only `<agent_base>/cache/cargo-target` directory exists (`TASK-135`)
/// by opening `<agent_base>` with `O_NOFOLLOW` and creating `cache` and `cargo-target` relative
/// to verified `OwnerOnlyDir` descriptors.
pub(crate) fn prepare_agent_cargo_target(agent_base: &Path) -> Option<PathBuf> {
    let office_uid = org_platform::fs::effective_uid().ok()?;
    let base = org_platform::fs::OwnerOnlyDir::open(agent_base, office_uid).ok()?;
    let cache = base
        .open_or_create_child(OsStr::new("cache"), office_uid)
        .ok()?;
    let _target = cache
        .open_or_create_child(OsStr::new("cargo-target"), office_uid)
        .ok()?;
    Some(canonicalize_with_tail(
        &agent_base.join("cache").join("cargo-target"),
    ))
}

/// Prepares an isolated per-agent `<agent_home>/.cargo` sharing read-only host Cargo caches
/// (`registry/{index,cache}`, `git/db`) while keeping `.package-cache*`, `.global-cache`,
/// `registry/src`, and `git/checkouts` private and writable per agent (`TASK-159`, `TASK-135`).
/// Never links host Cargo credentials or config files (`credentials*`, `config*`) and never
/// symlinks `.rustup` into `<agent_home>` (`TASK-163`).
///
/// Hardened against the agent-writable home (`B1`, `TASK-172`): `<agent_home>` and
/// `<agent_home>/.cargo` are opened by descriptor with `O_NOFOLLOW` and must be real owner-only
/// directories owned by the office's own account, and every link is read, removed, and created
/// relative to those descriptors, so a symlink swapped into the path can never redirect an
/// unsandboxed office write outside the home. A home owned by another account (a dedicated agent
/// OS user) or with a widened mode is refused outright: nothing is linked.
pub(crate) fn link_host_rust_caches(
    agent_home: &Path,
    _rustup_dir: Option<&Path>,
    cargo_dir: Option<&Path>,
    forbidden: &[PathBuf],
) -> Option<PathBuf> {
    let office_uid = org_platform::fs::effective_uid().ok()?;
    let home = org_platform::fs::OwnerOnlyDir::open(agent_home, office_uid).ok()?;
    // Clean up any pre-TASK-163 `<agent_home>/.rustup` symlink without touching real directories.
    let _ = home.remove_symlink(OsStr::new(".rustup"));
    let forbidden_canon: Vec<PathBuf> = forbidden
        .iter()
        .map(|p| canonicalize_with_tail(p))
        .collect();
    let host_cargo = cargo_dir?;
    let agent_cargo = home
        .open_or_create_child(OsStr::new(".cargo"), office_uid)
        .ok()?;
    let host_registry = host_cargo.join("registry");
    let host_git = host_cargo.join("git");
    let has_host_registry = host_registry.is_dir();
    let has_host_git = host_git.is_dir();
    let linked_registry = has_host_registry
        && prepare_split_registry(&agent_cargo, &host_registry, office_uid, &forbidden_canon);
    let linked_git =
        has_host_git && prepare_split_git(&agent_cargo, &host_git, office_uid, &forbidden_canon);
    let ready = if has_host_registry || has_host_git {
        linked_registry || linked_git
    } else {
        true
    };
    ready.then(|| canonicalize_with_tail(&agent_home.join(".cargo")))
}

pub(crate) struct RustToolchainEnvInput<'a> {
    pub(crate) agent_home: Option<&'a Path>,
    pub(crate) agent_base: Option<&'a Path>,
    pub(crate) rustup_explicit: Option<OsString>,
    pub(crate) cargo_explicit: Option<OsString>,
    pub(crate) host_home: Option<OsString>,
    pub(crate) forbidden: &'a [PathBuf],
    pub(crate) sandbox_enabled: bool,
}

pub(crate) fn append_rust_toolchain_env(
    captured: &mut Vec<(OsString, OsString)>,
    input: RustToolchainEnvInput<'_>,
) {
    let host_rustup = host_rust_dir_env_from(
        input.rustup_explicit,
        input.host_home.clone(),
        ".rustup",
        input.forbidden,
    );
    let host_cargo = host_rust_dir_env_from(
        input.cargo_explicit,
        input.host_home,
        ".cargo",
        input.forbidden,
    );
    if let Some(dir) = host_rustup.as_ref() {
        captured.push((OsString::from("RUSTUP_HOME"), dir.clone()));
    }
    let cargo_home = resolve_session_cargo_home(
        input.agent_home,
        host_rustup.as_deref().map(Path::new),
        host_cargo.as_deref().map(Path::new),
        input.forbidden,
        input.sandbox_enabled,
    );
    if let Some(dir) = cargo_home.as_ref() {
        captured.push((OsString::from("CARGO_HOME"), dir.clone()));
    }
    if let Some(target_dir) = input.agent_base.and_then(prepare_agent_cargo_target) {
        captured.push((
            OsString::from("CARGO_TARGET_DIR"),
            target_dir.into_os_string(),
        ));
    }
    if input.sandbox_enabled && (cargo_home.is_some() || host_cargo.is_some()) {
        captured.push((OsString::from("CARGO_NET_OFFLINE"), OsString::from("true")));
    }
}

fn resolve_session_cargo_home(
    agent_home: Option<&Path>,
    host_rustup: Option<&Path>,
    host_cargo: Option<&Path>,
    forbidden: &[PathBuf],
    sandbox_enabled: bool,
) -> Option<OsString> {
    match (agent_home, host_cargo) {
        (Some(home), Some(cargo_dir)) => {
            link_host_rust_caches(home, host_rustup, Some(cargo_dir), forbidden)
                .map(PathBuf::into_os_string)
        }
        (Some(home), None) if sandbox_enabled => {
            prepare_isolated_cargo_home(home).map(PathBuf::into_os_string)
        }
        (Some(_), None) => None,
        (None, Some(cargo_dir)) => Some(cargo_dir.as_os_str().to_os_string()),
        (None, None) => None,
    }
}

pub(super) fn canonicalize_with_tail(path: &Path) -> PathBuf {
    if let Ok(resolved) = path.canonicalize() {
        return resolved;
    }
    let mut tail = Vec::new();
    let mut cur = path;
    while let Some(parent) = cur.parent() {
        if let Some(name) = cur.file_name() {
            tail.push(name.to_os_string());
        }
        if let Ok(mut base) = parent.canonicalize() {
            for seg in tail.into_iter().rev() {
                base.push(seg);
            }
            return base;
        }
        cur = parent;
    }
    path.to_path_buf()
}

pub(crate) fn host_rust_dir_env_from(
    explicit: Option<OsString>,
    home: Option<OsString>,
    default_rel: &str,
    forbidden: &[PathBuf],
) -> Option<OsString> {
    let candidate = match explicit {
        Some(raw) => PathBuf::from(raw),
        None => PathBuf::from(home?).join(default_rel),
    };
    if !candidate.is_absolute() || !candidate.is_dir() {
        return None;
    }
    let resolved = candidate.canonicalize().ok()?;
    let forbidden_canon: Vec<PathBuf> = forbidden
        .iter()
        .map(|p| canonicalize_with_tail(p))
        .collect();
    if overlaps_any_root(&resolved, &forbidden_canon) {
        return None;
    }
    Some(resolved.into_os_string())
}
