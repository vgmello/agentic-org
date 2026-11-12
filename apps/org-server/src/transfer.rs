//! FIFO transfer control service, independent of Office and work execution.
mod channels;
use crate::{revocations::Revocations, startup};
pub(super) use channels::Channels;
use org_office_launch::v3::{self, ChannelKind, TransferRecord};
#[cfg(not(test))]
use org_socket::poll_readable;
use org_socket::rights::ReceiveError;
use std::io;
use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
#[cfg(not(test))]
use std::thread::Builder;
use std::thread::JoinHandle;
use std::time::Duration;
#[cfg(test)]
use tests::poll_readable;
#[cfg(test)]
use tests::Builder;

pub(super) struct Channel {
    kind: ChannelKind,
    office_id: String,
    channel_id: String,
    fd: OwnedFd,
}
pub(super) struct Service {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl Service {
    pub fn start(
        transport: startup::Transfer,
        revocations: Arc<Mutex<Revocations>>,
        mut channel: impl FnMut(Channel) -> io::Result<()> + Send + 'static,
    ) -> io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let thread = Builder::new().name("org-transfer".into()).spawn(move || {
            if run(transport, revocations, stopping, &mut channel).is_err() {
                fatal();
            }
        })?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
}
#[allow(unsafe_code)]
fn fatal() -> ! {
    use std::io::Write;
    let _ = io::stderr().write_all(b"org-server: fatal transfer failure\n");
    // Fatal receipt may leave unreported macOS rights. Terminate immediately,
    // with no ACK or C/atexit cleanup racing the live SQLite/OpenSSL threads.
    // SQLite's crash recovery rolls back open transactions on the next open.
    // SAFETY: _exit terminates this process with status 1 and never returns.
    unsafe { libc::_exit(1) }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid transfer record")
}
fn decode(payload: &[u8], descriptors: usize) -> io::Result<TransferRecord> {
    let record = v3::decode_transfer_record(payload).map_err(|_| invalid())?;
    if descriptors != record.fd_count() {
        return Err(invalid());
    }
    Ok(record)
}
// Own the receipt policy separately from semantic decoding: neither a later
// JSON refusal nor descriptor-count validation may mask an over-wide receive.
fn receive(
    transport: &mut startup::Transfer,
) -> Result<
    (
        [u8; v3::TRANSFER_HEADER_MAX_BYTES],
        org_socket::rights::Received,
    ),
    ReceiveError,
> {
    let mut payload = [0; v3::TRANSFER_HEADER_MAX_BYTES];
    let received = transport
        .receiver
        .receive_up_to(&mut payload, 1, &transport.fence)?;
    Ok((payload, received))
}
fn run(
    mut transport: startup::Transfer,
    revocations: Arc<Mutex<Revocations>>,
    stop: Arc<AtomicBool>,
    channel: &mut impl FnMut(Channel) -> io::Result<()>,
) -> io::Result<()> {
    while !stop.load(Ordering::SeqCst) {
        let (payload, mut received) = match receive(&mut transport) {
            Ok(received) => received,
            Err(ReceiveError::WouldBlock) => {
                let fd = transport.receiver.fd().ok_or_else(invalid)?.as_raw_fd();
                match poll_readable(fd, Duration::from_millis(100)) {
                    Ok(_) => continue,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(error),
                }
            }
            Err(ReceiveError::Fatal(_)) => return Err(invalid()),
        };
        match decode(&payload[..received.bytes], received.descriptors.len())? {
            TransferRecord::Revoked { session_ids } => {
                revocations
                    .lock()
                    .map_err(|_| invalid())?
                    .apply_batch(session_ids)?;
            }
            TransferRecord::Channel {
                kind,
                office_id,
                channel_id,
            } => channel(Channel {
                kind,
                office_id,
                channel_id,
                fd: received.descriptors.pop().ok_or_else(invalid)?,
            })?,
        }
    }
    Ok(())
}
#[cfg(test)]
pub(crate) mod tests;
