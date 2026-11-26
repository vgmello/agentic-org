use super::UserTempError;
use crate::Refused;
use std::path::{Path, PathBuf};

fn refusal() -> Refused {
    Refused::unsupported("instance locations")
}

pub(super) fn application_base(_home: &Path) -> Result<PathBuf, Refused> {
    Err(refusal())
}

pub(super) fn account_application_base(_home: &Path) -> Result<PathBuf, Refused> {
    Err(refusal())
}

pub(super) fn application_base_variables() -> Result<&'static [&'static str], Refused> {
    Err(refusal())
}

pub(super) fn location_variables() -> Result<&'static [&'static str], Refused> {
    Err(refusal())
}

pub(super) fn location_overrides(
    _home: &Path,
) -> Result<Vec<(&'static str, std::ffi::OsString)>, Refused> {
    Err(refusal())
}

pub(super) fn user_temp_base(_home: &Path) -> Result<PathBuf, UserTempError> {
    Err(UserTempError::Unsupported(refusal()))
}

pub(super) fn cli_cache_dir(_home: &Path) -> Result<PathBuf, Refused> {
    Err(refusal())
}

pub(super) fn credential_directory_base(_home: &Path) -> Result<PathBuf, Refused> {
    Err(refusal())
}

pub(super) fn environment_spelling(_base: &Path) -> Result<PathBuf, Refused> {
    Err(refusal())
}

pub(super) fn socket_path_limit() -> Result<usize, Refused> {
    Err(refusal())
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn disk_backed_temp() -> Result<PathBuf, Refused> {
    Err(refusal())
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn memory_backed_temp() -> Result<Option<PathBuf>, Refused> {
    Err(refusal())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_location_is_refused() {
        let home = Path::new("/home/x");
        assert_eq!(application_base(home), Err(refusal()));
        assert!(matches!(
            user_temp_base(home),
            Err(UserTempError::Unsupported(refused)) if refused == refusal()
        ));
        assert_eq!(credential_directory_base(home), Err(refusal()));
        assert_eq!(cli_cache_dir(home), Err(refusal()));
        assert_eq!(location_variables(), Err(refusal()));
        assert_eq!(environment_spelling(home), Err(refusal()));
        assert_eq!(socket_path_limit(), Err(refusal()));
        assert_eq!(disk_backed_temp(), Err(refusal()));
        assert_eq!(memory_backed_temp(), Err(refusal()));
    }
}
