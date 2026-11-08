//! # org-office-server: the office daemon, as a library
//!
//! Listens on a local Unix socket (mode 0600, inside the owner-only office home), speaks
//! `org-proto` and serves it from an `org-office` [`Office`]. `main.rs` only parses
//! arguments and calls [`Server::start`].
//!
//! Built to carry more than phase 1: connections are long-lived, a connection may have
//! several requests in flight (answers are matched by `id`), and new methods are added to
//! the [`Router`] without touching the listener or the connection code.
//!
//! Threads: one accept thread; per connection a reader and a writer; a fixed pool of
//! workers that run handlers; and the office's own single storage writer.

mod connection;
mod events;
mod handlers;
mod pool;
mod router;

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::BufReader;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use org_office::{KeyProvider, Office, OfficeError};
use org_proto::frame;
use org_proto::{salvage_id, ApiError, ErrorCode, Request, Response, UNREADABLE_REQUEST_ID};

use crate::connection::{write_loop, Connection};
use crate::pool::{Pool, Submitter};

pub use crate::connection::Connection as ClientConnection;
pub use crate::events::EventBus;
pub use crate::handlers::standard_router;
pub use crate::router::{Context, Router};

/// `sun_path` is 104 bytes on macOS, including the terminating NUL.
const MAX_SOCKET_PATH_BYTES: usize = 103;

#[derive(Debug, Clone)]
pub struct Config {
    /// The office's home directory (see `org_office::Office`).
    pub home: PathBuf,
    /// The Unix socket to listen on. Must be inside `home`'s owner-only tree.
    pub socket_path: PathBuf,
    /// Threads that run handlers.
    pub workers: usize,
    /// Connections beyond this are closed on arrival.
    pub max_connections: usize,
}

impl Config {
    pub fn new(home: impl Into<PathBuf>, socket_path: impl Into<PathBuf>) -> Self {
        Self {
            home: home.into(),
            socket_path: socket_path.into(),
            workers: 4,
            max_connections: 64,
        }
    }
}

#[derive(Debug)]
pub enum ServerError {
    Office(OfficeError),
    Io(std::io::Error),
    SocketPathTooLong,
    /// Something that is not a socket sits where the socket should go.
    NotASocket,
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServerError::Office(e) => write!(f, "{e}"),
            ServerError::Io(e) => write!(f, "i/o error: {e}"),
            ServerError::SocketPathTooLong => {
                write!(
                    f,
                    "the socket path is longer than {MAX_SOCKET_PATH_BYTES} bytes"
                )
            }
            ServerError::NotASocket => {
                write!(
                    f,
                    "a file that is not a socket is where the socket should be"
                )
            }
        }
    }
}

impl std::error::Error for ServerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ServerError::Office(e) => Some(e),
            ServerError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<OfficeError> for ServerError {
    fn from(e: OfficeError) -> Self {
        ServerError::Office(e)
    }
}

impl From<std::io::Error> for ServerError {
    fn from(e: std::io::Error) -> Self {
        ServerError::Io(e)
    }
}

/// What every connection and handler shares.
struct Shared {
    office: Office,
    router: Router,
    events: EventBus,
}

struct Live {
    connection: Arc<Connection>,
    thread: JoinHandle<()>,
}

/// A running office server. Dropping it (or calling [`Server::shutdown`]) stops it.
pub struct Server {
    socket_path: PathBuf,
    stopping: Arc<AtomicBool>,
    live: Arc<Mutex<HashMap<u64, Live>>>,
    accept: Option<JoinHandle<()>>,
    pool: Option<Pool>,
    shared: Option<Arc<Shared>>,
}

impl Server {
    /// Opens the office, binds the socket and starts serving.
    pub fn start(config: Config, keys: &dyn KeyProvider) -> Result<Self, ServerError> {
        Self::start_with(config, keys, standard_router())
    }

    /// Like [`Server::start`] with a custom dispatch table.
    pub fn start_with(
        config: Config,
        keys: &dyn KeyProvider,
        router: Router,
    ) -> Result<Self, ServerError> {
        if config.socket_path.as_os_str().len() > MAX_SOCKET_PATH_BYTES {
            return Err(ServerError::SocketPathTooLong);
        }
        // The office lock is taken here. Whoever holds it is the only server for this home,
        // so a socket file we find afterwards is stale by construction.
        let office = Office::open(&config.home, keys)?;
        let listener = bind(&config.socket_path)?;

        let shared = Arc::new(Shared {
            office,
            router,
            events: EventBus::default(),
        });
        let pool = Pool::new(config.workers)?;
        let stopping = Arc::new(AtomicBool::new(false));
        let live = Arc::new(Mutex::new(HashMap::new()));

        let accept = {
            let (shared, stopping, live) = (
                Arc::clone(&shared),
                Arc::clone(&stopping),
                Arc::clone(&live),
            );
            let submitter = pool.submitter();
            let max_connections = config.max_connections;
            std::thread::Builder::new()
                .name("org-office-accept".into())
                .spawn(move || {
                    accept_loop(
                        listener,
                        &shared,
                        &submitter,
                        &stopping,
                        &live,
                        max_connections,
                    )
                })?
        };

        Ok(Self {
            socket_path: config.socket_path,
            stopping,
            live,
            accept: Some(accept),
            pool: Some(pool),
            shared: Some(shared),
        })
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Stops accepting, closes every connection, waits for the threads, closes the office
    /// (which releases its lock) and removes the socket file.
    pub fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        // `accept` blocks: wake it with a connection of our own. Keep that connection open
        // until the accept thread is gone: on macOS a queued Unix-socket connection whose
        // peer already closed is discarded, and `accept` would never return.
        let wake = UnixStream::connect(&self.socket_path);
        match (self.accept.take(), &wake) {
            (Some(accept), Ok(_)) => {
                let _ = accept.join();
            }
            // The socket file is gone, so nothing can wake the accept thread. Never hang
            // shutdown on that: leave the thread to die with the process.
            (Some(_), Err(error)) => {
                eprintln!("org-office-server: could not wake the accept thread: {error}");
            }
            (None, _) => {}
        }
        drop(wake);
        let live: Vec<Live> = lock(&self.live).drain().map(|(_, live)| live).collect();
        for live in &live {
            live.connection.close();
        }
        for live in live {
            let _ = live.thread.join();
        }
        // Workers finish their current requests; then nothing else holds `shared`.
        self.pool.take();
        self.shared.take();
        let _ = fs::remove_file(&self.socket_path);
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Removes a stale socket and binds a fresh one, owner-only.
fn bind(path: &Path) -> Result<UnixListener, ServerError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() => fs::remove_file(path)?,
        Ok(_) => return Err(ServerError::NotASocket),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let listener = UnixListener::bind(path)?;
    // The owner-only home already keeps others out; this is the second lock.
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

fn accept_loop(
    listener: UnixListener,
    shared: &Arc<Shared>,
    submitter: &Submitter,
    stopping: &Arc<AtomicBool>,
    live: &Arc<Mutex<HashMap<u64, Live>>>,
    max_connections: usize,
) {
    let ids = AtomicU64::new(1);
    for stream in listener.incoming() {
        if stopping.load(Ordering::SeqCst) {
            return;
        }
        let Ok(stream) = stream else { continue };
        let mut table = lock(live);
        table.retain(|_, entry| !entry.thread.is_finished());
        if table.len() >= max_connections {
            continue; // `stream` drops: the client sees the connection close.
        }
        let id = ids.fetch_add(1, Ordering::SeqCst);
        let Ok(for_writer) = stream.try_clone() else {
            continue;
        };
        let (connection, queue) = Connection::new(id, for_writer);

        let thread = {
            let (shared, submitter, connection) = (
                Arc::clone(shared),
                submitter.clone(),
                Arc::clone(&connection),
            );
            std::thread::Builder::new()
                .name(format!("org-office-conn-{id}"))
                .spawn(move || serve_connection(&shared, &submitter, connection, queue, stream))
        };
        if let Ok(thread) = thread {
            table.insert(id, Live { connection, thread });
        }
    }
}

/// A connection's reader. Runs until the client goes away or sends something unreadable
/// at the framing level.
fn serve_connection(
    shared: &Arc<Shared>,
    submitter: &Submitter,
    connection: Arc<Connection>,
    queue: std::sync::mpsc::Receiver<connection::Outbound>,
    stream: UnixStream,
) {
    let writer = {
        let connection = Arc::clone(&connection);
        std::thread::Builder::new()
            .name(format!("org-office-conn-{}-out", connection.id))
            .spawn(move || write_loop(&connection, queue))
    };

    let mut reader = BufReader::new(stream);
    // Ends on a clean close, or on a framing error that leaves the stream out of step.
    while let Ok(Some(line)) = frame::read_line(&mut reader) {
        match serde_json::from_slice::<Request>(&line) {
            Ok(request) => {
                let (shared, connection) = (Arc::clone(shared), Arc::clone(&connection));
                if !submitter.submit(move || run_request(&shared, &connection, &request)) {
                    break;
                }
            }
            Err(_) => connection.send_response(Response::error(
                salvage_id(&line).unwrap_or(UNREADABLE_REQUEST_ID),
                ApiError::new(ErrorCode::InvalidRequest, "the request could not be read"),
            )),
        }
    }

    // Stop the writer, then close the socket (which also unblocks a write stuck on a client
    // that stopped reading).
    connection.finish();
    connection.close();
    drop(connection);
    if let Ok(writer) = writer {
        let _ = writer.join();
    }
}

/// Runs one request on a worker and queues its response. A panicking handler becomes an
/// error response; it must not take the worker (or the connection) down.
fn run_request(shared: &Shared, connection: &Arc<Connection>, request: &Request) {
    let context = Context {
        office: &shared.office,
        events: &shared.events,
        connection,
    };
    let response = catch_unwind(AssertUnwindSafe(|| {
        shared.router.dispatch(&context, request)
    }))
    .unwrap_or_else(|_| {
        eprintln!(
            "org-office-server: a handler panicked on `{}`",
            request.method
        );
        Response::error(
            request.id,
            ApiError::new(
                ErrorCode::StorageError,
                "the server failed to handle the request",
            ),
        )
    });
    connection.send_response(response);
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
