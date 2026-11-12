//! Nonblocking UNIX descriptor reception. Protocols supply counts and marker values.
//!
//! Never mix buffered/ordinary reads with these receivers: ancillary data belongs to the
//! bytes returned by `recvmsg`. Readiness waits and deadlines belong to the caller.
//! On macOS the caller must pass the same fence used by every concurrent spawn path;
//! it covers `recvmsg` through every `FD_CLOEXEC` update. Linux sets CLOEXEC atomically
//! with `MSG_CMSG_CLOEXEC` and does not acquire the fence.

use std::fmt;
use std::io;
use std::mem;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::{UnixDatagram, UnixStream};
use std::sync::Mutex;

/// A receive that did not produce a usable message.
#[derive(Debug)]
pub enum ReceiveError {
    /// Nothing was received; readiness may be awaited before retrying.
    WouldBlock,
    /// The receiver is closed and cannot be reused. Every reported SCM_RIGHTS fd is closed.
    ///
    /// The caller MUST terminate the process (X-02 §5.2): macOS can install unreported
    /// descriptors without CLOEXEC on ancillary truncation. This library never exits/aborts.
    Fatal(io::Error),
}

impl fmt::Display for ReceiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WouldBlock => f.write_str("descriptor receive would block"),
            Self::Fatal(error) => write!(f, "fatal descriptor receive: {error}"),
        }
    }
}

impl std::error::Error for ReceiveError {}

/// A datagram's byte count and descriptors, in arrival order. The payload stays in its buffer.
#[derive(Debug)]
pub struct Received {
    /// Bytes written into the caller's buffer.
    pub bytes: usize,
    /// Owned, close-on-exec descriptors in the count permitted by the receive call.
    pub descriptors: Vec<OwnedFd>,
}

/// An exclusive stream receiver; each receive reads exactly one byte.
pub struct StreamReceiver(Receiver);

impl StreamReceiver {
    /// Take sole ownership of the stream's read side. No other reader may consume its bytes.
    pub fn new(stream: UnixStream) -> Self {
        Self(Receiver(Some(stream.into())))
    }

    /// The live descriptor for readiness polling, absent after a fatal receive.
    pub fn fd(&self) -> Option<BorrowedFd<'_>> {
        self.0.fd()
    }

    /// Receive one byte and its exact descriptor count, preserving the next stream byte.
    /// Header readers use this too, so an early descriptor is never silently discarded.
    pub fn receive_byte(
        &mut self,
        expected: usize,
        fence: &Mutex<()>,
    ) -> Result<(u8, Vec<OwnedFd>), ReceiveError> {
        let mut byte = [0u8; 1];
        let received = self
            .0
            .receive(&mut byte, DescriptorCount::Exact(expected), fence)?;
        if received.bytes == 0 {
            drop(received.descriptors);
            return self
                .0
                .fatal(io::Error::new(io::ErrorKind::UnexpectedEof, "stream ended"));
        }
        Ok((byte[0], received.descriptors))
    }

    /// Receive one caller-defined marker and exactly `expected` descriptors, never past it.
    pub fn receive_marker(
        &mut self,
        expected: usize,
        marker: u8,
        fence: &Mutex<()>,
    ) -> Result<Vec<OwnedFd>, ReceiveError> {
        let (byte, descriptors) = self.receive_byte(expected, fence)?;
        if byte != marker {
            drop(descriptors);
            return self.0.fatal(invalid("wrong descriptor marker"));
        }
        Ok(descriptors)
    }
}

/// An exclusive datagram receiver. A truncated payload or ancillary message is fatal.
pub struct DatagramReceiver(Receiver);

impl DatagramReceiver {
    /// Take sole ownership of the datagram socket's read side.
    pub fn new(socket: UnixDatagram) -> Self {
        Self(Receiver(Some(socket.into())))
    }

    /// The live descriptor for readiness polling, absent after a fatal receive.
    pub fn fd(&self) -> Option<BorrowedFd<'_>> {
        self.0.fd()
    }

    /// Receive one datagram with zero through `max_fds` descriptors, in arrival order.
    ///
    /// Control storage is sized for `max_fds`; padding never permits extra descriptors.
    /// Protocols must validate the actual count against the decoded payload before use.
    /// Empty datagrams are valid here, including with a zero descriptor bound.
    /// Truncation, excess descriptors or an unrepresentable bound are fatal, close this
    /// receiver and all reported descriptors, and require the caller to terminate
    /// (see [`ReceiveError::Fatal`]). Readiness and spawn-fence rules match [`Self::receive`].
    pub fn receive_up_to(
        &mut self,
        payload: &mut [u8],
        max_fds: usize,
        fence: &Mutex<()>,
    ) -> Result<Received, ReceiveError> {
        self.0
            .receive(payload, DescriptorCount::AtMost(max_fds), fence)
    }

    /// Receive one datagram into `payload` with exactly `expected` descriptors.
    /// An empty datagram is valid here; it is not a stream EOF.
    pub fn receive(
        &mut self,
        payload: &mut [u8],
        expected: usize,
        fence: &Mutex<()>,
    ) -> Result<Received, ReceiveError> {
        self.0
            .receive(payload, DescriptorCount::Exact(expected), fence)
    }
}

#[derive(Clone, Copy)]
enum DescriptorCount {
    Exact(usize),
    AtMost(usize),
}

impl DescriptorCount {
    fn capacity(self) -> usize {
        match self {
            Self::Exact(count) | Self::AtMost(count) => count,
        }
    }

    fn accepts(self, actual: usize) -> bool {
        match self {
            Self::Exact(expected) => actual == expected,
            Self::AtMost(maximum) => actual <= maximum,
        }
    }
}

struct Receiver(Option<OwnedFd>);

impl Receiver {
    fn fd(&self) -> Option<BorrowedFd<'_>> {
        self.0.as_ref().map(AsFd::as_fd)
    }

    fn fatal<T>(&mut self, error: io::Error) -> Result<T, ReceiveError> {
        self.0.take();
        Err(ReceiveError::Fatal(error))
    }

    fn receive(
        &mut self,
        payload: &mut [u8],
        count: DescriptorCount,
        fence: &Mutex<()>,
    ) -> Result<Received, ReceiveError> {
        self.receive_with(
            payload,
            count,
            fence,
            system_receive,
            close_on_exec,
            control_buffer,
        )
    }

    // Private interposition supports allocation/EINTR/failure and fence-order tests.
    // Successful receive callbacks must preserve recvmsg's kernel/ownership guarantees.
    // Successful allocations must cover the requested byte count with usize alignment.
    fn receive_with(
        &mut self,
        payload: &mut [u8],
        count: DescriptorCount,
        fence: &Mutex<()>,
        mut receive: impl FnMut(RawFd, &mut libc::msghdr, i32) -> io::Result<usize>,
        mut set_flags: impl FnMut(RawFd) -> io::Result<()>,
        allocate: impl FnOnce(usize) -> io::Result<Vec<usize>>,
    ) -> Result<Received, ReceiveError> {
        let Some(socket) = self.0.as_ref() else {
            return Err(ReceiveError::Fatal(invalid("receiver is closed")));
        };
        let fd = socket.as_raw_fd();
        let Some(space) = control_space(count.capacity()) else {
            return self.fatal(invalid("descriptor count is too large"));
        };
        let mut control = match allocate(space) {
            Ok(control) => control,
            Err(error) => return self.fatal(error),
        };
        loop {
            #[cfg(not(target_os = "linux"))]
            let _guard = match fence.lock() {
                Ok(guard) => guard,
                Err(_) => return self.fatal(io::Error::other("descriptor fence is poisoned")),
            };
            #[cfg(target_os = "linux")]
            let _ = (fence, &mut set_flags);
            let mut data = libc::iovec {
                iov_base: payload.as_mut_ptr().cast(),
                iov_len: payload.len(),
            };
            // SAFETY: zero is valid for msghdr; pointers/counts below cover live buffers.
            let mut message: libc::msghdr = unsafe { mem::zeroed() };
            message.msg_iov = &mut data;
            message.msg_iovlen = 1;
            message.msg_control = control.as_mut_ptr().cast();
            message.msg_controllen = space as _;
            let bytes = match receive(fd, &mut message, receive_flags()) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    return Err(ReceiveError::WouldBlock);
                }
                Err(error) => return self.fatal(error),
            };
            // SAFETY: successful recvmsg reports headers within control (a truncated
            // header's cmsg_len can exceed that storage on macOS; take_rights bounds it).
            // SCM_RIGHTS entries are unique newly installed descriptors owned by us.
            let descriptors = unsafe { take_rights(&message) };
            if message.msg_flags & (libc::MSG_CTRUNC | libc::MSG_TRUNC) != 0 {
                drop(descriptors);
                return self.fatal(invalid("truncated descriptor message"));
            }
            if !count.accepts(descriptors.len()) {
                drop(descriptors);
                return self.fatal(invalid("unexpected descriptor count"));
            }
            #[cfg(not(target_os = "linux"))]
            for descriptor in &descriptors {
                if let Err(error) = set_flags(descriptor.as_raw_fd()) {
                    drop(descriptors);
                    return self.fatal(error);
                }
            }
            return Ok(Received { bytes, descriptors });
        }
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn control_buffer(space: usize) -> io::Result<Vec<usize>> {
    // usize alignment meets cmsghdr alignment on the supported UNIX platforms.
    let mut control = Vec::<usize>::new();
    let words = space.div_ceil(mem::size_of::<usize>());
    control
        .try_reserve_exact(words)
        .map_err(|_| io::Error::other("cannot allocate descriptor buffer"))?;
    control.resize(words, 0);
    Ok(control)
}

fn control_space(expected: usize) -> Option<usize> {
    let data = expected.checked_mul(mem::size_of::<RawFd>())?;
    // Check the macro's u32 alignment/addition before calling it.
    let overhead = unsafe { libc::CMSG_SPACE(0) } as usize;
    let _: u32 = data
        .checked_add(overhead)?
        .checked_add(mem::align_of::<libc::cmsghdr>() - 1)?
        .try_into()
        .ok()?;
    Some(unsafe { libc::CMSG_SPACE(data as u32) } as usize)
}

fn receive_flags() -> i32 {
    #[cfg(target_os = "linux")]
    {
        libc::MSG_DONTWAIT | libc::MSG_CMSG_CLOEXEC
    }
    #[cfg(not(target_os = "linux"))]
    {
        libc::MSG_DONTWAIT
    }
}

fn system_receive(fd: RawFd, message: &mut libc::msghdr, flags: i32) -> io::Result<usize> {
    // SAFETY: message's live, aligned buffers and fd are held by Receiver for this call.
    let bytes = unsafe { libc::recvmsg(fd, message, flags) };
    if bytes < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(bytes as usize)
    }
}

// One guard definition for production syscalls and the test-only failure seam.
macro_rules! cloexec_result {
    ($get:expr, $set:expr) => {{
        let flags = $get;
        if flags < 0 || ($set)(flags | libc::FD_CLOEXEC) < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }};
}

fn close_on_exec(fd: RawFd) -> io::Result<()> {
    // SAFETY: the receiver owns fd; these commands only inspect/change its descriptor flags.
    cloexec_result!(
        unsafe { libc::fcntl(fd, libc::F_GETFD) },
        |flags: i32| unsafe { libc::fcntl(fd, libc::F_SETFD, flags) }
    )
}

#[cfg(test)]
fn close_on_exec_with(fd: RawFd, mut call: impl FnMut(RawFd, i32, i32) -> i32) -> io::Result<()> {
    cloexec_result!(call(fd, libc::F_GETFD, 0), |flags| call(
        fd,
        libc::F_SETFD,
        flags
    ))
}

// libc uses size_t on Linux and socklen_t (u32) on macOS.
fn control_len(message: &libc::msghdr) -> usize {
    #[cfg(target_os = "linux")]
    {
        message.msg_controllen
    }
    #[cfg(not(target_os = "linux"))]
    {
        message.msg_controllen as usize
    }
}

unsafe fn take_rights(message: &libc::msghdr) -> Vec<OwnedFd> {
    let mut descriptors = Vec::new();
    // SAFETY: caller supplies successful recvmsg output with live control storage.
    let mut header = unsafe { libc::CMSG_FIRSTHDR(message) };
    while !header.is_null() {
        // SAFETY: libc locates kernel-written headers within msg_controllen.
        unsafe {
            let offset = header
                .cast::<u8>()
                .offset_from(message.msg_control.cast::<u8>()) as usize;
            let available = control_len(message) - offset;
            let reported = (*header).cmsg_len as usize;
            if (*header).cmsg_level == libc::SOL_SOCKET && (*header).cmsg_type == libc::SCM_RIGHTS {
                // macOS retains the original cmsg_len when only a prefix was reported.
                let bytes = reported.min(available) - libc::CMSG_LEN(0) as usize;
                let data = libc::CMSG_DATA(header).cast::<RawFd>();
                for index in 0..bytes / mem::size_of::<RawFd>() {
                    descriptors.push(OwnedFd::from_raw_fd(data.add(index).read_unaligned()));
                }
            }
            header = libc::CMSG_NXTHDR(message, header);
        }
    }
    descriptors
}

#[cfg(test)]
mod tests;
