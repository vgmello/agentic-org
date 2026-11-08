#![cfg(target_os = "macos")]
//! A plan's directories and files are written without ever following a symlink.

mod common;

use std::fs;
use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
use std::path::Path;

use common::*;
use org_runtime::{FileLocation, PlannedDir, PlannedFile, RelPath};
use org_supervise::{spawn_session, Error, PlanTargets};

fn rel(s: &str) -> RelPath {
    RelPath::try_from(s).unwrap()
}

fn file(location: FileLocation, path: &str, contents: &str, git_exclude: bool) -> PlannedFile {
    PlannedFile {
        location,
        rel_path: rel(path),
        contents: contents.as_bytes().to_vec(),
        git_exclude,
    }
}

fn run(
    tmp: &TempDir,
    ws: &Path,
    targets: &PlanTargets,
    f: impl FnOnce(&mut org_runtime::LaunchPlan),
) -> Result<(), Error> {
    let mark = mark("files");
    let mut p = plan("/usr/bin/true", &[], ws);
    f(&mut p);
    let child = spawn_session(&p, targets, &session_env(tmp, ws, &mark))?;
    let pid = child.pid();
    let exit = child.reap().unwrap();
    assert_eq!(exit.info.code, Some(0));
    assert_clean(&[pid], Some(&mark));
    Ok(())
}

fn mode(p: &Path) -> u32 {
    fs::symlink_metadata(p).unwrap().permissions().mode() & 0o7777
}

#[test]
fn dirs_and_files_are_written_with_their_modes_and_excludes() {
    let tmp = TempDir::new();
    let ws = tmp.dir("ws");
    let rt = tmp.dir("rt");
    let info = tmp.dir("work.git/info");
    let exclude = info.join("exclude");
    fs::write(&exclude, "# git's own\n*.log").unwrap();
    let targets = PlanTargets {
        runtime_dir: spec(&rt),
        git_exclude_file: Some(spec(&exclude)),
    };
    let setup = |p: &mut org_runtime::LaunchPlan| {
        p.dirs = vec![PlannedDir {
            location: FileLocation::RuntimeDir,
            rel_path: rel("claude/agents"),
        }];
        p.files = vec![
            file(
                FileLocation::RuntimeDir,
                "claude/agents/ada.md",
                "agent",
                false,
            ),
            file(FileLocation::Workspace, "CLAUDE.md", "instructions", true),
        ];
    };
    run(&tmp, &ws, &targets, setup).unwrap();
    assert_eq!(mode(&rt.join("claude")), 0o700);
    assert_eq!(mode(&rt.join("claude/agents")), 0o700);
    assert_eq!(
        fs::read_to_string(rt.join("claude/agents/ada.md")).unwrap(),
        "agent"
    );
    assert_eq!(mode(&rt.join("claude/agents/ada.md")), 0o600);
    assert_eq!(
        fs::read_to_string(ws.join("CLAUDE.md")).unwrap(),
        "instructions"
    );
    assert_eq!(mode(&ws.join("CLAUDE.md")), 0o600);
    assert_eq!(
        fs::read_to_string(&exclude).unwrap(),
        "# git's own\n*.log\n/CLAUDE.md\n"
    );

    // Rewritten on every start (an agent's edit never survives); the exclude isn't repeated;
    // no temporary file is left.
    fs::write(ws.join("CLAUDE.md"), "edited by the agent").unwrap();
    run(&tmp, &ws, &targets, setup).unwrap();
    assert_eq!(
        fs::read_to_string(ws.join("CLAUDE.md")).unwrap(),
        "instructions"
    );
    assert_eq!(
        fs::read_to_string(&exclude).unwrap(),
        "# git's own\n*.log\n/CLAUDE.md\n"
    );
    for dir in [&ws, &rt.join("claude/agents"), &info] {
        for e in fs::read_dir(dir).unwrap() {
            let name = e.unwrap().file_name();
            assert!(!name.to_string_lossy().ends_with(".tmp"), "{name:?}");
        }
    }
}

#[test]
fn a_symlinked_component_in_the_workspace_fails_the_launch() {
    let tmp = TempDir::new();
    let ws = tmp.dir("ws");
    let out = tmp.dir("out");
    symlink(&out, ws.join("sub")).unwrap();
    let t = targets(&tmp.dir("rt"));

    let err = run(&tmp, &ws, &t, |p| {
        p.files = vec![file(FileLocation::Workspace, "sub/f", "x", false)];
    })
    .unwrap_err();
    assert!(matches!(err, Error::Refused { .. }), "{err:?}");
    let err = run(&tmp, &ws, &t, |p| {
        p.dirs = vec![PlannedDir {
            location: FileLocation::Workspace,
            rel_path: rel("sub/d"),
        }];
    })
    .unwrap_err();
    assert!(matches!(err, Error::Refused { .. }), "{err:?}");
    assert_eq!(
        fs::read_dir(&out).unwrap().count(),
        0,
        "nothing written through the link"
    );
}

#[test]
fn a_symlink_or_hard_link_at_the_target_is_replaced_not_written_through() {
    let tmp = TempDir::new();
    let ws = tmp.dir("ws");
    let out = tmp.dir("out");
    fs::write(out.join("victim"), "orig").unwrap();
    fs::write(out.join("victim2"), "orig2").unwrap();
    symlink(out.join("victim"), ws.join("settings.json")).unwrap();
    fs::hard_link(out.join("victim2"), ws.join("hard.json")).unwrap();
    run(&tmp, &ws, &targets(&tmp.dir("rt")), |p| {
        p.files = vec![
            file(FileLocation::Workspace, "settings.json", "new", false),
            file(FileLocation::Workspace, "hard.json", "new2", false),
        ];
    })
    .unwrap();
    let meta = fs::symlink_metadata(ws.join("settings.json")).unwrap();
    assert!(meta.file_type().is_file(), "the link itself was replaced");
    assert_eq!(fs::read_to_string(ws.join("settings.json")).unwrap(), "new");
    assert_eq!(fs::read_to_string(out.join("victim")).unwrap(), "orig");
    assert_eq!(fs::read_to_string(ws.join("hard.json")).unwrap(), "new2");
    assert_eq!(fs::read_to_string(out.join("victim2")).unwrap(), "orig2");
    assert_eq!(fs::metadata(out.join("victim2")).unwrap().nlink(), 1);
}

#[test]
fn a_symlinked_location_or_program_is_refused() {
    let tmp = TempDir::new();
    let ws = tmp.dir("ws");
    let real_rt = tmp.dir("real-rt");
    let link_rt = tmp.path().join("rt-link");
    symlink(&real_rt, &link_rt).unwrap();
    let err = run(&tmp, &ws, &targets(&link_rt), |p| {
        p.files = vec![file(FileLocation::RuntimeDir, "f", "x", false)];
    })
    .unwrap_err();
    assert!(matches!(err, Error::Refused { .. }), "{err:?}");
    assert_eq!(fs::read_dir(&real_rt).unwrap().count(), 0);

    let sh = tmp.path().join("sh-link");
    symlink("/bin/sh", &sh).unwrap();
    let err = run(&tmp, &ws, &targets(&real_rt), |p| {
        p.program = sh.clone();
    })
    .unwrap_err();
    assert!(matches!(err, Error::Refused { .. }), "{err:?}");
    let err = run(&tmp, &ws, &targets(&real_rt), |p| {
        p.program = "sh".into();
    })
    .unwrap_err();
    assert!(matches!(err, Error::Invalid { .. }), "{err:?}");
}

#[test]
fn a_git_exclude_needs_a_workspace_file_and_an_exclude_file() {
    let tmp = TempDir::new();
    let ws = tmp.dir("ws");
    let rt = tmp.dir("rt");
    let err = run(&tmp, &ws, &targets(&rt), |p| {
        p.files = vec![file(FileLocation::Workspace, "A.md", "x", true)];
    })
    .unwrap_err();
    assert!(matches!(err, Error::Invalid { .. }), "{err:?}");
    let err = run(&tmp, &ws, &targets(&rt), |p| {
        p.files = vec![file(FileLocation::RuntimeDir, "A.md", "x", true)];
    })
    .unwrap_err();
    assert!(matches!(err, Error::Invalid { .. }), "{err:?}");
}

#[test]
fn the_child_starts_in_its_workspace_and_a_symlinked_workspace_is_refused() {
    let tmp = TempDir::new();
    let ws = tmp.dir("ws");
    let mark = mark("cwd");
    let mut child = spawn_session(
        &plan("/bin/pwd", &["-P"], &ws),
        &targets(&tmp.dir("rt")),
        &session_env(&tmp, &ws, &mark),
    )
    .unwrap();
    let pid = child.pid();
    let out = Reader::spawn(child.take_stdout().unwrap()).read_to_end();
    child.reap().unwrap();
    assert_eq!(out.trim(), ws.to_str().unwrap());
    assert_clean(&[pid], Some(&mark));

    // The workspace itself, or a folder above it, swapped for a symlink.
    let link_ws = tmp.path().join("ws-link");
    symlink(&ws, &link_ws).unwrap();
    let parent_link = tmp.path().join("parent-link");
    symlink(tmp.path(), &parent_link).unwrap();
    for cwd in [link_ws, parent_link.join("ws")] {
        let err = spawn_session(
            &plan("/usr/bin/true", &[], &cwd),
            &targets(&tmp.dir("rt")),
            &session_env(&tmp, &cwd, &mark),
        )
        .unwrap_err();
        assert!(matches!(err, Error::Refused { .. }), "{err:?}");
    }
}

#[test]
fn the_exclude_file_keeps_its_mode_and_may_hold_any_bytes() {
    let tmp = TempDir::new();
    let ws = tmp.dir("ws");
    let rt = tmp.dir("rt");
    let info = tmp.dir("work.git/info");
    let exclude = info.join("exclude");
    fs::write(&exclude, b"caf\xe9\n").unwrap();
    fs::set_permissions(&exclude, fs::Permissions::from_mode(0o640)).unwrap();
    let targets = PlanTargets {
        runtime_dir: spec(&rt),
        git_exclude_file: Some(spec(&exclude)),
    };
    run(&tmp, &ws, &targets, |p| {
        p.files = vec![file(FileLocation::Workspace, "A.md", "x", true)];
    })
    .unwrap();
    assert_eq!(fs::read(&exclude).unwrap(), b"caf\xe9\n/A.md\n");
    assert_eq!(mode(&exclude), 0o640);
}
