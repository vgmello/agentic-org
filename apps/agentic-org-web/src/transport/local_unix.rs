//! Consolidated local Unix-domain socket adapter for `agentic-org-web` (`TASK-232`).
//!
//! Isolates every `std::os::unix::net` and `tokio::net::Unix*` primitive in the crate:
//! - verified process rendezvous handoff (`handoff`),
//! - admitted control-plane socket & frame reader (`LocalUnixControlSocket`, `LocalUnixFrameReader`),
//! - SCM_RIGHTS office-channel transfer datagram (`LocalUnixTransferChannel`),
//! - async office stream split (`LocalUnixOfficeStream`).

use std::io::{self, Write};
use std::net::{Shutdown, TcpListener};
use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd};
use std::os::unix::net::{UnixDatagram as LocalDatagram, UnixStream as LocalStream};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use org_host_control::{
    ops::HOST_SOCK_MARKER,
    web::{
        LandlockEnforcement, LandlockStatus, WebFd, WebHandoff, WebReadyForSecrets, WEB_HANDOFF_FDS,
    },
};
use org_office_launch::{
    decode_transfer_record, ChannelKind, TransferRecord, TRANSFER_HEADER_MAX_BYTES,
};
use org_platform::confinement::Enforcement;
use org_socket::rights::{self, DatagramReceiver, ReceiveError, StreamReceiver};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::UnixStream as AsyncLocalStream;
use zeroize::Zeroizing;

use crate::ports::{Failure, MAX_CONTROL_BYTES};

const HANDOFF_WAIT: Duration = Duration::from_secs(5);
const HANDOFF_HEADER_MAX: usize = 1024;
static SPAWN_FENCE: Mutex<()> = Mutex::new(());

fn handoff_failure() -> io::Error {
    io::Error::other("authenticated web process startup unavailable")
}

fn send_handoff_line(stream: &mut LocalStream, message: &impl serde::Serialize) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(message).map_err(|_| handoff_failure())?;
    if bytes.len() >= HANDOFF_HEADER_MAX {
        return Err(handoff_failure());
    }
    bytes.push(b'\n');
    stream.write_all(&bytes)
}

fn receive_handoff_byte(
    receiver: &mut StreamReceiver,
    count: usize,
    until: Instant,
    fence: &Mutex<()>,
    fatal: fn() -> !,
) -> io::Result<(u8, Vec<OwnedFd>)> {
    loop {
        match receiver.receive_byte(count, fence) {
            Ok(value) => return Ok(value),
            Err(ReceiveError::WouldBlock) => {
                let remaining = until
                    .checked_duration_since(Instant::now())
                    .ok_or_else(handoff_failure)?;
                let fd = receiver.fd().ok_or_else(handoff_failure)?;
                if !org_socket::poll_readable(fd.as_raw_fd(), remaining)? {
                    return Err(handoff_failure());
                }
            }
            // Fatal ancillary framing requires process termination, even before
            // secret handoff: macOS may have installed unreported descriptors.
            Err(ReceiveError::Fatal(_)) => fatal(),
        }
    }
}

/// Decodes the handoff descriptors exactly as the host sends them: the marker byte
/// carries [`WEB_HANDOFF_FDS`] in order, `[Transfer, Listener]`, and nothing else.
/// Each descriptor's socket type is checked before any ACK is sent.
pub(crate) fn handoff_descriptors(
    marker: u8,
    fds: Vec<OwnedFd>,
) -> io::Result<(LocalDatagram, TcpListener)> {
    if marker != HOST_SOCK_MARKER || fds.len() != WEB_HANDOFF_FDS.len() {
        return Err(handoff_failure());
    }
    let mut transfer = None;
    let mut listener = None;
    for (kind, fd) in WEB_HANDOFF_FDS.into_iter().zip(fds) {
        match kind {
            WebFd::Transfer => transfer = Some(rights::unix_datagram(fd)?),
            WebFd::Listener => listener = Some(rights::tcp_listener(fd)?),
        }
    }
    Ok((
        transfer.ok_or_else(handoff_failure)?,
        listener.ok_or_else(handoff_failure)?,
    ))
}

fn ready_message(enforcement: Enforcement) -> WebReadyForSecrets {
    WebReadyForSecrets {
        landlock: enforcement.landlock_abi.map(|abi| LandlockStatus {
            abi,
            enforcement: LandlockEnforcement::FullyEnforced,
        }),
    }
}

/// Executes the verified web-process rendezvous handoff over the confined local Unix stream.
pub(crate) fn handoff(
    mut stream: LocalStream,
    enforcement: Enforcement,
    fatal: fn() -> !,
) -> io::Result<(OwnedFd, OwnedFd, TcpListener)> {
    stream.set_write_timeout(Some(HANDOFF_WAIT))?;
    send_handoff_line(&mut stream, &ready_message(enforcement))?;
    let mut receiver = StreamReceiver::new(stream.try_clone()?);
    let until = Instant::now() + HANDOFF_WAIT;
    let fence = Mutex::new(()); // still the only thread; no concurrent spawn exists
    let mut header = Vec::new();
    loop {
        let (value, descriptors) = receive_handoff_byte(&mut receiver, 0, until, &fence, fatal)?;
        drop(descriptors);
        if value == b'\n' {
            break;
        }
        if header.len() >= HANDOFF_HEADER_MAX - 1 {
            return Err(handoff_failure());
        }
        header.push(value);
    }
    let _: WebHandoff = serde_json::from_slice(&header).map_err(|_| handoff_failure())?;
    let (marker, fds) =
        receive_handoff_byte(&mut receiver, WEB_HANDOFF_FDS.len(), until, &fence, fatal)?;
    let (transfer, listener) = handoff_descriptors(marker, fds)?;
    // Type checks precede ACK, and sole receiver has consumed exactly through
    // the marker. Its duplicate is dropped before the actor becomes the reader.
    drop(receiver);
    send_handoff_line(
        &mut stream,
        &serde_json::json!({
            "v": org_host_control::API_VERSION,
            "event": "fds.received",
            "of": "handoff"
        }),
    )?;
    Ok((stream.into(), transfer.into(), listener))
}

/// Zeroizing reply buffer assembled byte-by-byte from the control socket.
pub(crate) struct ControlReplyBuffer {
    pub(crate) bytes: Zeroizing<Vec<u8>>,
}

impl ControlReplyBuffer {
    pub(crate) fn new() -> Self {
        Self {
            bytes: Zeroizing::new(Vec::with_capacity(MAX_CONTROL_BYTES.min(64 * 1024))),
        }
    }

    pub(crate) fn push(&mut self, byte: u8) -> Result<(), Failure> {
        if self.bytes.len() == MAX_CONTROL_BYTES {
            return Err(Failure::Unavailable);
        }
        if self.bytes.len() == self.bytes.capacity() {
            let capacity = (self.bytes.capacity() * 2).min(MAX_CONTROL_BYTES);
            let mut bytes = Zeroizing::new(Vec::with_capacity(capacity));
            bytes.extend_from_slice(&self.bytes);
            // Replacing the wrapper zeroes the old allocation before releasing
            // it; Vec's ordinary growth could leave a secret copy behind.
            self.bytes = bytes;
        }
        self.bytes.push(byte);
        Ok(())
    }
}

/// Blocking local Unix stream handle for the admitted web-to-host control connection.
pub(crate) struct LocalUnixControlSocket {
    stream: LocalStream,
}

impl LocalUnixControlSocket {
    pub(crate) fn from_handoff_fd(control: OwnedFd, write_timeout: Duration) -> io::Result<Self> {
        let stream = rights::connected_unix_stream(control)?;
        stream.set_nonblocking(false)?;
        stream.set_write_timeout(Some(write_timeout))?;
        Ok(Self { stream })
    }

    pub(crate) fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            stream: self.stream.try_clone()?,
        })
    }

    pub(crate) fn into_frame_reader(self) -> LocalUnixFrameReader {
        LocalUnixFrameReader {
            receiver: StreamReceiver::new(self.stream),
        }
    }

    pub(crate) fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.stream.write_all(bytes)
    }

    pub(crate) fn shutdown(&self) {
        let _ = self.stream.shutdown(Shutdown::Both);
    }
}

/// Event-driven reader over the admitted control-plane local Unix stream.
pub(crate) struct LocalUnixFrameReader {
    receiver: StreamReceiver,
}

impl LocalUnixFrameReader {
    /// Polls for readability up to `timeout`. Returns `Ok(None)` if the descriptor is closed.
    pub(crate) fn poll_readable(&self, timeout: Duration) -> io::Result<Option<bool>> {
        let Some(fd) = self.receiver.fd() else {
            return Ok(None);
        };
        org_socket::poll_readable(fd.as_raw_fd(), timeout).map(Some)
    }

    /// Reads one newline-delimited control frame into a zeroizing buffer by `until`.
    pub(crate) fn read_frame(
        &mut self,
        until: Instant,
        fatal: &dyn Fn(),
    ) -> Result<ControlReplyBuffer, Failure> {
        let mut reply = ControlReplyBuffer::new();
        loop {
            poll_fd_until(self.receiver.fd().ok_or(Failure::Unavailable)?, until)?;
            let (byte, _) = self
                .receiver
                .receive_byte(0, &SPAWN_FENCE)
                .map_err(|error| map_receive_error(error, fatal))?;
            if byte == b'\n' {
                return Ok(reply);
            }
            reply.push(byte)?;
        }
    }
}

/// Opaque local Unix stream received via host SCM_RIGHTS transfer for one office channel.
pub struct LocalUnixOfficeStream {
    stream: LocalStream,
}

impl LocalUnixOfficeStream {
    /// Converts this connected stream into non-blocking Tokio read and write halves.
    pub(crate) fn into_async_split(self) -> io::Result<(OwnedReadHalf, OwnedWriteHalf)> {
        self.stream.set_nonblocking(true)?;
        let stream = AsyncLocalStream::from_std(self.stream)?;
        Ok(stream.into_split())
    }

    #[cfg(test)]
    pub(crate) fn from_std(stream: LocalStream) -> Self {
        Self { stream }
    }
}

/// Serialized SCM_RIGHTS transfer socket receiving host-opened Web office channels.
pub(crate) struct LocalUnixTransferChannel {
    receiver: Mutex<DatagramReceiver>,
}

pub(crate) struct LocalUnixTransferGuard<'a> {
    guard: MutexGuard<'a, DatagramReceiver>,
}

impl LocalUnixTransferChannel {
    pub(crate) fn from_handoff_fd(transfer: OwnedFd) -> io::Result<Self> {
        let datagram = rights::unix_datagram(transfer)?;
        datagram.set_nonblocking(true)?;
        Ok(Self {
            receiver: Mutex::new(DatagramReceiver::new(datagram)),
        })
    }

    pub(crate) fn lock(&self) -> Result<LocalUnixTransferGuard<'_>, Failure> {
        self.receiver
            .lock()
            .map(|guard| LocalUnixTransferGuard { guard })
            .map_err(|_| Failure::Unavailable)
    }
}

impl LocalUnixTransferGuard<'_> {
    /// Receives one host-opened Web office channel descriptor and validates its header.
    pub(crate) fn receive_office_channel(
        &mut self,
        expected_office: &str,
        expected_channel: &str,
        until: Instant,
        fatal: &dyn Fn(),
    ) -> Result<LocalUnixOfficeStream, Failure> {
        let mut payload = [0u8; TRANSFER_HEADER_MAX_BYTES];
        let received = loop {
            match self.guard.receive(&mut payload, 1, &SPAWN_FENCE) {
                Ok(received) => break received,
                Err(ReceiveError::WouldBlock) => {
                    let fd = self.guard.fd().ok_or(Failure::Unavailable)?;
                    poll_fd_until(fd, until)?;
                }
                Err(error) => return Err(map_receive_error(error, fatal)),
            }
        };
        let mut descriptors = received.descriptors;
        if descriptors.len() != 1 {
            return Err(Failure::Unavailable);
        }
        let fd = descriptors.pop().ok_or(Failure::Unavailable)?;
        let record =
            decode_transfer_record(&payload[..received.bytes]).map_err(|_| Failure::Unavailable)?;
        match record {
            TransferRecord::Channel {
                kind: ChannelKind::Web,
                office_id,
                channel_id,
            } if office_id == expected_office && channel_id == expected_channel => {
                let stream = rights::connected_unix_stream(fd).map_err(|_| Failure::Unavailable)?;
                Ok(LocalUnixOfficeStream { stream })
            }
            _ => Err(Failure::Unavailable),
        }
    }
}

fn poll_fd_until(fd: BorrowedFd<'_>, until: Instant) -> Result<(), Failure> {
    let left = until
        .checked_duration_since(Instant::now())
        .ok_or(Failure::Unavailable)?;
    if org_socket::poll_readable(fd.as_raw_fd(), left).map_err(|_| Failure::Unavailable)? {
        Ok(())
    } else {
        Err(Failure::Unavailable)
    }
}

fn map_receive_error(error: ReceiveError, fatal: &dyn Fn()) -> Failure {
    if matches!(&error, ReceiveError::Fatal(e) if e.kind() != io::ErrorKind::UnexpectedEof) {
        fatal();
    }
    Failure::Unavailable
}
