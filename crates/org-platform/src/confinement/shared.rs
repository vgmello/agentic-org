//! Code the macOS and GNU/Linux adapters share. Not compiled for a refusing target.

use std::io;

use super::refused;

pub(super) fn fill_entropy(bytes: &mut [u8]) -> io::Result<()> {
    for chunk in bytes.chunks_mut(256) {
        loop {
            // SAFETY: chunk is exclusive, initialized, at most getentropy's 256-byte limit.
            let status = unsafe { libc::getentropy(chunk.as_mut_ptr().cast(), chunk.len()) };
            if status == 0 {
                break;
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(refused());
            }
        }
    }
    Ok(())
}
