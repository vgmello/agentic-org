//! The macOS keychain through `security find-generic-password`.
//!
//! `-w` prints only the item's password, followed by a newline. The tool inherits the
//! caller's environment, as the lookup needs the user's own keychain search list. A
//! missing item, or one in a locked keychain, makes it exit unsuccessfully: `Ok(None)`.

use super::{bounded::run_bounded_capture, SecretBytes};
use crate::Refused;
use std::{ffi::OsStr, path::Path, time::Duration};

const SECURITY: &str = "/usr/bin/security";

pub(super) fn find_generic_password(
    service: &str,
    max_bytes: usize,
    timeout: Duration,
) -> Result<Option<SecretBytes>, Refused> {
    let program = Path::new(SECURITY);
    if !program.is_file() {
        return Ok(None);
    }
    let args = [
        OsStr::new("find-generic-password"),
        OsStr::new("-s"),
        OsStr::new(service),
        OsStr::new("-w"),
    ];
    Ok(run_bounded_capture(program, &args, max_bytes, timeout))
}
