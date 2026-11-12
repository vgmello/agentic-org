//! Worker launch transport. The caller supplies the receive/spawn fence,
//! shared with every concurrent spawn path, protecting recvmsg→CLOEXEC on macOS.
//! All errors end the worker process;
//! particularly, a fatal rights error must never be caught and followed by serving.

use std::fs::File;
use std::io::{self, Read};
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

pub(super) enum Request {
    V2(HostMessage),
    V3 {
        line: HostFirstLine,
        handoff: Handoff,
    },
}

pub(super) struct Control {
    pipe: ControlPipe,
    rendezvous: Option<UnixStream>,
    receiver: Option<StreamReceiver>,
    // Owned before the ACK; kept until shutdown, then serviced by 2a.4.
    transfer: Option<OwnedFd>,
    fence: Arc<Mutex<()>>,
    #[cfg(test)]
    before_marker: Option<Box<dyn FnOnce() + Send>>,
}

impl Control {
    pub fn new(pipe: ControlPipe, fence: Arc<Mutex<()>>) -> Self {
        Self {
            pipe,
            rendezvous: None,
            receiver: None,
            transfer: None,
            fence,
            #[cfg(test)]
            before_marker: None,
        }
    }

    /// Reads only the first stdin line. Its buffered remainder is wiped and discarded.
    /// V3 header bytes are received exclusively with recvmsg and zero early descriptors.
    #[cfg(test)]
    pub fn read(&mut self, expected: FirstLine) -> Result<Option<Request>, LaunchError> {
        self.read_for_office(expected, None)
    }

    pub fn read_for_office(
        &mut self,
        expected: FirstLine,
        id: Option<&str>,
    ) -> Result<Option<Request>, LaunchError> {
        let input = self.pipe.from_host.try_clone().map_err(io_error)?;
        let mut reader = ZeroedReader::new(input);
        match v3::read_first_line_any(&mut reader, expected)? {
            None => Ok(None),
            Some(AnyFirstLine::V2(message)) => Ok(Some(Request::V2(message))),
            Some(AnyFirstLine::V3(line)) => {
                // Drop the pipe reader (and wipe it) before receiving secrets.
                drop(reader.take_inner());
                drop(reader);
                let office = line
                    .office()
                    .ok_or(LaunchError::Invalid("unknown launch op"))?;
                if id.is_some_and(|id| office.office_id != id) {
                    return Err(LaunchError::Invalid("office id differs from flag"));
                }
                // Drain before blocking on the handoff so host stdin writes cannot stall it.
                let pipe = self.pipe.from_host.try_clone().map_err(io_error)?;
                std::thread::Builder::new()
                    .name("org-stdin-drain".into())
                    .spawn(move || drain(pipe))
                    .map_err(io_error)?;
                let stream = UnixStream::connect(&office.rendezvous).map_err(io_error)?;
                self.rendezvous = Some(stream);
                let stream = self.rendezvous.as_mut().expect("just connected");
                frame::write_message(stream, &v3::ReadyForSecrets).map_err(LaunchError::Frame)?;
                let receiver = self
                    .receiver
                    .insert(StreamReceiver::new(stream.try_clone().map_err(io_error)?));
                let handoff = read_handoff(stream, receiver, &self.fence)?;
                if !matches!(
                    (&line, &handoff),
                    (HostFirstLine::Create(_), Handoff::Create { .. })
                        | (HostFirstLine::Start(_), Handoff::Start { .. })
                        | (HostFirstLine::Discard(_), Handoff::Discard { .. })
                ) {
                    return Err(LaunchError::Invalid("handoff op differs from first line"));
                }
                if !handoff.fds().is_empty() {
                    #[cfg(test)]
                    if let Some(before_marker) = self.before_marker.take() {
                        before_marker();
                    }
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

    /// A fatal receiver closes itself; unreported Mac descriptors require process exit.
    pub fn failed_rights(&self) -> bool {
        self.receiver
            .as_ref()
            .is_some_and(|receiver| receiver.fd().is_none())
    }

    pub fn lifetime(&mut self) -> io::Result<Lifetime> {
        let Some(receiver) = self.receiver.take() else {
            return self.pipe.from_host.try_clone().map(Lifetime::Pipe);
        };
        Ok(Lifetime::Rendezvous {
            receiver,
            fence: Arc::clone(&self.fence),
        })
    }

    pub fn emit(&mut self, event: &ServerEvent) -> Result<(), LaunchError> {
        match &mut self.rendezvous {
            Some(stream) => v3::write_server_event(stream, event),
            None => org_office_launch::write_server_event(&mut self.pipe.to_host, event),
        }
    }
}

/// Consume input without decoding it. EOF and non-interrupt errors end the drain.
pub(super) fn drain(mut source: impl Read) {
    let mut sink = [0; 4096];
    loop {
        match source.read(&mut sink) {
            Ok(n) if n > 0 => {}
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            _ => break,
        }
    }
}

impl Drop for Control {
    fn drop(&mut self) {
        if let Some(stream) = &self.rendezvous {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }
}

/// Only EOF is ordinary lifetime input; rights faults must not unwind into serving.
pub(super) enum Lifetime {
    Pipe(File),
    Rendezvous {
        receiver: StreamReceiver,
        fence: Arc<Mutex<()>>,
    },
}

impl Read for Lifetime {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        match self {
            Self::Pipe(pipe) => pipe.read(bytes),
            Self::Rendezvous { receiver, fence } => {
                let fd = receiver.fd().expect("live lifetime receiver").as_raw_fd();
                match receive_with_poll(
                    || match receiver.receive_byte(0, fence) {
                        Err(ReceiveError::Fatal(e)) if e.kind() == io::ErrorKind::UnexpectedEof => {
                            Ok(false)
                        }
                        other => other.map(|_| true),
                    },
                    || org_socket::poll_readable(fd, org_socket::READ_TICK),
                ) {
                    Ok(false) => Ok(0),
                    _ => std::process::exit(1),
                }
            }
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
                // EINTR leaves the receive pending; it is not a protocol failure.
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
