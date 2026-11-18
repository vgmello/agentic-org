use super::UserTempError;
use crate::Refused;
use std::io;
use std::path::{Path, PathBuf};

pub(super) fn application_base(home: &Path) -> Result<PathBuf, Refused> {
    Ok(home.join("Library/Application Support/agentic.org"))
}

pub(super) fn account_application_base(home: &Path) -> Result<PathBuf, Refused> {
    application_base(home)
}

pub(super) fn application_base_variables() -> Result<&'static [&'static str], Refused> {
    Ok(&[])
}

pub(super) fn user_temp_base(_home: &Path) -> Result<PathBuf, UserTempError> {
    let mut bytes = vec![0_u8; 4096];
    // SAFETY: module-owned writable storage and supported Darwin query.
    let n = unsafe {
        libc::confstr(
            libc::_CS_DARWIN_USER_TEMP_DIR,
            bytes.as_mut_ptr().cast(),
            bytes.len(),
        )
    };
    if n == 0 || n > bytes.len() || bytes[n - 1] != 0 {
        return Err(UserTempError::Unavailable);
    }
    bytes.truncate(n - 1);
    let path = PathBuf::from(String::from_utf8(bytes).map_err(|error| {
        UserTempError::Invalid(io::Error::new(io::ErrorKind::InvalidData, error))
    })?);
    path.canonicalize().map_err(UserTempError::Invalid)
}

pub(super) fn environment_spelling(base: &Path) -> Result<PathBuf, Refused> {
    if let Ok(suffix) = base.strip_prefix("/private/var") {
        // The session environment uses DARWIN_USER_TEMP_DIR's /var spelling.
        // Registry/denies retain the canonical /private/var spelling.
        return Ok(Path::new("/var").join(suffix));
    }
    Ok(base.to_owned())
}

/// `sun_path` is 104 bytes with its NUL.
pub(super) fn socket_path_limit() -> Result<usize, Refused> {
    Ok(103)
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn disk_backed_temp() -> Result<PathBuf, Refused> {
    Ok(PathBuf::from("/private/tmp"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_base_is_application_support() {
        assert_eq!(
            application_base(Path::new("/Users/a")).unwrap(),
            Path::new("/Users/a/Library/Application Support/agentic.org")
        );
    }

    #[test]
    fn darwin_environment_uses_var_without_changing_registry_paths() {
        let canonical = Path::new("/private/var/folders/example/T");
        assert_eq!(
            environment_spelling(canonical).unwrap(),
            Path::new("/var/folders/example/T")
        );
        assert_eq!(canonical, Path::new("/private/var/folders/example/T"));
        assert_eq!(
            environment_spelling(Path::new("/private/various/base")).unwrap(),
            Path::new("/private/various/base")
        );
    }

    #[test]
    fn the_user_temp_base_is_the_systems_resolved_private_directory() {
        let base = user_temp_base(Path::new("/Users/ignored")).unwrap();
        assert!(base.is_absolute(), "{}", base.display());
        assert_eq!(base.canonicalize().unwrap(), base);
        assert!(!base.starts_with("/Users/ignored"), "{}", base.display());
    }

    #[test]
    fn a_socket_path_holds_103_bytes() {
        assert_eq!(socket_path_limit(), Ok(103));
        // SAFETY: an all-zero socket address is a valid value.
        let address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        assert_eq!(address.sun_path.len() - 1, 103);
    }
}
