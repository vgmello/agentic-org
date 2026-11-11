//! Launch transport, staged for X-02 2a.3.2b.2. The caller supplies the receive/spawn fence,
//! shared with every concurrent spawn path, protecting recvmsg→CLOEXEC on macOS.
//! All errors end the worker process;
//! particularly, a fatal rights error must never be caught and followed by serving.

use std::io;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};

use org_office_launch::v3::{self, AnyFirstLine, Handoff, HostFirstLine, RendezvousEvent};
use org_office_launch::{FirstLine, HostMessage, LaunchError, ServerEvent};
use org_proto::frame::{self, FrameError};
use org_socket::rights::{ReceiveError, StreamReceiver};

use crate::fdswap::ControlPipe;
use crate::zeroed::{SecretBuffer, ZeroedReader};

// X-02 §5.1 uses §5.2's header framing, including its 1024-byte limit.
const HANDOFF_HEADER_MAX_BYTES: usize = v3::TRANSFER_HEADER_MAX_BYTES;

#[cfg_attr(not(test), expect(dead_code, reason = "activated by X-02 2a.3.2b.2"))]
pub(super) enum Request {
    V2(HostMessage),
    V3 {
        line: HostFirstLine,
        handoff: Handoff,
    },
}

#[cfg_attr(not(test), expect(dead_code, reason = "activated by X-02 2a.3.2b.2"))]
pub(super) struct Control {
    pipe: ControlPipe,
    rendezvous: Option<UnixStream>,
    // Owned before the ACK; kept until shutdown, then serviced by 2a.4.
    transfer: Option<OwnedFd>,
    fence: Arc<Mutex<()>>,
}

#[cfg_attr(not(test), expect(dead_code, reason = "activated by X-02 2a.3.2b.2"))]
impl Control {
    pub fn new(pipe: ControlPipe, fence: Arc<Mutex<()>>) -> Self {
        Self {
            pipe,
            rendezvous: None,
            transfer: None,
            fence,
        }
    }

    /// Reads only the first stdin line. Its buffered remainder is wiped and discarded.
    /// V3 header bytes are received exclusively with recvmsg and zero early descriptors.
    pub fn read(&mut self, expected: FirstLine) -> Result<Option<Request>, LaunchError> {
        let input = self.pipe.from_host.try_clone().map_err(io_error)?;
        let mut reader = ZeroedReader::new(input);
        match v3::read_first_line_any(&mut reader, expected)? {
            None => Ok(None),
            Some(AnyFirstLine::V2(message)) => Ok(Some(Request::V2(message))),
            Some(AnyFirstLine::V3(line)) => {
                // Drop the pipe reader (and wipe it) before receiving secrets.
                drop(reader);
                let office = line
                    .office()
                    .ok_or(LaunchError::Invalid("unknown launch op"))?;
                let stream = UnixStream::connect(&office.rendezvous).map_err(io_error)?;
                self.rendezvous = Some(stream);
                let stream = self.rendezvous.as_mut().expect("just connected");
                frame::write_message(stream, &v3::ReadyForSecrets).map_err(LaunchError::Frame)?;
                let mut receiver = StreamReceiver::new(stream.try_clone().map_err(io_error)?);
                let handoff = read_handoff(stream, &mut receiver, &self.fence)?;
                if !matches!(
                    (&line, &handoff),
                    (HostFirstLine::Create(_), Handoff::Create { .. })
                        | (HostFirstLine::Start(_), Handoff::Start { .. })
                        | (HostFirstLine::Discard(_), Handoff::Discard { .. })
                ) {
                    return Err(LaunchError::Invalid("handoff op differs from first line"));
                }
                if !handoff.fds().is_empty() {
                    let mut fds = receive(stream, || {
                        receiver.receive_marker(handoff.fds().len(), v3::FD_MARKER, &self.fence)
                    })?;
                    self.transfer = fds.pop();
                    v3::write_rendezvous_event(
                        stream,
                        &RendezvousEvent::FdsReceived {
                            of: "handoff".into(),
                        },
                    )?;
                }
                Ok(Some(Request::V3 { line, handoff }))
            }
            Some(_) => Err(LaunchError::Invalid("unknown launch version")),
        }
    }

    pub fn emit(&mut self, event: &ServerEvent) -> Result<(), LaunchError> {
        match &mut self.rendezvous {
            Some(stream) => v3::write_server_event(stream, event),
            None => org_office_launch::write_server_event(&mut self.pipe.to_host, event),
        }
    }
}

fn read_handoff(
    stream: &UnixStream,
    receiver: &mut StreamReceiver,
    fence: &Mutex<()>,
) -> Result<Handoff, LaunchError> {
    // Allocate once before keys arrive: never grow/reallocate a secret-bearing buffer.
    let mut header = SecretBuffer::new(HANDOFF_HEADER_MAX_BYTES + 1);
    let bytes = header.as_mut();
    for index in 0..bytes.len() {
        let (byte, _fds) = receive(stream, || receiver.receive_byte(0, fence))?;
        if byte == b'\n' {
            return v3::decode_handoff(&bytes[..index]).map_err(|error| match error {
                // serde may quote an unknown tag or field containing misplaced key bytes.
                LaunchError::Json(_) => LaunchError::Invalid("invalid secret handoff"),
                other => other,
            });
        }
        bytes[index] = byte;
    }
    Err(LaunchError::Frame(FrameError::TooLarge))
}

fn receive<T>(
    stream: &UnixStream,
    call: impl FnMut() -> Result<T, ReceiveError>,
) -> Result<T, LaunchError> {
    receive_with_poll(call, || {
        org_socket::poll_readable(stream.as_raw_fd(), org_socket::READ_TICK)
    })
}

fn receive_with_poll<T>(
    mut call: impl FnMut() -> Result<T, ReceiveError>,
    mut poll: impl FnMut() -> io::Result<bool>,
) -> Result<T, LaunchError> {
    loop {
        match call() {
            Ok(value) => return Ok(value),
            Err(ReceiveError::WouldBlock) => match poll() {
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(io_error(error)),
            },
            // Fixed diagnostic: never expose header bytes or secret decoder inputs.
            Err(ReceiveError::Fatal(_)) => {
                return Err(LaunchError::Invalid("invalid descriptor handoff"));
            }
        }
    }
}

fn io_error(error: io::Error) -> LaunchError {
    LaunchError::Frame(FrameError::Io(error))
}

#[cfg(test)]
mod tests;
