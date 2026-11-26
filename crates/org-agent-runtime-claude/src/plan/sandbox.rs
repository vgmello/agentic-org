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
        // `/**`/`/*` on concrete non-wildcard prefixes in `denyRead` so bwrap masks the directory
        // atomically at its boundary.
        *deny_read = normalize_linux_bwrap_deny_read(std::mem::take(deny_read));
        // Linux `bwrap` mounts `/` read-only (`--ro-bind / /`) and only `allowWrite` roots are
        // writable. Omitting `denyWrite` entries outside `allowWrite` keeps write protection
        // intact while preventing `@anthropic-ai/sandbox-runtime` from creating 0-byte `0444`
        // regular files on the real host for absent paths (`TASK-144`). Full write denies remain
        // enforced in `permissions.deny` and the office approval bridge.
        *deny_write = filter_linux_bwrap_deny_write(std::mem::take(deny_write), writable);
    }
}

pub(super) fn strip_concrete_dir_glob(raw: &str) -> &str {
    raw.strip_suffix("/**")
        .or_else(|| raw.strip_suffix("/*"))
        .filter(|prefix| {
            !prefix.is_empty()
                && !prefix.contains('*')
                && !prefix.contains('?')
                && !prefix.contains('[')
        })
        .unwrap_or(raw)
}

fn normalize_linux_bwrap_deny_read(deny_read: Vec<String>) -> Vec<String> {
    let mut out = Vec::with_capacity(deny_read.len());
    for entry in deny_read {
        let stripped = strip_concrete_dir_glob(&entry);
        let candidate = if stripped.len() == entry.len() {
            entry
        } else {
            stripped.to_owned()
        };
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
        append_linux_privileged_socket_denies, build_sandbox_network_json,
        filter_linux_bwrap_deny_write, normalize_linux_bwrap_deny_read,
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
        let normalized = normalize_linux_bwrap_deny_read(vec![
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
}
