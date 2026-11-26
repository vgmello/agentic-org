mod a2a_live_three_runtimes;
mod a2a_messaging;
mod external_auth;
mod org_event_bursts;
mod transfer_composition;

use std::path::Path;

use crate::failure::fail_code;
use crate::office_runtime::socket_path;
use org_office::OfficeError;
use org_office_launch::FailCode;

use super::*;

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn the_office_home_is_the_override_or_the_systems_base_for_an_absolute_home() {
    let home = "/Users/me";
    let base = org_platform::dirs::application_base(Path::new(home)).ok();
    assert!(base.is_some());
    assert_eq!(
        office_home(Some("/tmp/o1".into()), Some(home.into())),
        Some("/tmp/o1".into())
    );
    assert_eq!(office_home(None, Some(home.into())), base);
    for ignored in ["", "tmp/o1"] {
        assert_eq!(office_home(Some(ignored.into()), Some(home.into())), base);
    }
    for no_home in [None, Some(""), Some("me")] {
        assert_eq!(office_home(None, no_home.map(Into::into)), None);
    }
    assert_eq!(office_home(Some("o1".into()), Some("me".into())), None);
}

#[test]
fn explicit_runtime_is_required_in_release_and_invalid_values_never_fall_back() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let home = Path::new("/very/long/data/root");
    assert!(runtime_dir_from(home, None, false).is_err());
    assert_eq!(
        runtime_dir_from(home, None, true).unwrap(),
        home.join("run")
    );
    for debug in [false, true] {
        for invalid in ["", "relative"] {
            assert!(runtime_dir_from(home, Some(invalid.into()), debug).is_err());
        }
        let runtime = runtime_dir_from(home, Some("/short/runtime".into()), debug).unwrap();
        assert_eq!(
            socket_path(&runtime, "OFFICE1").unwrap(),
            Path::new("/short/runtime/o/OFFICE1/s.sock")
        );
    }
}

#[test]
fn the_office_socket_folders_are_made_owner_only_and_never_followed() {
    use std::os::unix::fs::PermissionsExt as _;
    let _spawn = crate::transfer::tests::without_process_spawn();
    let dir = tempfile::tempdir().unwrap();
    let runtime = dir.path().join("run");
    std::fs::create_dir(&runtime).unwrap();
    let socket = socket_path(&runtime, "OFFICE1").unwrap();
    let sockets = runtime.join("o");
    let own = sockets.join("OFFICE1");
    assert_eq!(socket.parent(), Some(own.as_path()));
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o7777;

    // Missing (a standalone or test runtime): both created owner-only, and a bind then succeeds.
    crate::office_runtime::prepare_socket_dir(&socket).unwrap();
    assert_eq!((mode(&sockets), mode(&own)), (0o700, 0o700));
    drop(crate::serve::bind(&socket).unwrap());
    // Present but loose, or with a special bit: tightened to exactly 0700.
    std::fs::set_permissions(&sockets, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(&own, std::fs::Permissions::from_mode(0o1777)).unwrap();
    crate::office_runtime::prepare_socket_dir(&socket).unwrap();
    assert_eq!((mode(&sockets), mode(&own)), (0o700, 0o700));

    // A link where either folder belongs, or a missing runtime folder, is refused.
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir(&elsewhere).unwrap();
    std::fs::set_permissions(&elsewhere, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::remove_dir_all(&own).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &own).unwrap();
    assert!(crate::office_runtime::prepare_socket_dir(&socket).is_err());
    assert_eq!(mode(&elsewhere), 0o755, "the link's target was touched");
    std::fs::remove_dir_all(&sockets).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &sockets).unwrap();
    assert!(crate::office_runtime::prepare_socket_dir(&socket).is_err());
    assert_eq!(mode(&elsewhere), 0o755, "the link's target was touched");
    assert!(
        !elsewhere.join("OFFICE1").exists(),
        "created through the link"
    );
    let orphan = socket_path(&dir.path().join("missing"), "OFFICE1").unwrap();
    assert!(crate::office_runtime::prepare_socket_dir(&orphan).is_err());
    assert!(!dir.path().join("missing").exists());
}

#[test]
fn every_office_error_has_its_code() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    use org_office::NameError;
    let storage = || OfficeError::Storage("disk".into());
    for (error, code) in [
        (
            OfficeError::InvalidName(NameError::Empty),
            FailCode::InvalidName,
        ),
        (OfficeError::OfficeExists, FailCode::AlreadyExists),
        (OfficeError::AlreadyExists, FailCode::AlreadyExists),
        (OfficeError::InUse, FailCode::InUse),
        (OfficeError::Invalid("x"), FailCode::StorageError),
        (OfficeError::NotFound, FailCode::InvalidRequest),
        (storage(), FailCode::StorageError),
    ] {
        assert_eq!(fail_code(&error), code, "{error:?}");
    }
}

#[test]
fn the_three_modes_and_nothing_else() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    assert_eq!(parse_args(&args(&["--create"])), Ok(Mode::Create));
    assert_eq!(
        parse_args(&args(&["--office", "ABC"])),
        Ok(Mode::Serve("ABC".into()))
    );
    assert_eq!(
        parse_args(&args(&["--discard", "ABC"])),
        Ok(Mode::Discard("ABC".into()))
    );
    for bad in [
        args(&[]),
        args(&["--create", "ABC"]),
        args(&["--office"]),
        args(&["--discard"]),
        args(&["--serve", "ABC"]),
        args(&["--office", "ABC", "extra"]),
    ] {
        assert!(parse_args(&bad).is_err(), "{bad:?}");
    }
}
