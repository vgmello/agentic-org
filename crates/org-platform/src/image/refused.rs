//! The refusing image adapter: no image is identified and no release target is named.

use super::{OwnExecutable, ReleasePlatform};
use crate::Refused;
use std::io;

pub(super) fn open_own_executable() -> io::Result<OwnExecutable> {
    Err(image_identity().into())
}

/// The refusal of the loaded-image queries. The Linux adapter returns it too until it
/// has a rule of its own.
pub(super) fn image_identity() -> Refused {
    Refused::unsupported("image identity")
}

/// The refusal of the release target. The contract returns it too for an architecture
/// no release is built for.
pub(super) fn release_target() -> Refused {
    Refused::unsupported("release target")
}

pub(super) fn own_loaded_cdhash() -> io::Result<[u8; 20]> {
    Err(image_identity().into())
}

pub(super) fn loaded_cdhash_for_audit_token(_token: &[u8; 32]) -> io::Result<[u8; 20]> {
    Err(image_identity().into())
}

pub(super) fn release_platform() -> Result<ReleasePlatform, Refused> {
    Err(release_target())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_refused(result: io::Result<[u8; 20]>) {
        let error = result.expect_err("the refusing adapter identified an image");
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        assert_eq!(
            error
                .get_ref()
                .and_then(|source| source.downcast_ref::<Refused>()),
            Some(&image_identity())
        );
    }

    #[test]
    fn no_image_is_identified() {
        assert_refused(own_loaded_cdhash());
        assert_refused(loaded_cdhash_for_audit_token(&[0; 32]));
        assert_refused(loaded_cdhash_for_audit_token(&[7; 32]));
    }

    #[test]
    fn no_release_target_is_named() {
        let Refused::Unsupported { capability, os } = release_platform().unwrap_err();
        assert_eq!(capability, "release target");
        assert_eq!(os, std::env::consts::OS);
    }
}
