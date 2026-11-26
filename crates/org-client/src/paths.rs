//! Where the host and the offices listen: the office home and the socket paths, checked so
//! that nothing planted in the office home can redirect a connection.

use std::io::ErrorKind;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};

use org_proto::{location, OfficeInfo};

use super::error::ClientError;

/// The office home on this machine, found the way the server finds it: `ORG_OFFICE_HOME` when
/// it is an absolute path, else this system's application base for the user whose home folder
/// `HOME` names (`org_platform::dirs`). `None` if neither gives one: `HOME` is unset or
/// relative too, or this system has no base.
pub fn default_office_home() -> Option<PathBuf> {
    office_home(
        std::env::var_os(location::OFFICE_HOME_ENV),
        std::env::var_os("HOME"),
    )
}

/// The rule of [`default_office_home`] on the two variables' values.
pub(crate) fn office_home(
    override_home: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    let base = home
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .and_then(|home| org_platform::dirs::application_base(&home).ok());
    location::office_home_from(override_home, base)
}

/// The socket of this machine's host: `<office home>/host.sock`, with the office home found
/// the way the server finds it ([`default_office_home`]).
pub fn default_host_socket_path() -> Result<PathBuf, ClientError> {
    host_socket_in(&default_office_home().ok_or(ClientError::NoOfficeHome)?)
}

/// Checks `socket` against this system's limit on the length of a socket path.
pub(crate) fn check_socket_path(socket: &Path) -> Result<(), ClientError> {
    let limit = org_platform::dirs::socket_path_limit()
        .map_err(|refused| ClientError::Io(refused.into()))?;
    location::check_socket_path(socket, limit).map_err(ClientError::SocketPathTooLong)
}

pub(crate) fn host_socket_in(home: &Path) -> Result<PathBuf, ClientError> {
    if !home.is_absolute() {
        // The server's working directory is `/`: a relative home would name another socket.
        return Err(ClientError::NoOfficeHome);
    }
    let socket = location::host_socket_path(home);
    check_socket_path(&socket)?;
    Ok(socket)
}

/// Where to connect for `office`. The host's answer is untrusted input from a local
/// process, so nothing it says can point anywhere but an office socket folder's socket,
/// `<home>/run/o/<handle>/s.sock`.
pub(crate) fn office_socket(office: &OfficeInfo, home: &Path) -> Result<PathBuf, ClientError> {
    if !home.is_absolute() {
        return Err(ClientError::NoOfficeHome);
    }
    let run = home.join("run");
    let socket = match &office.endpoint {
        Some(endpoint) => {
            // Normalized so a trailing `/` can never make a later `lstat` follow a link.
            let endpoint: PathBuf = Path::new(endpoint).components().collect();
            if !location::is_office_socket_in(&endpoint, &run) {
                return Err(ClientError::OfficeNotReachable);
            }
            endpoint
        }
        None => return office_socket_by_id(home, &office.id),
    };
    check_socket_path(&socket)?;
    check_not_redirected(&run, &socket)?;
    Ok(socket)
}

/// Where office `office_id` listens, by the rule in `org_proto::location`, with the same checks
/// as [`office_socket`]. Supports both selected UUIDv7 office IDs and legacy plain office IDs.
pub(crate) fn office_socket_by_id(home: &Path, office_id: &str) -> Result<PathBuf, ClientError> {
    if !home.is_absolute() {
        return Err(ClientError::NoOfficeHome);
    }
    let run = home.join("run");
    let socket = location::selected_office_socket_in(&run, office_id)
        .or_else(|| location::office_socket_path(home, office_id))
        .ok_or(ClientError::OfficeNotReachable)?;
    check_socket_path(&socket)?;
    check_not_redirected(&run, &socket)?;
    Ok(socket)
}

/// Where a selected UUIDv7 office listens inside an explicit runtime directory.
pub(crate) fn selected_office_socket(
    runtime_dir: &Path,
    office_id: &str,
) -> Result<PathBuf, ClientError> {
    if !runtime_dir.is_absolute() {
        return Err(ClientError::NoOfficeHome);
    }
    let socket = location::selected_office_socket_in(runtime_dir, office_id)
        .ok_or(ClientError::OfficeNotReachable)?;
    check_socket_path(&socket)?;
    check_not_redirected(runtime_dir, &socket)?;
    Ok(socket)
}

/// The path rules above are lexical. This looks at the file system: `run`, its office sockets
/// folder (`run/o`) and the office's own socket folder (`run/o/<handle>`, the socket's parent)
/// must be real directories (not links to somewhere else) and the socket, if it exists, a real
/// socket (not a link or a file), so nothing planted in the office home can redirect the
/// connection. Each path is normalized first, so a trailing `/` never makes `lstat` follow a
/// link. (It is a check, not a guarantee: the path is looked up again when connecting.)
fn check_not_redirected(run: &Path, socket: &Path) -> Result<(), ClientError> {
    let run: PathBuf = run.components().collect();
    let socket: PathBuf = socket.components().collect();
    let socket_dir = socket.parent().ok_or(ClientError::OfficeNotReachable)?;
    for dir in [
        run.as_path(),
        location::office_sockets_dir(&run).as_path(),
        socket_dir,
    ] {
        match std::fs::symlink_metadata(dir) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            // Missing is fine: the connection then fails as "not running".
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
            _ => return Err(ClientError::OfficeNotReachable),
        }
    }
    match std::fs::symlink_metadata(&socket) {
        Ok(metadata) if metadata.file_type().is_socket() => Ok(()),
        Ok(_) => Err(ClientError::OfficeNotReachable),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(ClientError::Io(e)),
    }
}
