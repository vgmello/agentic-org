//! # org-server: one process per office
//!
//! Started by the host on the launch protocol (`org-office-launch`):
//! ```text
//! org-server --create        # first stdin line: the create message; makes the office, serves it
//! org-server --office <id>   # first stdin line: the start message; serves the office
//! org-server --discard <id>  # first stdin line: the discard message (the key or null); removes an unfinished create
//! ```
//! V2 events travel on stdout and stdin EOF ends the worker. V3 events travel on the
//! authenticated rendezvous; its EOF closes the databases and exits 0. V3 stdin is drained
//! unparsed and is not a lifetime signal. Logs go to stderr.
//! The office, its folder, lock, key and databases are `org_office::Office`; this crate is the
//! process around it: launch control, typed authority and the socket. The server never touches
//! a Keychain.

#![deny(unsafe_code)]

mod bounded;
pub mod fdswap;
mod follow;
mod launch;
mod live;
mod msg_calls;
mod revocations;
mod serve;
mod signals;
mod startup;
mod transfer;
mod zeroed;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use launch::Launch;
use org_office::{Office, OfficeError};
use org_office_launch::{
    office_dir, FailCode, FirstLine, LaunchError, OfficeFile, ServerEvent, CREATE_FLAG,
    DISCARD_FLAG, OFFICE_FLAG,
};
use org_proto::location::{
    check_socket_path, default_office_home, office_socket_in, OFFICE_RUNTIME_DIR_ENV,
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
        eprintln!("org-server: internal error{place}");
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
        _ => Err("usage: org-server --create | --office <id> | --discard <id>"),
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
    let Some(home) = default_office_home() else {
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

/// `--discard`: the first line carries the office's key, or null when the host holds none. The
/// key is the host's and is never made, stored or deleted here.
fn discard(mut control: Control, home: &Path, id: &str) -> i32 {
    let (message, _from_host) = match first_line(&mut control, FirstLine::Discard, Some(id)) {
        Ok(first) => first,
        Err(code) => return code,
    };
    let Launch::Discard {
        key,
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
    match Office::discard(folder, id, key.as_ref()) {
        Ok(()) => {
            emit(&mut control, &ServerEvent::Discarded);
            0
        }
        // The contract: a discard that cannot check the databases reports `storage_error`, not
        // `key_missing` (a person looks; the host never retries it on its own).
        Err(OfficeError::KeyMissing) => fail(
            &mut control,
            FailCode::StorageError,
            "the key given does not open the databases",
        ),
        Err(e) => fail_with(&mut control, &e),
    }
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
    let _recovery = create._recovery;
    let _owner_member_id = create.owner_member_id;
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
    // The host holds the key; both versions use the same Office construction path.
    let created = match create.instance.as_deref() {
        Some(instance) => Office::create_for_instance(folder, &create.office, create.key, instance),
        None => Office::create(folder, &create.office, create.key),
    };
    let office = match created {
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
    serve_office(control, from_host, runtime, office, create.auth)
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
    let instance = office.map(|identity| {
        let _office_id = identity.office_id;
        identity.instance_id
    });
    let opened = match instance.as_deref() {
        Some(instance) => Office::open_for_instance(folder, key, instance),
        None => Office::open(folder, key),
    };
    let office = match opened {
        Ok(office) => office,
        Err(e) => return fail_with(&mut control, &e),
    };
    serve_office(control, from_host, runtime, office, auth)
}

/// Reads the first line from the host. `Err(code)` is the exit code to return: 0 when the host
/// is gone, otherwise a `failed` line has been written.
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

/// Binds the socket, says `ready`, serves until the host goes away or a stop signal arrives,
/// then closes the office (databases, lock) and exits 0.
fn serve_office(
    mut control: Control,
    from_host: Lifetime,
    runtime: &Path,
    office: Office,
    auth: serve::LaunchAuth,
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
            eprintln!("org-server: bind failed: {error}");
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
    // Live events (messages, status lines) for subscribers; a publish wakes the followers.
    let live = live_events(&office);
    let mailbox = match office.mailbox(live.clone()) {
        Ok(mailbox) => Arc::new(mailbox),
        Err(e) => {
            let _ = std::fs::remove_file(&socket);
            return fail_with(&mut control, &e);
        }
    };
    let shared = Arc::new(serve::Shared::new(
        &info,
        &socket,
        serve::OfficePorts {
            work: {
                let office = Arc::clone(&office);
                Box::new(move |caller, call| {
                    let method = call.method();
                    work_answer(method, office.work_call(caller, call))
                })
            },
            operation_get: {
                let office = Arc::clone(&office);
                Box::new(move |id| {
                    office
                        .org_operation_get(id)
                        .map_err(|e| logged("operation.get", &e))
                })
            },
            snapshot: {
                let office = Arc::clone(&office);
                Box::new(move || office.org_snapshot().map_err(|e| snapshot_failure(&e)))
            },
            who: {
                let office = Arc::clone(&office);
                Box::new(move |params| {
                    // Nothing reports presence yet: all offline, with the status lines set.
                    office
                        .who_with_status(params)
                        .map_err(|e| logged("who", &e))
                })
            },
            status: {
                let office = Arc::clone(&office);
                let live = Arc::clone(&live);
                // One status call at a time for a member, so the events go out in the order the
                // lines were set (two calls of one agent could otherwise publish out of commit
                // order). Other members' calls do not wait for each other.
                let turns = std::sync::Mutex::new(std::collections::HashMap::<
                    String,
                    Arc<std::sync::Mutex<()>>,
                >::new());
                Box::new(move |member, params| {
                    let turn = Arc::clone(
                        turns
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .entry(member.to_owned())
                            .or_default(),
                    );
                    let _turn = turn.lock().unwrap_or_else(|p| p.into_inner());
                    let answer = office
                        .set_status(member, params)
                        .map_err(|e| logged("status", &e))?;
                    if let Ok((set, current)) = &answer {
                        live.publish_presence(org_office::presence_after_status(
                            member,
                            set,
                            current.clone(),
                        ));
                    }
                    Ok(answer.map(|(set, _)| set))
                })
            },
            live: Arc::clone(&live),
            feed: Box::new(OfficeFeed(Arc::clone(&office))),
            mailbox,
            directory: Arc::new(office.member_directory()),
            submit: {
                let office = Arc::clone(&office);
                Box::new(move |command| {
                    office
                        .submit_org(command)
                        .map_err(|e| logged("command", &e))
                })
            },
        },
        auth,
    ));
    // The launch transport selects stdin for v2, authenticated rendezvous for v3.
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

    emit(
        &mut control,
        &ServerEvent::Ready {
            socket: socket.display().to_string(),
            pid: std::process::id(),
        },
    );

    let revocations = Arc::new(std::sync::Mutex::new(revocations::Revocations::default()));
    let transfers = match control
        .transfer()
        .map(|transport| transfer::Service::start(transport, revocations))
        .transpose()
    {
        Ok(service) => service,
        Err(_) => {
            let _ = std::fs::remove_file(&socket);
            return 1;
        }
    };
    let stop = || host_gone.load(Ordering::SeqCst) || signals::stop_requested();
    serve::run(&listener, &shared, &stop);
    drop(transfers);

    drop(listener);
    let _ = std::fs::remove_file(&socket);
    // Every connection thread is gone (joined by `serve::run`): the only owners of the office
    // left are `shared` and this function.
    drop(shared);
    drop(office);
    0
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
    eprintln!("org-server: {what} failed: {error}");
    log_causes(error);
    error.to_string()
}

/// Map committed or remembered work answers at the server composition boundary.
fn work_answer(
    method: &str,
    result: Result<org_work::TxnOutput, org_work::StoreError>,
) -> Result<Result<org_proto::work::WorkAccepted, ApiError>, String> {
    match result {
        // C2 publishes changed; the committed parts remain in work_feed.
        Ok(org_work::TxnOutput::Committed { accepted, .. }) => Ok(Ok(accepted)),
        Ok(org_work::TxnOutput::Refused(error)) => Ok(Err(error)),
        Ok(org_work::TxnOutput::Replayed(answer)) => Ok(answer),
        Err(error) => {
            eprintln!("org-server: {method} failed: {error}");
            log_causes(&error);
            Err(error.to_string())
        }
    }
}

/// The causes behind a storage failure (a path, the OS error, SQLite's text) belong in the log
/// and not on the pipe or the socket; they never hold the key or the token.
fn log_causes(error: &dyn std::error::Error) {
    let mut cause = std::error::Error::source(error);
    while let Some(inner) = cause {
        eprintln!("org-server:   caused by: {inner}");
        cause = inner.source();
    }
}

/// The live events of this office: a publish wakes the followers asleep on the office's change
/// signal, so a hint goes out at once and not at the end of their idle poll.
fn live_events(office: &Arc<Office>) -> Arc<live::LiveEvents> {
    let office = Arc::clone(office);
    Arc::new(live::LiveEvents::new(Box::new(move || {
        office.wake_followers();
    })))
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

/// Launch-v2 runtime comes from the host's clean environment. No invalid override falls back.
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
    let socket = office_socket_in(runtime, office_id).ok_or("not a plain office id")?;
    check_socket_path(&socket).map_err(|_| "the socket path is too long")?;
    Ok(socket)
}

fn emit(control: &mut Control, event: &ServerEvent) {
    // The host may already be gone; the server notices at EOF.
    let _ = control.emit(event);
}

fn fail(control: &mut Control, code: FailCode, message: &str) -> i32 {
    // The host's log is this process's stderr: say what failed (a fixed text, never a secret).
    eprintln!("org-server: {message}");
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
    use org_office::{Key, NewOffice};

    #[test]
    fn c1_production_work_adapter_preserves_committed_refused_and_replayed_answers() {
        use org_proto::ErrorCode;
        use org_work::{StoreError, TxnOutput};
        let accepted: org_proto::work::WorkAccepted = serde_json::from_value(serde_json::json!({
            "status":"committed", "position":{"incarnation":2,"seq":5}, "items":[]
        }))
        .unwrap();
        let refusal = ApiError::new(ErrorCode::Conflict, "changed since version 1");
        for result in [
            TxnOutput::Committed {
                accepted: accepted.clone(),
                changed: vec![],
            },
            TxnOutput::Replayed(Ok(accepted.clone())),
        ] {
            assert_eq!(
                work_answer("task.create", Ok(result)),
                Ok(Ok(accepted.clone()))
            );
        }
        for result in [
            TxnOutput::Refused(refusal.clone()),
            TxnOutput::Replayed(Err(refusal.clone())),
        ] {
            assert_eq!(
                work_answer("task.create", Ok(result)),
                Ok(Err(refusal.clone()))
            );
        }
        let error = StoreError::new(std::io::Error::other("private database path and SQL"));
        assert_eq!(
            work_answer("task.create", Err(error)),
            Err("the office could not read or write its work storage".into())
        );
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn explicit_runtime_is_required_in_release_and_invalid_values_never_fall_back() {
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
    fn a_publish_on_the_live_events_wakes_the_offices_followers() {
        // The production wiring: publishing moves the office's change token, which is what a
        // follower asleep in `wait_for_org_change` is waiting on.
        let dir = tempfile::Builder::new().prefix("lv").tempdir().unwrap();
        let office = Arc::new(
            Office::create(
                dir.path().join("offices").join("OFFICE1"),
                &NewOffice {
                    office_id: "OFFICE1".into(),
                    office_name: "HQ".into(),
                    role: org_office::OfficeRole::Hq,
                    org_id: "ORG1".into(),
                    org_name: "Acme".into(),
                },
                Key::from_db_key(org_proto::DbKey::from_bytes([4; 32])),
            )
            .unwrap(),
        );
        let live = live_events(&office);
        let before = office.org_change_token();
        live.publish_presence(org_proto::directory::MemberPresenceChangedData {
            member_id: "M".into(),
            presence: org_proto::directory::MemberPresence::Offline,
            last_seen_at: "2026-11-09T12:00:00.000Z".into(),
            status: None,
            status_expires_at: None,
            current_task_id: None,
        });
        assert_ne!(office.org_change_token(), before);
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
