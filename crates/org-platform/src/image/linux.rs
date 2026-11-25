//! The GNU/Linux image adapter. Releases are built for Linux. The kernel has no
//! CodeDirectory hash and no audit token, so both image queries refuse here for good.
//! The Linux rule is the SHA-256 of the executed bytes, and this crate cannot hash
//! (it depends on the C library only): the host applies the rule in
//! `org_platform_host::admission` (`own_image`, and the sealed image of a child).

use super::{refused, OwnExecutable, ReleasePlatform};
use crate::Refused;
use std::fs;
use std::io;

pub(super) fn own_loaded_cdhash() -> io::Result<[u8; 20]> {
    Err(refused::image_identity().into())
}

pub(super) fn open_own_executable() -> io::Result<OwnExecutable> {
    match fs::File::open("/proc/self/exe") {
        Ok(file) => Ok(OwnExecutable::File(file)),
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            let is_memfd = fs::read_link("/proc/self/exe")
                .ok()
                .and_then(|p| p.to_str().map(|s| s.starts_with("/memfd:")))
                .unwrap_or(false);
            // SAFETY: PR_GET_DUMPABLE is a read-only process attribute query with no pointer args.
            let dumpable = unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) };
            if is_memfd && dumpable == 0 {
                Ok(OwnExecutable::SealedMemfd)
            } else {
                Err(error)
            }
        }
        Err(error) => Err(error),
    }
}

pub(super) fn loaded_cdhash_for_audit_token(_token: &[u8; 32]) -> io::Result<[u8; 20]> {
    Err(refused::image_identity().into())
}

pub(super) fn release_platform() -> Result<ReleasePlatform, Refused> {
    Ok(ReleasePlatform::Linux)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_names_its_release_platform_and_identifies_no_image_by_a_code_hash() {
        assert_eq!(release_platform(), Ok(ReleasePlatform::Linux));
        for result in [own_loaded_cdhash(), loaded_cdhash_for_audit_token(&[7; 32])] {
            let error = result.expect_err("Linux has no CodeDirectory hash");
            assert_eq!(error.kind(), io::ErrorKind::Unsupported);
            assert_eq!(
                error
                    .get_ref()
                    .and_then(|source| source.downcast_ref::<Refused>()),
                Some(&refused::image_identity())
            );
        }
    }
}
