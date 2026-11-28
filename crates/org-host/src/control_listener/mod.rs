//! The host's minimal `org-proto` listener: `host.sock`, one reader thread per connection,
//! a thread per request, and one writer thread per connection so responses and events never
//! interleave mid-line. Knows nothing about methods: every readable [`Request`] goes to the
//! [`Handler`]. It grows no router; it is replaced by the shared server crate once that exists.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use org_proto::Request;

use crate::cloexec::SPAWN_LOCK;
use crate::host_log;
use crate::peer::{Accepted, Received};
use crate::sync::lock;

mod connection;
mod connections;
mod outbound;
mod request;
mod serve;
pub(crate) use connection::{ChannelReceipt, Connection, DescriptorError, ServingReport};
#[cfg(test)]
pub(crate) use connections::LocalConnection;
pub(crate) use connections::WebConnection;
pub(crate) use outbound::SecretLine;
use serve::serve;
#[cfg(test)]
pub(crate) use serve::serve_pair;

/// How long the accept thread waits for a connection before checking whether to stop.
const ACCEPT_POLL: Duration = Duration::from_millis(100);

/// The pause after an accept error (e.g. out of fds), so a lasting one doesn't spin.
const ACCEPT_ERROR_PAUSE: Duration = Duration::from_millis(50);

/// Answers requests through `connection`. Each call runs on its own thread, except those
/// [`Handler::inline`] picks, which run on the connection's reader thread.
pub(crate) trait Handler: Send + Sync + 'static {
    fn handle(&self, request: Request, connection: &Arc<Connection>, received: Received);

    /// Authority ends at reader EOF, before draining outstanding responses.
    fn disconnected(&self, _connection: &Arc<Connection>) {}

    /// Whether `request` is handled on the connection's reader thread, before anything more is
    /// read from the connection, including handshake decisions before later pipelined calls.
    fn inline(&self, request: &Request, connection: &Connection) -> bool {
        let _ = (request, connection);
        false
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ListenerConfig {
    /// Connections beyond this are closed on arrival.
    pub(crate) max_connections: usize,
    /// Requests one connection may have running at once; reading pauses beyond it.
    pub(crate) max_in_flight: usize,
    /// Only peers running as this user are served.
    pub(crate) expected_uid: u32,
}

/// Why the socket couldn't be bound.
#[derive(Debug)]
pub(crate) enum BindError {
    /// Something that isn't a socket is where the socket goes.
    NotASocket,
    Io(io::Error),
}

/// Binds `path` owner-only (0600), first removing a stale socket left there. Only a socket is
/// ever removed: the caller holds `host.lock`, so a socket there can only be a dead host's.
pub(crate) fn bind(path: &Path) -> Result<UnixListener, BindError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_socket() => fs::remove_file(path).map_err(BindError::Io)?,
        Ok(_) => return Err(BindError::NotASocket),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(BindError::Io(e)),
    }
    // See `cloexec`: the socket is made close-on-exec in a second step.
    let listener = {
        let _no_spawn_meanwhile = lock(&SPAWN_LOCK);
        UnixListener::bind(path).map_err(BindError::Io)?
    };
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(BindError::Io)?;
    Ok(listener)
}

/// A running listener.
pub(crate) struct Listener {
    stopping: Arc<AtomicBool>,
    live: Arc<Mutex<HashMap<u64, Live>>>,
    accept: Option<JoinHandle<()>>,
}

struct Live {
    connection: Arc<Connection>,
    reader: JoinHandle<()>,
}

impl Listener {
    /// Serves `listener` until [`Listener::close`].
    pub(crate) fn start(
        listener: UnixListener,
        config: ListenerConfig,
        handler: Arc<dyn Handler>,
    ) -> io::Result<Self> {
        // Non-blocking, so the accept thread can poll and take `SPAWN_LOCK` only to accept.
        listener.set_nonblocking(true)?;
        let stopping = Arc::new(AtomicBool::new(false));
        let live = Arc::new(Mutex::new(HashMap::new()));
        let accept = {
            let (stopping, live) = (Arc::clone(&stopping), Arc::clone(&live));
            thread::Builder::new()
                .name("host-accept".into())
                .spawn(move || accept_loop(&listener, &config, &handler, &stopping, &live))?
        };
        Ok(Self {
            stopping,
            live,
            accept: Some(accept),
        })
    }

    /// Stops accepting and closes every connection, and returns at once. The accept thread
    /// ends within [`ACCEPT_POLL`], or when the accept it is making ends (it may wait for
    /// `SPAWN_LOCK` while a server is spawned): a connection it accepts from now on is
    /// closed on arrival. Requests still running finish (their answers are dropped);
    /// [`Listener::join`] waits for them and for the accept thread.
    pub(crate) fn close(&mut self) {
        // Under the table's lock: the accept thread looks at `stopping` under it before it
        // adds a connection, so every connection is either closed here or never added.
        let live = lock(&self.live);
        self.stopping.store(true, Ordering::SeqCst);
        for live in live.values() {
            live.connection.close();
        }
    }

    /// Waits for the accept thread and every connection's threads, including its running
    /// requests.
    pub(crate) fn join(&mut self) {
        self.close();
        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }
        let live: Vec<Live> = lock(&self.live).drain().map(|(_, live)| live).collect();
        for live in live {
            live.connection.close();
            let _ = live.reader.join();
        }
    }

    /// [`Listener::join`], until `until` at the latest: an accept that has not ended by
    /// then, or a connection whose running request has not (one waiting for a write lock of
    /// the registry, say), is left to end on its own, its answer dropped. Says whether
    /// everything ended.
    pub(crate) fn join_by(&mut self, until: std::time::Instant) -> bool {
        self.close();
        let accepted = self
            .accept
            .take()
            .is_none_or(|accept| crate::sync::join_by(accept, until));
        let live: Vec<Live> = lock(&self.live).drain().map(|(_, live)| live).collect();
        for live in &live {
            live.connection.close();
        }
        // Every connection is waited for, until the same bound.
        let left = live
            .into_iter()
            .map(|live| crate::sync::join_by(live.reader, until))
            .filter(|ended| !ended)
            .count();
        accepted && left == 0
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.join();
    }
}

fn accept_loop(
    listener: &UnixListener,
    config: &ListenerConfig,
    handler: &Arc<dyn Handler>,
    stopping: &AtomicBool,
    live: &Mutex<HashMap<u64, Live>>,
) {
    let mut next_id = 1u64;
    let mut failing = false;
    while !stopping.load(Ordering::SeqCst) {
        if !readable(listener, ACCEPT_POLL) {
            continue;
        }
        // See `cloexec`: on macOS an accepted socket is made close-on-exec in a second step,
        // so no spawn may happen in between. (Gone once servers are spawned with
        // `POSIX_SPAWN_CLOEXEC_DEFAULT` through `org-supervise`.)
        let accepted = {
            let _no_spawn_meanwhile = lock(&SPAWN_LOCK);
            listener.accept()
        };
        let stream = match accepted {
            Ok((stream, _)) => {
                failing = false;
                stream
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            Err(e) => {
                if !failing {
                    host_log::log(&format!("accepting a connection failed: {e}"));
                    failing = true;
                }
                thread::sleep(ACCEPT_ERROR_PAUSE);
                continue;
            }
        };
        // macOS hands out accepted sockets non-blocking like the listener; the threads below
        // read and write them blocking.
        if stream.set_nonblocking(false).is_err() {
            continue;
        }
        let Some(admitted) = admission(
            org_socket::peer_allowed(&stream, config.expected_uid),
            || root_peer(&stream),
        ) else {
            host_log::log("refused a connection from another user");
            continue; // dropping `stream` closes it
        };
        let mut table = lock(live);
        // Accepted after the listener was closed (this accept waited for `SPAWN_LOCK`
        // meanwhile): closed on arrival, as [`Listener::close`] closed every other one.
        if stopping.load(Ordering::SeqCst) {
            break;
        }
        table.retain(|_, live| !live.reader.is_finished());
        if table.len() >= config.max_connections {
            continue;
        }
        let Ok(for_writer) = stream.try_clone() else {
            continue;
        };
        let id = next_id;
        next_id += 1;
        let (connection, queue) = Connection::new(for_writer, Accepted::sample(&stream));
        if admitted == Admitted::Root {
            connection.mark_root_authority();
            host_log::log("admitted root on host.sock, for admin.reset only");
        }
        let reader = {
            let (handler, connection) = (Arc::clone(handler), Arc::clone(&connection));
            let max_in_flight = config.max_in_flight;
            thread::Builder::new()
                .name(format!("host-conn-{id}"))
                .spawn(move || serve(stream, connection, queue, handler, max_in_flight))
        };
        if let Ok(reader) = reader {
            table.insert(id, Live { connection, reader });
        }
    }
}

/// Whether `listener` has a connection waiting, within `wait`.
fn readable(listener: &UnixListener, wait: Duration) -> bool {
    let mut slot = [org_platform::posix::poll_slot(
        listener.as_raw_fd(),
        libc::POLLIN,
    )];
    org_platform::posix::poll(&mut slot, Some(wait)).is_ok_and(|ready| ready > 0)
}

/// Who a peer of `host.sock` was admitted as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Admitted {
    /// The host's own user: every call is judged by its own rule.
    User,
    /// Root, on a system that names a peer by its user alone: the local authority that
    /// may reset the administrator (SIGN-33, ADR-0017 decision 11). It is admitted for
    /// that call and the greeting, and for nothing else (`serve::root_may_call`).
    Root,
}

/// The admission decision, pure: `own_user` is whether the kernel names the host's own
/// user as the peer, and `root` asks whether it names root (asked only otherwise). Any
/// other user is not admitted at all.
fn admission(own_user: bool, root: impl FnOnce() -> bool) -> Option<Admitted> {
    if own_user {
        Some(Admitted::User)
    } else if root() {
        Some(Admitted::Root)
    } else {
        None
    }
}

/// Whether the kernel names root as the peer of `stream`, through the platform's peer
/// identity. Only a system that names a peer by its user alone reports a user at all
/// (`Identity::uid`): where a peer is identified by its image, root has no route here,
/// and a system that cannot identify a peer admits nobody this way.
fn root_peer(stream: &UnixStream) -> bool {
    crate::peer::identity(stream).is_ok_and(|identity| identity.uid == Some(0))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod recovery_eof_tests;

#[cfg(test)]
mod channel_lifecycle_tests;
