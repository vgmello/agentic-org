//! The office's socket: JSON lines (`org_proto::frame`), one thread per connection. Part 1 of
//! the server core answers `hello`; every other method is `unknown_method` until the organization
//! calls arrive.

use std::io::BufReader;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use org_proto::frame;
use org_proto::{
    ApiError, ErrorCode, Hello, HelloResult, OfficeHello, OfficeInfo, OfficeRole, OfficeStatus,
    Request, Response, API_VERSION,
};

/// Most connections served at once; more are closed at once.
const MAX_CONNECTIONS: usize = 64;

/// What every connection shares.
pub struct Shared {
    pub hello: HelloResult,
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
    ) -> Self {
        Self {
            hello: HelloResult {
                api_version: API_VERSION,
                server_version: env!("CARGO_PKG_VERSION").to_owned(),
                host: "This Mac".to_owned(),
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
        Ok(metadata) if metadata.file_type().is_symlink() || metadata.is_dir() => {
            return Err(std::io::Error::other("the socket path is not a socket"));
        }
        Ok(_) => std::fs::remove_file(path)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}

/// Accepts connections until `stop` says to quit.
pub fn run(listener: &UnixListener, shared: &Arc<Shared>, stop: &dyn Fn() -> bool) {
    let live = Arc::new(AtomicUsize::new(0));
    while !stop() {
        match listener.accept() {
            Ok((stream, _)) => {
                if live.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                    live.fetch_sub(1, Ordering::SeqCst);
                    continue; // dropped: closed at once
                }
                let (shared, counted) = (Arc::clone(shared), Arc::clone(&live));
                let spawned = std::thread::Builder::new()
                    .name("org-office-conn".into())
                    .spawn(move || {
                        let _ = stream.set_nonblocking(false);
                        serve_connection(stream, &shared);
                        counted.fetch_sub(1, Ordering::SeqCst);
                    });
                if spawned.is_err() {
                    // Out of threads: the connection is dropped and the count is given back.
                    live.fetch_sub(1, Ordering::SeqCst);
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
