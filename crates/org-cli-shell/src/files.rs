//! The one way the command line reads a small file it does not own the writer of (the
//! completion lists, `agentic-org.toml`, an invitation file): [`read_regular`], and
//! [`read_bounded_into`] for any bounded stream (an invitation on standard input).
//!
//! The file is opened without following a symbolic link in its last component and without
//! blocking on a FIFO (`org_platform::fs::open_nofollow_ro`: `O_NOFOLLOW | O_NONBLOCK |
//! O_CLOEXEC`), checked to be a regular file on the opened handle (`fstat`, never a separate
//! `stat` before the open), and read at most `limit + 1` bytes, so a larger file is refused
//! without being read whole. The bytes land in one buffer sized up front and are never moved
//! (no reallocation, no stack probe), so a caller holding a secret can zero every copy.

use std::fmt;
use std::io::{self, Read};
use std::path::Path;

/// Why [`read_regular`] read nothing.
#[derive(Debug)]
pub enum ReadRefusal {
    /// A symbolic link, a directory, a FIFO or a device: not a regular file. (A socket
    /// cannot be opened at all and is an [`ReadRefusal::Io`] error.)
    NotRegular,
    /// More than the limit's bytes.
    TooLarge { limit: u64 },
    /// Missing, not readable, a path with a NUL byte, or a read failed.
    Io(io::Error),
}

impl fmt::Display for ReadRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRegular => f.write_str("not a regular file"),
            Self::TooLarge { limit } => write!(f, "larger than {limit} bytes"),
            Self::Io(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ReadRefusal {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::NotRegular | Self::TooLarge { .. } => None,
        }
    }
}

/// The whole regular file at `path`, at most `limit` bytes (see the module documentation).
pub fn read_regular(path: &Path, limit: u64) -> Result<Vec<u8>, ReadRefusal> {
    let mut bytes = Vec::new();
    read_regular_into(path, limit, &mut bytes)?;
    Ok(bytes)
}

/// [`read_regular`] into `out`. On return, success or refusal, `out` holds exactly the bytes
/// that were read (none when the file was refused before reading), so a caller can zero them.
pub fn read_regular_into(path: &Path, limit: u64, out: &mut Vec<u8>) -> Result<(), ReadRefusal> {
    out.clear();
    // A NUL byte is refused by the open as `InvalidInput`, which the adapter also uses for an
    // opened entry that is not a regular file: tell the two apart before opening.
    if path.as_os_str().as_encoded_bytes().contains(&0) {
        return Err(ReadRefusal::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the path holds a NUL byte",
        )));
    }
    let mut file = org_platform::fs::open_nofollow_ro(path).map_err(|error| {
        // `O_NOFOLLOW` refuses a symbolic link with `ELOOP`; the adapter refuses an opened
        // entry that is not a regular file with `InvalidInput`.
        if error.raw_os_error() == Some(org_platform::fs::ELOOP)
            || error.kind() == io::ErrorKind::InvalidInput
        {
            ReadRefusal::NotRegular
        } else {
            ReadRefusal::Io(error)
        }
    })?;
    if !file.metadata().map_err(ReadRefusal::Io)?.is_file() {
        return Err(ReadRefusal::NotRegular);
    }
    read_bounded_into(&mut file, limit, out)
}

/// Reads at most `limit + 1` bytes of `reader` into `out`, refusing more than `limit` with
/// [`ReadRefusal::TooLarge`]. `out` is sized once, up front, and filled in place: the bytes
/// are never copied elsewhere. On return, success or refusal, `out` holds exactly the bytes
/// read, so a caller holding a secret can zero them.
pub fn read_bounded_into(
    reader: &mut dyn Read,
    limit: u64,
    out: &mut Vec<u8>,
) -> Result<(), ReadRefusal> {
    let room = usize::try_from(limit.saturating_add(1)).map_err(|_| {
        ReadRefusal::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the read limit does not fit in memory",
        ))
    })?;
    // Zeroed by the allocator: pages a small read never touches cost nothing.
    *out = vec![0; room];
    let mut filled = 0;
    let outcome = loop {
        if filled == room {
            break Ok(());
        }
        match reader.read(&mut out[filled..]) {
            Ok(0) => break Ok(()),
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => break Err(ReadRefusal::Io(error)),
        }
    };
    out.truncate(filled);
    outcome?;
    if filled as u64 > limit {
        return Err(ReadRefusal::TooLarge { limit });
    }
    Ok(())
}

#[cfg(test)]
mod tests;
