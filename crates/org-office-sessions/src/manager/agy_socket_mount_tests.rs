//! Agy `--sandbox` reaches its office socket through this office's own socket folder
//! `<run>/o/<handle>` only (`TASK-164`): never the runtime folder `<run>`, the shared `<run>/o`,
//! another office's socket folder, `host.sock`, the launch rendezvous `<run>/r`, per-agent
//! temporary folders `<run>/t`, or any folder a crafted socket, toolchain or helper path names.
//! The runtime folder's privileged entries stay denied for every runtime.

use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use org_agent_runtime::PathPattern;
use org_office::{Office, TestOffice};

use super::sandbox_rules::collect_agy_read_only_dirs;
use super::{ensure_bwrap_deny_dirs, SessionManager, SessionManagerConfig};

/// This office and another one sharing its runtime folder.
const THIS: &str = "019a8928-1337-7456-8456-0123456789ac";
const OTHER: &str = "019a8928-1337-7456-8456-ffffffffffff";

/// A host-shaped runtime folder: owner-only `<run>` holding `host.sock`, `r/<rendezvous>`,
/// `t/<slug>/`, a sibling temporary folder `abc/`, and the office sockets folder `o/` with this
/// office's socket folder and another office's, each holding its live `s.sock`.
struct Runtime {
    _dir: tempfile::TempDir,
    root: PathBuf,
    run: PathBuf,
    sockets: PathBuf,
    own: PathBuf,
    other: PathBuf,
    socket: PathBuf,
    other_socket: PathBuf,
    _listeners: Vec<UnixListener>,
}

impl Runtime {
    fn new() -> Self {
        use org_proto::location::{
            office_sockets_dir, selected_office_socket_dir_in, selected_office_socket_in,
        };
        let dir = tempfile::Builder::new().prefix("a").tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let run = root.join("run");
        let sockets = office_sockets_dir(&run);
        let own = selected_office_socket_dir_in(&run, THIS).unwrap();
        let other = selected_office_socket_dir_in(&run, OTHER).unwrap();
        for d in [
            &run,
            &run.join("r"),
            &run.join("t"),
            &run.join("t").join("0badc0de"),
            &run.join("abc"),
            &sockets,
            &own,
            &other,
        ] {
            org_platform::fs::create_owner_only_dir(d).unwrap();
        }
        let socket = selected_office_socket_in(&run, THIS).unwrap();
        let other_socket = selected_office_socket_in(&run, OTHER).unwrap();
        assert_eq!(socket.parent(), Some(own.as_path()));
        assert_eq!(own.parent(), Some(sockets.as_path()));
        let listeners = [
            run.join("host.sock"),
            run.join("r").join("0".repeat(16)),
            socket.clone(),
            other_socket.clone(),
        ]
        .iter()
        .map(|p| UnixListener::bind(p).unwrap())
        .collect();
        Self {
            _dir: dir,
            root,
            run,
            sockets,
            own,
            other,
            socket,
            other_socket,
            _listeners: listeners,
        }
    }

    /// What Agy would mount for `socket` alone: no helper folder or toolchain exists.
    fn mounts(&self, socket: &Path, forbidden: &[PathBuf]) -> Vec<PathBuf> {
        self.mounts_with(socket, &self.root.join("no-helpers"), None, forbidden)
    }

    /// What Agy would mount for `socket` with this helper folder, and `RUSTUP_HOME` and
    /// `CARGO_HOME` both set to `toolchain`.
    fn mounts_with(
        &self,
        socket: &Path,
        helper_dir: &Path,
        toolchain: Option<&Path>,
        forbidden: &[PathBuf],
    ) -> Vec<PathBuf> {
        collect_agy_read_only_dirs(
            helper_dir,
            Some(socket),
            toolchain.map(Into::into),
            toolchain.map(Into::into),
            None,
            forbidden,
        )
        .iter()
        .map(|spec| PathBuf::from(spec.as_str()))
        .collect()
    }

    /// No mount but this office's own socket folder holds, equals or sits in the runtime folder.
    fn assert_nothing_else_in_runtime(&self, mounts: &[PathBuf]) {
        for mount in mounts {
            assert!(mount != Path::new("/"), "{mounts:?}");
            if *mount != self.own {
                assert!(
                    !mount.starts_with(&self.run) && !self.run.starts_with(mount),
                    "{mount:?} overlaps the runtime folder: {mounts:?}"
                );
            }
        }
    }

    /// Every place in the runtime folder a crafted toolchain or helper path could name: the
    /// folder itself, `host.sock`, `r/`, `t/`, a sibling temporary folder, the shared `o/`, this
    /// office's socket folder and another office's.
    fn targets(&self) -> Vec<PathBuf> {
        vec![
            self.run.clone(),
            self.run.join("host.sock"),
            self.run.join("r"),
            self.run.join("t"),
            self.run.join("abc"),
            self.sockets.clone(),
            self.own.clone(),
            self.other.clone(),
        ]
    }

    /// Makes `target` (a folder) look like a Cargo home and a helper folder, and links
    /// `<root>/helpers/agent-os` to its `agent-os` (or to `target` itself when it isn't a
    /// folder). Returns that helper folder, which is outside the runtime folder.
    fn furnish(&self, target: &Path) -> PathBuf {
        let mut agent_os = target.to_path_buf();
        if target.symlink_metadata().is_ok_and(|m| m.is_dir()) {
            for sub in ["bin", "registry", "git"] {
                std::fs::create_dir_all(target.join(sub)).unwrap();
            }
            agent_os = target.join("agent-os");
            std::fs::write(&agent_os, b"#!/bin/sh\n").unwrap();
        }
        let helpers = self.root.join("helpers");
        org_platform::fs::create_owner_only_dir(&helpers).unwrap();
        org_platform::fs::create_symlink(&agent_os, &helpers.join("agent-os")).unwrap();
        helpers
    }
}

#[test]
fn agy_mounts_exactly_this_offices_own_socket_folder() {
    let rt = Runtime::new();
    let mounts = rt.mounts(&rt.socket, &[rt.root.join("home"), rt.root.join("ws")]);
    assert_eq!(mounts, std::slice::from_ref(&rt.own));
    rt.assert_nothing_else_in_runtime(&mounts);
    // The other office's agents get only theirs: neither sees the other's or the shared `o/`.
    assert_eq!(
        rt.mounts(&rt.other_socket, &[]),
        std::slice::from_ref(&rt.other)
    );
    // No socket (or none configured) mounts nothing.
    let none = collect_agy_read_only_dirs(&rt.root.join("no-helpers"), None, None, None, None, &[]);
    assert!(none.is_empty(), "{none:?}");
}

#[test]
fn agy_never_mounts_a_sibling_office_folder_in_place_of_its_own() {
    // This office's folder is missing.
    let rt = Runtime::new();
    std::fs::remove_dir_all(&rt.own).unwrap();
    assert!(rt.mounts(&rt.socket, &[]).is_empty());
    // It is a link to the other office's folder.
    org_platform::fs::create_symlink(&rt.other, &rt.own).unwrap();
    assert!(rt.mounts(&rt.socket, &[]).is_empty());
    // Its `s.sock` is a link to the other office's socket.
    std::fs::remove_file(&rt.own).unwrap();
    org_platform::fs::create_owner_only_dir(&rt.own).unwrap();
    org_platform::fs::create_symlink(&rt.other_socket, &rt.socket).unwrap();
    assert!(rt.mounts(&rt.socket, &[]).is_empty());
    // Its `s.sock` is a regular file.
    std::fs::remove_file(&rt.socket).unwrap();
    std::fs::write(&rt.socket, b"").unwrap();
    assert!(rt.mounts(&rt.socket, &[]).is_empty());
}

#[test]
fn agy_never_mounts_anything_for_a_crafted_socket_path() {
    let rt = Runtime::new();
    // The former layouts, `<run>/<handle>.sock` and `<run>/o/<handle>.sock`, would need `<run>`
    // or the shared `<run>/o`; a reserved handle or a sibling temporary folder is no office's.
    let reserved = rt.sockets.join("r").join("s.sock");
    org_platform::fs::create_owner_only_dir(reserved.parent().unwrap()).unwrap();
    let live = [
        rt.run.join("0123456789ac.sock"),
        rt.sockets.join("0123456789ac.sock"),
        reserved,
        rt.run.join("abc").join("s.sock"),
    ];
    for socket in &live {
        drop(UnixListener::bind(socket).unwrap());
    }
    let crafted = live.into_iter().chain([
        rt.run.join("host.sock"),
        rt.run.join("r").join("0".repeat(16)),
        rt.run.join("t").join("0badc0de").join("s.sock"),
        rt.own.join("..").join("0123456789ac").join("s.sock"),
        rt.own.join("x.sock"),
        rt.own.clone(),
        rt.sockets.join("s.sock"),
        PathBuf::from("run/o/0123456789ac/s.sock"),
        PathBuf::from("/o/0123456789ac/s.sock"),
    ]);
    for socket in crafted {
        let mounts = rt.mounts(&socket, &[]);
        assert!(mounts.is_empty(), "{socket:?} mounted {mounts:?}");
    }
}

/// A name planted in this office's socket folder and how to plant it.
type Plant = (&'static str, fn(&Runtime, &Path));

#[test]
fn agy_refuses_a_socket_folder_holding_anything_but_its_socket() {
    let plant: [Plant; 8] = [
        ("host.sock", |_, p| drop(UnixListener::bind(p).unwrap())),
        ("x.sock", |_, p| drop(UnixListener::bind(p).unwrap())),
        ("r", |_, p| std::fs::create_dir(p).unwrap()),
        ("t", |_, p| std::fs::create_dir(p).unwrap()),
        ("sub", |_, p| std::fs::create_dir(p).unwrap()),
        ("secrets.json", |_, p| std::fs::write(p, b"{}").unwrap()),
        ("link.sock", |_, p| {
            org_platform::fs::create_symlink(Path::new("/etc/hosts"), p).unwrap();
        }),
        ("other", |rt, p| {
            org_platform::fs::create_symlink(&rt.other, p).unwrap();
        }),
    ];
    for (name, plant) in plant {
        let rt = Runtime::new();
        plant(&rt, &rt.own.join(name));
        let mounts = rt.mounts(&rt.socket, &[]);
        assert!(
            mounts.is_empty(),
            "o/<handle> holding {name} mounted {mounts:?}"
        );
    }
    // Anything beside the office folders in the shared `o/` never blocks or widens the mount.
    let rt = Runtime::new();
    std::fs::write(rt.sockets.join("stray.json"), b"{}").unwrap();
    assert_eq!(rt.mounts(&rt.socket, &[]), std::slice::from_ref(&rt.own));
}

#[test]
fn agy_refuses_linked_loose_or_forbidden_socket_folders() {
    // `o/<handle>` is a link: to the runtime folder, the shared `o/`, or a private copy.
    let rt = Runtime::new();
    let copy = rt.root.join("elsewhere");
    std::fs::rename(&rt.own, &copy).unwrap();
    for target in [&rt.run, &rt.sockets, &copy] {
        org_platform::fs::create_symlink(target, &rt.own).unwrap();
        let mounts = rt.mounts(&rt.socket, &[]);
        assert!(mounts.is_empty(), "linked to {target:?}: {mounts:?}");
        std::fs::remove_file(&rt.own).unwrap();
    }
    // `o` is a link: to the runtime folder, or to a private copy holding this office's folder.
    let rt = Runtime::new();
    let copy = rt.root.join("o-copy");
    std::fs::rename(&rt.sockets, &copy).unwrap();
    for target in [&rt.run, &copy] {
        org_platform::fs::create_symlink(target, &rt.sockets).unwrap();
        let mounts = rt.mounts(&rt.socket, &[]);
        assert!(mounts.is_empty(), "o linked to {target:?}: {mounts:?}");
        std::fs::remove_file(&rt.sockets).unwrap();
    }
    // Either folder not exactly 0700: loose, missing an owner bit, or with a special bit.
    for (own, mode) in [
        (true, 0o755),
        (true, 0o710),
        (true, 0o500),
        (true, 0o1700),
        (false, 0o755),
        (false, 0o711),
        (false, 0o300),
        (false, 0o1700),
    ] {
        let rt = Runtime::new();
        let dir = if own { &rt.own } else { &rt.sockets };
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode)).unwrap();
        let mounts = rt.mounts(&rt.socket, &[]);
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(mounts.is_empty(), "{dir:?} at {mode:o}: {mounts:?}");
    }
    // Inside, equal to, or holding a forbidden folder (symmetric overlap).
    let rt = Runtime::new();
    for forbidden in [
        rt.root.clone(),
        rt.run.clone(),
        rt.sockets.clone(),
        rt.own.clone(),
        rt.own.join("nested"),
    ] {
        let mounts = rt.mounts(&rt.socket, std::slice::from_ref(&forbidden));
        assert!(mounts.is_empty(), "{forbidden:?}: {mounts:?}");
    }
}

#[test]
fn agy_derives_the_runtime_folder_itself_and_mounts_no_toolchain_or_helper_inside_it() {
    // The caller forbids nothing: `<run>` comes from the office socket alone.
    let probe = Runtime::new();
    for target in probe.targets() {
        let target = target.strip_prefix(&probe.root).unwrap().to_path_buf();
        let rt = Runtime::new();
        let target = rt.root.join(target);
        let helpers = rt.furnish(&target);
        for (helper_dir, toolchain) in [(&target, Some(target.as_path())), (&helpers, None)] {
            let mounts = rt.mounts_with(&rt.socket, helper_dir, toolchain, &[]);
            rt.assert_nothing_else_in_runtime(&mounts);
            // A furnished own folder is polluted, so not even it is mounted.
            assert_eq!(mounts.contains(&rt.own), target != rt.own, "{mounts:?}");
            assert!(!mounts.contains(&rt.other) && !mounts.contains(&rt.sockets));
            assert_eq!(
                mounts.contains(&helpers),
                *helper_dir == helpers,
                "{mounts:?}"
            );
        }
    }
}

fn strings(patterns: &[PathPattern]) -> Vec<String> {
    patterns.iter().map(|p| p.as_str().to_owned()).collect()
}

/// An office's session manager whose office socket is `rt.socket`, with this helper folder.
fn manager(rt: &Runtime, helper_dir: PathBuf) -> (SessionManager, String) {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let mut config = SessionManagerConfig::production(rt.socket.clone());
    config.helper_dir = Some(helper_dir);
    let mgr = SessionManager::new(Arc::clone(&office), config);
    let me = office.list_employees().unwrap()[0].id.as_str().to_owned();
    (mgr, me)
}

fn agy_mounts(rt: &Runtime, mgr: &SessionManager) -> Vec<PathBuf> {
    let agy = org_agent_runtime_agy::AgyRuntime::new();
    mgr.collect_session_read_only_dirs(&agy, &rt.root.join("home"), &rt.root.join("ws"), true)
        .iter()
        .map(|spec| PathBuf::from(spec.as_str()))
        .collect()
}

#[test]
fn the_session_manager_mounts_no_helper_folder_inside_the_runtime_folder() {
    let probe = Runtime::new();
    for target in probe.targets() {
        let target = target.strip_prefix(&probe.root).unwrap().to_path_buf();
        let rt = Runtime::new();
        let target = rt.root.join(target);
        let helpers = rt.furnish(&target);
        for helper_dir in [target.clone(), helpers.clone()] {
            let (mgr, _) = manager(&rt, helper_dir);
            let mounts = agy_mounts(&rt, &mgr);
            rt.assert_nothing_else_in_runtime(&mounts);
            assert_eq!(mounts.contains(&rt.own), target != rt.own, "{mounts:?}");
            assert!(!mounts.contains(&rt.other) && !mounts.contains(&rt.sockets));
        }
    }
}

#[test]
fn the_session_manager_mounts_its_own_socket_folder_for_agy_only_and_keeps_runtime_entries_denied()
{
    let rt = Runtime::new();
    let (mgr, me) = manager(&rt, rt.root.join("no-helpers"));
    let home = rt.root.join("home");
    let ws = rt.root.join("ws");
    let agy = org_agent_runtime_agy::AgyRuntime::new();

    let mounts = agy_mounts(&rt, &mgr);
    assert!(mounts.contains(&rt.own), "{mounts:?}");
    assert!(!mounts.contains(&rt.sockets) && !mounts.contains(&rt.other));
    rt.assert_nothing_else_in_runtime(&mounts);
    assert!(mgr
        .collect_session_read_only_dirs(&agy, &home, &ws, false)
        .is_empty());
    let claude = org_agent_runtime_claude::ClaudeRuntime::new();
    assert!(mgr
        .collect_session_read_only_dirs(&claude, &home, &ws, true)
        .is_empty());

    // `host.sock`, `<run>/r/**` and other agents' `<run>/t/<slug>/**` stay denied in the runtime
    // folder (not inside `o/`), and the office socket itself is never denied.
    let denied = strings(&mgr.denied_office_globs_for(Some(&me)));
    let at = |p: PathBuf| p.display().to_string();
    for wanted in [
        at(rt.run.join("host.sock")),
        format!("{}/**", at(rt.run.join("r"))),
        format!("{}/**", at(rt.run.join("t").join("0badc0de"))),
        "/var/run/docker.sock".to_owned(),
        "/run/podman/**".to_owned(),
    ] {
        assert!(denied.contains(&wanted), "{wanted} not denied: {denied:?}");
    }
    let socket = at(rt.socket.clone());
    let sockets = at(rt.sockets.clone());
    for pattern in &denied {
        assert!(
            !pattern.starts_with(&format!("{sockets}/")) && *pattern != sockets,
            "{pattern} denies inside o/"
        );
        assert_ne!(*pattern, socket);
    }

    // Per-agent temporary folders stay in `<run>/t`, never inside `o/` (a socket outside the
    // `o/<handle>/` layout keeps its own folder's `t/`).
    let fallback = Path::new("/durable/fallback/that/is/longer/than/the/short/one");
    for (socket, short_root) in [("/r/o/0123456789ac/s.sock", "/r/t"), ("/r/x.sock", "/r/t")] {
        let tmpdir = crate::host_auth::resolve_agent_tmpdir(Path::new(socket), fallback, "o1", &me);
        assert_eq!(tmpdir.parent(), Some(Path::new(short_root)), "{socket}");
    }
    ensure_bwrap_deny_dirs(&mgr, &me).unwrap();
    assert!(!rt.sockets.join("t").exists() && !rt.own.join("t").exists());
    assert!(rt.run.join("t").is_dir());
    // Preparing the sandbox leaves this office's folder mountable: it still holds only its socket.
    assert!(agy_mounts(&rt, &mgr).contains(&rt.own));
    for dir in [&rt.sockets, &rt.own] {
        let mode = std::fs::metadata(dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o7777, 0o700, "{dir:?}");
    }
}
