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
            Path::new("/short/runtime/OFFICE1.sock")
        );
    }
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
