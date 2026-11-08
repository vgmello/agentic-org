//! Framing: one JSON object per line.
//!
//! Compact JSON never contains a raw newline (newlines inside strings are escaped), so a
//! line is exactly one message. Both sides cap a line at [`MAX_FRAME_BYTES`].
//!
//! After [`FrameError::Json`] the stream is still in step (the bad line was consumed whole),
//! so a server can answer it and keep reading; the same holds for
//! [`FrameError::TooLargeToSend`], where nothing was written. After any other error, close
//! the connection (see [`FrameError::closes_stream`]).
//!
//! These helpers are for **blocking** readers and writers. On a non-blocking stream, bytes
//! read before a `WouldBlock` would be lost; an async transport must do its own line
//! splitting with the same [`MAX_FRAME_BYTES`] cap.

use std::fmt;
use std::io::{self, BufRead, Read, Write};

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::MAX_FRAME_BYTES;

#[derive(Debug)]
pub enum FrameError {
    Io(io::Error),
    /// An incoming line is longer than [`MAX_FRAME_BYTES`].
    TooLarge,
    /// An outgoing message would be longer than [`MAX_FRAME_BYTES`]. Nothing was written,
    /// so the stream is still in step: answer with an error instead.
    TooLargeToSend,
    /// The stream ended in the middle of a line.
    Truncated,
    /// The line isn't valid JSON for the expected type (reading), or the message couldn't
    /// be serialized (writing).
    Json(serde_json::Error),
}

impl FrameError {
    /// Whether the stream is out of step after this error, so the connection must close.
    pub fn closes_stream(&self) -> bool {
        !matches!(self, FrameError::Json(_) | FrameError::TooLargeToSend)
    }
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameError::Io(e) => write!(f, "i/o error: {e}"),
            FrameError::TooLarge => write!(f, "frame larger than {MAX_FRAME_BYTES} bytes"),
            FrameError::TooLargeToSend => {
                write!(f, "message larger than {MAX_FRAME_BYTES} bytes, not sent")
            }
            FrameError::Truncated => write!(f, "stream ended mid-frame"),
            FrameError::Json(e) => write!(f, "invalid frame: {e}"),
        }
    }
}

impl std::error::Error for FrameError {}

impl From<io::Error> for FrameError {
    fn from(e: io::Error) -> Self {
        FrameError::Io(e)
    }
}

/// Read one raw line, without its `\n`. `Ok(None)` means the stream ended cleanly.
pub fn read_line(r: &mut impl BufRead) -> Result<Option<Vec<u8>>, FrameError> {
    let mut buf = Vec::new();
    let limit = MAX_FRAME_BYTES as u64 + 1; // room for the newline
    r.by_ref().take(limit).read_until(b'\n', &mut buf)?;
    if buf.last() == Some(&b'\n') {
        buf.pop();
        return Ok(Some(buf));
    }
    if buf.len() as u64 >= limit {
        return Err(FrameError::TooLarge);
    }
    if buf.is_empty() {
        Ok(None)
    } else {
        Err(FrameError::Truncated)
    }
}

/// Read and decode one message. `Ok(None)` means the stream ended cleanly.
pub fn read_message<T: DeserializeOwned>(r: &mut impl BufRead) -> Result<Option<T>, FrameError> {
    match read_line(r)? {
        Some(line) => serde_json::from_slice(&line)
            .map(Some)
            .map_err(FrameError::Json),
        None => Ok(None),
    }
}

/// Encode and write one message, then flush.
pub fn write_message<T: Serialize>(w: &mut impl Write, msg: &T) -> Result<(), FrameError> {
    let mut line = serde_json::to_vec(msg).map_err(FrameError::Json)?;
    if line.len() > MAX_FRAME_BYTES {
        return Err(FrameError::TooLargeToSend);
    }
    line.push(b'\n');
    w.write_all(&line)?;
    w.flush()?;
    Ok(())
}
