use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;

use super::testing::{check_registered, MemoryManager};
use super::*;

const INSTANCE: &str = "01JD3V8Q4K7M2N9P5R6S0T1W2X";

fn instance() -> InstanceId {
    InstanceId::new(INSTANCE).expect("a ULID is a valid instance id")
}

fn program() -> PathBuf {
    PathBuf::from("/opt/agentic-org/bin/agentic-org")
}

fn args() -> Vec<String> {
    vec!["serve".into(), "--instance".into(), INSTANCE.into()]
}

fn unit() -> Unit {
    Unit::new(
        instance(),
        program(),
        args(),
        vec![("ORG_LOG".into(), "info".into())],
    )
    .expect("a well-formed unit")
}

fn reason(result: Result<Unit, ServiceError>) -> &'static str {
    match result {
        Err(ServiceError::InvalidUnit { reason }) => reason,
        other => panic!("expected an invalid unit, got {other:?}"),
    }
}

fn with_env(env: &[(&str, &str)]) -> Result<Unit, ServiceError> {
    let env = env
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect();
    Unit::new(instance(), program(), args(), env)
}

supported! {
    #[test]
    fn foreground_mode_keeps_its_rules_and_registered_mode_refuses_until_written() {
        let manager = service(ServiceMode::Foreground).expect("foreground mode on this system");
        super::testing::check_foreground(manager.as_ref(), &unit());

        let refused = service(ServiceMode::Registered)
            .err()
            .expect("no registered-mode adapter is written yet");
        assert_eq!(refused, Refused::unsupported("registered service"));
    }
}

#[test]
fn the_in_memory_manager_keeps_the_registered_mode_rules() {
    let different = Unit::new(instance(), program(), vec!["serve".into()], Vec::new()).unwrap();
    for availability in [
        Availability::Always,
        Availability::WhileLoggedIn {
            remedy: Some("loginctl enable-linger me".into()),
        },
    ] {
        let manager = MemoryManager::new(availability.clone());
        check_registered(&manager, &unit(), &different);

        manager.register(&unit()).unwrap();
        assert_eq!(manager.registered(&instance()), Some(unit()));
        assert_eq!(
            manager.status(&instance()),
            Ok(ServiceStatus::Registered {
                running: false,
                availability
            })
        );
    }
}

#[test]
fn a_unit_exposes_what_it_was_built_from() {
    let unit = unit();
    assert_eq!(unit.instance().as_str(), INSTANCE);
    assert_eq!(unit.instance().to_string(), INSTANCE);
    assert_eq!(unit.program(), program());
    assert_eq!(unit.args(), args());
    assert_eq!(unit.env(), [("ORG_LOG".to_owned(), "info".to_owned())]);
}

#[test]
fn instance_ids_are_short_and_have_no_separator() {
    for good in [
        "a",
        "default",
        "A-b_9",
        "0195f3a0-7c1e-7b2a-9d4e-2f6a8c0b1d3e",
        &"x".repeat(MAX_INSTANCE_ID_BYTES),
    ] {
        assert_eq!(InstanceId::new(good).unwrap().as_str(), good);
    }
    for bad in [
        "",
        "a/b",
        "..",
        "a.b",
        "a b",
        "a\n",
        "a\0",
        "é",
        &"x".repeat(MAX_INSTANCE_ID_BYTES + 1),
    ] {
        assert_eq!(
            InstanceId::new(bad),
            Err(ServiceError::InvalidUnit {
                reason: "the instance id is not 1 to 64 ASCII letters, digits, '-' or '_'"
            }),
            "{bad:?}"
        );
    }
}

#[test]
fn the_program_path_is_absolute_utf8() {
    let relative = Unit::new(instance(), "bin/agentic-org".into(), args(), Vec::new());
    assert_eq!(reason(relative), "the program path is not absolute");

    let not_utf8 = PathBuf::from(OsString::from_vec(b"/opt/\xff/agentic-org".to_vec()));
    let not_utf8 = Unit::new(instance(), not_utf8, args(), Vec::new());
    assert_eq!(reason(not_utf8), "the program path is not UTF-8");
}

#[test]
fn nothing_carries_a_control_character() {
    for control in ['\0', '\n', '\r', '\t', '\u{1b}', '\u{7f}', '\u{85}'] {
        let path = PathBuf::from(format!("/opt/agentic{control}org"));
        let bad = Unit::new(instance(), path, args(), Vec::new());
        assert_eq!(
            reason(bad),
            "the program path has a control character",
            "{control:?}"
        );

        let bad = Unit::new(
            instance(),
            program(),
            vec![format!("a{control}b")],
            Vec::new(),
        );
        assert_eq!(
            reason(bad),
            "an argument has a control character",
            "{control:?}"
        );

        let value = format!("v{control}ExecStartPre=/bin/sh");
        assert_eq!(
            reason(with_env(&[("A", &value)])),
            "an environment value has a control character",
            "{control:?}"
        );
    }
}

#[test]
fn environment_names_are_identifiers_and_appear_once() {
    for name in ["", "A=B", "MY-VAR", "1X", "A B", "A\n", "É"] {
        assert_eq!(
            reason(with_env(&[(name, "v")])),
            "an environment name is not a letter or '_' followed by letters, digits or '_'",
            "{name:?}"
        );
    }
    for name in ["A", "_", "_a1", "ORG_LOG", "lower_case"] {
        assert!(with_env(&[(name, "")]).is_ok(), "{name:?}");
    }
    assert_eq!(
        reason(with_env(&[("A", "1"), ("B", "2"), ("A", "3")])),
        "an environment name appears twice"
    );
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
        assert_eq!(
            reason(with_env(&[(name, "x")])),
            "the environment sets a loader variable",
            "{name}"
        );
    }
    for name in ["LDFLAGS", "MY_LD_PRELOAD", "GLIBC_TUNABLES_X", "ld_preload"] {
        assert!(with_env(&[(name, "x")]).is_ok(), "{name}");
    }
}

#[test]
fn every_error_says_what_failed() {
    let texts = [
        (
            ServiceError::InvalidUnit { reason: "why" },
            "invalid service unit: why",
        ),
        (
            ServiceError::NotRegistered,
            "the instance has no registered service",
        ),
        (
            ServiceError::Conflict,
            "the instance is already registered with a different service unit",
        ),
        (
            ServiceError::Foreign,
            "something else is registered under this instance's service name; it was left alone",
        ),
        (
            ServiceError::Unmanaged,
            "the instance runs in the foreground; whoever started it starts and stops it",
        ),
        (
            ServiceError::Manager {
                action: "daemon-reload",
                detail: "exit status 1".into(),
            },
            "the service manager failed at daemon-reload: exit status 1",
        ),
    ];
    for (error, text) in texts {
        assert_eq!(error.to_string(), text);
    }
}
