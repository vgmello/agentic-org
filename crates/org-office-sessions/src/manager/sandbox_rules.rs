//! Pure sandbox path-rule builders shared by the session sandbox planners.

use std::ffi::OsString;
use std::os::unix::fs::FileTypeExt as _;
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

/// Privileged runtime-folder entries (`org_proto::location`): `host.sock`, the launch rendezvous
/// `r/` and per-agent temporary folders `t/`. No Agy mount may hold or sit inside one.
const PRIVILEGED_RUNTIME_ENTRIES: [&str; 3] =
    [org_proto::location::HOST_SOCKET_FILE_NAME, "r", "t"];

/// Mounts this office's own socket folder, `<run>/o/<handle>` (the parent of `office_socket`,
/// `org_proto::location`), for Agy `--sandbox`, so `agent-os` can `connect()` to
/// `AGENT_OS_SOCKET`: connecting to a Unix socket needs search permission on its folders and
/// write permission on the socket inode, never a writable mount (`TASK-164`). Never the shared
/// `<run>/o` or the runtime folder `<run>`, so another office's socket stays invisible even if Agy
/// mounted the folder writable. Fails closed (mounts nothing) unless:
/// - `office_socket` is `<run>/o/<handle>/s.sock` with a plain handle and `<run>` not `/`
///   ([`org_proto::location::runtime_dir_of_office_socket`]), and still is once resolved;
/// - both `<run>/o` and `<run>/o/<handle>` are real folders (never links) of this account with
///   mode exactly 0700 (no special bits);
/// - `<run>/o/<handle>` holds nothing but the socket `s.sock` ([`holds_own_socket_only`]);
/// - it overlaps no forbidden folder ([`push_safe_ro_dir`]).
fn push_office_socket_dir(
    out: &mut Vec<SpecPath>,
    office_socket: &Path,
    forbidden_canon: &[PathBuf],
    host_home_canon: Option<&Path>,
) {
    use org_proto::location::{runtime_dir_of_office_socket, OFFICE_SOCKET_FILE_NAME};
    if runtime_dir_of_office_socket(office_socket).is_none() {
        return;
    }
    let Some(socket_dir) = office_socket.parent() else {
        return;
    };
    let Some(sockets_dir) = socket_dir.parent() else {
        return;
    };
    // The two leaves are checked unresolved (a link is refused, not followed), then resolved:
    // ancestors may be system links (macOS `/var`), but the shape must hold on the real path.
    let Ok(uid) = org_platform::fs::effective_uid() else {
        return;
    };
    if !is_private_dir(sockets_dir, uid)
        || !is_private_dir(socket_dir, uid)
        || !holds_own_socket_only(socket_dir)
    {
        return;
    }
    let Ok(resolved) = socket_dir.canonicalize() else {
        return;
    };
    if resolved.file_name() != socket_dir.file_name()
        || runtime_dir_of_office_socket(&resolved.join(OFFICE_SOCKET_FILE_NAME)).is_none()
    {
        return;
    }
    push_safe_ro_dir(out, &resolved, forbidden_canon, host_home_canon, &[]);
}

/// Whether `dir` is a real folder (`lstat`, never a link) owned by `uid` with mode exactly 0700:
/// no group or other access and no setuid, setgid or sticky bit.
fn is_private_dir(dir: &Path, uid: u32) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    org_platform::fs::verify_owner_only_dir(dir, uid).is_ok()
        && std::fs::symlink_metadata(dir)
            .is_ok_and(|meta| meta.is_dir() && meta.permissions().mode() & 0o7777 == 0o700)
}

/// Whether `dir` holds nothing but the office socket: every entry is a Unix socket (not a link)
/// named `s.sock`, so no other socket, file, folder or link. Any unreadable entry fails closed.
fn holds_own_socket_only(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut entries| {
        entries.all(|entry| {
            entry.is_ok_and(|entry| {
                entry.file_name() == org_proto::location::OFFICE_SOCKET_FILE_NAME
                    && entry.file_type().is_ok_and(|ft| ft.is_socket())
            })
        })
    })
}

/// Collects safe read-only directories for Agy `--sandbox` (`--add-dir`) so `agent-os`, its
/// office socket and the host Rust toolchain work without `BypassSandbox` (`TASK-164`). Mounts
/// only individual safe Cargo subdirs (`bin`, `registry`, `git`) and this office's own socket
/// folder `<run>/o/<handle>` ([`push_office_socket_dir`]) — NEVER host `~/.cargo` root, `$HOME`,
/// the shared `<run>/o`, or any ancestor of a forbidden office/workspace directory, so secrets
/// stay inaccessible.
///
/// The runtime folder `<run>` is derived here from `office_socket`
/// ([`crate::host_auth::office_runtime_dir`]), never left to the caller's `forbidden`: no helper,
/// `agent-os` release, `RUSTUP_HOME` or Cargo mount may hold or sit anywhere inside `<run>` (its
/// `host.sock`, `r/`, `t/`, sibling temporary folders and every office's socket folder), and the
/// office socket folder, the one mount inside it, may hold none of `<run>`'s privileged entries.
pub(crate) fn collect_agy_read_only_dirs(
    helper_dir: &Path,
    office_socket: Option<&Path>,
    rustup_explicit: Option<OsString>,
    cargo_explicit: Option<OsString>,
    host_home: Option<OsString>,
    forbidden: &[PathBuf],
) -> Vec<SpecPath> {
    let canonical = |paths: &[PathBuf]| -> Vec<PathBuf> {
        paths.iter().map(|p| canonicalize_with_tail(p)).collect()
    };
    let runtime = office_socket.and_then(crate::host_auth::office_runtime_dir);
    let privileged: Vec<PathBuf> = runtime
        .iter()
        .flat_map(|run| {
            PRIVILEGED_RUNTIME_ENTRIES
                .iter()
                .map(|entry| run.join(entry))
        })
        .collect();
    // Raw for the toolchain lookups (they resolve it themselves), canonical for the mounts.
    let non_socket_raw: Vec<PathBuf> = forbidden
        .iter()
        .cloned()
        .chain(runtime.map(Path::to_path_buf))
        .chain(privileged.iter().cloned())
        .collect();
    let non_socket_forbidden = canonical(&non_socket_raw);
    let mut socket_forbidden = canonical(forbidden);
    socket_forbidden.extend(canonical(&privileged));
    let host_home_canon = host_home
        .as_deref()
        .map(Path::new)
        .filter(|p| p.is_absolute())
        .and_then(|p| p.canonicalize().ok());
    let host_home_canon = host_home_canon.as_deref();
    let mut out = Vec::new();
    push_safe_ro_dir(
        &mut out,
        helper_dir,
        &non_socket_forbidden,
        host_home_canon,
        &[],
    );
    // If `<helper_dir>/agent-os` is a symlink into a release-store directory, also mount the
    // canonical release directory holding the target binary.
    if let Ok(agent_os_real) = helper_dir.join("agent-os").canonicalize() {
        if let Some(parent) = agent_os_real.parent() {
            push_safe_ro_dir(
                &mut out,
                parent,
                &non_socket_forbidden,
                host_home_canon,
                &[],
            );
        }
    }
    if let Some(rustup) = host_rust_dir_env_from(
        rustup_explicit,
        host_home.clone(),
        ".rustup",
        &non_socket_raw,
    ) {
        push_safe_ro_dir(
            &mut out,
            Path::new(&rustup),
            &non_socket_forbidden,
            host_home_canon,
            &[],
        );
    }
    if let Some(socket) = office_socket {
        push_office_socket_dir(&mut out, socket, &socket_forbidden, host_home_canon);
    }
    if let Some(cargo) =
        host_rust_dir_env_from(cargo_explicit, host_home, ".cargo", &non_socket_raw)
    {
        let cargo_path = Path::new(&cargo);
        let cargo_canon = canonicalize_with_tail(cargo_path);
        for sub in ["bin", "registry", "git"] {
            push_safe_ro_dir(
                &mut out,
                &cargo_path.join(sub),
                &non_socket_forbidden,
                host_home_canon,
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

fn is_plain_relative_path(rel: &str) -> bool {
    let path = Path::new(rel);
    !rel.is_empty()
        && path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

fn push_relative_credential_denies(base: &Path, rel_paths: &[&str], out: &mut Vec<PathPattern>) {
    for &rel in rel_paths {
        if is_plain_relative_path(rel) {
            push_exact_pattern(out, &base.join(rel));
        }
    }
}

pub(super) fn append_runtime_credential_denies(
    runtimes: &crate::runtime_registry::RuntimeRegistry,
    out: &mut Vec<PathPattern>,
) {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let home = PathBuf::from(home);
    append_runtime_credential_denies_for_home(runtimes, &home, out);
}

pub(super) fn append_runtime_credential_denies_for_home(
    runtimes: &crate::runtime_registry::RuntimeRegistry,
    home: &Path,
    out: &mut Vec<PathPattern>,
) {
    if !home.is_absolute() {
        return;
    }
    for adapter in runtimes.adapters() {
        let profile = adapter.host_profile();
        if let Some(subdir) = profile
            .host_config_subdir
            .filter(|s| is_plain_relative_path(s))
        {
            push_relative_credential_denies(
                &home.join(subdir),
                profile.config_credential_files,
                out,
            );
        }
        push_relative_credential_denies(home, profile.host_credential_files, out);
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

/// `agent-os` external sign-in state folders this account may have (`TASK-137`): workspace keys,
/// fast tokens and `exec` token files live under `<application base>/agent-os/`. Every managed
/// sandbox denies them, so a managed agent can never read another agent's external key or token.
///
/// - `<application base of $HOME>/agent-os` and, when it differs (GNU/Linux `XDG_DATA_HOME`),
///   `<account application base>/agent-os`;
/// - in a development build (`development`), the roots `agent-os` itself may be moved to:
///   `AGENT_OS_STATE_DIR` and `ORG_DEV_ACCOUNT_BASE/agent-os` (absolute paths only).
pub(crate) fn agent_os_state_roots_from(
    env: &dyn Fn(&str) -> Option<OsString>,
    development: bool,
) -> Vec<PathBuf> {
    let absolute = |name: &str| {
        env(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut add = |root: PathBuf| {
        if !roots.contains(&root) {
            roots.push(root);
        }
    };
    if let Some(home) = absolute("HOME") {
        for base in [
            org_platform::dirs::application_base(&home),
            org_platform::dirs::account_application_base(&home),
        ]
        .into_iter()
        .flatten()
        {
            add(base.join("agent-os"));
        }
    }
    if development {
        if let Some(dir) = absolute("AGENT_OS_STATE_DIR") {
            add(dir);
        }
        if let Some(base) = absolute("ORG_DEV_ACCOUNT_BASE") {
            add(base.join("agent-os"));
        }
    }
    roots
}
