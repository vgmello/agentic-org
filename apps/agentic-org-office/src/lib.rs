//! # agentic-org-office: one process per office
//!
//! Started by the host on the launch protocol (`org-office-launch`):
//! ```text
//! agentic-org-office --create        # first stdin line: the create message; makes the office, serves it
//! agentic-org-office --office <id>   # first stdin line: the start message; serves the office
//! agentic-org-office --discard <id>  # first stdin line: the discard message (the key or null); removes an unfinished create
//! ```
//! One launch version: events travel on the authenticated rendezvous; its EOF closes the office
//! and exits 0. stdin is drained unparsed after the first line and is not a lifetime signal;
//! stdout carries nothing. Logs go to stderr.
//! The office, its folder, lock, key and store are `org_office::Office`; this
//! crate is the process around it: launch control, the host's writer custody, typed authority
//! and the socket. Every create and start takes the host's writer control first, before it
//! creates or opens anything. The server never touches a Keychain.

#![deny(unsafe_code)]

mod bounded;
mod directory;
pub mod fdswap;
mod follow;
mod grant;
mod hq_creation;
pub mod inbox;
mod launch;
mod live;
mod msg_calls;
mod revocations;
mod serve;
mod signals;
mod startup;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
mod transfer;
mod zeroed;

#[cfg(test)]
#[path = "../tests/support/teardown.rs"]
mod test_teardown;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use launch::Launch;
use org_office::Office;
use org_office::OfficeError;
use org_office_launch::{
    office_dir, FailCode, FirstLine, LaunchError, OfficeFile, ServerEvent, CREATE_FLAG,
    DISCARD_FLAG, OFFICE_FLAG,
};
use org_proto::location::{
    check_socket_path, office_home_from, office_socket_in, OFFICE_HOME_ENV, OFFICE_RUNTIME_DIR_ENV,
};
use org_proto::{ApiError, OfficeRole};
use startup::{Control, Lifetime};

use fdswap::ControlPipe;

/// A panic prints where it happened and nothing else: its message could carry a path or data.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let place = info
            .location()
            .map_or_else(String::new, |l| format!(" at {}:{}", l.file(), l.line()));
        eprintln!("agentic-org-office: internal error{place}");
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
        _ => Err("usage: agentic-org-office --create | --office <id> | --discard <id>"),
    }
}

/// Runs the server and returns its exit code.
pub fn run(args: &[String], control: ControlPipe) -> i32 {
    // No child-spawn paths exist today. Any future path must share this receive/spawn fence.
    let mut control = Control::new(control, Arc::new(std::sync::Mutex::new(())));
    let mode = match parse_args(args) {
        Ok(mode) => mode,
        Err(usage) => return fail(&mut control, FailCode::InvalidRequest, usage),
    };
    let Some(home) = office_home(std::env::var_os(OFFICE_HOME_ENV), std::env::var_os("HOME"))
    else {
        return fail(&mut control, FailCode::InvalidRequest, "no office home");
    };
    let runtime = match runtime_dir_from(
        &home,
        std::env::var_os(OFFICE_RUNTIME_DIR_ENV),
        cfg!(debug_assertions),
    ) {
        Ok(runtime) => runtime,
        Err(message) => return fail(&mut control, FailCode::InvalidRequest, message),
    };
    match mode {
        Mode::Discard(id) => discard(control, &home, &id),
        Mode::Create => create_and_serve(control, &home, &runtime),
        Mode::Serve(id) => open_and_serve(control, &home, &runtime, &id),
    }
}

/// `--discard`: needs no key, because the databases are plain SQLite (ADR-0016); a key the
/// handoff carries is dropped when the request is parsed.
fn discard(mut control: Control, home: &Path, id: &str) -> i32 {
    let (message, _from_host) = match first_line(&mut control, FirstLine::Discard, Some(id)) {
        Ok(first) => first,
        Err(code) => return code,
    };
    let Launch::Discard {
        office: _office_identity,
    } = message
    else {
        return fail(
            &mut control,
            FailCode::InvalidRequest,
            "not a discard message",
        );
    };
    let Some(folder) = office_dir(home, id) else {
        return fail(
            &mut control,
            FailCode::InvalidRequest,
            "not a plain office id",
        );
    };
    // A folder the host reserved writer custody for (its marker, or a staging file of one) is
    // never discarded: a discard claims none. It is the reservation's, not a finished office,
    // so the refusal is the same fixed storage error for both, whatever the folder holds.
    if holds_writer_custody(&folder) {
        return fail(
            &mut control,
            FailCode::StorageError,
            "selected writer custody is required",
        );
    }
    match Office::discard(folder, id) {
        Ok(()) => {
            emit(&mut control, &ServerEvent::Discarded);
            0
        }
        Err(e) => fail_with(&mut control, &e),
    }
}

/// Whether the host reserved writer custody in `folder` (`org_office_launch::writer::marker`).
/// A folder that can't be listed counts too: it is kept. A missing one holds nothing.
fn holds_writer_custody(folder: &Path) -> bool {
    org_office_launch::writer::marker::holds_custody(folder)
        .unwrap_or_else(|error| error.kind() != std::io::ErrorKind::NotFound)
}

fn create_and_serve(mut control: Control, home: &Path, runtime: &Path) -> i32 {
    let (message, from_host) = match first_line(&mut control, FirstLine::Create, None) {
        Ok(first) => first,
        Err(code) => return code,
    };
    let Launch::Create(create) = message else {
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
    let Some(folder) = office_dir(home, &create.office.office_id) else {
        return fail(
            &mut control,
            FailCode::InvalidRequest,
            "not a plain office id",
        );
    };
    // The socket path depends only on the home and the id: refuse a path that is too long
    // BEFORE anything is stored, so a create never makes an office that can never start.
    if let Err(message) = socket_path(runtime, &create.office.office_id) {
        return fail(&mut control, FailCode::InvalidRequest, message);
    }
    // The contract: names travel trimmed, and a name that isn't is refused, not quietly trimmed
    // (the host would take the changed name for a different office).
    for name in [&create.office.org_name, &create.office.office_name] {
        if name.trim() != name.as_str() {
            return fail(&mut control, FailCode::InvalidName, "a name is not trimmed");
        }
    }
    // HQ creation-v1 is the only create: it consumes the recovery code before serving.
    let Some(context) = create.hq_creation else {
        return fail(
            &mut control,
            FailCode::InvalidRequest,
            "missing HQ creation binding",
        );
    };
    let Some(recovery) = create.recovery else {
        return fail(
            &mut control,
            FailCode::InvalidRequest,
            "missing recovery code",
        );
    };
    // The host's writer control comes first: nothing is created without custody.
    let mut transfer = control.transfer();
    let admitted = match admit(
        &mut control,
        &mut transfer,
        &create.office.office_id,
        &create.instance,
    ) {
        Ok(admitted) => admitted,
        Err(code) => return code,
    };
    // The admission is for this organization and this owner; the office checks its roots
    // against the intent it freezes.
    if admitted.admission.identity.organization_id != create.office.org_id
        || admitted.admission.owner_member != create.owner_member_id
    {
        return fail(
            &mut control,
            FailCode::StorageError,
            "protected writer admission requires recovery",
        );
    }
    let binding = org_office::hq_creation::Binding {
        operation_id: context.operation_id,
        owner_member_id: create.owner_member_id,
        bootstrap: context.bootstrap.map(|b| *b),
    };
    let created = hq_creation::prepare(
        &folder,
        &create.office,
        Some(create.key),
        &binding,
        admitted.roots,
        admitted.custody,
        recovery,
    );
    let office = match created {
        Ok(office) => office,
        Err(e) => return fail_with(&mut control, &e),
    };
    let info = office.info();
    let file = if org_office_launch::is_selected_entity_id(&info.office_id) {
        match OfficeFile::selected(
            info.office_id.clone(),
            info.office_name.clone(),
            OfficeRole::Hq,
            info.org_id.clone(),
            info.org_name.clone(),
            info.created_at.clone(),
        ) {
            Ok(file) => file,
            Err(_) => {
                return fail(
                    &mut control,
                    FailCode::StorageError,
                    "selected office marker rejected",
                );
            }
        }
    } else {
        OfficeFile::new(
            info.office_id.clone(),
            info.office_name.clone(),
            OfficeRole::Hq,
            info.org_id.clone(),
            info.org_name.clone(),
            info.created_at.clone(),
        )
    };
    // `office.json` is in place and durable: from here the office is stored, whatever happens.
    emit(&mut control, &ServerEvent::Created { office: file });
    serve_office(
        control,
        transfer,
        from_host,
        runtime,
        office,
        create.auth,
        create.instance,
    )
}

fn open_and_serve(mut control: Control, home: &Path, runtime: &Path, id: &str) -> i32 {
    let (message, from_host) = match first_line(&mut control, FirstLine::Start, Some(id)) {
        Ok(first) => first,
        Err(code) => return code,
    };
    let Launch::Start { key, auth, office } = message else {
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
    if let Err(message) = socket_path(runtime, id) {
        return fail(&mut control, FailCode::InvalidRequest, message);
    }
    let instance = office.instance_id;
    // The host's writer control comes first: a start without it is refused.
    let mut transfer = control.transfer();
    let admitted = match admit(&mut control, &mut transfer, id, &instance) {
        Ok(admitted) => admitted,
        Err(code) => return code,
    };
    // The office checks the admission's organization and roots against its creation intent.
    let opened = Office::open(folder, key, &instance, admitted.roots, admitted.custody);
    let office = match opened {
        Ok(office) => office,
        Err(e) => return fail_with(&mut control, &e),
    };
    match hq_creation::verify_existing(&office) {
        Ok(hq_creation::CaStatus::Unavailable) => {
            // No key material, path or SQL in the log: the office keeps serving its data.
            eprintln!(
                "agentic-org-office: organization CA unavailable: the office secrets key is \
                 absent or does not unwrap it"
            );
        }
        Ok(hq_creation::CaStatus::NotHq | hq_creation::CaStatus::Available) => {}
        Err(error) => return fail_with(&mut control, &error),
    }
    serve_office(
        control, transfer, from_host, runtime, office, auth, instance,
    )
}

/// Takes the host's writer control and its admission, before the office is created or
/// opened. `Err(code)` is the exit code: every office needs custody.
fn admit(
    control: &mut Control,
    transfer: &mut Option<startup::Transfer>,
    office_id: &str,
    instance: &str,
) -> Result<transfer::Admitted, i32> {
    let admitted = match (transfer.as_mut(), control.writer()) {
        (Some(transport), Some(out)) => {
            transfer::install_initial(transport, office_id, instance, &out)
        }
        _ => Err(std::io::Error::other("missing protected writer control")),
    };
    admitted.map_err(|error| {
        // The log (stderr) is the only place the cause goes; the host gets a fixed text.
        eprintln!("agentic-org-office: protected writer admission failed: {error}");
        log_causes(&error);
        fail(
            control,
            FailCode::StorageError,
            "protected writer admission requires recovery",
        )
    })
}

/// Reads the first line from the host. `Err(code)` is the exit code to return: 0 when the host
/// is gone; otherwise the failure is logged and reported on the rendezvous if one exists.
fn first_line(
    control: &mut Control,
    expected: FirstLine,
    id: Option<&str>,
) -> Result<(Launch, Lifetime), i32> {
    let result = control
        .read_for_office(expected, id)
        .and_then(|request| request.map(Launch::from_request).transpose());
    match result {
        Ok(Some(message)) => control
            .lifetime()
            .map(|lifetime| (message, lifetime))
            .map_err(|_| {
                fail(
                    control,
                    FailCode::StorageError,
                    "cannot read the control pipe",
                )
            }),
        Ok(None) => Err(0),
        Err(_) if control.failed_rights() => Err(1),
        Err(e) if e.is_host_gone() => Err(0),
        Err(e) => {
            let message = if matches!(e, LaunchError::Json(_)) {
                "invalid launch message".to_owned()
            } else {
                e.to_string()
            };
            Err(fail(control, e.fail_code(), &message))
        }
    }
}

/// Binds the socket, says `ready`, serves until the host goes away, a stop signal arrives or
/// the office's store stops, then closes the office and exits: 0 after an orderly stop, 1
/// when the store stopped (the host restarts it).
fn serve_office(
    mut control: Control,
    transfer: Option<startup::Transfer>,
    from_host: Lifetime,
    runtime: &Path,
    office: Office,
    auth: serve::LaunchAuth,
    instance: String,
) -> i32 {
    let info = office.info().clone();
    let office = Arc::new(office);
    let socket = match socket_path(runtime, &info.office_id) {
        Ok(socket) => socket,
        Err(message) => return fail(&mut control, FailCode::InvalidRequest, message),
    };
    let listener = match serve::bind(&socket) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("agentic-org-office: bind failed: {error}");
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
    // Nothing publishes live events while messaging, status lines and `subscribe` are not
    // served (B6): the ports keep an empty source.
    let live = Arc::new(live::LiveEvents::new(Box::new(|| {})));
    let inbox = match inbox::Hq::for_office(Arc::clone(&office)) {
        Ok(inbox) => inbox,
        Err(error) => {
            eprintln!("agentic-org-office: inbox clock initialization failed: {error}");
            log_causes(&error);
            let _ = std::fs::remove_file(&socket);
            return fail(
                &mut control,
                FailCode::StorageError,
                "cannot initialize inbox clock",
            );
        }
    };
    let mut shared = serve::Shared::new(
        &info,
        &socket,
        serve::OfficePorts {
            // Calls only the old tables served answer `unsupported` until their family is
            // rebuilt on the office store: work in B4, operation lookup in B2.
            work: Box::new(|_, _| Ok(Err(unsupported("work", "B4")))),
            operation_get: Box::new(|_| Ok(Err(unsupported("operation lookup", "B2")))),
            snapshot: {
                let office = Arc::clone(&office);
                Box::new(move || office.org_snapshot().map_err(|e| snapshot_failure(&e)))
            },
            who: {
                let office = Arc::clone(&office);
                // Status lines and runtime presence are unavailable until B6: everyone reads
                // as offline.
                Box::new(move |params| office.who(params).map_err(|e| logged("who", &e)))
            },
            status: Box::new(|_, _| Ok(Err(unsupported("status lines", "B6")))),
            live,
            feed: None,
            mailbox: None,
            directory: Arc::new(directory::OfficeDirectory::of(Arc::clone(&office))),
            submit: {
                let office = Arc::clone(&office);
                Box::new(move |member, command| {
                    office
                        .submit_org(member, command)
                        .map_err(|e| logged("command", &e))
                })
            },
        },
        auth,
    );
    // The organization's directory generation: the member watcher looks again when it moves.
    // A read that fails gives a value no generation has, so the next good read differs.
    let directory_office = Arc::clone(&office);
    shared.watch_directory_changes(move || {
        directory_office
            .org_snapshot()
            .map_or(u64::MAX, |snapshot| snapshot.position.seq)
    });
    shared.register_inbox(inbox, office.is_hq());
    // The host's lifetime is the authenticated rendezvous (stdin is drained elsewhere).
    // The fixed-buffer watcher never parses later input.
    let host_gone = Arc::new(AtomicBool::new(false));
    let watcher = {
        let host_gone = Arc::clone(&host_gone);
        std::thread::Builder::new()
            .name("org-office-control".into())
            .spawn(move || {
                startup::drain(from_host);
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

    announce_ready(&mut control, &socket, &mut shared);
    let shared = Arc::new(shared);

    let revocations = Arc::new(std::sync::Mutex::new(revocations::Revocations::default()));
    let transfers = match transfer
        .map(|transport| {
            let mut channels = transfer::Channels::new(
                Arc::clone(&shared),
                Arc::clone(&office),
                instance,
                control.writer().expect("launch rendezvous writer"),
                Arc::clone(&revocations),
            );
            transfer::Service::start(transport, revocations, move |channel| {
                channels.open(channel)
            })
        })
        .transpose()
    {
        Ok(service) => service,
        Err(_) => {
            let _ = std::fs::remove_file(&socket);
            eprintln!("agentic-org-office: cannot start transfer service");
            return 1;
        }
    };
    let stopped = Arc::new(AtomicBool::new(false));
    let stop = || {
        // A store that stopped (a failed flush or append) is fatal: log its fixed text and
        // exit 1 so the host restarts the office from what is on the drive.
        if let Some(fatal) = office.fatal() {
            if !stopped.swap(true, Ordering::SeqCst) {
                eprintln!("agentic-org-office: {fatal}");
            }
            return true;
        }
        host_gone.load(Ordering::SeqCst) || signals::stop_requested()
    };
    serve::run(&listener, &shared, &stop);
    drop(transfers);

    drop(listener);
    let _ = std::fs::remove_file(&socket);
    // Every connection thread is gone (joined by `serve::run`): the only owners of the office
    // left are `shared` and this function.
    drop(shared);
    if stopped.load(Ordering::SeqCst) {
        return 1;
    }
    close(office)
}

/// The orderly stop: the store closes (flushed, confirmed, written) and leaves its clean-stop
/// marker. An office something still holds is dropped instead: a stop without the marker.
fn close(office: Arc<Office>) -> i32 {
    match Arc::try_unwrap(office) {
        Ok(office) => match office.close() {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("agentic-org-office: the office did not close cleanly: {error}");
                log_causes(&error);
                1
            }
        },
        Err(office) => {
            eprintln!("agentic-org-office: the office is still in use at stop; it stops uncleanly");
            drop(office);
            0
        }
    }
}

/// Optional live hints cannot prevent the worker's Ready acknowledgement.
fn announce_ready(control: &mut Control, socket: &Path, shared: &mut serve::Shared) {
    if shared.enable_inbox_hints().is_err() {
        eprintln!("agentic-org-office: inbox hints disabled (cursor unavailable)");
    }
    emit(
        control,
        &ServerEvent::Ready {
            socket: socket.display().to_string(),
            pid: std::process::id(),
        },
    );
}

/// Preserve the organization's retryable availability failure across the read port.
fn snapshot_failure(error: &OfficeError) -> serve::SnapshotFailure {
    logged("snapshot", error);
    match error {
        OfficeError::SnapshotPending => serve::SnapshotFailure::Unavailable,
        _ => serve::SnapshotFailure::Storage,
    }
}

/// Logs why the office could not answer (to stderr, the host's log: `OfficeError`'s text has no
/// path, SQL or secret in it) and returns it for the connection, which tells the client only a
/// fixed text.
fn logged(what: &str, error: &OfficeError) -> String {
    eprintln!("agentic-org-office: {what} failed: {error}");
    log_causes(error);
    error.to_string()
}

/// The answer to a call this office does not serve yet: `what` is rebuilt on the office store
/// by increment `by`.
fn unsupported(what: &str, by: &str) -> ApiError {
    ApiError::new(
        org_proto::ErrorCode::Unsupported,
        format!("this office does not serve {what} yet ({by})"),
    )
}

/// The causes behind a storage failure (a path, the OS error, SQLite's text) belong in the log
/// and not on the pipe or the socket; they never hold the key or the token.
fn log_causes(error: &dyn std::error::Error) {
    let mut cause = std::error::Error::source(error);
    while let Some(inner) = cause {
        eprintln!("agentic-org-office:   caused by: {inner}");
        cause = inner.source();
    }
}

/// The office home: `ORG_OFFICE_HOME` when it is an absolute path, else this system's
/// application base for the user whose home folder `HOME` names (`org_platform::dirs`), the
/// rule the host and every client use. `None` if neither gives one.
fn office_home(
    override_home: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Option<std::path::PathBuf> {
    let base = home
        .map(std::path::PathBuf::from)
        .filter(|home| home.is_absolute())
        .and_then(|home| org_platform::dirs::application_base(&home).ok());
    office_home_from(override_home, base)
}

/// The runtime directory comes from the host's clean environment. No invalid override falls back.
fn runtime_dir_from(
    home: &Path,
    value: Option<std::ffi::OsString>,
    debug: bool,
) -> Result<std::path::PathBuf, &'static str> {
    match value {
        Some(value) if Path::new(&value).is_absolute() => Ok(value.into()),
        Some(_) => Err("office runtime directory must be absolute"),
        None if debug => Ok(home.join("run")),
        None => Err("release worker requires an explicit office runtime directory"),
    }
}

/// The office's socket path, or why it cannot be one.
fn socket_path(runtime: &Path, office_id: &str) -> Result<std::path::PathBuf, &'static str> {
    let socket = if org_proto::is_uuid_v7(office_id) {
        // The current office marker/profile and initial writer admission were
        // verified before serving. Shape alone never grants authority.
        org_proto::location::selected_office_socket_in(runtime, office_id)
    } else {
        office_socket_in(runtime, office_id)
    }
    .ok_or("not a plain office id")?;
    let limit = org_platform::dirs::socket_path_limit()
        .map_err(|_| "office sockets are not available on this system in this release")?;
    check_socket_path(&socket, limit).map_err(|_| "the socket path is too long")?;
    Ok(socket)
}

fn emit(control: &mut Control, event: &ServerEvent) {
    // The host may already be gone; the server notices at EOF.
    let _ = control.emit(event);
}

fn fail(control: &mut Control, code: FailCode, message: &str) -> i32 {
    // The host's log is this process's stderr: say what failed (a fixed text, never a secret).
    eprintln!("agentic-org-office: {message}");
    emit(
        control,
        &ServerEvent::Failed {
            code,
            message: message.to_owned(),
        },
    );
    1
}

fn fail_with(control: &mut Control, error: &OfficeError) -> i32 {
    log_causes(error);
    // `Display` never carries a path, SQL or a key.
    fail(control, fail_code(error), &error.to_string())
}

/// The code the host is told for an office error.
fn fail_code(error: &OfficeError) -> FailCode {
    match error {
        OfficeError::InvalidName(_) => FailCode::InvalidName,
        OfficeError::OfficeExists | OfficeError::AlreadyExists => FailCode::AlreadyExists,
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

    mod transfer_composition;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn the_office_home_is_the_override_or_the_systems_base_for_an_absolute_home() {
        let home = "/Users/me";
        let base = org_platform::dirs::application_base(Path::new(home)).ok();
        assert!(base.is_some());
        assert_eq!(
            office_home(Some("/tmp/o1".into()), Some(home.into())),
            Some("/tmp/o1".into())
        );
        assert_eq!(office_home(None, Some(home.into())), base);
        for ignored in ["", "tmp/o1"] {
            assert_eq!(office_home(Some(ignored.into()), Some(home.into())), base);
        }
        for no_home in [None, Some(""), Some("me")] {
            assert_eq!(office_home(None, no_home.map(Into::into)), None);
        }
        assert_eq!(office_home(Some("o1".into()), Some("me".into())), None);
    }

    #[test]
    fn explicit_runtime_is_required_in_release_and_invalid_values_never_fall_back() {
        let _spawn = crate::transfer::tests::without_process_spawn();
        let home = Path::new("/very/long/data/root");
        assert!(runtime_dir_from(home, None, false).is_err());
        assert_eq!(
            runtime_dir_from(home, None, true).unwrap(),
            home.join("run")
        );
        for debug in [false, true] {
            for invalid in ["", "relative"] {
                assert!(runtime_dir_from(home, Some(invalid.into()), debug).is_err());
            }
            let runtime = runtime_dir_from(home, Some("/short/runtime".into()), debug).unwrap();
            assert_eq!(
                socket_path(&runtime, "OFFICE1").unwrap(),
                Path::new("/short/runtime/OFFICE1.sock")
            );
        }
    }

    #[test]
    fn every_office_error_has_its_code() {
        let _spawn = crate::transfer::tests::without_process_spawn();
        use org_office::NameError;
        let storage = || OfficeError::Storage("disk".into());
        for (error, code) in [
            (
                OfficeError::InvalidName(NameError::Empty),
                FailCode::InvalidName,
            ),
            (OfficeError::OfficeExists, FailCode::AlreadyExists),
            (OfficeError::AlreadyExists, FailCode::AlreadyExists),
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
        let _spawn = crate::transfer::tests::without_process_spawn();
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
