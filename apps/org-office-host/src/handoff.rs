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
//!    host writes the hand-off line (`write_hand_off`, which borrows the secrets and zeroes its
//!    encoded copy), **unlinks** the socket, and closes the connection. Any other connection
//!    already queued is closed with nothing sent, and logged; after the unlink, nobody can
//!    connect.
//! 4. A timeout, a refused peer or any error: nothing is written and the socket is unlinked; the
//!    caller kills the child and reports `storage_error`. A child that exits before connecting
//!    (or whose connection is refused once it has exited) got nothing either: its exit (and any
//!    `failed` line) is reported as usual.
//!
//! **Why the peer check means anything.** `LOCAL_PEERTOKEN` (and `LOCAL_PEERPID`) don't name the
//! process that made the connection: they name the process that last created, connected, sent
//! or received on it (the kernel's `last_pid`), looked up when asked. `fork`, `exec` and passing
//! the descriptor (`SCM_RIGHTS`) don't change it. So a connection made by the pre-exec image of
//! the admitted pid would show the genuine server's token once that pid exec'd it, while a
//! helper forked before the exec kept the descriptor. Two things rule that out: (i) the socket
//! exists only after the genuine image is admitted and re-checked, so no connection to it can
//! predate the exec; and (ii) the server's first act is fd hygiene (the contract): fds 0-2 must
//! be pipes, every other inherited fd is closed, and its exception ports are reset, so the only
//! socket it ever touches is the one it creates itself. The admitted pid's `last_pid` can then
//! only be on its own connection. Debug builds' pid rule rests on the same two facts. The open
//! residuals (the early resumer still picks the server's launch context, stdio pipes and Mach
//! special ports) and the release gate that closes them are in `org-office-launch`'s docs.
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
        let file = Unlink(Some(path));
        listener.set_nonblocking(true)?;
        Ok(Self { listener, id, file })
    }

    /// The id the first line carries.
    pub(crate) fn id(&self) -> &KeySocketId {
        &self.id
    }

    #[cfg(test)]
    pub(crate) fn path(&self) -> &Path {
        self.file.0.as_deref().expect("not unlinked yet")
    }

    /// Hands `secrets` to the one connection the server makes, if `rule` admits its peer: see the
    /// module docs. Waits at most `wait`; `exited` says whether the child has exited (then the
    /// wait ends at once, with nothing sent). The socket is unlinked whatever the outcome. An
    /// error is why nothing was sent (never a secret): the caller kills the child.
    pub(crate) fn hand_off(
        mut self,
        rule: &PeerRule,
        wait: Duration,
        mut exited: impl FnMut() -> bool,
        secrets: Secrets<'_>,
    ) -> Result<HandOff, String> {
        let deadline = Instant::now() + wait;
        let stream = loop {
            match self.accept() {
                Ok(Some(stream)) => break stream,
                Ok(None) => {}
                Err(_) if exited() => return Ok(HandOff::Exited),
                Err(e) => return Err(format!("couldn't accept on its key socket: {e}")),
            }
            if exited() {
                return Ok(HandOff::Exited);
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "it didn't connect to its key socket within {} ms",
                    wait.as_millis()
                ));
            }
            thread::sleep(ACCEPT_POLL);
        };
        let admitted = rule.admits(&sample(rule, &stream));
        let sent = match admitted {
            Ok(()) => send(&stream, secrets, deadline),
            Err(why) => Err(format!("refused the connection to its key socket: {why}")),
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
            return Err(why);
        }
        if let Err(e) = unlinked {
            // The connection is done; a leftover file is removed at the next startup.
            host_log::log(&format!(
                "couldn't remove key socket {} after its hand-off: {e}",
                self.id
            ));
        }
        Ok(HandOff::Sent)
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

/// A file to remove: when dropped, unless [`Unlink::now`] already did.
struct Unlink(Option<PathBuf>);

impl Unlink {
    fn now(&mut self) -> io::Result<()> {
        match self.0.take().map(fs::remove_file) {
            Some(Err(e)) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
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
        let socket = KeySocket::bind(dir.path()).unwrap();
        let path = socket.path().to_owned();
        assert_eq!(path, key_socket_path(dir.path(), socket.id()));
        // 0600 while it exists, in the 0700 run/.
        let meta = fs::symlink_metadata(&path).unwrap();
        assert!(meta.file_type().is_socket());
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        let client = UnixStream::connect(&path).unwrap();
        let (token, key) = (AppToken::from_bytes([1; 32]), DbKey::from_bytes([2; 32]));
        let outcome = socket.hand_off(&me(), HAND_OFF_WAIT, never, secrets(&token, Some(&key)));
        assert_eq!(outcome, Ok(HandOff::Sent));
        assert!(!path.exists(), "the socket outlived its hand-off");
        let hand_off = read_hand_off(&mut BufReader::new(client), FirstLine::Start).unwrap();
        assert!(hand_off.app_token == token);
        assert!(hand_off.db_key == Some(key));
    }

    #[test]
    fn a_discard_without_a_key_gets_null() {
        let dir = home();
        let socket = KeySocket::bind(dir.path()).unwrap();
        let client = UnixStream::connect(socket.path()).unwrap();
        let token = AppToken::from_bytes([1; 32]);
        let outcome = socket.hand_off(&me(), HAND_OFF_WAIT, never, secrets(&token, None));
        assert_eq!(outcome, Ok(HandOff::Sent));
        let hand_off = read_hand_off(&mut BufReader::new(client), FirstLine::Discard).unwrap();
        assert!(hand_off.db_key.is_none());
    }

    /// A peer the rule doesn't admit (here: any pid but ours) gets nothing, and the socket goes.
    #[test]
    fn a_peer_that_isnt_the_child_gets_nothing() {
        let dir = home();
        let socket = KeySocket::bind(dir.path()).unwrap();
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
        assert!(err.contains("another pid"), "{err}");
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
        let socket = KeySocket::bind(dir.path()).unwrap();
        let path = socket.path().to_owned();
        let (passed, connector) = crate::peer::connection_made_elsewhere(&path).unwrap();
        assert_ne!(connector as u32, std::process::id());
        // This process (the "admitted" one under `me()`) now holds the only copy, untouched.
        let token = AppToken::from_bytes([1; 32]);
        let err = socket
            .hand_off(&me(), HAND_OFF_WAIT, never, secrets(&token, None))
            .unwrap_err();
        assert!(err.contains("another pid"), "{err}");
        assert_eq!(read_all(UnixStream::from(passed)), b"");
        assert!(!path.exists());
    }

    /// A refused peer while the child has already exited: nothing sent, and reported as the
    /// child's exit.
    #[test]
    fn a_refused_peer_after_the_child_exited_is_its_exit() {
        let dir = home();
        let socket = KeySocket::bind(dir.path()).unwrap();
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
        let socket = KeySocket::bind(dir.path()).unwrap();
        let client = UnixStream::connect(socket.path()).unwrap();
        // Its end is closed and its receive side shut: the host's write fails.
        drop(client);
        thread::sleep(Duration::from_millis(50));
        let token = AppToken::from_bytes([1; 32]);
        let outcome = socket.hand_off(&me(), HAND_OFF_WAIT, || true, secrets(&token, None));
        assert_eq!(outcome, Ok(HandOff::Exited));
    }

    /// A peer that wrote before it was checked is refused, even with the right identity.
    #[test]
    fn a_peer_that_sent_something_first_gets_nothing() {
        let dir = home();
        let socket = KeySocket::bind(dir.path()).unwrap();
        let mut client = UnixStream::connect(socket.path()).unwrap();
        client.write_all(b"hi").unwrap();
        thread::sleep(Duration::from_millis(50));
        let token = AppToken::from_bytes([1; 32]);
        let err = socket
            .hand_off(&me(), HAND_OFF_WAIT, never, secrets(&token, None))
            .unwrap_err();
        assert!(err.contains("sent something"), "{err}");
        assert_eq!(read_all(client), b"");
    }

    /// Only the first connection gets the secrets; one queued behind it is closed with nothing
    /// sent; and once the hand-off is done, nobody can connect.
    #[test]
    fn a_second_connection_gets_nothing() {
        let dir = home();
        let socket = KeySocket::bind(dir.path()).unwrap();
        let path = socket.path().to_owned();
        let first = UnixStream::connect(&path).unwrap();
        let second = UnixStream::connect(&path).unwrap();
        let token = AppToken::from_bytes([1; 32]);
        let key = DbKey::from_bytes([2; 32]);
        let outcome = socket.hand_off(&me(), HAND_OFF_WAIT, never, secrets(&token, Some(&key)));
        assert_eq!(outcome, Ok(HandOff::Sent));
        assert!(read_hand_off(&mut BufReader::new(first), FirstLine::Create).is_ok());
        assert_eq!(read_all(second), b"", "a second connection got something");
        assert!(UnixStream::connect(&path).is_err());
    }

    #[test]
    fn no_connection_in_time_is_an_error_and_the_socket_goes() {
        let dir = home();
        let socket = KeySocket::bind(dir.path()).unwrap();
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
        assert!(err.contains("didn't connect"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(!path.exists());
    }

    #[test]
    fn a_child_that_exits_first_ends_the_wait_with_nothing_sent() {
        let dir = home();
        let socket = KeySocket::bind(dir.path()).unwrap();
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
        let socket = KeySocket::bind(dir.path()).unwrap();
        let path = socket.path().to_owned();
        assert!(path.exists());
        drop(socket);
        assert!(!path.exists());
        let socket = KeySocket::bind(dir.path()).unwrap();
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
