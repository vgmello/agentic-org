//! The key hand-off (launch version 3): how a server the host started gets its secrets, the
//! office's app token and database key, without them ever travelling on a descriptor it
//! inherited.
//!
//! Why: a same-user process can resume a suspended child before the host takes its token, let
//! its image fork a helper that keeps stdin (or a connection it made), and turn itself into the
//! genuine server, which then passes the host's check (see `spawn`'s module docs). So stdin
//! carries no secret, and the secrets go only to a connection the admitted server opens itself,
//! after its exec. Per spawn:
//!
//! 1. The child is spawned suspended, admitted (`server_check`), resumed and re-checked
//!    (`supervisor::child`).
//! 2. **Only then**, [`KeySocket::bind`] makes a fresh [`KeySocketId`] from 8 random bytes (another
//!    one if that name is taken) and listens on `<home>/run/k-<16 hex>.sock`
//!    ([`key_socket_path`]): checked against the 103-byte limit, in the 0700 `run/`, and 0600
//!    before it listens (`spawn::listen_owner_only`), so it is never wider. Its first line, which
//!    carries only the id, is written next. The server connects only after reading that line, so
//!    binding first can't deadlock.
//! 3. [`KeySocket::hand_off`] accepts **one** connection within the hand-off wait
//!    (`Timings::hand_off_timeout`, 10 s). At accept it samples the peer: its identity first (the
//!    audit token, or in debug builds its pid and uid), then the bytes it already sent. The peer
//!    must be the admitted one (`server_check::PeerRule`) and must have sent nothing. Then the
//!    **peer-fd check** (`peer_fds`): the admitted pid must hold the connection's other end on
//!    exactly one fd, close-on-exec, not 0-2, and not shared; then the peer is sampled again
//!    ([`PeerRule::still`]). Then the host writes the hand-off line (`write_hand_off`, which borrows the secrets and zeroes its
//!    encoded copy), half-closes its end and waits for the server to close its own (EOF; the
//!    server checks who its peer is, and a closed peer can't be asked), then **unlinks** the
//!    socket and closes the connection. A byte from the server, or no EOF in time, is a failure
//!    after the delivery. Any other connection already queued is closed with nothing sent, and
//!    logged; after the unlink, nobody can connect.
//! 4. A timeout, a refused peer or any error before the line: nothing is written and the socket
//!    is unlinked; the caller kills the child and reports `storage_error`. A failure after the
//!    line (the server sent bytes, or didn't close in time) is the one exception: the key was
//!    delivered, the error says so ([`HandOffError::delivered`]), and the child is killed too. A
//!    child that exits before connecting (or whose connection is refused once it has exited) got
//!    nothing either: its exit (and any `failed` line) is reported as usual.
//!
//! **Why the peer check means anything.** `LOCAL_PEERTOKEN` (and `LOCAL_PEERPID`) don't name the
//! process that made the connection: they name the process that last created, connected, sent
//! or received on it (the kernel's `last_pid`), looked up when asked. `fork`, `exec` and passing
//! the descriptor (`SCM_RIGHTS`) don't change it. So a connection made by the pre-exec image of
//! the admitted pid would show the genuine server's token once that pid exec'd it, while a
//! helper forked before the exec kept the descriptor. Three things rule that out: (i) the socket
//! exists only after the genuine image is admitted and re-checked, so no connection to it can
//! predate the exec; (ii) from `main` on, the server's first act is fd hygiene (the contract):
//! fds 0-2 must be pipes, every other inherited fd is closed, and its exception ports are reset,
//! so the only socket it touches is the one it creates itself; and (iii) the peer-fd check. (ii)
//! doesn't cover what runs before `main`: libobjc writes to fd 2 under `OBJC_HELP` (the hardened
//! runtime doesn't strip `OBJC_*`), and a write that fails still makes the writer a socket's last
//! user, with nothing sent. An early resumer that puts a socket on the genuine server's fd 2, keeps
//! a copy in a helper and connects it to the key socket would pass the token check; (iii) refuses
//! it, since that fd was inherited (it can't be close-on-exec) and is shared with the helper.
//! Debug builds' pid rule rests on the same facts. (A zombie peer, which `LOCAL_PEERPID` still
//! answers for, has no fds: it fails the peer-fd check, and the hand-off ends as the child's
//! exit.)
//!
//! The host also checks the socket's file: right after the bind and the chmod it `lstat`s the
//! node (a socket, ours, 0600) and keeps its `(dev, ino)`; after the accept it requires the same
//! node again, or nothing is written. That only **detects** a same-user process replacing the
//! socket: a server that connected to the replacement never reaches the host, which times out
//! and kills it. What keeps the server from using forged secrets is its own check of the host
//! (the contract: the peer of its connection must be the host). A swap in the instant between the
//! bind and that first `lstat` isn't detected at all (same user only; the server's check still
//! protects); the same holds for `spawn::listen_owner_only`, which, if its chmod or listen fails,
//! removes the path only while it is still the node it `lstat`ed right after the bind. The open residuals (the early resumer still picks the server's post-`main` launch
//! context and its Mach special ports, and the checks rely on the kernel's `last_pid` and libproc
//! semantics) and the release gate that closes them are in `org-office-launch`'s docs.
//!
//! The socket file never outlives its spawn: it is unlinked on every path, by a guard that also
//! runs on an early return or a panic. A host that dies mid-spawn leaves one behind; the next
//! host removes stale `k-*.sock` files at startup ([`remove_stale_key_sockets`]). Logs name
//! sockets by their id only (the id isn't secret).

use std::fs::{self, File};
use std::io::{self, Read};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use org_office_launch::{
    key_socket_path, write_hand_off, KeySocketId, LaunchError, KEY_SOCKET_PREFIX, KEY_SOCKET_SUFFIX,
};
use org_proto::frame::FrameError;
use org_proto::{AppToken, DbKey};

use crate::cloexec::SPAWN_LOCK;
use crate::host_log;
use crate::peer;
use crate::server_check::{PeerRule, PeerSample};
use crate::sync::lock;

/// How often the wait looks for a connection (and for the child's exit).
const ACCEPT_POLL: Duration = Duration::from_millis(5);

/// Connections the kernel queues before the host accepts one: room to see (and refuse) the
/// others.
const BACKLOG: u8 = 4;

/// How many fresh ids a bind tries when the name is already taken.
const BIND_ATTEMPTS: u32 = 3;

/// The shortest time the host gives the hand-off line's write, even near the deadline (the line
/// is small and fits the socket's buffer, so it doesn't block in practice).
const MIN_SEND_WAIT: Duration = Duration::from_secs(1);

/// What a spawn hands its server, borrowed: never copied.
#[derive(Clone, Copy)]
pub(crate) struct Secrets<'a> {
    pub(crate) app_token: &'a AppToken,
    /// `None` only for a discard of an office the host holds no key for.
    pub(crate) db_key: Option<&'a DbKey>,
}

/// Why a hand-off failed. Its text never holds a secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HandOffError {
    pub(crate) why: String,
    /// Whether the key had already gone out: the failure came after the line was written.
    pub(crate) delivered: bool,
}

impl HandOffError {
    fn before(why: String) -> Self {
        Self {
            why,
            delivered: false,
        }
    }

    fn after(why: String) -> Self {
        Self {
            why,
            delivered: true,
        }
    }
}

impl std::fmt::Display for HandOffError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.delivered {
            write!(f, "the key was delivered, then {}", self.why)
        } else {
            f.write_str(&self.why)
        }
    }
}

/// How a hand-off ended without an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HandOff {
    /// The admitted server took the secrets.
    Sent,
    /// The child exited before it connected: nothing was sent.
    Exited,
}

/// One spawn's key socket, listening. Dropping it (on any path) unlinks it.
pub(crate) struct KeySocket {
    listener: UnixListener,
    id: KeySocketId,
    file: Unlink,
    /// Tests only: a stand-in for the peer-fd scan (`None`: the real one).
    #[cfg(all(test, target_os = "macos"))]
    scan: Option<Scan>,
}

impl KeySocket {
    /// Listens on a fresh key socket in `home`'s `run/` folder, which must be the host's own:
    /// a real folder, ours, 0700 (`home::Layout::prepare` made it so).
    pub(crate) fn bind(home: &Path) -> io::Result<Self> {
        let mut attempts = 0;
        loop {
            attempts += 1;
            match Self::bind_new(home) {
                // A name already taken (a leftover, or a collision): a fresh id.
                Err(e) if e.kind() == io::ErrorKind::AddrInUse && attempts < BIND_ATTEMPTS => {}
                bound => return bound,
            }
        }
    }

    fn bind_new(home: &Path) -> io::Result<Self> {
        let id = new_id()?;
        let path = key_socket_path(home, &id);
        org_proto::location::check_socket_path(&path).map_err(|e| {
            io::Error::other(format!(
                "the key socket's path is {} bytes; macOS allows at most {}",
                e.len,
                org_proto::location::MAX_SOCKET_PATH_BYTES
            ))
        })?;
        owner_only_folder(path.parent().unwrap_or(home))?;
        let listener = {
            // See `cloexec`: the socket is made close-on-exec in a second step.
            let _no_spawn_meanwhile = lock(&SPAWN_LOCK);
            crate::spawn::listen_owner_only(&path, BACKLOG)?
        };
        // The node just made, as it must be: the host's socket, 0600. Its identity is kept, to
        // check after the accept that nobody replaced it, and before unlinking it (a same-user
        // swap between that check and the unlink isn't prevented).
        let node = match own_socket_node(&path, None) {
            Ok(node) => node,
            Err(why) => {
                // Not removed: what is there may not be ours. A leftover socket of ours is
                // removed at the next startup.
                host_log::log(&format!(
                    "key socket {id} isn't as it was bound ({why}); left in place"
                ));
                return Err(io::Error::other(why));
            }
        };
        let file = Unlink(Some((path, node)));
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            id,
            file,
            #[cfg(all(test, target_os = "macos"))]
            scan: None,
        })
    }

    /// The id the first line carries.
    pub(crate) fn id(&self) -> &KeySocketId {
        &self.id
    }

    #[cfg(test)]
    pub(crate) fn path(&self) -> &Path {
        &self.file.0.as_ref().expect("not unlinked yet").0
    }

    /// Hands `secrets` to the one connection the server makes, if `rule` admits its peer: see the
    /// module docs. Waits at most `wait`; `exited` says whether the child has exited (then the
    /// wait ends at once, with nothing sent). The socket is unlinked whatever the outcome. An
    /// error says why (never a secret) and whether the key had already been delivered
    /// ([`HandOffError::delivered`]: the server sent bytes, or didn't close in time, after the
    /// line); either way the caller kills the child.
    pub(crate) fn hand_off(
        mut self,
        rule: &PeerRule,
        wait: Duration,
        mut exited: impl FnMut() -> bool,
        secrets: Secrets<'_>,
    ) -> Result<HandOff, HandOffError> {
        let deadline = Instant::now() + wait;
        let stream = loop {
            match self.accept() {
                Ok(Some(stream)) => break stream,
                Ok(None) => {}
                Err(_) if exited() => return Ok(HandOff::Exited),
                Err(e) => {
                    return Err(HandOffError::before(format!(
                        "couldn't accept on its key socket: {e}"
                    )))
                }
            }
            if exited() {
                return Ok(HandOff::Exited);
            }
            if Instant::now() >= deadline {
                return Err(HandOffError::before(format!(
                    "it didn't connect to its key socket within {} ms",
                    wait.as_millis()
                )));
            }
            thread::sleep(ACCEPT_POLL);
        };
        // Still the node the host bound (a same-user process can replace the path with its own
        // socket: this detects it; the server's check of the host is what keeps it from using
        // forged secrets). Then the peer, while `stream` stays open (`admit`).
        let sent = match self.file.still_ours() {
            Err(why) => Err(format!("its key socket was replaced: {why}")),
            Ok(()) => match self.admit(rule, &stream) {
                Ok(()) => send(&stream, secrets, deadline),
                Err(why) => Err(format!("refused the connection to its key socket: {why}")),
            },
        };
        // Written: the host's end stays open until the server has closed its own, so the server
        // can still ask who its peer is (it can't once the peer has closed). Anything it sends
        // instead is a failure, though the secrets are out by then.
        let closed = match &sent {
            Ok(()) => await_close(&stream, deadline),
            Err(_) => Ok(()),
        };
        // Unlinked before the connection closes, sent or not.
        let unlinked = self.file.now();
        drop(stream);
        // After the unlink, so this sees every connection there will ever be: nobody can connect
        // to a socket without a path, and the listener closes when this returns.
        self.refuse_queued();
        if let Err(why) = sent {
            if exited() {
                // The child is gone: a peer that couldn't be sampled or wasn't the child got
                // nothing, and a write to the admitted server failed because it exited. Either
                // way the child's own exit (or its `failed` line) says what happened.
                host_log::log(&format!(
                    "key socket {}: {why}; the server had already exited",
                    self.id
                ));
                return Ok(HandOff::Exited);
            }
            return Err(HandOffError::before(why));
        }
        // The key is out: a failure now is one after the delivery.
        closed.map_err(HandOffError::after)?;
        if let Err(e) = unlinked {
            // The connection is done; a leftover file is removed at the next startup.
            host_log::log(&format!(
                "couldn't remove key socket {} after its hand-off: {e}",
                self.id
            ));
        }
        Ok(HandOff::Sent)
    }

    /// Whether the peer of `stream`, the one connection accepted, may take the secrets. Decided
    /// with `stream` open throughout (the kernel handles the fd check compares are reused once a
    /// socket is freed), before anything is written, in this order:
    /// 1. who it is, then what it already sent: `rule` (its audit token, or in debug builds its
    ///    pid and uid), and nothing waiting (`FIONREAD`);
    /// 2. how the admitted server holds its end ([`Self::holds_alone`]): on exactly one fd, its
    ///    own, close-on-exec, not 0-2, not shared;
    /// 3. who it is again, and still nothing waiting: [`PeerRule::still`] (in a release build the
    ///    admitted pid must also still be the admitted image), so the fds scanned were the
    ///    admitted server's and a peer that died or was replaced during the scan can't pass.
    fn admit(&self, rule: &PeerRule, stream: &UnixStream) -> Result<(), String> {
        rule.admits(&sample(rule, stream))?;
        self.holds_alone(rule.pid(), stream)?;
        rule.still(&sample(rule, stream))
            .map_err(|why| format!("after its descriptors were checked, {why}"))
    }

    /// The peer-fd check (`peer_fds`): process `pid`, the admitted server, must hold the other
    /// end of `stream` on exactly one fd, not 0, 1 or 2, close-on-exec, and not shared. An fd it
    /// inherited across `exec` can't be close-on-exec, and an unshared one has no other holder,
    /// so only the admitted image reads what the host writes. Any libproc error fails closed.
    #[cfg(target_os = "macos")]
    fn holds_alone(&self, pid: u32, stream: &UnixStream) -> Result<(), String> {
        use std::os::fd::AsFd;
        let pid = i32::try_from(pid).map_err(|_| "its pid is out of range".to_owned())?;
        let conn = crate::peer_fds::own_conn(stream.as_fd())
            .map_err(|e| format!("its connection couldn't be identified: {e}"))?;
        #[cfg(test)]
        if let Some(scan) = &self.scan {
            return holder_verdict((scan.0)(pid, &conn));
        }
        holder_verdict(crate::peer_fds::check_holder(pid, &conn))
    }

    #[cfg(not(target_os = "macos"))]
    fn holds_alone(&self, _pid: u32, _stream: &UnixStream) -> Result<(), String> {
        Err("its descriptors can't be checked on this platform".into())
    }

    /// Tests only: checks the holder with `scan` instead of libproc.
    #[cfg(all(test, target_os = "macos"))]
    fn scanning_with(
        mut self,
        scan: impl Fn(i32, &crate::peer_fds::Conn<'_>) -> io::Result<Result<i32, crate::peer_fds::Refusal>>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        self.scan = Some(Scan(std::sync::Arc::new(scan)));
        self
    }

    /// One connection, if one is waiting.
    fn accept(&self) -> io::Result<Option<UnixStream>> {
        // See `cloexec`: an accepted connection is made close-on-exec in a second step.
        let _no_spawn_meanwhile = lock(&SPAWN_LOCK);
        match self.listener.accept() {
            Ok((stream, _)) => Ok(Some(stream)),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    /// Closes every other connection already queued, with nothing sent, and logs each.
    fn refuse_queued(&self) {
        while let Ok(Some(stream)) = self.accept() {
            drop(stream);
            host_log::log(&format!(
                "refused another connection to key socket {}: only one is accepted; it got nothing",
                self.id
            ));
        }
    }
}

/// A scan's result as the hand-off's verdict: the refusal, or the error, says why (never a
/// secret).
#[cfg(target_os = "macos")]
fn holder_verdict(scan: io::Result<Result<i32, crate::peer_fds::Refusal>>) -> Result<(), String> {
    match scan {
        Ok(Ok(_fd)) => Ok(()),
        Ok(Err(refusal)) => Err(refusal.to_string()),
        Err(e) => Err(format!("its descriptors couldn't be checked: {e}")),
    }
}

/// Tests only: a stand-in for the holder scan.
#[cfg(all(test, target_os = "macos"))]
#[derive(Clone)]
#[allow(clippy::type_complexity)]
struct Scan(
    std::sync::Arc<
        dyn Fn(i32, &crate::peer_fds::Conn<'_>) -> io::Result<Result<i32, crate::peer_fds::Refusal>>
            + Send
            + Sync,
    >,
);

/// The key socket's file, to remove when dropped unless [`Unlink::now`] already did. It checks
/// that the path still names the node the host bound (`(dev, ino)`) before it unlinks; a
/// same-user swap between that check and the unlink isn't prevented.
struct Unlink(Option<(PathBuf, (u64, u64))>);

impl Unlink {
    fn now(&mut self) -> io::Result<()> {
        let Some((path, node)) = self.0.take() else {
            return Ok(());
        };
        if own_socket_node(&path, Some(node)).is_err() {
            // Gone already, or not ours any more: leave it.
            return Ok(());
        }
        match fs::remove_file(&path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// Whether the path still names the host's node, a socket of ours, 0600.
    fn still_ours(&self) -> Result<(), String> {
        match &self.0 {
            Some((path, node)) => own_socket_node(path, Some(*node)).map(drop),
            None => Err("already unlinked".into()),
        }
    }
}

/// `path`'s node (`lstat`, so never through a symlink) if it is a socket owned by us, mode 0600,
/// and, when `expected` is given, that same node (`(dev, ino)`); else why not.
fn own_socket_node(path: &Path, expected: Option<(u64, u64)>) -> Result<(u64, u64), String> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    let node = (meta.dev(), meta.ino());
    if !meta.file_type().is_socket() {
        Err("not a socket".into())
    } else if meta.uid() != crate::home::euid() {
        Err("another owner".into())
    } else if meta.permissions().mode() & 0o777 != 0o600 {
        Err("not 0600".into())
    } else if expected.is_some_and(|expected| expected != node) {
        Err("another file".into())
    } else {
        Ok(node)
    }
}

impl Drop for Unlink {
    fn drop(&mut self) {
        let _ = self.now();
    }
}

/// What the host reads about a connection's peer at accept, for `rule`: its identity first, then
/// the bytes it already sent (anything sent before the identity was read counts).
fn sample(rule: &PeerRule, stream: &UnixStream) -> PeerSample {
    let mut sample = PeerSample::default();
    match rule {
        PeerRule::Token { .. } => sample.token = token_of(stream),
        #[cfg(any(debug_assertions, test))]
        PeerRule::Pid { .. } => {
            sample.pid = pid_of(stream);
            sample.uid = peer::peer_uid(stream).ok();
        }
    }
    sample.waiting = peer::bytes_waiting(stream).ok();
    sample
}

#[cfg(target_os = "macos")]
fn token_of(stream: &UnixStream) -> Option<peer::AuditToken> {
    peer::audit_token(stream).ok()
}

#[cfg(not(target_os = "macos"))]
fn token_of(_stream: &UnixStream) -> Option<peer::AuditToken> {
    None
}

#[cfg(all(target_os = "macos", any(debug_assertions, test)))]
fn pid_of(stream: &UnixStream) -> Option<u32> {
    peer::pid(stream)
        .ok()
        .and_then(|pid| u32::try_from(pid).ok())
}

#[cfg(all(not(target_os = "macos"), any(debug_assertions, test)))]
fn pid_of(_stream: &UnixStream) -> Option<u32> {
    None
}

/// Writes the hand-off line, blocking, for at most what's left of the wait (and at least
/// [`MIN_SEND_WAIT`]). The error never holds a secret.
fn send(stream: &UnixStream, secrets: Secrets<'_>, deadline: Instant) -> Result<(), String> {
    let left = deadline
        .saturating_duration_since(Instant::now())
        .max(MIN_SEND_WAIT);
    // macOS hands out accepted sockets non-blocking, like the listener.
    stream
        .set_nonblocking(false)
        .and_then(|()| stream.set_write_timeout(Some(left)))
        .map_err(|e| format!("couldn't set up its key socket's connection: {e}"))?;
    let mut out = stream;
    write_hand_off(&mut out, secrets.app_token, secrets.db_key).map_err(|e| match e {
        LaunchError::Frame(FrameError::Io(e)) => format!("couldn't write its key: {e}"),
        _ => "couldn't encode its key".to_owned(),
    })
}

/// After the line: half-closes the host's end (`SHUT_WR`, so the server sees the line end) and
/// waits, at most what's left of the wait (and at least [`MIN_SEND_WAIT`]), for the server to
/// close its end: EOF. Polls with non-blocking reads, so it never sets a timeout on a socket whose
/// peer may already have closed (macOS refuses that). A reset counts as closed; a byte from the
/// server, or no EOF in time, is an error.
fn await_close(stream: &UnixStream, deadline: Instant) -> Result<(), String> {
    let until = deadline.max(Instant::now() + MIN_SEND_WAIT);
    // `ENOTCONN` here means the server has closed already: nothing to half-close.
    let _ = stream.shutdown(std::net::Shutdown::Write);
    stream
        .set_nonblocking(true)
        .map_err(|e| format!("couldn't wait for its key socket to close: {e}"))?;
    let mut byte = [0u8; 1];
    loop {
        match (&*stream).read(&mut byte) {
            Ok(0) => return Ok(()),
            Ok(_) => {
                return Err(
                    "it sent something on its key socket after the line (the key had \
                            gone out)"
                        .into(),
                )
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::ConnectionReset | io::ErrorKind::NotConnected
                ) =>
            {
                return Ok(())
            }
            Err(e) => return Err(format!("couldn't wait for its key socket to close: {e}")),
        }
        if Instant::now() >= until {
            return Err(
                "it didn't close its key socket in time after the line (the key had \
                        gone out)"
                    .into(),
            );
        }
        thread::sleep(ACCEPT_POLL);
    }
}

/// A fresh id: 8 bytes from `/dev/urandom`.
fn new_id() -> io::Result<KeySocketId> {
    let mut bytes = [0u8; 8];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(KeySocketId::generate_from(bytes))
}

/// Refuses a `run/` folder that isn't a real folder of ours, 0700.
fn owner_only_folder(dir: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(dir)?;
    if meta.file_type().is_dir()
        && meta.uid() == crate::home::euid()
        && meta.permissions().mode() & 0o777 == 0o700
    {
        Ok(())
    } else {
        Err(io::Error::other(
            "the office home's run folder isn't an owner-only folder of this user",
        ))
    }
}

/// Removes the key sockets a host that died mid-spawn left in `run` (`k-<16 hex>.sock`, and only
/// sockets), returning their ids. An entry it can't check or remove is logged and skipped. Run at
/// startup, under `host.lock`, before any spawn.
pub(crate) fn remove_stale_key_sockets(run: &Path) -> io::Result<Vec<KeySocketId>> {
    let mut removed = Vec::new();
    for entry in fs::read_dir(run)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                host_log::log(&format!("couldn't read an entry of the run folder: {e}"));
                continue;
            }
        };
        let name = entry.file_name();
        let Some(id) = name
            .to_str()
            .and_then(|name| name.strip_prefix(KEY_SOCKET_PREFIX))
            .and_then(|rest| rest.strip_suffix(KEY_SOCKET_SUFFIX))
            .and_then(|hex| KeySocketId::parse(hex).ok())
        else {
            continue;
        };
        let path = entry.path();
        let removal = fs::symlink_metadata(&path).and_then(|meta| {
            if meta.file_type().is_socket() {
                fs::remove_file(&path).map(|()| true)
            } else {
                Ok(false)
            }
        });
        match removal {
            Ok(true) => removed.push(id),
            Ok(false) => {}
            Err(e) => host_log::log(&format!("couldn't remove stale key socket {id}: {e}")),
        }
    }
    Ok(removed)
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    use std::io::{BufReader, Write};
    use std::os::unix::fs::DirBuilderExt;

    use org_office_launch::{read_hand_off, FirstLine};

    const HAND_OFF_WAIT: Duration = Duration::from_secs(10);

    /// An office home with its owner-only `run/`.
    fn home() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::DirBuilder::new()
            .mode(0o700)
            .create(dir.path().join("run"))
            .unwrap();
        dir
    }

    /// A key socket in `dir` whose peer-fd scan trusts the peer: this test process holds the
    /// client end itself, and every process spawned meanwhile by the other tests in this binary
    /// marks its fds shared for a moment (`peer_fds`'s docs), so a real scan of it would be
    /// flaky. The real scan is tested against real peer processes: `peer_fds`'s tests,
    /// `tests/handoff.rs` and `tests/binary.rs`; these tests check what the hand-off does with
    /// its verdict.
    fn bind(dir: &tempfile::TempDir) -> KeySocket {
        KeySocket::bind(dir.path())
            .unwrap()
            .scanning_with(|_, _| Ok(Ok(3)))
    }

    /// The rule that admits this test process (the peer of its own connections).
    fn me() -> PeerRule {
        PeerRule::Pid {
            pid: std::process::id(),
            uid: crate::home::euid(),
        }
    }

    fn secrets<'a>(token: &'a AppToken, key: Option<&'a DbKey>) -> Secrets<'a> {
        Secrets {
            app_token: token,
            db_key: key,
        }
    }

    fn never() -> bool {
        false
    }

    /// The server's side, on its own thread: reads the hand-off line from `client`, then closes
    /// it (the host waits for that close).
    fn server_reads(
        client: UnixStream,
        first: FirstLine,
    ) -> thread::JoinHandle<Result<org_office_launch::HandOff, LaunchError>> {
        thread::spawn(move || read_hand_off(&mut BufReader::new(client), first))
    }

    /// Everything a connection gets until EOF.
    fn read_all(mut stream: UnixStream) -> Vec<u8> {
        // macOS refuses (EINVAL) a timeout on a socket whose peer already closed: then the read
        // below ends at once anyway.
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut got = Vec::new();
        stream.read_to_end(&mut got).unwrap();
        got
    }

    #[test]
    fn the_admitted_peer_gets_the_secrets_and_the_socket_is_gone() {
        let dir = home();
        let socket = bind(&dir);
        let path = socket.path().to_owned();
        assert_eq!(path, key_socket_path(dir.path(), socket.id()));
        // 0600 while it exists, in the 0700 run/.
        let meta = fs::symlink_metadata(&path).unwrap();
        assert!(meta.file_type().is_socket());
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        let client = server_reads(UnixStream::connect(&path).unwrap(), FirstLine::Start);
        let (token, key) = (AppToken::from_bytes([1; 32]), DbKey::from_bytes([2; 32]));
        let outcome = socket.hand_off(&me(), HAND_OFF_WAIT, never, secrets(&token, Some(&key)));
        assert_eq!(outcome, Ok(HandOff::Sent));
        assert!(!path.exists(), "the socket outlived its hand-off");
        let hand_off = client.join().unwrap().unwrap();
        assert!(hand_off.app_token == token);
        assert!(hand_off.db_key == Some(key));
    }

    #[test]
    fn a_discard_without_a_key_gets_null() {
        let dir = home();
        let socket = bind(&dir);
        let client = server_reads(
            UnixStream::connect(socket.path()).unwrap(),
            FirstLine::Discard,
        );
        let token = AppToken::from_bytes([1; 32]);
        let outcome = socket.hand_off(&me(), HAND_OFF_WAIT, never, secrets(&token, None));
        assert_eq!(outcome, Ok(HandOff::Sent));
        let hand_off = client.join().unwrap().unwrap();
        assert!(hand_off.db_key.is_none());
    }

    /// A peer the rule doesn't admit (here: any pid but ours) gets nothing, and the socket goes.
    #[test]
    fn a_peer_that_isnt_the_child_gets_nothing() {
        let dir = home();
        let socket = bind(&dir);
        let path = socket.path().to_owned();
        let client = UnixStream::connect(&path).unwrap();
        let rule = PeerRule::Pid {
            pid: std::process::id() + 1,
            uid: crate::home::euid(),
        };
        let token = AppToken::from_bytes([1; 32]);
        let err = socket
            .hand_off(&rule, HAND_OFF_WAIT, never, secrets(&token, None))
            .unwrap_err();
        assert!(err.why.contains("another pid"), "{err}");
        assert_eq!(read_all(client), b"", "a refused peer got something");
        assert!(!path.exists());
    }

    /// A connection made by another process and passed to the admitted one (`SCM_RIGHTS`) is
    /// still that other process's: refused, and nothing reaches the process that holds it now.
    /// (A connection made before the admission can't exist at all: the socket is bound only
    /// after it, `supervisor`'s `no_key_socket_exists_until_the_child_is_admitted_and_rechecked`.)
    #[test]
    fn a_connection_passed_from_another_process_gets_nothing() {
        let dir = home();
        let socket = bind(&dir);
        let path = socket.path().to_owned();
        let (passed, connector) = crate::peer::connection_made_elsewhere(&path).unwrap();
        assert_ne!(connector as u32, std::process::id());
        // This process (the "admitted" one under `me()`) now holds the only copy, untouched.
        let token = AppToken::from_bytes([1; 32]);
        let err = socket
            .hand_off(&me(), HAND_OFF_WAIT, never, secrets(&token, None))
            .unwrap_err();
        assert!(err.why.contains("another pid"), "{err}");
        assert_eq!(read_all(UnixStream::from(passed)), b"");
        assert!(!path.exists());
    }

    /// A refused peer while the child has already exited: nothing sent, and reported as the
    /// child's exit.
    #[test]
    fn a_refused_peer_after_the_child_exited_is_its_exit() {
        let dir = home();
        let socket = bind(&dir);
        let client = UnixStream::connect(socket.path()).unwrap();
        let rule = PeerRule::Pid {
            pid: std::process::id() + 1,
            uid: crate::home::euid(),
        };
        let token = AppToken::from_bytes([1; 32]);
        let outcome = socket.hand_off(&rule, HAND_OFF_WAIT, || true, secrets(&token, None));
        assert_eq!(outcome, Ok(HandOff::Exited));
        assert_eq!(read_all(client), b"");
    }

    /// The admitted server connects and exits before the line is written: the failed write is
    /// its exit, not a hand-off failure.
    #[test]
    fn an_admitted_peer_that_exits_before_the_write_is_its_exit() {
        let dir = home();
        let socket = bind(&dir);
        let client = UnixStream::connect(socket.path()).unwrap();
        // Its end is closed and its receive side shut: the host's write fails.
        drop(client);
        thread::sleep(Duration::from_millis(50));
        let token = AppToken::from_bytes([1; 32]);
        let outcome = socket.hand_off(&me(), HAND_OFF_WAIT, || true, secrets(&token, None));
        assert_eq!(outcome, Ok(HandOff::Exited));
    }

    /// A peer that holds its end badly (any refusal of the peer-fd check) gets nothing: the line
    /// isn't written, the socket goes, and the error says why.
    #[test]
    fn a_peer_the_fd_check_refuses_gets_nothing() {
        use crate::peer_fds::Refusal;
        for refusal in [
            Refusal::NoMatch,
            Refusal::SeveralMatches(vec![3, 9]),
            Refusal::StdFd(2),
            Refusal::NotCloexec(5),
            Refusal::Shared(5),
        ] {
            let dir = home();
            let returned = refusal.clone();
            let socket = KeySocket::bind(dir.path())
                .unwrap()
                .scanning_with(move |_, _| Ok(Err(returned.clone())));
            let path = socket.path().to_owned();
            let client = UnixStream::connect(&path).unwrap();
            let token = AppToken::from_bytes([1; 32]);
            let err = socket
                .hand_off(&me(), HAND_OFF_WAIT, never, secrets(&token, None))
                .unwrap_err();
            assert!(err.why.contains(&refusal.to_string()), "{err}");
            assert!(!err.delivered, "{err:?}");
            assert_eq!(read_all(client), b"", "{refusal:?}: the peer got something");
            assert!(!path.exists());
        }
    }

    /// A scan that can't be done (libproc refuses, or the process is gone) fails closed.
    #[test]
    fn a_failed_fd_check_fails_closed() {
        let dir = home();
        let socket = KeySocket::bind(dir.path())
            .unwrap()
            .scanning_with(|_, _| Err(io::Error::from_raw_os_error(libc::EPERM)));
        let client = UnixStream::connect(socket.path()).unwrap();
        let token = AppToken::from_bytes([1; 32]);
        let err = socket
            .hand_off(&me(), HAND_OFF_WAIT, never, secrets(&token, None))
            .unwrap_err();
        assert!(err.why.contains("couldn't be checked"), "{err}");
        assert_eq!(read_all(client), b"");
    }

    /// The scan is of the admitted pid, and runs after the identity check: a peer that isn't the
    /// admitted one is refused without a scan.
    #[test]
    fn the_fd_check_scans_the_admitted_pid() {
        let dir = home();
        let scanned = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = scanned.clone();
        let socket = KeySocket::bind(dir.path())
            .unwrap()
            .scanning_with(move |pid, _| {
                seen.lock().unwrap().push(pid);
                Ok(Ok(3))
            });
        let client = server_reads(
            UnixStream::connect(socket.path()).unwrap(),
            FirstLine::Discard,
        );
        let token = AppToken::from_bytes([1; 32]);
        let outcome = socket.hand_off(&me(), HAND_OFF_WAIT, never, secrets(&token, None));
        assert_eq!(outcome, Ok(HandOff::Sent));
        assert!(client.join().unwrap().is_ok());
        assert_eq!(*scanned.lock().unwrap(), [std::process::id() as i32]);

        let dir = home();
        let socket = KeySocket::bind(dir.path())
            .unwrap()
            .scanning_with(|_, _| panic!("scanned a peer that isn't the admitted one"));
        let _client = UnixStream::connect(socket.path()).unwrap();
        let rule = PeerRule::Pid {
            pid: std::process::id() + 1,
            uid: crate::home::euid(),
        };
        let err = socket
            .hand_off(&rule, HAND_OFF_WAIT, never, secrets(&token, None))
            .unwrap_err();
        assert!(err.why.contains("another pid"), "{err}");
    }

    /// The peer is sampled again after the scan: one that sent something meanwhile is refused.
    #[test]
    fn the_peer_is_checked_again_after_the_fd_check() {
        let dir = home();
        let socket_dir = dir.path().to_owned();
        let socket = KeySocket::bind(&socket_dir).unwrap();
        let client = UnixStream::connect(socket.path()).unwrap();
        let writer = client.try_clone().unwrap();
        let socket = socket.scanning_with(move |_, _| {
            (&writer).write_all(b"x").unwrap();
            thread::sleep(Duration::from_millis(50));
            Ok(Ok(3))
        });
        let token = AppToken::from_bytes([1; 32]);
        let err = socket
            .hand_off(&me(), HAND_OFF_WAIT, never, secrets(&token, None))
            .unwrap_err();
        assert!(
            err.why.contains("after its descriptors were checked"),
            "{err}"
        );
        assert!(err.why.contains("sent something"), "{err}");
        assert_eq!(read_all(client), b"");
    }

    /// The real scan (libproc), on a refusal no other test can disturb: this process holds the
    /// client end on two fds.
    #[test]
    fn a_peer_holding_its_end_on_two_fds_gets_nothing() {
        let dir = home();
        let socket = KeySocket::bind(dir.path()).unwrap();
        let path = socket.path().to_owned();
        let client = UnixStream::connect(&path).unwrap();
        let copy = client.try_clone().unwrap();
        let token = AppToken::from_bytes([1; 32]);
        let err = socket
            .hand_off(&me(), HAND_OFF_WAIT, never, secrets(&token, None))
            .unwrap_err();
        assert!(err.why.contains("several fds"), "{err}");
        drop(copy);
        assert_eq!(read_all(client), b"");
        assert!(!path.exists());
    }

    /// Tells [`hand_off_peer_entry`] to run.
    const PEER_ENV: &str = "ORG_HANDOFF_TEST_PEER";

    /// The entry point of the peer process [`two_hundred_hand_offs_to_a_real_peer`] spawns (does
    /// nothing otherwise): for each socket path on stdin, it connects (one close-on-exec fd, as
    /// the contract says), reads the line to EOF, closes, and answers `PEERDONE <bytes>`.
    #[test]
    #[ignore = "the entry point of a peer process another test spawns"]
    fn hand_off_peer_entry() {
        if std::env::var_os(PEER_ENV).is_none() {
            return;
        }
        let mut out = std::io::stdout();
        for path in std::io::stdin().lines() {
            let mut stream = UnixStream::connect(path.unwrap()).unwrap();
            let mut line = Vec::new();
            stream.read_to_end(&mut line).unwrap();
            drop(stream);
            writeln!(out, "PEERDONE {}", line.len()).unwrap();
            out.flush().unwrap();
        }
    }

    /// 200 sequential hand-offs to a real peer process, with the real peer-fd check: each is
    /// `Sent`. Prints p50 and p99 of the check (`check_holder`) and of the whole hand-off (run
    /// with `--nocapture`).
    #[test]
    fn two_hundred_hand_offs_to_a_real_peer() {
        use std::io::BufRead;
        use std::process::{Command, Stdio};
        let mut peer = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "handoff::tests::hand_off_peer_entry",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(PEER_ENV, "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let rule = PeerRule::Pid {
            pid: peer.id(),
            uid: crate::home::euid(),
        };
        let mut to_peer = peer.stdin.take().unwrap();
        let mut from_peer = BufReader::new(peer.stdout.take().unwrap());
        let checks = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut hand_offs = Vec::new();
        let token = AppToken::from_bytes([1; 32]);
        for _ in 0..200 {
            let dir = home();
            let timed = checks.clone();
            let socket = KeySocket::bind(dir.path())
                .unwrap()
                .scanning_with(move |pid, conn| {
                    let start = Instant::now();
                    let verdict = crate::peer_fds::check_holder(pid, conn);
                    timed.lock().unwrap().push(start.elapsed());
                    verdict
                });
            writeln!(to_peer, "{}", socket.path().display()).unwrap();
            let start = Instant::now();
            let outcome = socket.hand_off(&rule, HAND_OFF_WAIT, never, secrets(&token, None));
            hand_offs.push(start.elapsed());
            assert_eq!(outcome, Ok(HandOff::Sent));
            // Past the test harness's own output (it may share the line).
            let mut line = String::new();
            let got = loop {
                line.clear();
                assert!(
                    from_peer.read_line(&mut line).unwrap() > 0,
                    "the peer exited"
                );
                if let Some(got) = line.split("PEERDONE ").nth(1) {
                    break got.trim_end().to_owned();
                }
            };
            assert_ne!(got, "0", "the peer got nothing");
        }
        drop(to_peer);
        let _ = peer.kill();
        let _ = peer.wait();
        let mut checks = checks.lock().unwrap().clone();
        assert_eq!(checks.len(), 200);
        checks.sort();
        hand_offs.sort();
        println!(
            "200 hand-offs: the check p50 {:?}, p99 {:?}, max {:?}; the whole hand-off p50 {:?}, \
             p99 {:?}",
            checks[99], checks[197], checks[199], hand_offs[99], hand_offs[197]
        );
    }

    /// A same-user process that replaces the key socket's path between the bind and the accept
    /// is detected: nothing is written, even to a connection made to the host's own socket, and
    /// the host leaves the replacement alone. (Detection only: a server that connected to the
    /// replacement never reaches the host; the server's own check of the host stops it using
    /// forged secrets.)
    #[test]
    fn a_key_socket_replaced_before_the_accept_gets_nothing() {
        let dir = home();
        let socket = bind(&dir);
        let path = socket.path().to_owned();
        let client = UnixStream::connect(&path).unwrap();
        fs::remove_file(&path).unwrap();
        let impostor = UnixListener::bind(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let token = AppToken::from_bytes([1; 32]);
        let err = socket
            .hand_off(&me(), HAND_OFF_WAIT, never, secrets(&token, None))
            .unwrap_err();
        assert!(err.why.contains("replaced"), "{err}");
        assert_eq!(
            read_all(client),
            b"",
            "a secret went out on a replaced socket"
        );
        assert!(path.exists(), "the host removed a node it didn't bind");
        drop(impostor);
    }

    /// The host keeps its end open until the server has closed its own: a server that asks who
    /// its peer is a while after the line arrived can still be answered (a closed peer can't be
    /// asked: `ENOTCONN`).
    #[test]
    fn the_host_end_stays_open_until_the_server_closes() {
        let dir = home();
        let socket = bind(&dir);
        let client = UnixStream::connect(socket.path()).unwrap();
        let server = thread::spawn(move || {
            let hand_off = read_hand_off(&mut BufReader::new(&client), FirstLine::Discard);
            thread::sleep(Duration::from_millis(300));
            let peer = crate::peer::pid(&client).map(|pid| pid as u32);
            (hand_off.is_ok(), peer)
        });
        let token = AppToken::from_bytes([1; 32]);
        let outcome = socket.hand_off(&me(), HAND_OFF_WAIT, never, secrets(&token, None));
        assert_eq!(outcome, Ok(HandOff::Sent));
        let (read, peer) = server.join().unwrap();
        assert!(read);
        assert_eq!(
            peer.ok(),
            Some(std::process::id()),
            "the host's end was gone"
        );
    }

    /// A server that sends anything on the key socket fails the hand-off (the key is out by then,
    /// so the caller kills it).
    #[test]
    fn a_server_that_answers_on_the_key_socket_fails() {
        let dir = home();
        let socket = bind(&dir);
        let mut client = UnixStream::connect(socket.path()).unwrap();
        let server = thread::spawn(move || {
            let mut reader = BufReader::new(client.try_clone().unwrap());
            let read = read_hand_off(&mut reader, FirstLine::Discard).is_ok();
            client.write_all(b"ack").unwrap();
            thread::sleep(Duration::from_millis(300));
            read
        });
        let token = AppToken::from_bytes([1; 32]);
        let err = socket
            .hand_off(&me(), HAND_OFF_WAIT, never, secrets(&token, None))
            .unwrap_err();
        assert!(err.why.contains("sent something"), "{err}");
        // After the line: the error says the key went out, never that it got nothing.
        assert!(err.delivered, "{err:?}");
        assert!(
            err.to_string().starts_with("the key was delivered, then"),
            "{err}"
        );
        assert!(server.join().unwrap());
    }

    /// A server that keeps the key socket open after the line fails the hand-off too, also as a
    /// failure after the delivery.
    #[test]
    fn a_server_that_doesnt_close_the_key_socket_fails_after_the_delivery() {
        let dir = home();
        let socket = bind(&dir);
        let client = UnixStream::connect(socket.path()).unwrap();
        let server = thread::spawn(move || {
            let read = read_hand_off(&mut BufReader::new(&client), FirstLine::Discard).is_ok();
            thread::sleep(Duration::from_millis(1500));
            read
        });
        let token = AppToken::from_bytes([1; 32]);
        let err = socket
            .hand_off(
                &me(),
                Duration::from_millis(200),
                never,
                secrets(&token, None),
            )
            .unwrap_err();
        assert!(err.why.contains("didn't close"), "{err}");
        assert!(err.delivered, "{err:?}");
        assert!(server.join().unwrap());
    }

    /// A peer that wrote before it was checked is refused, even with the right identity.
    #[test]
    fn a_peer_that_sent_something_first_gets_nothing() {
        let dir = home();
        let socket = bind(&dir);
        let mut client = UnixStream::connect(socket.path()).unwrap();
        client.write_all(b"hi").unwrap();
        thread::sleep(Duration::from_millis(50));
        let token = AppToken::from_bytes([1; 32]);
        let err = socket
            .hand_off(&me(), HAND_OFF_WAIT, never, secrets(&token, None))
            .unwrap_err();
        assert!(err.why.contains("sent something"), "{err}");
        assert!(!err.delivered, "{err:?}");
        assert_eq!(read_all(client), b"");
    }

    /// Only the first connection gets the secrets; one queued behind it is closed with nothing
    /// sent; and once the hand-off is done, nobody can connect.
    #[test]
    fn a_second_connection_gets_nothing() {
        let dir = home();
        let socket = bind(&dir);
        let path = socket.path().to_owned();
        let first = server_reads(UnixStream::connect(&path).unwrap(), FirstLine::Create);
        let second = UnixStream::connect(&path).unwrap();
        let token = AppToken::from_bytes([1; 32]);
        let key = DbKey::from_bytes([2; 32]);
        let outcome = socket.hand_off(&me(), HAND_OFF_WAIT, never, secrets(&token, Some(&key)));
        assert_eq!(outcome, Ok(HandOff::Sent));
        assert!(first.join().unwrap().is_ok());
        assert_eq!(read_all(second), b"", "a second connection got something");
        assert!(UnixStream::connect(&path).is_err());
    }

    #[test]
    fn no_connection_in_time_is_an_error_and_the_socket_goes() {
        let dir = home();
        let socket = bind(&dir);
        let path = socket.path().to_owned();
        let token = AppToken::from_bytes([1; 32]);
        let started = Instant::now();
        let err = socket
            .hand_off(
                &me(),
                Duration::from_millis(100),
                never,
                secrets(&token, None),
            )
            .unwrap_err();
        assert!(err.why.contains("didn't connect"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(!path.exists());
    }

    #[test]
    fn a_child_that_exits_first_ends_the_wait_with_nothing_sent() {
        let dir = home();
        let socket = bind(&dir);
        let path = socket.path().to_owned();
        let token = AppToken::from_bytes([1; 32]);
        let outcome = socket.hand_off(&me(), HAND_OFF_WAIT, || true, secrets(&token, None));
        assert_eq!(outcome, Ok(HandOff::Exited));
        assert!(!path.exists());
    }

    /// The guard: dropped unused (an early return, a failed spawn, a panic), it unlinks.
    #[test]
    fn a_socket_dropped_unused_is_unlinked() {
        let dir = home();
        let socket = bind(&dir);
        let path = socket.path().to_owned();
        assert!(path.exists());
        drop(socket);
        assert!(!path.exists());
        let socket = bind(&dir);
        let path = socket.path().to_owned();
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _held = socket;
            panic!("mid-spawn");
        }));
        assert!(unwound.is_err());
        assert!(!path.exists(), "a panic left the socket behind");
    }

    #[test]
    fn ids_are_fresh_and_a_run_folder_that_isnt_owner_only_is_refused() {
        let dir = home();
        let (a, b) = (
            KeySocket::bind(dir.path()).unwrap(),
            KeySocket::bind(dir.path()).unwrap(),
        );
        assert_ne!(a.id(), b.id());
        fs::set_permissions(dir.path().join("run"), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(KeySocket::bind(dir.path()).is_err());
        // No run/ at all.
        let bare = tempfile::tempdir().unwrap();
        assert!(KeySocket::bind(bare.path()).is_err());
    }

    #[test]
    fn stale_key_sockets_are_removed_and_nothing_else() {
        let dir = home();
        let run = dir.path().join("run");
        let stale = KeySocketId::generate_from([0xab; 8]);
        let stale_path = key_socket_path(dir.path(), &stale);
        // Dropping the listener leaves its file behind.
        drop(UnixListener::bind(&stale_path).unwrap());
        // Not ours to remove: a file with a key socket's name, and another socket.
        let file = key_socket_path(dir.path(), &KeySocketId::generate_from([0xcd; 8]));
        fs::write(&file, b"not a socket").unwrap();
        let other = run.join("01AAAAAAAAAA.sock");
        drop(UnixListener::bind(&other).unwrap());
        let odd = run.join("k-NOTHEX.sock");
        drop(UnixListener::bind(&odd).unwrap());
        assert_eq!(remove_stale_key_sockets(&run).unwrap(), [stale]);
        assert!(!stale_path.exists());
        assert!(file.exists() && other.exists() && odd.exists());
    }
}
