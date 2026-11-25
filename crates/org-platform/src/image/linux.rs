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
            // SAFETY: PR_GET_DUMPABLE is a read-only process attribute query with no pointer args.
            let dumpable = unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) };
            if dumpable != 0 {
                return Err(error);
            }
            // Non-dumpable processes (such as an execute-only 0111 sealed memfd child) are
            // refused both open() and readlink() on /proc/self/exe by the Linux kernel.
            // If readlink still succeeds, require it to name a memfd image.
            let memfd_or_unreadable = match fs::read_link("/proc/self/exe") {
                Ok(link) => link.to_str().is_some_and(|s| s.starts_with("/memfd:")),
                Err(link_err) if link_err.kind() == io::ErrorKind::PermissionDenied => true,
                Err(_) => false,
            };
            if memfd_or_unreadable {
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
