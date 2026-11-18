use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;

use super::*;

fn unit() -> Unit {
    Unit {
        instance_id: "0195f3a0-7c1e-7b2a-9d4e-2f6a8c0b1d3e".into(),
        program: PathBuf::from("/opt/agentic-org/bin/agentic-org"),
        args: vec!["serve".into(), "--instance".into(), "default".into()],
        env: vec![("ORG_LOG".into(), "info".into())],
    }
}

fn reason(unit: &Unit) -> &'static str {
    match unit.check() {
        Err(ServiceError::InvalidUnit { reason }) => reason,
        other => panic!("expected an invalid unit, got {other:?}"),
    }
}

#[test]
fn every_mode_refuses_until_an_adapter_exists() {
    for mode in [ServiceMode::Registered, ServiceMode::Foreground] {
        let refused = service(mode).err().expect("no adapter is written yet");
        assert_eq!(refused, Refused::unsupported("service"));
    }
}

#[test]
fn a_well_formed_unit_passes() {
    assert_eq!(unit().check(), Ok(()));
}

#[test]
fn instance_ids_are_short_and_have_no_separator() {
    for good in ["a", "default", "A-b_9", &"x".repeat(MAX_INSTANCE_ID_BYTES)] {
        assert_eq!(check_instance_id(good), Ok(()), "{good}");
    }
    for bad in [
        "",
        "a/b",
        "..",
        "a.b",
        "a b",
        "a\n",
        "é",
        &"x".repeat(MAX_INSTANCE_ID_BYTES + 1),
    ] {
        assert!(
            matches!(
                check_instance_id(bad),
                Err(ServiceError::InvalidUnit { .. })
            ),
            "{bad:?}"
        );
    }
    let mut bad = unit();
    bad.instance_id = "../other".into();
    assert!(reason(&bad).starts_with("the instance id"));
}

#[test]
fn the_program_path_is_absolute() {
    let mut bad = unit();
    bad.program = PathBuf::from("bin/agentic-org");
    assert_eq!(reason(&bad), "the program path is not absolute");
}

#[test]
fn nothing_carries_a_nul_or_a_line_break() {
    let mut bad = unit();
    bad.program = PathBuf::from("/opt/agentic\norg");
    assert_eq!(reason(&bad), "the program path has a NUL or a line break");

    let mut bad = unit();
    bad.args.push(OsString::from_vec(b"a\0b".to_vec()));
    assert_eq!(reason(&bad), "an argument has a NUL or a line break");

    let mut bad = unit();
    bad.env = vec![("A\rB".into(), "v".into())];
    assert_eq!(
        reason(&bad),
        "an environment name has a NUL or a line break"
    );

    let mut bad = unit();
    bad.env = vec![("A".into(), "v\nExecStartPre=/bin/sh".into())];
    assert_eq!(
        reason(&bad),
        "an environment value has a NUL or a line break"
    );
}

#[test]
fn environment_names_are_well_formed() {
    for name in ["", "A=B"] {
        let mut bad = unit();
        bad.env = vec![(name.into(), "v".into())];
        assert_eq!(reason(&bad), "an environment name is empty or contains '='");
    }
}

#[test]
fn a_unit_never_sets_a_loader_variable() {
    for name in [
        "LD_PRELOAD",
        "LD_LIBRARY_PATH",
        "LD_AUDIT",
        "DYLD_INSERT_LIBRARIES",
        "DYLD_LIBRARY_PATH",
        "GLIBC_TUNABLES",
    ] {
        let mut bad = unit();
        bad.env = vec![(name.into(), "x".into())];
        assert_eq!(
            reason(&bad),
            "the environment sets a loader variable",
            "{name}"
        );
    }
    for name in ["LDFLAGS", "MY_LD_PRELOAD", "GLIBC_TUNABLES_X", "ld_preload"] {
        let mut good = unit();
        good.env = vec![(name.into(), "x".into())];
        assert_eq!(good.check(), Ok(()), "{name}");
    }
}

#[test]
fn errors_say_what_failed() {
    assert_eq!(
        ServiceError::NotRegistered.to_string(),
        "the instance has no registered service"
    );
    assert_eq!(
        ServiceError::Manager {
            action: "daemon-reload",
            detail: "exit status 1".into()
        }
        .to_string(),
        "the service manager failed at daemon-reload: exit status 1"
    );
}
