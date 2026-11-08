//! The zombie-window cleanup of a runtime's stale socket, and what it refuses.

mod common;

use std::fs;
use std::os::unix::fs::{symlink, FileTypeExt, MetadataExt};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::time::Duration;

use common::*;
use org_runtime::CleanupPath;
use org_supervise::{remove_stale, spawn_session, CleanupOutcome, PlanTargets};

fn template(s: &str) -> CleanupPath {
    CleanupPath::try_from(s).unwrap()
}

/// Leaves a socket file at `path` (the listener is closed; the file stays).
fn socket_at(path: &Path) {
    drop(UnixListener::bind(path).unwrap());
    assert!(path.exists());
}

/// A pid no process has: a child's, after it's reaped.
fn dead_pid(tmp: &TempDir) -> u32 {
    let ws = tmp.dir("dead-ws");
    let mark = mark("dead");
    let child = spawn_session(
        &plan("/usr/bin/true", &[], &ws),
        &targets(&tmp.dir("rt")),
        &session_env(tmp, &ws, &mark),
    )
    .unwrap();
    let pid = child.pid();
    child.reap().unwrap();
    assert_clean(&[pid], None);
    pid
}

#[test]
fn reap_removes_the_socket_while_the_child_is_a_zombie() {
    let tmp = TempDir::new();
    let ws = tmp.dir("ws");
    let socks = tmp.dir("cc-socks"); // stands in for /private/tmp/cc-socks
    let mark = mark("sock");
    let mut p = plan("/bin/sh", &["-c", "read x; exit 0"], &ws);
    p.cleanup_after_exit = vec![template(&format!("{}/{{pid}}.sock", socks.display()))];
    let t = PlanTargets {
        cleanup_root: Some(spec(&socks)),
        ..targets(&tmp.dir("rt"))
    };
    let mut child = spawn_session(&p, &t, &session_env(&tmp, &ws, &mark)).unwrap();
    let pid = child.pid();
    let sock = socks.join(format!("{pid}.sock"));
    socket_at(&sock);
    let other = socks.join("999999.sock"); // another process's socket stays
    socket_at(&other);

    child.close_stdin();
    assert!(child.wait_exit(Some(Duration::from_secs(5))).unwrap());
    let exit = child.reap().unwrap();
    assert_eq!(exit.info.code, Some(0));
    assert_eq!(exit.cleanup.len(), 1);
    assert_eq!(exit.cleanup[0].path, sock);
    assert!(
        matches!(exit.cleanup[0].outcome, CleanupOutcome::Removed),
        "{exit:?}"
    );
    assert!(!sock.exists());
    assert!(other.exists());
    assert_clean(&[pid], Some(&mark));
}

#[test]
fn a_plan_with_cleanup_paths_needs_a_root() {
    let tmp = TempDir::new();
    let ws = tmp.dir("ws");
    let mut p = plan("/usr/bin/true", &[], &ws);
    p.cleanup_after_exit = vec![template("/private/tmp/cc-socks/{pid}.sock")];
    let err = spawn_session(
        &p,
        &targets(&tmp.dir("rt")),
        &session_env(&tmp, &ws, &mark("r")),
    )
    .unwrap_err();
    assert!(
        matches!(err, org_supervise::Error::Invalid { .. }),
        "{err:?}"
    );
}

fn refused(report: &org_supervise::CleanupReport) -> &'static str {
    match report.outcome {
        CleanupOutcome::Refused(why) => why,
        ref other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn it_refuses_a_symlinked_component_a_non_socket_and_a_path_outside_the_root() {
    let tmp = TempDir::new();
    let pid = dead_pid(&tmp);
    let socks = tmp.dir("cc-socks");
    let root = spec(&socks);

    // A symlinked directory on the way (planted in a shared folder, say).
    let real = tmp.dir("cc-socks/real");
    symlink(&real, socks.join("link")).unwrap();
    let target = real.join(format!("{pid}.sock"));
    socket_at(&target);
    let r = remove_stale(
        &template(&format!("{}/link/{{pid}}.sock", socks.display())),
        &root,
        pid,
    );
    assert!(refused(&r).contains("symlink"), "{r:?}");
    assert!(target.exists());

    // A symlink at the leaf is not a socket: left alone, and its target too.
    let leaf_link = socks.join(format!("{pid}.lnk"));
    symlink(&target, &leaf_link).unwrap();
    let r = remove_stale(
        &template(&format!("{}/{{pid}}.lnk", socks.display())),
        &root,
        pid,
    );
    assert_eq!(refused(&r), "not a socket");
    assert!(fs::symlink_metadata(&leaf_link).is_ok() && target.exists());

    // A directory.
    fs::create_dir(socks.join(format!("{pid}.dir"))).unwrap();
    let r = remove_stale(
        &template(&format!("{}/{{pid}}.dir", socks.display())),
        &root,
        pid,
    );
    assert_eq!(refused(&r), "not a socket");
    assert!(socks.join(format!("{pid}.dir")).is_dir());

    // A regular file.
    fs::write(socks.join(format!("{pid}.txt")), "x").unwrap();
    let r = remove_stale(
        &template(&format!("{}/{{pid}}.txt", socks.display())),
        &root,
        pid,
    );
    assert_eq!(refused(&r), "not a socket");

    // Outside the allowed root.
    let outside = tmp.dir("elsewhere").join(format!("{pid}.sock"));
    socket_at(&outside);
    let r = remove_stale(
        &template(&format!("{}/elsewhere/{{pid}}.sock", tmp.path().display())),
        &root,
        pid,
    );
    assert_eq!(refused(&r), "outside the allowed root");
    assert!(outside.exists());

    // And a socket of ours in the root is removed; a missing one is absent.
    let ours = socks.join(format!("{pid}.sock"));
    socket_at(&ours);
    let t = template(&format!("{}/{{pid}}.sock", socks.display()));
    assert!(matches!(
        remove_stale(&t, &root, pid).outcome,
        CleanupOutcome::Removed
    ));
    assert!(!ours.exists());
    assert!(matches!(
        remove_stale(&t, &root, pid).outcome,
        CleanupOutcome::Absent
    ));
}

#[test]
fn it_refuses_while_a_process_has_the_pid() {
    let tmp = TempDir::new();
    let socks = tmp.dir("cc-socks");
    let me = std::process::id();
    let sock = socks.join(format!("{me}.sock"));
    socket_at(&sock);
    let r = remove_stale(
        &template(&format!("{}/{{pid}}.sock", socks.display())),
        &spec(&socks),
        me,
    );
    assert_eq!(refused(&r), "a process has that pid");
    assert!(sock.exists());
}

/// A socket owned by another user: a hard link (in our own folder) to a root-owned system
/// socket. Skipped where none can be made. The link is ours to remove afterwards; the
/// original is never touched.
#[test]
fn it_refuses_another_users_socket() {
    let tmp = TempDir::new();
    let pid = dead_pid(&tmp);
    let socks = tmp.dir("cc-socks");
    let link = socks.join(format!("{pid}.sock"));
    let sources = ["/private/var/run/mDNSResponder", "/private/var/run/syslog"];
    let our_uid = fs::metadata(tmp.path()).unwrap().uid();
    let made = sources.iter().any(|s| {
        fs::symlink_metadata(s).is_ok_and(|m| m.file_type().is_socket() && m.uid() != our_uid)
            && fs::hard_link(s, &link).is_ok()
    });
    if !made {
        eprintln!("skipped: no other user's socket could be linked here");
        return;
    }
    let r = remove_stale(
        &template(&format!("{}/{{pid}}.sock", socks.display())),
        &spec(&socks),
        pid,
    );
    assert_eq!(refused(&r), "owned by another user");
    assert!(fs::symlink_metadata(&link).is_ok());
    fs::remove_file(&link).unwrap();
}
