//! Unix-socket I/O primitives. No protocol, storage or domain knowledge.
//! Buffered reads check liveness periodically without imposing an idle timeout.

#![deny(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]

use std::io::{self, BufRead, BufReader, Read};
use std::os::fd::{AsRawFd, RawFd};
use std::time::Duration;

/// Maximum interval between liveness checks on a reader waiting for bytes or EOF.
pub const READ_TICK: Duration = Duration::from_secs(5);

/// A buffered reader that checks liveness between bounded readiness waits.
///
/// `stop` returns true when this connection must end. A quiet, live connection stays open.
/// Unlike read timeouts, a tick never escapes into a framing parser or discards a partial line.
pub struct Ticked<R, Stop, Poll = fn(RawFd, Duration) -> io::Result<bool>> {
    inner: BufReader<Waiting<R, Stop, Poll>>,
}

struct Waiting<R, Stop, Poll> {
    stream: R,
    stop: Stop,
    poll: Poll,
    tick: Duration,
}

impl<R: Read + AsRawFd, Stop: FnMut() -> bool> Ticked<R, Stop> {
    /// Use the production five-second tick and platform readiness poll.
    pub fn new(stream: R, stop: Stop) -> Self {
        Self::with_poll(stream, READ_TICK, stop, poll_readable)
    }
}

impl<
        R: Read + AsRawFd,
        Stop: FnMut() -> bool,
        Poll: FnMut(RawFd, Duration) -> io::Result<bool>,
    > Ticked<R, Stop, Poll>
{
    /// Inject timing and polling for deterministic tests or an embedding transport.
    ///
    /// The poll must return true only for pending bytes, EOF or a socket error. The stream
    /// must have exactly one reader, so another reader cannot consume readiness before recv.
    /// A zero tick is invalid: it would turn an idle connection into a busy loop.
    pub fn with_poll(stream: R, tick: Duration, stop: Stop, poll: Poll) -> Self {
        assert!(!tick.is_zero(), "a read tick must be nonzero");
        Self {
            inner: BufReader::new(Waiting {
                stream,
                stop,
                poll,
                tick,
            }),
        }
    }

    /// Already-buffered bytes, without reading or polling the socket.
    pub fn buffer(&self) -> &[u8] {
        self.inner.buffer()
    }
}

impl<
        R: Read + AsRawFd,
        Stop: FnMut() -> bool,
        Poll: FnMut(RawFd, Duration) -> io::Result<bool>,
    > Read for Waiting<R, Stop, Poll>
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            match (self.poll)(self.stream.as_raw_fd(), self.tick) {
                Ok(true) => match self.stream.read(buf) {
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    result => return result,
                },
                Ok(false) => {}
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
            // A tick is internal to this read: the framing parser keeps its partial line.
            if (self.stop)() {
                return Ok(0);
            }
        }
    }
}

impl<
        R: Read + AsRawFd,
        Stop: FnMut() -> bool,
        Poll: FnMut(RawFd, Duration) -> io::Result<bool>,
    > Read for Ticked<R, Stop, Poll>
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl<
        R: Read + AsRawFd,
        Stop: FnMut() -> bool,
        Poll: FnMut(RawFd, Duration) -> io::Result<bool>,
    > BufRead for Ticked<R, Stop, Poll>
{
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.inner.fill_buf()
    }

    fn consume(&mut self, amount: usize) {
        self.inner.consume(amount);
    }
}

/// Wait for bytes, EOF or a socket error on a descriptor owned by the caller.
/// Invalid descriptors fail; interruption is returned for the caller to handle.
pub fn poll_readable(fd: RawFd, tick: Duration) -> io::Result<bool> {
    let mut slot = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let millis = tick.as_millis().clamp(1, libc::c_int::MAX as u128) as libc::c_int;
    // SAFETY: slot is one initialized pollfd, valid for the duration of this call.
    let result = unsafe { libc::poll(&mut slot, 1, millis) };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }
    if slot.revents & libc::POLLNVAL != 0 {
        return Err(io::Error::from_raw_os_error(libc::EBADF));
    }
    Ok(slot.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::io::Write;
    use std::os::unix::net::UnixStream;

    #[test]
    fn pending_eof_is_repolled_after_one_missed_wakeup() {
        let (stream, peer) = UnixStream::pair().unwrap();
        drop(peer);
        let polls = Cell::new(0);
        let ticks = Cell::new(0);
        let mut reader = Ticked::with_poll(
            stream,
            READ_TICK,
            || {
                ticks.set(ticks.get() + 1);
                false
            },
            |fd, tick| {
                assert_eq!(tick, READ_TICK);
                polls.set(polls.get() + 1);
                if polls.get() == 1 {
                    Ok(false)
                } else {
                    poll_readable(fd, Duration::from_millis(1))
                }
            },
        );
        assert_eq!(reader.read(&mut [0; 1]).unwrap(), 0);
        assert_eq!(polls.get(), 2);
        assert_eq!(ticks.get(), 1);
    }

    struct NoReadyRead;
    impl AsRawFd for NoReadyRead {
        fn as_raw_fd(&self) -> RawFd {
            -1 // The injected poll never touches this sentinel descriptor.
        }
    }
    impl Read for NoReadyRead {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            panic!("a reader without readiness must never enter recv");
        }
    }

    #[test]
    fn an_idle_stopping_or_revoked_reader_ends_at_the_first_tick() {
        let ticks = Cell::new(0);
        let mut reader = Ticked::with_poll(
            NoReadyRead,
            READ_TICK,
            || {
                ticks.set(ticks.get() + 1);
                true
            },
            |_, _| Ok(false),
        );
        assert!(reader.fill_buf().unwrap().is_empty());
        assert_eq!(ticks.get(), 1);
    }

    #[test]
    fn a_line_split_across_a_tick_remains_intact() {
        let (stream, mut peer) = UnixStream::pair().unwrap();
        peer.write_all(b"first ").unwrap();
        let polls = Cell::new(0);
        let ticks = Cell::new(0);
        let mut reader = Ticked::with_poll(
            stream,
            READ_TICK,
            || {
                ticks.set(ticks.get() + 1);
                false
            },
            |fd, _| {
                polls.set(polls.get() + 1);
                if polls.get() == 2 {
                    peer.write_all(b"second\n").unwrap();
                    Ok(false)
                } else {
                    poll_readable(fd, Duration::from_millis(1))
                }
            },
        );
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line, "first second\n");
        assert_eq!(polls.get(), 3);
        assert_eq!(ticks.get(), 1);
    }
}
