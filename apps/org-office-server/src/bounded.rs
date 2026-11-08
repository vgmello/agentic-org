//! Writes to a client that has a deadline for the WHOLE frame, not for each `write` call: a
//! client that reads a trickle (always some progress, never done) must not hold a thread of the
//! office for as long as it likes.

use std::io::{self, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use org_proto::frame::{self, FrameError};
use serde::Serialize;

/// The most one `write` call is given. A socket's write timeout restarts every time the kernel
/// waits for room, so a single large write to a client that reads a trickle would never time out;
/// small calls let the deadline be checked between them.
const CHUNK: usize = 16 * 1024;

/// A writer that fails with `TimedOut` once `deadline` has passed, however the bytes trickle.
struct Bounded<'a> {
    stream: &'a UnixStream,
    deadline: Instant,
}

impl Write for Bounded<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let left = self.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        self.stream.set_write_timeout(Some(left))?;
        (&*self.stream).write(&buf[..buf.len().min(CHUNK)])
    }

    fn flush(&mut self) -> io::Result<()> {
        (&*self.stream).flush()
    }
}

/// Writes one frame, giving up after `limit` in all. A failure other than `TooLargeToSend`
/// means the client is gone or too slow: the caller closes the connection.
pub fn write_message<T: Serialize>(
    stream: &UnixStream,
    message: &T,
    limit: Duration,
) -> Result<(), FrameError> {
    frame::write_message(
        &mut Bounded {
            stream,
            deadline: Instant::now() + limit,
        },
        message,
    )
}
