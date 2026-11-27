//! A connection's writer thread: the messages queued for it and how each is written.

use std::io::{self, BufWriter};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Duration;

use org_proto::frame::{self, FrameError};
use org_proto::{ApiError, ErrorCode, Event, Response};
use serde_json::Value;

pub(super) enum Outbound {
    Response(Response),
    /// A response line already encoded, holding a secret: written as is, then zeroed.
    Secret(SecretLine),
    Descriptor {
        response: Response,
        descriptor: std::os::fd::OwnedFd,
        receipt: Receiver<()>,
        gateway: Option<Arc<std::os::unix::net::UnixDatagram>>,
        office: String,
        channel: String,
    },
    /// `seq` is given by the writer, so it counts without gaps in the order events are written.
    Event {
        name: String,
        data: Value,
    },
    /// Ends the writer once everything queued before it is written.
    Close,
}

/// An encoded verified-session response line holding a secret. It never passes through a
/// `serde_json::Value` or the writer's buffer, and is zeroed when dropped (best effort: the
/// socket's kernel buffers still hold it until the client reads it).
pub(crate) struct SecretLine(Vec<u8>);

impl SecretLine {
    /// `bytes` is one JSON line, `\n` included.
    pub(crate) fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

impl Drop for SecretLine {
    fn drop(&mut self) {
        crate::secrets::zero(&mut self.0);
    }
}

/// Shuts a socket down both ways: writing first, then reading, each on its own. Not
/// `Shutdown::Both`: on macOS that fails (`ENOTCONN`) once the client has shut down its own
/// writing, and then shuts nothing down, so a client that half-closed and reads to EOF would
/// wait forever (the socket stays open while anything holds a copy of it).
pub(super) fn shut_down(stream: &UnixStream) {
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let _ = stream.shutdown(std::net::Shutdown::Read);
}

/// The writer: writes queued messages until [`Outbound::Close`], until every sender is gone,
/// or until a write fails (then it closes the socket, which also ends the reader).
pub(super) fn write_loop(stream: &UnixStream, queue: Receiver<Outbound>, closed: &AtomicBool) {
    struct MarkClosed<'a>(&'a AtomicBool);
    impl Drop for MarkClosed<'_> {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let _closed = MarkClosed(closed);
    let mut out = BufWriter::new(stream);
    let mut seq = 0u64;
    for message in queue {
        let written = match message {
            Outbound::Close => return,
            Outbound::Response(response) => write_response(&mut out, response),
            Outbound::Secret(line) => {
                // Past the writer's buffer, straight to the socket, so no copy stays there.
                let written = io::Write::flush(&mut out).and_then(|()| {
                    let mut socket: &UnixStream = out.get_ref();
                    io::Write::write_all(&mut socket, &line.0)
                });
                if written.is_err() {
                    shut_down(stream);
                    return;
                }
                continue;
            }
            Outbound::Descriptor {
                response,
                descriptor,
                receipt,
                gateway,
                office,
                channel,
            } => {
                use std::os::fd::AsFd;
                let sent = write_response(&mut out, response)
                    .and_then(|()| io::Write::flush(&mut out).map_err(FrameError::Io));
                let until = std::time::Instant::now() + Duration::from_secs(5);
                let delivered = if sent.is_err() {
                    false
                } else if let Some(gateway) = gateway {
                    org_office_launch::encode_transfer_record(
                        &org_office_launch::TransferRecord::Channel {
                            kind: org_office_launch::ChannelKind::Web,
                            office_id: office,
                            channel_id: channel,
                        },
                    )
                    .ok()
                    .is_some_and(|header| {
                        org_socket::rights::send(
                            gateway.as_fd(),
                            &header,
                            &[descriptor.as_fd()],
                            until,
                        )
                        .is_ok()
                    })
                } else {
                    org_socket::rights::send(
                        out.get_ref().as_fd(),
                        &[org_host_control::ops::HOST_SOCK_MARKER],
                        &[descriptor.as_fd()],
                        until,
                    )
                    .is_ok()
                };
                if sent.is_err()
                    || !delivered
                    || receipt.recv_timeout(Duration::from_secs(5)).is_err()
                {
                    shut_down(stream);
                    return;
                }
                continue;
            }
            Outbound::Event { name, data } => {
                seq += 1;
                frame::write_message(
                    &mut out,
                    &Event {
                        event: name,
                        seq,
                        data,
                    },
                )
            }
        };
        if let Err(e) = written {
            if e.closes_stream() {
                shut_down(stream);
                return;
            }
        }
    }
}

/// Writes a response; one too large to send is answered with an error instead.
fn write_response(out: &mut impl io::Write, response: Response) -> Result<(), FrameError> {
    match frame::write_message(out, &response) {
        Err(FrameError::TooLargeToSend) => frame::write_message(
            out,
            &Response::error(
                response.id,
                ApiError::new(ErrorCode::StorageError, "the answer is too large to send"),
            ),
        ),
        other => other,
    }
}
