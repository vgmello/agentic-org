//! Host Rust toolchain environment and read-only Cargo cache links (`TASK-126`, `TASK-159`).

use super::*;

/// Points `<agent_home>/.cargo/<name>` at a host read-only cache directory, relative to the
/// already-verified `.cargo` descriptor. Refuses a target under a forbidden root.
pub(super) fn link_readonly_cache(
    agent_cargo: &org_platform::fs::OwnerOnlyDir,
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
    if forbidden_canon
        .iter()
        .any(|root| resolved_target.starts_with(root))
    {
        return false;
    }
    // An existing real directory is the agent's own cache and is kept; a regular file is refused.
    agent_cargo
        .ensure_symlink(std::ffi::OsStr::new(name), &resolved_target)
        .is_ok()
}

/// Symlinks safe read-only host Cargo caches (`.cargo/registry`, `.cargo/git`) into an agent's
/// isolated home (`TASK-159`) without ever linking host Cargo credentials or config files
/// (`credentials*`, `config*`). Does NOT symlink `.rustup` into `<agent_home>`: `RUSTUP_HOME` is
/// preserved directly in the environment (surviving `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB=1`), and
/// removing any legacy `<agent_home>/.rustup` symlink prevents Linux `bwrap` sibling-directory
/// traversal from ever masking host `.rustup` (`TASK-163`).
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
    let _ = home.remove_symlink(std::ffi::OsStr::new(".rustup"));
    let forbidden_canon: Vec<PathBuf> = forbidden
        .iter()
        .map(|p| canonicalize_with_tail(p))
        .collect();
    let host_cargo = cargo_dir?;
    let agent_cargo = home
        .open_or_create_child(std::ffi::OsStr::new(".cargo"), office_uid)
        .ok()?;
    let linked_registry = link_readonly_cache(
        &agent_cargo,
        "registry",
        &host_cargo.join("registry"),
        &forbidden_canon,
    );
    let linked_git = link_readonly_cache(
        &agent_cargo,
        "git",
        &host_cargo.join("git"),
        &forbidden_canon,
    );
    (linked_registry || linked_git).then(|| agent_home.join(".cargo"))
}

pub(super) fn append_rust_toolchain_env(
    captured: &mut Vec<(OsString, OsString)>,
    agent_home: Option<&Path>,
    rustup_explicit: Option<OsString>,
    cargo_explicit: Option<OsString>,
    host_home: Option<OsString>,
    forbidden: &[PathBuf],
    sandbox_enabled: bool,
) {
    let host_rustup =
        host_rust_dir_env_from(rustup_explicit, host_home.clone(), ".rustup", forbidden);
    let host_cargo = host_rust_dir_env_from(cargo_explicit, host_home, ".cargo", forbidden);
    let linked_cargo = agent_home.and_then(|home| {
        link_host_rust_caches(
            home,
            host_rustup.as_deref().map(Path::new),
            host_cargo.as_deref().map(Path::new),
            forbidden,
        )
    });
    if let Some(dir) = host_rustup {
        captured.push((OsString::from("RUSTUP_HOME"), dir));
    }
    if let Some(agent_cargo) = linked_cargo {
        captured.push((OsString::from("CARGO_HOME"), agent_cargo.into_os_string()));
        if sandbox_enabled {
            captured.push((OsString::from("CARGO_NET_OFFLINE"), OsString::from("true")));
        }
    } else if agent_home.is_none() {
        if let Some(dir) = host_cargo {
            captured.push((OsString::from("CARGO_HOME"), dir));
            if sandbox_enabled {
                captured.push((OsString::from("CARGO_NET_OFFLINE"), OsString::from("true")));
            }
        }
    } else if host_cargo.is_some() && sandbox_enabled {
        // Agent home was present (do not expose raw host CARGO_HOME on tamper/empty cache),
        // still keep Cargo offline inside the sandbox.
        captured.push((OsString::from("CARGO_NET_OFFLINE"), OsString::from("true")));
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
    if forbidden_canon
        .iter()
        .any(|root| resolved.starts_with(root))
    {
        return None;
    }
    Some(resolved.into_os_string())
}
