//! Agy `--sandbox` reaches its office socket through the isolated office sockets folder
//! `<run>/o` only (`TASK-164`): never the runtime folder `<run>`, `host.sock`, the launch
//! rendezvous `<run>/r`, per-agent temporary folders `<run>/t`, or any folder a crafted socket
//! path names. The runtime folder's privileged entries stay denied for every runtime.

use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use org_agent_runtime::{PathPattern, RuntimeId};
use org_office::{Office, TestOffice};

use super::sandbox_rules::collect_agy_read_only_dirs;
use super::{ensure_bwrap_deny_dirs, SessionManager, SessionManagerConfig};

/// A host-shaped runtime folder: owner-only `<run>` holding `host.sock`, `r/<rendezvous>`,
/// `t/<slug>/` and the office sockets folder `o/` with this office's socket and another's.
struct Runtime {
    _dir: tempfile::TempDir,
    root: PathBuf,
    run: PathBuf,
    sockets: PathBuf,
    socket: PathBuf,
    _listeners: Vec<UnixListener>,
}

impl Runtime {
    fn new() -> Self {
        let dir = tempfile::Builder::new().prefix("a").tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let run = root.join("run");
        let sockets = org_proto::location::office_sockets_dir(&run);
        for d in [&run, &run.join("r"), &run.join("t"), &sockets] {
            org_platform::fs::create_owner_only_dir(d).unwrap();
        }
        org_platform::fs::create_owner_only_dir(&run.join("t").join("0badc0de")).unwrap();
        let socket = org_proto::location::selected_office_socket_in(
            &run,
            "019a8928-1337-7456-8456-0123456789ac",
        )
        .unwrap();
        assert_eq!(socket.parent(), Some(sockets.as_path()));
        let listeners = [
            run.join("host.sock"),
            run.join("r").join("0".repeat(16)),
            socket.clone(),
            sockets.join("ffffffffffff.sock"),
        ]
        .iter()
        .map(|p| UnixListener::bind(p).unwrap())
        .collect();
        Self {
            _dir: dir,
            root,
            run,
            sockets,
            socket,
            _listeners: listeners,
        }
    }

    /// What Agy would mount for `socket` alone: no helper folder or toolchain exists.
    fn mounts(&self, socket: &Path, forbidden: &[PathBuf]) -> Vec<PathBuf> {
        collect_agy_read_only_dirs(
            &self.root.join("no-helpers"),
            Some(socket),
            None,
            None,
            None,
            forbidden,
        )
        .iter()
        .map(|spec| PathBuf::from(spec.as_str()))
        .collect()
    }

    fn assert_nothing_privileged(&self, mounts: &[PathBuf]) {
        for mount in mounts {
            assert!(mount != Path::new("/"), "{mounts:?}");
            assert!(
                !self.run.starts_with(mount),
                "mounted the runtime folder or an ancestor: {mounts:?}"
            );
            for entry in ["host.sock", "r", "t"] {
                let privileged = self.run.join(entry);
                assert!(
                    !mount.starts_with(&privileged) && !privileged.starts_with(mount),
                    "{mount:?} overlaps {privileged:?}"
                );
            }
        }
    }
}

#[test]
fn agy_mounts_exactly_the_isolated_office_sockets_folder() {
    let rt = Runtime::new();
    let mounts = rt.mounts(&rt.socket, &[rt.root.join("home"), rt.root.join("ws")]);
    assert_eq!(mounts, std::slice::from_ref(&rt.sockets));
    rt.assert_nothing_privileged(&mounts);
    // No socket (or none configured) mounts nothing.
    let none = collect_agy_read_only_dirs(&rt.root.join("no-helpers"), None, None, None, None, &[]);
    assert!(none.is_empty(), "{none:?}");
}

#[test]
fn agy_never_mounts_the_runtime_folder_for_a_socket_outside_an_o_folder() {
    let rt = Runtime::new();
    // The former layout, `<run>/<handle>.sock`, would need `<run>` itself.
    let legacy = rt.run.join("0123456789ac.sock");
    drop(UnixListener::bind(&legacy).unwrap());
    let crafted = [
        legacy,
        rt.run.join("host.sock"),
        rt.run.join("r").join("0".repeat(16)),
        rt.run.join("t").join("0badc0de").join("x.sock"),
        rt.sockets.join("..").join("o").join("0123456789ac.sock"),
        rt.sockets.join("0123456789ac.txt"),
        PathBuf::from("run/o/0123456789ac.sock"),
        PathBuf::from("/o/0123456789ac.sock"),
    ];
    for socket in crafted {
        let mounts = rt.mounts(&socket, &[]);
        assert!(mounts.is_empty(), "{socket:?} mounted {mounts:?}");
    }
}

/// A name planted in `o/` and how to plant it.
type Plant = (&'static str, fn(&Path));

#[test]
fn agy_refuses_an_o_folder_holding_privileged_or_unexpected_entries() {
    let plant: [Plant; 5] = [
        ("host.sock", |p| drop(UnixListener::bind(p).unwrap())),
        ("r", |p| std::fs::create_dir(p).unwrap()),
        ("t", |p| std::fs::create_dir(p).unwrap()),
        ("secrets.json", |p| std::fs::write(p, b"{}").unwrap()),
        ("link.sock", |p| {
            org_platform::fs::create_symlink(Path::new("/etc/hosts"), p).unwrap();
        }),
    ];
    for (name, plant) in plant {
        let rt = Runtime::new();
        plant(&rt.sockets.join(name));
        let mounts = rt.mounts(&rt.socket, &[]);
        assert!(mounts.is_empty(), "o/ holding {name} mounted {mounts:?}");
    }
}

#[test]
fn agy_refuses_a_linked_loose_or_forbidden_o_folder() {
    // `o` is a link to the runtime folder: following it would expose `host.sock`, `r/`, `t/`.
    let rt = Runtime::new();
    let real = rt.run.join("o-real");
    std::fs::rename(&rt.sockets, &real).unwrap();
    org_platform::fs::create_symlink(&rt.run, &rt.sockets).unwrap();
    assert!(rt.mounts(&rt.socket, &[]).is_empty());
    std::fs::remove_file(&rt.sockets).unwrap();
    org_platform::fs::create_symlink(&real, &rt.sockets).unwrap();
    assert!(
        rt.mounts(&rt.socket, &[]).is_empty(),
        "a linked o/ is never followed"
    );

    // Not owner-only.
    let rt = Runtime::new();
    std::fs::set_permissions(&rt.sockets, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(rt.mounts(&rt.socket, &[]).is_empty());

    // Inside, equal to, or holding a forbidden folder (symmetric overlap).
    let rt = Runtime::new();
    for forbidden in [rt.root.clone(), rt.run.clone(), rt.sockets.clone()] {
        let mounts = rt.mounts(&rt.socket, std::slice::from_ref(&forbidden));
        assert!(mounts.is_empty(), "{forbidden:?}: {mounts:?}");
    }
    let mounts = rt.mounts(&rt.socket, &[rt.sockets.join("nested")]);
    assert!(mounts.is_empty(), "{mounts:?}");
}

fn strings(patterns: &[PathPattern]) -> Vec<String> {
    patterns.iter().map(|p| p.as_str().to_owned()).collect()
}

#[test]
fn the_session_manager_mounts_o_for_agy_only_and_keeps_runtime_entries_denied() {
    let rt = Runtime::new();
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let mut config = SessionManagerConfig::production(rt.socket.clone());
    config.helper_dir = Some(rt.root.join("no-helpers"));
    let mgr = SessionManager::new(Arc::clone(&office), config);
    let me = office.list_employees().unwrap()[0].id.as_str().to_owned();
    let home = rt.root.join("home");
    let ws = rt.root.join("ws");
    let agy = RuntimeId::try_from("agy").unwrap();

    let mounts: Vec<PathBuf> = mgr
        .collect_session_read_only_dirs(agy.clone(), &home, &ws, true)
        .iter()
        .map(|spec| PathBuf::from(spec.as_str()))
        .collect();
    assert!(mounts.contains(&rt.sockets), "{mounts:?}");
    rt.assert_nothing_privileged(&mounts);
    assert!(mgr
        .collect_session_read_only_dirs(agy, &home, &ws, false)
        .is_empty());
    let claude = RuntimeId::try_from("claude").unwrap();
    assert!(mgr
        .collect_session_read_only_dirs(claude, &home, &ws, true)
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
    // `o/` layout keeps its own folder's `t/`).
    let fallback = Path::new("/durable/fallback/that/is/longer/than/the/short/one");
    for (socket, short_root) in [("/r/o/0123456789ac.sock", "/r/t"), ("/r/x.sock", "/r/t")] {
        let tmpdir = crate::host_auth::resolve_agent_tmpdir(Path::new(socket), fallback, "o1", &me);
        assert_eq!(tmpdir.parent(), Some(Path::new(short_root)), "{socket}");
    }
    ensure_bwrap_deny_dirs(&mgr, &me).unwrap();
    assert!(!rt.sockets.join("t").exists());
    assert!(rt.run.join("t").is_dir());
    let sockets_mode = std::fs::metadata(&rt.sockets).unwrap().permissions().mode();
    assert_eq!(sockets_mode & 0o777, 0o700);
}
