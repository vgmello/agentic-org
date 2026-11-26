//! `agentic-org-office --help` and `--version` run by hand: they answer on standard output,
//! exit 0 and open nothing, before the control-pipe handoff that a host launch needs.

use std::process::Command;

fn office() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_agentic-org-office"));
    command.env_clear();
    command
}

#[test]
fn help_and_version_answer_without_a_host_handoff() {
    let sandbox = tempfile::tempdir().unwrap();
    for (args, expected) in [
        (["--help"], "usage: agentic-org-office --create"),
        (["-h"], "usage: agentic-org-office --create"),
        (["--version"], "agentic-org-office "),
        (["-V"], "agentic-org-office "),
    ] {
        let out = office()
            .args(args)
            .env("HOME", sandbox.path())
            .current_dir(sandbox.path())
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{args:?}: {out:?}");
        let stdout = String::from_utf8(out.stdout).unwrap();
        assert!(stdout.contains(expected), "{args:?}: {stdout}");
        assert!(out.stderr.is_empty(), "{args:?}");
    }
    assert_eq!(
        std::fs::read_dir(sandbox.path()).unwrap().count(),
        0,
        "--help or --version created a file"
    );
}

#[test]
fn a_closed_standard_output_exits_0() {
    for flag in ["--help", "--version"] {
        let (reader, writer) = std::io::pipe().unwrap();
        drop(reader);
        let out = office().arg(flag).stdout(writer).output().unwrap();
        assert_eq!(out.status.code(), Some(0), "{flag}: {out:?}");
        assert!(out.stderr.is_empty(), "{flag}");
    }
}

#[test]
fn a_bare_run_without_the_handoff_still_fails_closed() {
    let out = office().arg("--create").output().unwrap();
    assert_ne!(out.status.code(), Some(0), "{out:?}");
    assert!(out.stdout.is_empty());
}
