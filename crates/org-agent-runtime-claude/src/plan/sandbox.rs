//! Claude Code's `sandbox` settings: the Bash sandbox's filesystem and network lists, and
//! what Linux `bwrap` needs changed in them.

use serde_json::{json, Map, Value};

use org_agent_runtime::{LaunchContext, SessionSpec, SpecPath};

pub(super) fn build_sandbox_json(spec: &SessionSpec, ctx: &LaunchContext) -> Value {
    if !spec.sandbox.enabled {
        return json!({ "enabled": false });
    }
    let mut writable: Vec<String> = spec
        .sandbox
        .writable_roots
        .iter()
        .map(|p| p.as_str().to_owned())
        .collect();
    if !writable.iter().any(|p| p == ctx.tmpdir.as_str()) {
        writable.push(ctx.tmpdir.as_str().to_owned());
    }

    let mut deny_write: Vec<String> = spec
        .sandbox
        .deny_write
        .iter()
        .map(|p| p.as_str().to_owned())
        .collect();
    for dir in &spec.read_only_dirs {
        let path = dir.as_str();
        if !deny_write.iter().any(|p| p == path) {
            deny_write.push(path.to_owned());
        }
    }
    let mut deny_read: Vec<String> = spec
        .sandbox
        .deny_read
        .iter()
        .map(|p| p.as_str().to_owned())
        .collect();
    if let Some(cfg_dir) = &ctx.config_dir {
        if !deny_write.iter().any(|p| p == cfg_dir.as_str()) {
            deny_write.push(cfg_dir.as_str().to_owned());
        }
        let cred_file = format!(
            "{}/.credentials.json",
            cfg_dir.as_str().trim_end_matches('/')
        );
        if !deny_read.iter().any(|p| p == &cred_file) {
            deny_read.push(cred_file);
        }
    }

    let is_linux_bwrap = !org_platform::confinement::unix_socket_path_filter_supported();
    let allow_all_unix = !spec.sandbox.allowed_unix_sockets.is_empty() && is_linux_bwrap;
    apply_linux_bwrap_sandbox_rules(
        &mut deny_read,
        &mut deny_write,
        &writable,
        is_linux_bwrap,
        allow_all_unix,
    );
    let network = build_sandbox_network_json(&spec.sandbox.allowed_unix_sockets, allow_all_unix);

    json!({
        "enabled": true,
        "autoAllowBashIfSandboxed": false,
        "allowUnsandboxedCommands": false,
        "filesystem": {
            "allowWrite": writable,
            "denyWrite": deny_write,
            "denyRead": deny_read,
        },
        "network": network,
    })
}

fn apply_linux_bwrap_sandbox_rules(
    deny_read: &mut Vec<String>,
    deny_write: &mut Vec<String>,
    writable: &[String],
    is_linux_bwrap: bool,
    allow_all_unix: bool,
) {
    if allow_all_unix {
        append_linux_privileged_socket_denies(deny_read);
    }
    if is_linux_bwrap {
        // Linux `bwrap` mounts `--tmpfs <dir>` cleanly on concrete directory paths, whereas
        // `dir/**` causes `@anthropic-ai/sandbox-runtime` to walk inside denied directories and
        // follow symlinks (e.g. a sibling agent's `.cargo/registry`, `TASK-163`). Strip trailing
        // `/**`/`/*` on concrete non-wildcard prefixes in `denyRead` and `denyWrite` so bwrap
        // masks the directory atomically at its boundary (`TASK-190`).
        *deny_read = normalize_linux_bwrap_concrete_globs(std::mem::take(deny_read));
        // Linux `bwrap` mounts `/` read-only (`--ro-bind / /`) and only `allowWrite` roots are
        // writable. Omitting `denyWrite` entries outside `allowWrite` keeps write protection
        // intact while preventing `@anthropic-ai/sandbox-runtime` from creating 0-byte `0444`
        // regular files on the real host for absent paths (`TASK-144`). Full write denies remain
        // enforced in `permissions.deny` and the office approval bridge.
        let kept = filter_linux_bwrap_deny_write(std::mem::take(deny_write), writable);
        *deny_write = normalize_linux_bwrap_concrete_globs(kept);
    } else {
        *deny_read = collapse_trailing_globstars(std::mem::take(deny_read));
        *deny_write = collapse_trailing_globstars(std::mem::take(deny_write));
    }
}

fn collapse_trailing_globstars(patterns: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(patterns.len());
    for entry in patterns {
        let (base, had_globstar) = split_trailing_globstar(&entry);
        let candidate = if had_globstar && !base.is_empty() {
            format!("{base}/**")
        } else {
            entry
        };
        if !out.iter().any(|existing| existing == &candidate) {
            out.push(candidate);
        }
    }
    out
}

/// `raw` without trailing `/` and without any run of trailing `/**` segments, and whether at
/// least one `/**` was removed. Callers append at most one `/**` back, so a malformed
/// `dir/**/**` can never be emitted (`TASK-190`).
pub(super) fn split_trailing_globstar(raw: &str) -> (&str, bool) {
    let mut base = raw.trim_end_matches('/');
    let mut had_globstar = false;
    while let Some(stripped) = base.strip_suffix("/**") {
        base = stripped.trim_end_matches('/');
        had_globstar = true;
    }
    (base, had_globstar)
}

/// `true` for a non-empty path with no glob metacharacters.
pub(super) fn is_concrete_path(path: &str) -> bool {
    !path.is_empty() && !path.contains(['*', '?', '['])
}

/// Linux `bwrap` form of a deny pattern: a concrete `dir/**` or `dir/*` becomes `dir`, so
/// `@anthropic-ai/sandbox-runtime` masks the directory atomically at its boundary instead of
/// walking inside it and following symlinks into sibling homes (`TASK-163`, `TASK-190`). A
/// wildcard pattern keeps exactly one trailing `/**` (or its `/*`); the root is left alone.
pub(super) fn normalize_bwrap_pattern(raw: &str) -> String {
    let (base, had_globstar) = split_trailing_globstar(raw);
    let (base, had_star) = match base.strip_suffix("/*") {
        Some(dir) if !had_globstar => (dir.trim_end_matches('/'), true),
        _ => (base, false),
    };
    if base.is_empty() {
        return raw.to_owned();
    }
    if is_concrete_path(base) {
        return base.to_owned();
    }
    if had_globstar {
        format!("{base}/**")
    } else if had_star {
        format!("{base}/*")
    } else {
        base.to_owned()
    }
}

fn normalize_linux_bwrap_concrete_globs(patterns: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(patterns.len());
    for entry in patterns {
        let candidate = normalize_bwrap_pattern(&entry);
        if !out.iter().any(|existing| existing == &candidate) {
            out.push(candidate);
        }
    }
    out
}

fn split_path_segments(raw: &str) -> Vec<&str> {
    raw.split('/').filter(|seg| !seg.is_empty()).collect()
}

/// Returns `true` when `pattern` could match any path at, inside, or enclosing `root`.
/// Used on Linux `bwrap` (where `/` is read-only) so `denyWrite` retains every write deny that
/// overlaps an `allowWrite` root while dropping disjoint entries outside `allowWrite`.
fn pattern_overlaps_root(pattern: &str, root: &str) -> bool {
    let trimmed = pattern
        .strip_suffix("/**")
        .or_else(|| pattern.strip_suffix("/*"))
        .unwrap_or(pattern);
    let pat_segs = split_path_segments(trimmed);
    let root_segs = split_path_segments(root);
    for (p, r) in pat_segs.iter().zip(root_segs.iter()) {
        if *p == "**" {
            return true;
        }
        if p.contains('*') || p.contains('?') || p.contains('[') {
            continue;
        }
        if p != r {
            return false;
        }
    }
    true
}

fn filter_linux_bwrap_deny_write(deny_write: Vec<String>, writable: &[String]) -> Vec<String> {
    deny_write
        .into_iter()
        .filter(|pat| writable.iter().any(|root| pattern_overlaps_root(pat, root)))
        .collect()
}

const LINUX_PRIVILEGED_SOCKET_DENIES: &[&str] = &[
    "/run/user/*/bus",
    "/run/user/*/systemd/**",
    "/run/user/*/podman/**",
    "/run/dbus/system_bus_socket",
    "/var/run/dbus/system_bus_socket",
    "/var/run/docker.sock",
    "/run/docker.sock",
    "/run/podman/**",
];

fn append_linux_privileged_socket_denies(deny_read: &mut Vec<String>) {
    // Defense-in-depth when Linux seccomp requires `allowAllUnixSockets`: hide well-known
    // privileged host sockets (systemd user D-Bus, system D-Bus, Docker/Podman) inside bwrap.
    // Under Linux `bwrap` `/` is already read-only, so these only need `denyRead` (`TASK-144`).
    for &pat in LINUX_PRIVILEGED_SOCKET_DENIES {
        if !deny_read.iter().any(|p| p == pat) {
            deny_read.push(pat.to_owned());
        }
    }
}

fn build_sandbox_network_json(
    allowed_unix_sockets: &[SpecPath],
    allow_all_unix_sockets: bool,
) -> Value {
    let allow_sockets: Vec<Value> = allowed_unix_sockets
        .iter()
        .map(|p| Value::String(p.as_str().to_owned()))
        .collect();
    let mut network = Map::new();
    network.insert("allowUnixSockets".to_owned(), Value::Array(allow_sockets));
    if allow_all_unix_sockets {
        // Linux seccomp filters `socket(AF_UNIX)` without inspecting `connect()` paths,
        // so Claude's `bwrap` sandbox blocks `agent-os` unless `allowAllUnixSockets` is set.
        network.insert("allowAllUnixSockets".to_owned(), Value::Bool(true));
    }
    Value::Object(network)
}

#[cfg(test)]
mod tests {
    use super::{
        append_linux_privileged_socket_denies, apply_linux_bwrap_sandbox_rules,
        build_sandbox_network_json, filter_linux_bwrap_deny_write, normalize_bwrap_pattern,
        normalize_linux_bwrap_concrete_globs, split_trailing_globstar,
    };
    use org_agent_runtime::SpecPath;
    use serde_json::json;

    #[test]
    fn build_sandbox_network_json_sets_allow_all_only_when_requested() {
        let sock = SpecPath::try_from("/run/office/off.sock").unwrap();
        let strict = build_sandbox_network_json(std::slice::from_ref(&sock), false);
        assert_eq!(strict["allowUnixSockets"], json!(["/run/office/off.sock"]));
        assert!(strict.get("allowAllUnixSockets").is_none());

        let linux = build_sandbox_network_json(std::slice::from_ref(&sock), true);
        assert_eq!(linux["allowUnixSockets"], json!(["/run/office/off.sock"]));
        assert_eq!(linux["allowAllUnixSockets"], json!(true));
    }

    #[test]
    fn linux_privileged_socket_denies_mask_dbus_and_docker_without_duplicates() {
        let mut deny_read = vec!["/run/docker.sock".to_owned()];
        append_linux_privileged_socket_denies(&mut deny_read);
        for expected in [
            "/run/user/*/bus",
            "/run/user/*/systemd/**",
            "/run/dbus/system_bus_socket",
            "/var/run/docker.sock",
            "/run/docker.sock",
        ] {
            assert!(
                deny_read.iter().any(|p| p == expected),
                "deny_read missing {expected}"
            );
        }
        assert_eq!(
            deny_read
                .iter()
                .filter(|p| *p == "/run/docker.sock")
                .count(),
            1
        );

        // `TASK-163`: concrete `dir/**` entries normalize to `dir` (deduplicated) while wildcard
        // patterns like `/run/user/*/systemd/**` stay intact.
        let normalized = normalize_linux_bwrap_concrete_globs(vec![
            "/office/agents/sibling-1/**".into(),
            "/office/agents/sibling-1".into(),
            "/office/durable/state/**".into(),
            "/run/user/*/systemd/**".into(),
            "/home/agents/.cargo/credentials.toml".into(),
        ]);
        assert_eq!(
            normalized,
            vec![
                "/office/agents/sibling-1",
                "/office/durable/state",
                "/run/user/*/systemd/**",
                "/home/agents/.cargo/credentials.toml",
            ]
        );
    }

    #[test]
    fn trailing_globstars_collapse_and_bwrap_drops_them_only_on_concrete_dirs() {
        assert_eq!(split_trailing_globstar("/a/b/**/**/"), ("/a/b", true));
        assert_eq!(split_trailing_globstar("/a/b/"), ("/a/b", false));
        assert_eq!(split_trailing_globstar("/a/*/b/**"), ("/a/*/b", true));
        assert_eq!(split_trailing_globstar("/**"), ("", true));
        for (raw, expected) in [
            ("/office/agents/sib/**/**", "/office/agents/sib"),
            ("/office/agents/sib/**", "/office/agents/sib"),
            ("/office/agents/sib/*", "/office/agents/sib"),
            ("/office/agents/sib/", "/office/agents/sib"),
            ("/office/offices/*/**/**", "/office/offices/*/**"),
            ("/run/user/*/bus", "/run/user/*/bus"),
            ("/**", "/**"),
        ] {
            assert_eq!(normalize_bwrap_pattern(raw), expected, "{raw}");
        }
        let normalized = normalize_linux_bwrap_concrete_globs(vec![
            "/work/t-1/.git/hooks/**".into(),
            "/work/t-1/.git/hooks".into(),
            "/work/t-1/.git/hooks/**/**".into(),
        ]);
        assert_eq!(normalized, vec!["/work/t-1/.git/hooks"]);
    }

    #[test]
    fn co_hosted_neighbour_masks_are_atomic_bwrap_masks_and_never_bwrap_write_denies() {
        // What `org-office-sessions` denies a co-hosted satellite's agent (`TASK-204`): another
        // organization, its HQ's data roots, a sibling satellite and the office folders, each
        // exactly and as `dir/**`, in both `denyRead` and `denyWrite`.
        let org = "/data/0195a000-0000-7000-8000-00000000a001";
        let neighbours = [
            "/data/0195a000-0000-7000-8000-00000000b001".to_owned(),
            format!("{org}/durable"),
            format!("{org}/runtime"),
            format!("{org}/offices/0195a000-0000-7000-8000-00000000a0a2"),
            "/home/office-state/offices".to_owned(),
        ];
        let patterns: Vec<String> = neighbours
            .iter()
            .flat_map(|n| [n.clone(), format!("{n}/**")])
            .collect();
        let worktree = format!("{org}/offices/0195a000-0000-7000-8000-00000000a0a1/runtime/w/me");
        let hooks = format!("{worktree}/.git/hooks/**");
        let bwrap = |is_linux_bwrap: bool| {
            let mut deny_read = patterns.clone();
            let mut deny_write = patterns.clone();
            deny_write.push(hooks.clone());
            apply_linux_bwrap_sandbox_rules(
                &mut deny_read,
                &mut deny_write,
                std::slice::from_ref(&worktree),
                is_linux_bwrap,
                false,
            );
            (deny_read, deny_write)
        };
        // Linux `bwrap`: one atomic mask per neighbour, and no write deny outside the writable
        // worktree (`/` is mounted read-only), so nothing is ever made on the host for one.
        let (deny_read, deny_write) = bwrap(true);
        assert_eq!(deny_read, neighbours);
        assert_eq!(deny_write, vec![format!("{worktree}/.git/hooks")]);
        // Elsewhere both forms are kept as given.
        let (deny_read, deny_write) = bwrap(false);
        assert_eq!(deny_read, patterns);
        assert_eq!(deny_write.len(), patterns.len() + 1);
    }

    #[test]
    fn filter_linux_bwrap_deny_write_keeps_only_entries_overlapping_allow_write() {
        let writable = vec![
            "/work/t-1".to_owned(),
            "/run/user/1001/t/abc".to_owned(),
            "/var/lib/org/repos/r1.git/objects".to_owned(),
            "/var/lib/org/repos/r1.git/worktrees/wt1".to_owned(),
        ];
        let deny_write = vec![
            "/work/t-1/.git/hooks/**".to_owned(),
            "/var/lib/org/repos/r1.git/objects/info".to_owned(),
            "/var/lib/org/repos/r1.git/objects/info/**".to_owned(),
            "/var/lib/org/repos/r1.git/worktrees/wt1/commondir".to_owned(),
            "/var/lib/org/repos/r1.git/hooks".to_owned(),
            "/var/lib/org/repos/r1.git/hooks/**".to_owned(),
            "/var/lib/org/repos/r1.git/config".to_owned(),
            "/office/durable/authority/**".to_owned(),
            "/home/agents/.claude".to_owned(),
            "/run/user/*/bus".to_owned(),
            "/var/lib/org/repos/**".to_owned(),
        ];
        let kept = filter_linux_bwrap_deny_write(deny_write, &writable);
        for must_keep in [
            "/work/t-1/.git/hooks/**",
            "/var/lib/org/repos/r1.git/objects/info",
            "/var/lib/org/repos/r1.git/objects/info/**",
            "/var/lib/org/repos/r1.git/worktrees/wt1/commondir",
            "/var/lib/org/repos/**",
        ] {
            assert!(
                kept.iter().any(|p| p == must_keep),
                "expected {must_keep} to remain in denyWrite: {kept:?}"
            );
        }
        for must_drop in [
            "/var/lib/org/repos/r1.git/hooks",
            "/var/lib/org/repos/r1.git/hooks/**",
            "/var/lib/org/repos/r1.git/config",
            "/office/durable/authority/**",
            "/home/agents/.claude",
            "/run/user/*/bus",
        ] {
            assert!(
                !kept.iter().any(|p| p == must_drop),
                "expected {must_drop} outside allowWrite to be dropped: {kept:?}"
            );
        }
    }

    #[test]
    fn seatbelts_keep_attachments_dir_in_deny_write_while_linux_bwrap_filters_non_overlapping() {
        let att_dir = "/office/agents/ada/runtime/attachments";
        let writable = vec!["/work/t-1".to_owned(), "/tmp/t-1".to_owned()];

        // macOS Seatbelt (`is_linux_bwrap = false`): `denyWrite` retains `att_dir` and `att_dir/**`.
        let mut mac_read = Vec::new();
        let mut mac_write = vec![att_dir.to_owned(), format!("{att_dir}/**")];
        super::apply_linux_bwrap_sandbox_rules(
            &mut mac_read,
            &mut mac_write,
            &writable,
            false,
            false,
        );
        assert!(
            mac_write.iter().any(|p| p == att_dir),
            "macOS Seatbelt denyWrite must contain exact attachments dir: {mac_write:?}"
        );
        assert!(
            mac_write.iter().any(|p| p == &format!("{att_dir}/**")),
            "macOS Seatbelt denyWrite must contain attachments dir glob: {mac_write:?}"
        );

        // Linux bwrap (`is_linux_bwrap = true`): non-overlapping `att_dir` is cleanly dropped so
        // bwrap never creates host 0-byte stubs, while an overlapping path stays normalized.
        let mut lnx_read = Vec::new();
        let mut lnx_write = vec![
            att_dir.to_owned(),
            format!("{att_dir}/**"),
            "/work/t-1/attachments/**".to_owned(),
        ];
        super::apply_linux_bwrap_sandbox_rules(
            &mut lnx_read,
            &mut lnx_write,
            &writable,
            true,
            false,
        );
        assert!(
            !lnx_write.iter().any(|p| p.contains(att_dir)),
            "Linux bwrap must drop non-overlapping attachments dir from denyWrite: {lnx_write:?}"
        );
        assert_eq!(lnx_write, vec!["/work/t-1/attachments"]);
    }
}
