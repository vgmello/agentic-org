//! The two PEM files of a supplied pair, as setup's configuration names them.

use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use zeroize::Zeroizing;

/// The most a certificate or key file may hold.
pub const FILE_LIMIT: usize = 64 * 1024;

/// The text of the file at `path`: opened without following a symbolic link in its last
/// component, without waiting for a writer (a FIFO) and without taking it as a controlling
/// terminal; it must be a regular file of at most [`FILE_LIMIT`] bytes of UTF-8. The text is
/// zeroed when it is dropped, since a key file's is a secret. An error names the path and the
/// rule, never the contents.
pub fn read_pem_file(path: &Path) -> Result<Zeroizing<String>, String> {
    let shown = path.display();
    let refused = |why: &str| format!("{shown}: {why}");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_NOCTTY | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| match e.raw_os_error() {
            Some(libc::ELOOP) => refused("it is a symbolic link; name the file itself"),
            _ => refused(&e.to_string()),
        })?;
    let metadata = file.metadata().map_err(|e| refused(&e.to_string()))?;
    if !metadata.is_file() {
        return Err(refused("it is not a regular file"));
    }
    let mut bytes = Zeroizing::new(Vec::with_capacity(FILE_LIMIT + 1));
    file.take(FILE_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| refused(&e.to_string()))?;
    if bytes.len() > FILE_LIMIT {
        return Err(refused("it is larger than 64 KiB"));
    }
    match String::from_utf8(std::mem::take(&mut *bytes)) {
        Ok(text) => Ok(Zeroizing::new(text)),
        Err(error) => {
            // Zeroed as it drops.
            drop(Zeroizing::new(error.into_bytes()));
            Err(refused("it is not PEM text"))
        }
    }
}
