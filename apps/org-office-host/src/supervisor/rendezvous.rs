//! Launch v3's per-spawn rendezvous (X-02 §5.1 steps 3, 4 and 7): how an admitted server gets
//! its keys and its transfer socket, never on a descriptor it inherited.
//!
//! 1. [`Rendezvous::bind`], only after the child is admitted: a listener at
//!    `<runtime>/r/<16 hex>`, in a 0700 folder of this user's (made if missing, refused if it
//!    is anything else), its name from 8 random bytes. The first line names it.
//! 2. [`Rendezvous::accept_one`]: exactly one connection by the deadline, then the listener is
//!    closed and unlinked, whatever happens next (a guard unlinks it on every path).
//! 3. [`authenticate`]: the peer's `LOCAL_PEERTOKEN` is the admitted token and the child's task
//!    token is unchanged; then one `ready_for_secrets` line,
//!    read a byte at a time so nothing past it is consumed, with nothing waiting after it
//!    (`FIONREAD` 0) and both tokens checked again (app_auth's rules).
//! 4. [`hand_off`]: the [`Handoff`] header, then for a start or a create one `sendmsg` of
//!    [`FD_MARKER`] with the worker's end of a new transfer socket. The worker's first event
//!    must be `fds_received {of: "handoff"}` within [`ACK_TIMEOUT`]; the host holds its copy of
//!    the passed end until then (§5.2 sender retention). A discard gets no socket, no marker,
//!    and owes no acknowledgement.
//!
//! Every error is a fixed reason, never a secret: the caller kills and reaps the child.

#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "wired into the spawn by river/host-launch-v3-life"
    )
)]

use std::fs::{self, DirBuilder, File};
use std::io::{self, BufReader, Read, Write};
use std::os::fd::AsFd;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::os::unix::net::{UnixDatagram, UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use org_office_launch::v3::{
    self, Handoff, ReadyForSecrets, RendezvousEvent, FD_MARKER, TRANSFER_SOCKET_BUFFER_BYTES,
};
use org_proto::location::check_socket_path;

use crate::cloexec::SPAWN_LOCK;
use crate::peer::{self, AuditToken};
use crate::sync::lock;

/// How long the host waits for a handoff's acknowledgement (X-02 §5.2).
pub(crate) const ACK_TIMEOUT: Duration = Duration::from_secs(5);

/// The longest `ready_for_secrets` line read (it is 37 bytes).
const READY_MAX_BYTES: usize = 64;

/// The longest single wait for a connection, so a child's exit is seen promptly.
const ACCEPT_POLL: Duration = Duration::from_millis(5);

/// One spawn's listener. Dropping it unlinks its path.
pub(crate) struct Rendezvous {
    listener: UnixListener,
    path: Unlink,
}

/// A path removed when dropped.
struct Unlink(PathBuf);

impl Drop for Unlink {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

impl Rendezvous {
    /// Listens on a fresh path under `runtime`'s owner-only `r` folder.
    pub(crate) fn bind(runtime: &Path) -> io::Result<Self> {
        let dir = runtime.join("r");
        match DirBuilder::new().mode(0o700).create(&dir) {
            Err(e) if e.kind() != io::ErrorKind::AlreadyExists => return Err(e),
            _ => {}
        }
        let meta = fs::symlink_metadata(&dir)?;
        if !meta.is_dir() || meta.uid() != crate::home::euid() || meta.mode() & 0o777 != 0o700 {
            return Err(io::Error::other(
                "the rendezvous folder isn't an owner-only folder of this user",
            ));
        }
        let mut name = [0u8; 8];
        File::open("/dev/urandom")?.read_exact(&mut name)?;
        let path = dir.join(name.iter().map(|b| format!("{b:02x}")).collect::<String>());
        check_socket_path(&path).map_err(io::Error::other)?;
        let listener = {
            // See `cloexec`: std marks the socket close-on-exec in a second step.
            let _no_spawn_meanwhile = lock(&SPAWN_LOCK);
            UnixListener::bind(&path)?
        };
        let rendezvous = Self {
            listener,
            path: Unlink(path),
        };
        rendezvous.listener.set_nonblocking(true)?;
        Ok(rendezvous)
    }

    /// The path the first line carries.
    pub(crate) fn path(&self) -> &Path {
        &self.path.0
    }

    /// Accepts exactly one connection by `deadline`, then closes and unlinks the listener (any
    /// other connection queued is closed unanswered). `Ok(None)`: the child exited first.
    pub(crate) fn accept_one(
        self,
        deadline: Instant,
        mut exited: impl FnMut() -> bool,
    ) -> Result<Option<UnixStream>, String> {
        loop {
            let accepted = {
                let _no_spawn_meanwhile = lock(&SPAWN_LOCK);
                self.listener.accept()
            };
            match accepted {
                Ok((stream, _)) => {
                    // macOS hands out accepted sockets non-blocking, like the listener.
                    stream
                        .set_nonblocking(false)
                        .map_err(|e| format!("its rendezvous connection: {e}"))?;
                    return Ok(Some(stream));
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(format!("couldn't accept on its rendezvous: {e}")),
            }
            if exited() {
                return Ok(None);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err("it didn't connect to its rendezvous in time".into());
            }
            let fd = std::os::fd::AsRawFd::as_raw_fd(&self.listener);
            let _ = org_socket::poll_readable(fd, left.min(ACCEPT_POLL));
        }
    }
}

/// The admitted child: its token at admission, and the re-check of its task token.
pub(crate) struct Admitted<'a> {
    pub(crate) token: AuditToken,
    pub(crate) still: &'a dyn Fn() -> Result<(), String>,
}

impl Admitted<'_> {
    fn check(&self, stream: &UnixStream) -> Result<(), String> {
        if peer_token(stream) != Some(self.token) {
            return Err("its rendezvous peer isn't the admitted server".into());
        }
        (self.still)()
    }
}

#[cfg(target_os = "macos")]
fn peer_token(stream: &UnixStream) -> Option<AuditToken> {
    peer::audit_token(stream).ok()
}

/// No `LOCAL_PEERTOKEN` here: every peer is refused (X-02 §5.1 step 5 is Linux's own design).
#[cfg(not(target_os = "macos"))]
fn peer_token(_stream: &UnixStream) -> Option<AuditToken> {
    None
}

/// Step 4 on the accepted connection: see the module docs.
pub(crate) fn authenticate(
    stream: &UnixStream,
    admitted: &Admitted<'_>,
    deadline: Instant,
) -> Result<(), String> {
    // Bytes may already wait here: the worker writes right after it connects. They can't
    // predate the checked image (this socket was made after the admission, and an exec since
    // changes the task token), and both tokens are checked again after the line.
    admitted.check(stream)?;
    let mut line = Vec::with_capacity(READY_MAX_BYTES);
    let mut byte = [0u8; 1];
    while line.len() < READY_MAX_BYTES {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err("no ready_for_secrets in time".into());
        }
        stream
            .set_read_timeout(Some(left))
            .map_err(|e| format!("its rendezvous connection: {e}"))?;
        match (&*stream).read(&mut byte) {
            Ok(0) => return Err("its rendezvous closed before ready_for_secrets".into()),
            Ok(_) if byte[0] == b'\n' => break,
            Ok(_) => line.push(byte[0]),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return Err("no ready_for_secrets in time".into()),
        }
    }
    if serde_json::from_slice::<ReadyForSecrets>(&line).is_err() {
        return Err("its first rendezvous line isn't ready_for_secrets".into());
    }
    if peer::bytes_waiting(stream).ok() != Some(0) {
        return Err("something followed its ready_for_secrets".into());
    }
    admitted.check(stream)
}

/// What a completed handoff leaves: the reader of the worker's events and, unless a discard,
/// the host's end of the transfer socket, held for the worker's whole life.
pub(crate) struct HandedOff {
    pub(crate) events: BufReader<UnixStream>,
    pub(crate) transfer: Option<UnixDatagram>,
}

/// Step 7: see the module docs. The handoff is consumed, so its keys are zeroed on every path.
pub(crate) fn hand_off(
    stream: UnixStream,
    handoff: Handoff,
    deadline: Instant,
) -> Result<HandedOff, String> {
    let pair = if handoff.fds().is_empty() {
        None
    } else {
        Some(transfer_pair().map_err(|e| format!("couldn't make its transfer socket: {e}"))?)
    };
    let mut header = v3::encode_handoff(&handoff).map_err(|_| "couldn't encode its handoff")?;
    drop(handoff);
    let left = deadline.saturating_duration_since(Instant::now());
    let written = stream
        .set_write_timeout(Some(left.max(Duration::from_millis(1))))
        .and_then(|()| (&stream).write_all(&header));
    header.fill(0);
    drop(header);
    written.map_err(|e| format!("couldn't send its handoff: {e}"))?;
    let mut events = BufReader::new(stream);
    let Some((host_end, worker_end)) = pair else {
        return Ok(HandedOff {
            events,
            transfer: None,
        });
    };
    let socket = events.get_ref().as_fd();
    org_socket::rights::send(socket, &[FD_MARKER], &[worker_end.as_fd()], deadline)
        .map_err(|e| format!("couldn't pass its transfer socket: {e}"))?;
    let stream = events.get_ref();
    stream
        .set_read_timeout(Some(ACK_TIMEOUT))
        .map_err(|e| format!("its rendezvous connection: {e}"))?;
    match v3::read_server_event(&mut events) {
        Ok(Some(RendezvousEvent::FdsReceived { of })) if of == "handoff" => {}
        _ => return Err("no fds_received for its handoff in time".into()),
    }
    // Retained until acknowledged (X-02 §5.2), released only now.
    drop(worker_end);
    // macOS refuses (EINVAL) a timeout change on a connection its peer already closed (a
    // worker that failed right after its receipt): its reader then just reads to the EOF.
    let _ = events.get_ref().set_read_timeout(None);
    Ok(HandedOff {
        events,
        transfer: Some(host_end),
    })
}

/// A transfer socket (X-02 §5.2): (the host's end, the worker's end), both with 64 KiB
/// buffers; the worker's end can't send.
fn transfer_pair() -> io::Result<(UnixDatagram, UnixDatagram)> {
    let (host, worker) = {
        // See `cloexec`: std marks the pair close-on-exec in a second step.
        let _no_spawn_meanwhile = lock(&SPAWN_LOCK);
        UnixDatagram::pair()?
    };
    for end in [&host, &worker] {
        org_socket::rights::set_buffer_sizes(end.as_fd(), TRANSFER_SOCKET_BUFFER_BYTES)?;
    }
    worker.shutdown(std::net::Shutdown::Write)?;
    Ok((host, worker))
}

#[cfg(test)]
mod tests;
