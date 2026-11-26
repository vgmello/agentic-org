use super::*;

/// A host environment holding every allowed name plus others (the development key folder,
/// a canary, a lookalike): only the allowed names reach the server, with their values, then
/// the office home and runtime folder.
#[test]
fn a_server_gets_only_the_allowed_environment() {
    let host_var = |name: &str| Some(OsString::from(format!("value-of-{name}")));
    let asked = std::cell::RefCell::new(Vec::new());
    let env = server_env(Path::new("/h"), Some(Path::new("/r")), None, |name| {
        asked.borrow_mut().push(name.to_owned());
        host_var(name)
    });
    // Only the allowlist is ever read from the host: nothing else can be passed on.
    assert_eq!(*asked.borrow(), PASSED_ENV);
    let names: Vec<&OsStr> = env.iter().map(|(n, _)| n.as_os_str()).collect();
    // Spelled out, not read from `PASSED_ENV`: widening the allowlist must change this test.
    let want = [
        "HOME",
        "USER",
        "LOGNAME",
        "PATH",
        "TMPDIR",
        "LANG",
        FEDERATION_BIND_ENV,
        FEDERATION_ADVERTISE_ENV,
        OFFICE_HOME_ENV,
        OFFICE_RUNTIME_DIR_ENV,
    ]
    .map(OsStr::new);
    assert_eq!(names, want);
    for (name, value) in &env[..PASSED_ENV.len()] {
        assert_eq!(value, &host_var(name.to_str().unwrap()).unwrap());
    }
    assert_eq!(env[PASSED_ENV.len()].1, "/h");
    assert_eq!(env[PASSED_ENV.len() + 1].1, "/r");
    for excluded in [
        "ORG_OFFICE_DEV_KEY_DIR",
        "ORG_HOST_TEST_CANARY",
        "HOMEX",
        "DYLD_INSERT_LIBRARIES",
    ] {
        assert!(
            !names.contains(&OsStr::new(excluded)),
            "{excluded} passed on"
        );
    }
}

/// Persisted setup federation config overrides ambient host federation variables cleanly
/// without duplicating keys or leaking stale advertised hosts.
#[test]
fn persisted_federation_config_overrides_ambient_environment() {
    let fed = crate::instance::setup::FederationConfig::parse(
        "0.0.0.0:7443",
        vec!["hq.example.com".into(), "10.0.0.5".into()],
    )
    .unwrap();
    let env = server_env(Path::new("/h"), None, Some(&fed), |name| match name {
        "PATH" => Some(OsString::from("/bin")),
        FEDERATION_BIND_ENV => Some(OsString::from("127.0.0.1:9999")),
        FEDERATION_ADVERTISE_ENV => Some(OsString::from("stale.invalid")),
        _ => None,
    });
    assert_eq!(
        env,
        vec![
            (OsString::from("PATH"), OsString::from("/bin")),
            (
                OsString::from(FEDERATION_BIND_ENV),
                OsString::from("0.0.0.0:7443")
            ),
            (
                OsString::from(FEDERATION_ADVERTISE_ENV),
                OsString::from("hq.example.com,10.0.0.5")
            ),
            (OsString::from(OFFICE_HOME_ENV), OsString::from("/h")),
        ]
    );
    // Without advertise entries, stale ambient AGENTIC_ORG_FEDERATION_ADVERTISE is omitted.
    let bind_only =
        crate::instance::setup::FederationConfig::parse("127.0.0.1:7443", vec![]).unwrap();
    let env_bind_only = server_env(Path::new("/h"), None, Some(&bind_only), |name| match name {
        FEDERATION_ADVERTISE_ENV => Some(OsString::from("stale.invalid")),
        _ => None,
    });
    assert_eq!(
        env_bind_only,
        vec![
            (
                OsString::from(FEDERATION_BIND_ENV),
                OsString::from("127.0.0.1:7443")
            ),
            (OsString::from(OFFICE_HOME_ENV), OsString::from("/h")),
        ]
    );
}

/// A name the host doesn't have is left out, and no runtime folder means no variable for it.
#[test]
fn missing_host_names_and_no_runtime_folder_are_left_out() {
    let env = server_env(Path::new("/h"), None, None, |name| {
        (name == "PATH").then(|| OsString::from("/bin"))
    });
    let names: Vec<&OsStr> = env.iter().map(|(n, _)| n.as_os_str()).collect();
    assert_eq!(names, [OsStr::new("PATH"), OsStr::new(OFFICE_HOME_ENV)]);
}
