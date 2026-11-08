//! # org-office-server: one process per office
//!
//! Started by the host on the launch protocol (`org-office-launch`):
//! ```text
//! org-office-server --create        # first stdin line: the create message; makes the office, serves it
//! org-office-server --office <id>   # first stdin line: the start message; serves the office
//! org-office-server --discard <id>  # removes an unfinished create; reads no line
//! ```
//! stdout carries only [`ServerEvent`] lines (`created`, `ready`, `discarded`, `failed`), logs go
//! to stderr, and EOF on stdin means the host is gone: the server closes its databases and exits 0.
//! The office, its folder, lock, key and databases are `org_office::Office`; this crate is the
//! process around it: the control pipe, the Keychain, the socket.

#![deny(unsafe_code)]

mod bounded;
pub mod fdswap;
mod follow;
mod keys;
mod serve;
mod signals;
mod zeroed;

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use org_office::{NewOffice, Office, OfficeError};
use org_office_launch::{
    office_dir, read_first_host_message, write_server_event, FailCode, FirstLine, HostMessage,
    OfficeFile, ServerEvent, CREATE_FLAG, DISCARD_FLAG, OFFICE_FLAG,
};
use org_proto::location::{check_socket_path, default_office_home, office_socket_path};
use org_proto::{AppToken, OfficeRole};

use fdswap::ControlPipe;

/// A panic prints where it happened and nothing else: its message could carry a path or data.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let place = info
            .location()
            .map_or_else(String::new, |l| format!(" at {}:{}", l.file(), l.line()));
        eprintln!("org-office-server: internal error{place}");
    }));
}

/// What the server was asked to do.
#[derive(Debug, PartialEq, Eq)]
enum Mode {
    Create,
    Serve(String),
    Discard(String),
}

fn parse_args(args: &[String]) -> Result<Mode, &'static str> {
    match args {
        [flag] if flag == CREATE_FLAG => Ok(Mode::Create),
        [flag, id] if flag == OFFICE_FLAG => Ok(Mode::Serve(id.clone())),
        [flag, id] if flag == DISCARD_FLAG => Ok(Mode::Discard(id.clone())),
        _ => Err("usage: org-office-server --create | --office <id> | --discard <id>"),
    }
}

/// Runs the server and returns its exit code.
pub fn run(args: &[String], mut control: ControlPipe) -> i32 {
    let mode = match parse_args(args) {
        Ok(mode) => mode,
        Err(usage) => return fail(&mut control, FailCode::InvalidRequest, usage),
    };
    let Some(home) = default_office_home() else {
        return fail(&mut control, FailCode::InvalidRequest, "no office home");
    };
    let keys = match keys::provider() {
        Ok(keys) => keys,
        Err(why) => return fail(&mut control, FailCode::StorageError, &why),
    };
    match mode {
        Mode::Discard(id) => discard(&mut control, &home, &id, &*keys),
        Mode::Create => create_and_serve(control, &home, &*keys),
        Mode::Serve(id) => open_and_serve(control, &home, &id, &*keys),
    }
}

fn discard(
    control: &mut ControlPipe,
    home: &Path,
    id: &str,
    keys: &dyn org_office::KeyProvider,
) -> i32 {
    let Some(folder) = office_dir(home, id) else {
        return fail(control, FailCode::InvalidRequest, "not a plain office id");
    };
    match Office::discard(folder, id, keys) {
        Ok(()) => {
            emit(control, &ServerEvent::Discarded);
            0
        }
        Err(e) => fail_with(control, &e),
    }
}

fn create_and_serve(
    mut control: ControlPipe,
    home: &Path,
    keys: &dyn org_office::KeyProvider,
) -> i32 {
    let (message, from_host) = match first_line(&mut control, FirstLine::Create) {
        Ok(first) => first,
        Err(code) => return code,
    };
    let HostMessage::Create(create) = message else {
        return fail(
            &mut control,
            FailCode::InvalidRequest,
            "not a create message",
        );
    };
    if create.role != OfficeRole::Hq {
        return fail(
            &mut control,
            FailCode::InvalidRequest,
            "this version creates only HQ offices",
        );
    }
    let Some(folder) = office_dir(home, &create.office_id) else {
        return fail(
            &mut control,
            FailCode::InvalidRequest,
            "not a plain office id",
        );
    };
    // The socket path depends only on the home and the id: refuse a path that is too long
    // BEFORE anything is stored, so a create never makes an office that can never start.
    if let Err(message) = socket_path(home, &create.office_id) {
        return fail(&mut control, FailCode::InvalidRequest, message);
    }
    // The contract: names travel trimmed, and a name that isn't is refused, not quietly trimmed
    // (the host would take the changed name for a different office).
    for name in [&create.org_name, &create.office_name] {
        if name.trim() != name.as_str() {
            return fail(&mut control, FailCode::InvalidName, "a name is not trimmed");
        }
    }
    let new = NewOffice {
        office_id: create.office_id.clone(),
        office_name: create.office_name.clone(),
        role: org_office::OfficeRole::Hq,
        org_id: create.org_id.clone(),
        org_name: create.org_name.clone(),
    };
    let office = match Office::create(folder, &new, keys) {
        Ok(office) => office,
        Err(e) => return fail_with(&mut control, &e),
    };
    let info = office.info();
    let file = OfficeFile::new(
        info.office_id.clone(),
        info.office_name.clone(),
        OfficeRole::Hq,
        info.org_id.clone(),
        info.org_name.clone(),
        info.created_at.clone(),
    );
    // `office.json` is in place and durable: from here the office is stored, whatever happens.
    emit(&mut control, &ServerEvent::Created { office: file });
    serve_office(control, from_host, home, office, create.app_token)
}

fn open_and_serve(
    mut control: ControlPipe,
    home: &Path,
    id: &str,
    keys: &dyn org_office::KeyProvider,
) -> i32 {
    let (message, from_host) = match first_line(&mut control, FirstLine::Start) {
        Ok(first) => first,
        Err(code) => return code,
    };
    let HostMessage::Start(start) = message else {
        return fail(
            &mut control,
            FailCode::InvalidRequest,
            "not a start message",
        );
    };
    let Some(folder) = office_dir(home, id) else {
        return fail(
            &mut control,
            FailCode::InvalidRequest,
            "not a plain office id",
        );
    };
    if let Err(message) = socket_path(home, id) {
        return fail(&mut control, FailCode::InvalidRequest, message);
    }
    let office = match Office::open(folder, keys) {
        Ok(office) => office,
        Err(e) => return fail_with(&mut control, &e),
    };
    serve_office(control, from_host, home, office, start.app_token)
}

/// Reads the first line from the host. `Err(code)` is the exit code to return: 0 when the host
/// is gone, otherwise a `failed` line has been written.
fn first_line(control: &mut ControlPipe, expected: FirstLine) -> Result<(HostMessage, File), i32> {
    let from_host = control.from_host.try_clone().map_err(|_| {
        fail(
            control,
            FailCode::StorageError,
            "cannot read the control pipe",
        )
    })?;
    // A small buffer we control, since the line carries the app token.
    let mut reader = zeroed::ZeroedReader::new(from_host);
    match read_first_host_message(&mut reader, expected) {
        Ok(Some(message)) => Ok((message, reader.take_inner())),
        Ok(None) => Err(0),
        Err(e) if e.is_host_gone() => Err(0),
        Err(e) => Err(fail(control, e.fail_code(), &e.to_string())),
    }
}

/// Binds the socket, says `ready`, serves until the host goes away or a stop signal arrives,
/// then closes the office (databases, lock) and exits 0.
fn serve_office(
    mut control: ControlPipe,
    from_host: File,
    home: &Path,
    office: Office,
    app_token: AppToken,
) -> i32 {
    let info = office.info().clone();
    let office = Arc::new(office);
    let socket = match socket_path(home, &info.office_id) {
        Ok(socket) => socket,
        Err(message) => return fail(&mut control, FailCode::InvalidRequest, message),
    };
    let listener = match serve::bind(&socket) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("org-office-server: bind failed: {error}");
            return fail(
                &mut control,
                FailCode::StorageError,
                "cannot bind the socket",
            );
        }
    };
    // Stop signals are handled only from here: before this the default disposition applies, so a
    // server waiting for its first line or for the office lock can still be terminated.
    signals::install();
    let shared = Arc::new(serve::Shared::new(
        &info,
        &socket,
        serve::OfficePorts {
            snapshot: {
                let office = Arc::clone(&office);
                Box::new(move || office.org_snapshot().map_err(|e| logged("snapshot", &e)))
            },
            who: {
                let office = Arc::clone(&office);
                Box::new(move |params| {
                    // Nothing reports presence yet (agents bind with messaging): all offline.
                    office
                        .who(params, &org_office::directory::NothingLive)
                        .map_err(|e| logged("who", &e))
                })
            },
            feed: Box::new(OfficeFeed(Arc::clone(&office))),
            submit: {
                let office = Arc::clone(&office);
                Box::new(move |command| {
                    office
                        .submit_org(command)
                        .map_err(|e| logged("command", &e))
                })
            },
        },
        app_token,
    ));
    // EOF on the control pipe means the host is gone. What the host writes after the first line
    // is read into a fixed buffer and thrown away, unparsed, so nothing on a later line can
    // stop the server from seeing the end.
    let host_gone = Arc::new(AtomicBool::new(false));
    let watcher = {
        let host_gone = Arc::clone(&host_gone);
        std::thread::Builder::new()
            .name("org-office-control".into())
            .spawn(move || {
                let mut from_host = from_host;
                let mut sink = [0u8; 4096];
                loop {
                    match from_host.read(&mut sink) {
                        Ok(n) if n > 0 => {}
                        // A signal interrupting the read is not the host leaving.
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                        _ => break,
                    }
                }
                host_gone.store(true, Ordering::SeqCst);
            })
    };
    if watcher.is_err() {
        let _ = std::fs::remove_file(&socket);
        return fail(
            &mut control,
            FailCode::StorageError,
            "cannot watch the control pipe",
        );
    }

    emit(
        &mut control,
        &ServerEvent::Ready {
            socket: socket.display().to_string(),
            pid: std::process::id(),
        },
    );

    let stop = || host_gone.load(Ordering::SeqCst) || signals::stop_requested();
    serve::run(&listener, &shared, &stop);

    drop(listener);
    let _ = std::fs::remove_file(&socket);
    // Every connection thread is gone (joined by `serve::run`): the only owners of the office
    // left are `shared` and this function.
    drop(shared);
    drop(office);
    0
}

/// Logs why the office could not answer (to stderr, the host's log: `OfficeError`'s text has no
/// path, SQL or secret in it) and returns it for the connection, which tells the client only a
/// fixed text.
fn logged(what: &str, error: &OfficeError) -> String {
    eprintln!("org-office-server: {what} failed: {error}");
    log_causes(error);
    error.to_string()
}

/// The causes behind a storage failure (a path, the OS error, SQLite's text) belong in the log
/// and not on the pipe or the socket; they never hold the key or the token.
fn log_causes(error: &OfficeError) {
    let mut cause = std::error::Error::source(error);
    while let Some(inner) = cause {
        eprintln!("org-office-server:   caused by: {inner}");
        cause = inner.source();
    }
}

/// The office's log, for `subscribe`.
struct OfficeFeed(Arc<Office>);

impl follow::OrgFeed for OfficeFeed {
    fn head(&self) -> Result<org_proto::org::Position, String> {
        self.0.org_head().map_err(|e| logged("head", &e))
    }

    fn can_resume_from(&self, after: org_proto::org::Position) -> Result<bool, String> {
        self.0
            .org_can_resume_from(after)
            .map_err(|e| logged("resume check", &e))
    }

    fn changes_after(
        &self,
        after: org_proto::org::Position,
        limit: usize,
    ) -> Result<Vec<org_proto::org::OrgChangedData>, String> {
        self.0
            .org_changes_after(after, limit)
            .map_err(|e| logged("log read", &e))
    }

    fn token(&self) -> u64 {
        self.0.org_change_token()
    }

    fn wait(&self, seen: u64, timeout: std::time::Duration) -> u64 {
        self.0.wait_for_org_change(seen, timeout)
    }
}

/// The office's socket path, or why it cannot be one.
fn socket_path(home: &Path, office_id: &str) -> Result<std::path::PathBuf, &'static str> {
    let socket = office_socket_path(home, office_id).ok_or("not a plain office id")?;
    check_socket_path(&socket).map_err(|_| "the socket path is too long")?;
    Ok(socket)
}

fn emit(control: &mut ControlPipe, event: &ServerEvent) {
    // The host may already be gone; the server notices at EOF.
    let _ = write_server_event(&mut control.to_host, event);
    let _ = control.to_host.flush();
}

fn fail(control: &mut ControlPipe, code: FailCode, message: &str) -> i32 {
    // The host's log is this process's stderr: say what failed (a fixed text, never a secret).
    eprintln!("org-office-server: {message}");
    emit(
        control,
        &ServerEvent::Failed {
            code,
            message: message.to_owned(),
        },
    );
    1
}

fn fail_with(control: &mut ControlPipe, error: &OfficeError) -> i32 {
    log_causes(error);
    // `Display` never carries a path, SQL or a key.
    fail(control, fail_code(error), &error.to_string())
}

/// The code the host is told for an office error.
fn fail_code(error: &OfficeError) -> FailCode {
    match error {
        OfficeError::InvalidName(_) => FailCode::InvalidName,
        OfficeError::OfficeExists | OfficeError::AlreadyExists | OfficeError::KeyExists => {
            FailCode::AlreadyExists
        }
        OfficeError::KeyMissing => FailCode::KeyMissing,
        OfficeError::InUse => FailCode::InUse,
        // `NotFound` on `--office`: serving an office that does not exist is a bad argument.
        OfficeError::NotFound => FailCode::InvalidRequest,
        // Everything else `Invalid` that the office still reports describes the disk (databases
        // of another office, a folder that is not plain, a stray file): the server has already
        // checked the arguments and the first line. `invalid_request` is fatal for the host until
        // a new binary; a repaired folder must be restartable.
        OfficeError::Invalid(_) => FailCode::StorageError,
        _ => FailCode::StorageError,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn every_office_error_has_its_code() {
        use org_office::NameError;
        let storage = || OfficeError::Storage("disk".into());
        for (error, code) in [
            (
                OfficeError::InvalidName(NameError::Empty),
                FailCode::InvalidName,
            ),
            (OfficeError::OfficeExists, FailCode::AlreadyExists),
            (OfficeError::AlreadyExists, FailCode::AlreadyExists),
            (OfficeError::KeyExists, FailCode::AlreadyExists),
            (OfficeError::KeyMissing, FailCode::KeyMissing),
            (OfficeError::InUse, FailCode::InUse),
            (OfficeError::Invalid("x"), FailCode::StorageError),
            (OfficeError::NotFound, FailCode::InvalidRequest),
            (storage(), FailCode::StorageError),
        ] {
            assert_eq!(fail_code(&error), code, "{error:?}");
        }
    }

    #[test]
    fn the_three_modes_and_nothing_else() {
        assert_eq!(parse_args(&args(&["--create"])), Ok(Mode::Create));
        assert_eq!(
            parse_args(&args(&["--office", "ABC"])),
            Ok(Mode::Serve("ABC".into()))
        );
        assert_eq!(
            parse_args(&args(&["--discard", "ABC"])),
            Ok(Mode::Discard("ABC".into()))
        );
        for bad in [
            args(&[]),
            args(&["--create", "ABC"]),
            args(&["--office"]),
            args(&["--discard"]),
            args(&["--serve", "ABC"]),
            args(&["--office", "ABC", "extra"]),
        ] {
            assert!(parse_args(&bad).is_err(), "{bad:?}");
        }
    }
}
