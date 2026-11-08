//! The office's socket: JSON lines (`org_proto::frame`), one thread per connection. It answers
//! `hello` and `org.snapshot`; every other method is `unknown_method` until the organization
//! commands arrive. When the server stops, every connection is closed and its thread joined
//! BEFORE the office is, so no thread touches a database that is closing.

use std::collections::HashMap;
use std::io::BufReader;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use org_proto::frame;
use org_proto::{
    ApiError, ErrorCode, Hello, HelloResult, OfficeHello, OfficeInfo, OfficeRole, OfficeStatus,
    OrgSnapshot, OrgSnapshotCall, Request, Response, API_VERSION,
};

/// Reads the organization's snapshot (the office's `org_snapshot`), or says why it cannot.
pub type SnapshotFn = Box<dyn Fn() -> Result<OrgSnapshot, String> + Send + Sync>;

/// Most connections served at once; more are closed at once.
pub(crate) const MAX_CONNECTIONS: usize = 64;

/// What every connection shares.
pub struct Shared {
    pub hello: HelloResult,
    snapshot: SnapshotFn,
}

impl Shared {
    /// The `hello` this office answers with.
    pub fn new(
        organization_name: &str,
        office_id: &str,
        organization_id: &str,
        office_name: &str,
        role: OfficeRole,
        socket: &Path,
        snapshot: SnapshotFn,
    ) -> Self {
        Self {
            snapshot,
            hello: HelloResult {
                api_version: API_VERSION,
                server_version: env!("CARGO_PKG_VERSION").to_owned(),
                host: "This Mac".to_owned(),
                // Part 1 implements only what every server of this api_version has.
                capabilities: Vec::new(),
                office: Some(OfficeHello {
                    organization_name: organization_name.to_owned(),
                    office: OfficeInfo {
                        id: office_id.to_owned(),
                        organization_id: organization_id.to_owned(),
                        name: office_name.to_owned(),
                        role,
                        host: "This Mac".to_owned(),
                        endpoint: Some(socket.display().to_string()),
                        status: OfficeStatus::Active,
                        status_reason: None,
                        next_restart_at: None,
                    },
                }),
            },
        }
    }
}

/// Binds `path` (owner-only), replacing a stale socket file a crashed server left: the caller
/// holds the office lock, so no other server of this office is alive.
pub fn bind(path: &Path) -> std::io::Result<UnixListener> {
    match std::fs::symlink_metadata(path) {
        // Only a stale socket (a crashed server's) is replaced; anything else is not ours.
        Ok(metadata) if metadata.file_type().is_socket() => {
            // A socket someone still answers on is a live server, not a stale file.
            if UnixStream::connect(path).is_ok() {
                return Err(std::io::Error::other("a server is already on this socket"));
            }
            std::fs::remove_file(path)?
        }
        Ok(_) => return Err(std::io::Error::other("the socket path is not a socket")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}

/// Accepts connections until `stop` says to quit, then closes every connection and waits for
/// its thread, so nothing is still reading the office when the caller closes it.
pub fn run(listener: &UnixListener, shared: &Arc<Shared>, stop: &dyn Fn() -> bool) {
    let live = Arc::new(AtomicUsize::new(0));
    // A handle on each connection, to close it at shutdown.
    let open: Arc<Mutex<HashMap<u64, UnixStream>>> = Arc::default();
    let mut threads: Vec<JoinHandle<()>> = Vec::new();
    let mut next_id = 0u64;
    while !stop() {
        match listener.accept() {
            Ok((stream, _)) => {
                if live.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                    live.fetch_sub(1, Ordering::SeqCst);
                    continue; // dropped: closed at once
                }
                next_id += 1;
                let id = next_id;
                if let Ok(handle) = stream.try_clone() {
                    open.lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .insert(id, handle);
                }
                let shared = Arc::clone(shared);
                let slot = Slot(Arc::clone(&live));
                let registry = Arc::clone(&open);
                // If the thread cannot start, the closure (and its slot) is dropped with the
                // error: the slot is given back and the connection closes.
                if let Ok(thread) = std::thread::Builder::new()
                    .name("org-office-conn".into())
                    .spawn(move || {
                        let _slot = slot; // given back however the thread ends, panics included
                        let _ = stream.set_nonblocking(false);
                        serve_connection(stream, &shared);
                        registry
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .remove(&id);
                    })
                {
                    threads.retain(|t| !t.is_finished());
                    threads.push(thread);
                } else {
                    open.lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            // A connection that died before accept: carry on.
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    // Stop: close every connection (which ends its blocked read), then wait for the threads.
    for stream in open.lock().unwrap_or_else(|p| p.into_inner()).values() {
        let _ = stream.shutdown(std::net::Shutdown::Both);
    }
    for thread in threads {
        let _ = thread.join();
    }
}

/// One of the connection slots; dropping it gives the slot back, even if the thread panics.
struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn serve_connection(stream: UnixStream, shared: &Shared) {
    let Ok(mut out) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    // A line the framing refuses (too long, not JSON) ends the connection: the stream is out of
    // step and nothing in it can be trusted.
    while let Ok(Some(request)) = frame::read_message::<Request>(&mut reader) {
        let response = answer(&request, shared);
        if frame::write_message(&mut out, &response).is_err() {
            return;
        }
    }
}

fn answer(request: &Request, shared: &Shared) -> Response {
    if request.is::<OrgSnapshotCall>() {
        return match (shared.snapshot)() {
            Ok(snapshot) => Response::ok::<OrgSnapshotCall>(request.id, &snapshot)
                .unwrap_or_else(|_| internal(request.id)),
            Err(_) => internal(request.id),
        };
    }
    if request.is::<Hello>() {
        return Response::ok::<Hello>(request.id, &shared.hello)
            .unwrap_or_else(|_| internal(request.id));
    }
    Response::error(
        request.id,
        ApiError::new(
            ErrorCode::UnknownMethod,
            format!("this office does not know `{}`", request.method),
        ),
    )
}

fn internal(id: u64) -> Response {
    Response::error(
        id,
        ApiError::new(ErrorCode::StorageError, "the office could not answer"),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::*;

    fn shared(path: &Path) -> Arc<Shared> {
        Arc::new(Shared::new(
            "Acme",
            "OFFICE1",
            "ORG1",
            "HQ",
            OfficeRole::Hq,
            path,
            Box::new(|| Err("none".to_owned())),
        ))
    }

    fn hello_ok(stream: &mut UnixStream) -> bool {
        let request = Request::new::<Hello>(1, &org_proto::Empty {}).unwrap();
        if frame::write_message(&mut *stream, &request).is_err() {
            return false;
        }
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        matches!(
            frame::read_message::<Response>(&mut BufReader::new(&*stream)),
            Ok(Some(_))
        )
    }

    #[test]
    fn only_so_many_connections_are_served_and_a_closed_one_gives_its_slot_back() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let listener = bind(&path).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let server = {
            let (shared, stop) = (shared(&path), Arc::clone(&stop));
            std::thread::spawn(move || run(&listener, &shared, &|| stop.load(Ordering::SeqCst)))
        };

        let mut held: Vec<UnixStream> = (0..MAX_CONNECTIONS)
            .map(|_| UnixStream::connect(&path).unwrap())
            .collect();
        // All of them are served (each answers hello), so all slots are taken.
        for stream in &mut held {
            assert!(hello_ok(stream));
        }
        // One more is accepted and closed at once, with no answer.
        let mut extra = UnixStream::connect(&path).unwrap();
        assert!(
            !hello_ok(&mut extra),
            "a connection over the limit was served"
        );
        // A slot comes back when a client goes away.
        drop(held.pop());
        let mut again = loop {
            let mut stream = UnixStream::connect(&path).unwrap();
            if hello_ok(&mut stream) {
                break stream;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(hello_ok(&mut again));
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn when_the_server_stops_every_connection_thread_is_gone_before_run_returns() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let listener = bind(&path).unwrap();
        let shared = shared(&path);
        let stop = Arc::new(AtomicBool::new(false));
        let server = {
            let (shared, stop) = (Arc::clone(&shared), Arc::clone(&stop));
            std::thread::spawn(move || run(&listener, &shared, &|| stop.load(Ordering::SeqCst)))
        };
        // Idle clients, each served by a thread blocked reading.
        let mut clients: Vec<UnixStream> = (0..8)
            .map(|_| UnixStream::connect(&path).unwrap())
            .collect();
        for client in &mut clients {
            assert!(hello_ok(client));
        }
        assert!(
            Arc::strong_count(&shared) > 8,
            "the threads hold the shared state"
        );

        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();

        // `run` closed the clients and joined the threads: nothing but us holds the state.
        assert_eq!(Arc::strong_count(&shared), 1);
    }

    #[test]
    fn a_live_socket_is_not_replaced_and_a_stale_one_is() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let live = bind(&path).unwrap();
        assert!(bind(&path).is_err(), "a live server's socket was taken");
        drop(live); // the file stays, nobody answers
        assert!(bind(&path).is_ok());
    }
}
