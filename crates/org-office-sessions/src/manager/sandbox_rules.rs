//! Pure sandbox path-rule builders shared by the session sandbox planners.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use org_agent_runtime::{PathPattern, SpecPath};

use super::rust_toolchain::{canonicalize_with_tail, host_rust_dir_env_from};

pub(super) fn push_safe_ro_dir(
    out: &mut Vec<SpecPath>,
    candidate: &Path,
    forbidden_canon: &[PathBuf],
    host_home_canon: Option<&Path>,
    disallowed_containers: &[PathBuf],
) {
    if !candidate.is_absolute() || !candidate.is_dir() {
        return;
    }
    let Ok(resolved) = candidate.canonicalize() else {
        return;
    };
    let overlaps_forbidden = resolved.parent().is_none()
        || host_home_canon.is_some_and(|h| resolved == h || h.starts_with(&resolved))
        || forbidden_canon
            .iter()
            .any(|root| resolved.starts_with(root) || root.starts_with(&resolved))
        || disallowed_containers
            .iter()
            .any(|c| c.starts_with(&resolved));
    if overlaps_forbidden {
        return;
    }
    let text = resolved.display().to_string();
    if let Ok(spec) = SpecPath::try_from(text.as_str()) {
        if !spec.is_root() && !out.contains(&spec) {
            out.push(spec);
        }
    }
}

/// Collects safe read-only directories for Agy `--sandbox` (`--add-dir`) so `agent-os` and the
/// host Rust toolchain execute without `BypassSandbox` (`TASK-164`). Mounts only individual safe
/// Cargo subdirs (`bin`, `registry`, `git`) — NEVER host `~/.cargo` root, `$HOME`, or any
/// ancestor of a forbidden office/workspace directory, so secrets stay inaccessible.
pub(crate) fn collect_agy_read_only_dirs(
    helper_dir: &Path,
    rustup_explicit: Option<OsString>,
    cargo_explicit: Option<OsString>,
    host_home: Option<OsString>,
    forbidden: &[PathBuf],
) -> Vec<SpecPath> {
    let forbidden_canon: Vec<PathBuf> = forbidden
        .iter()
        .map(|p| canonicalize_with_tail(p))
        .collect();
    let host_home_canon = host_home
        .as_deref()
        .map(Path::new)
        .filter(|p| p.is_absolute())
        .and_then(|p| p.canonicalize().ok());
    let mut out = Vec::new();
    push_safe_ro_dir(
        &mut out,
        helper_dir,
        &forbidden_canon,
        host_home_canon.as_deref(),
        &[],
    );
    // If `<helper_dir>/agent-os` is a symlink into a release-store directory, also mount the
    // canonical release directory holding the target binary.
    if let Ok(agent_os_real) = helper_dir.join("agent-os").canonicalize() {
        if let Some(parent) = agent_os_real.parent() {
            push_safe_ro_dir(
                &mut out,
                parent,
                &forbidden_canon,
                host_home_canon.as_deref(),
                &[],
            );
        }
    }
    if let Some(rustup) =
        host_rust_dir_env_from(rustup_explicit, host_home.clone(), ".rustup", forbidden)
    {
        push_safe_ro_dir(
            &mut out,
            Path::new(&rustup),
            &forbidden_canon,
            host_home_canon.as_deref(),
            &[],
        );
    }
    if let Some(cargo) = host_rust_dir_env_from(cargo_explicit, host_home, ".cargo", forbidden) {
        let cargo_path = Path::new(&cargo);
        let cargo_canon = canonicalize_with_tail(cargo_path);
        for sub in ["bin", "registry", "git"] {
            push_safe_ro_dir(
                &mut out,
                &cargo_path.join(sub),
                &forbidden_canon,
                host_home_canon.as_deref(),
                std::slice::from_ref(&cargo_canon),
            );
        }
    }
    out
}

pub(super) fn collect_existing_worktree_dirs(
    dir: &Path,
    depth: usize,
    out: &mut std::collections::BTreeSet<PathBuf>,
) {
    if depth > 6 {
        return;
    }
    if dir.join(".git").exists() {
        out.insert(canonicalize_with_tail(dir));
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_dir() {
            collect_existing_worktree_dirs(&entry.path(), depth + 1, out);
        }
    }
}

pub(super) fn is_safe_worktree_admin_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains("..")
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

pub(crate) fn collect_self_active_admins(
    git_state: &org_office::git::OfficeGitState,
    employee_id: &str,
) -> std::collections::BTreeMap<String, std::collections::BTreeSet<String>> {
    let mut by_repo: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        std::collections::BTreeMap::new();
    for ws in git_state.workspaces.values() {
        if ws.employee_id == employee_id
            && ws.state == "active"
            && is_safe_worktree_admin_name(&ws.repository_id)
            && is_safe_worktree_admin_name(&ws.admin_dir_name)
        {
            by_repo
                .entry(ws.repository_id.clone())
                .or_default()
                .insert(ws.admin_dir_name.clone());
        }
    }
    by_repo
}

pub(super) fn append_cargo_credential_denies(out: &mut Vec<PathPattern>) {
    let mut dirs = Vec::new();
    if let Some(raw) = std::env::var_os("CARGO_HOME") {
        let p = PathBuf::from(raw);
        if p.is_absolute() {
            dirs.push(p);
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let p = PathBuf::from(home).join(".cargo");
        if p.is_absolute() {
            dirs.push(p);
        }
    }
    // Deny Cargo registry token files (`credentials`, `credentials.toml`) only. Do NOT read-deny
    // `config` / `config.toml`: Cargo unconditionally opens them on every `cargo` invocation, and
    // Linux `bwrap` turns read-denies on non-existent files into `EACCES` stubs, breaking `cargo`.
    // `$CARGO_HOME` stays outside `writable_roots`, so agents still cannot modify `config.toml`.
    for dir in dirs {
        for file in ["credentials", "credentials.toml"] {
            push_exact_pattern(out, &dir.join(file));
        }
    }
}

pub(super) fn append_agy_credential_denies(out: &mut Vec<PathPattern>) {
    if let Some(home) = std::env::var_os("HOME") {
        let gemini = PathBuf::from(home).join(".gemini");
        if gemini.is_absolute() {
            for rel in org_agent_runtime_agy::AGY_CONFIG_CREDENTIAL_FILES {
                push_exact_pattern(out, &gemini.join(rel));
            }
        }
    }
}

pub(super) fn append_bare_cache_sandbox_rules(
    cache_dir: &Path,
    self_admin_names: &std::collections::BTreeSet<String>,
    writable: &mut Vec<SpecPath>,
    write_denies: &mut Vec<PathPattern>,
) {
    let canon = canonicalize_with_tail(cache_dir);
    for rel in ["objects", "refs/heads/task", "logs/refs/heads/task"] {
        let p = canon.join(rel);
        if let Ok(spec) = SpecPath::try_from(p.to_string_lossy().as_ref()) {
            writable.push(spec);
        }
    }
    // Grant ONLY concrete self worktree admin directories (`doc-35` §Sandbox / `FC-103`),
    // never the parent `<cache>/worktrees` directory.
    for admin in self_admin_names {
        let admin_dir = canon.join("worktrees").join(admin);
        if let Ok(spec) = SpecPath::try_from(admin_dir.to_string_lossy().as_ref()) {
            writable.push(spec);
        }
        for admin_file in ["commondir", "gitdir", "config.worktree"] {
            push_raw_pattern(
                write_denies,
                &format!("{}/worktrees/{admin}/{admin_file}", canon.display()),
            );
        }
    }
    // Always protect directories inside writable roots (`objects/info`) and core bare-repo dirs
    // (`hooks`, `info`, `refs/remotes`, `refs/tags`). Note `org-git::cache::init_bare_cache`
    // deletes `<cache>/hooks`; keeping `hooks` write-denied prevents re-creation. Guard optional
    // dirs (`refs/office`, `refs/replace`) via `symlink_metadata` (catches dangling symlinks too)
    // so `bwrap` never turns absent `refs/replace` into an `EACCES` stub; `<cache>` and
    // `<cache>/refs` remain outside `writable_roots` (enforced by OS sandbox and approval bridge).
    for rel_dir in ["hooks", "info", "objects/info", "refs/remotes", "refs/tags"] {
        push_raw_pattern(write_denies, &format!("{}/{rel_dir}", canon.display()));
        push_raw_pattern(write_denies, &format!("{}/{rel_dir}/**", canon.display()));
    }
    for rel_dir in ["refs/office", "refs/replace"] {
        if std::fs::symlink_metadata(canon.join(rel_dir)).is_ok() {
            push_raw_pattern(write_denies, &format!("{}/{rel_dir}", canon.display()));
            push_raw_pattern(write_denies, &format!("{}/{rel_dir}/**", canon.display()));
        }
    }
    // Always protect core bare-repo metadata files (`config`, `HEAD`, `description`). Guard
    // optional Git files (`packed-refs`, `shallow`, `FETCH_HEAD`, `config.worktree`) ONLY when
    // present (`symlink_metadata().is_ok()` catches dangling symlinks too): `<cache>` stays
    // outside `writable_roots` (enforced by both OS sandbox and approval bridge), and listing
    // absent `packed-refs` in `denyWrite` makes Linux `bwrap` return `EACCES` instead of `ENOENT`.
    for rel_file in ["config", "HEAD", "description"] {
        push_raw_pattern(write_denies, &format!("{}/{rel_file}", canon.display()));
    }
    for rel_file in ["packed-refs", "shallow", "FETCH_HEAD", "config.worktree"] {
        if std::fs::symlink_metadata(canon.join(rel_file)).is_ok() {
            push_raw_pattern(write_denies, &format!("{}/{rel_file}", canon.display()));
        }
    }
}

pub(super) fn push_raw_pattern(out: &mut Vec<PathPattern>, raw: &str) {
    if let Ok(pat) = PathPattern::try_from(raw) {
        if !out.contains(&pat) {
            out.push(pat);
        }
    }
}

pub(super) fn push_glob_pattern(out: &mut Vec<PathPattern>, path: &Path) {
    let resolved = canonicalize_with_tail(path);
    push_raw_pattern(out, &format!("{}/**", resolved.display()));
}

pub(super) fn push_exact_pattern(out: &mut Vec<PathPattern>, path: &Path) {
    push_raw_pattern(out, &path.display().to_string());
    let resolved = canonicalize_with_tail(path);
    push_raw_pattern(out, &resolved.display().to_string());
}

pub(super) fn append_privileged_socket_patterns(out: &mut Vec<PathPattern>) {
    // Keep non-wildcard system socket paths plus concrete `$XDG_RUNTIME_DIR` children here
    // (avoiding `/run/user/*/...` wildcards which Codex's `path_pattern_base` truncates to
    // `/run/user`, blocking `/run/user/<uid>` sockets/tmpdir; Claude adds `/run/user/*/...`
    // privately in `org-agent-runtime-claude/src/plan.rs`).
    for privileged in [
        "/run/dbus/system_bus_socket",
        "/var/run/dbus/system_bus_socket",
        "/var/run/docker.sock",
        "/run/docker.sock",
        "/run/podman/**",
    ] {
        push_raw_pattern(out, privileged);
    }
    if let Some(xdg) = std::env::var_os("XDG_RUNTIME_DIR") {
        let xdg_path = PathBuf::from(xdg);
        if xdg_path.is_absolute() {
            push_exact_pattern(out, &xdg_path.join("bus"));
            push_glob_pattern(out, &xdg_path.join("systemd"));
            push_glob_pattern(out, &xdg_path.join("podman"));
        }
    }
}

pub(super) fn collect_non_self_admin_names(
    wt_dir: &Path,
    repo_id: Option<&str>,
    git_state: &org_office::git::OfficeGitState,
    self_admins: &std::collections::BTreeSet<String>,
) -> std::collections::BTreeSet<String> {
    let mut other_admins = std::collections::BTreeSet::new();
    if let Ok(entries) = std::fs::read_dir(wt_dir) {
        for name in entries
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| !self_admins.contains(n))
        {
            other_admins.insert(name);
        }
    }
    if let Some(rid) = repo_id {
        for ws in git_state.workspaces.values() {
            if ws.repository_id == rid
                && is_safe_worktree_admin_name(&ws.admin_dir_name)
                && !self_admins.contains(&ws.admin_dir_name)
            {
                other_admins.insert(ws.admin_dir_name.clone());
            }
        }
    }
    other_admins
}
