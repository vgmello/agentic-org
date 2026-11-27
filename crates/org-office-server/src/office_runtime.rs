//! Serving an opened office: admission, runtime preparation, the serve loop, transfers, the session
//! manager, the channel watcher, readiness and close.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::failure::{emit, fail, log_causes};
use crate::federation_runtime::{
    build_federation_delivery_notifier, build_sync_wake_fn, FederationRuntime, HqFederationSlot,
};
use crate::federation_stream::SatSyncWake;
use crate::launch::Launch;
use crate::office_ports::{build_messaging_ports, build_office_ports};
use crate::startup::{self, Control, Lifetime};
use crate::{
    federation, inbox, live, org_feed, provider_refresh, revocations, serve, signals, transfer,
    watcher,
};
use org_office::Office;
use org_office_launch::{FailCode, FirstLine, LaunchError, ServerEvent};
use org_proto::location::{check_socket_path, office_socket_in};

/// Starts the host's writer control and its admission, before the office is created or
/// opened. `Err(code)` is the exit code: every office needs custody.
pub(crate) fn admit(
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
pub(crate) fn first_line(
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
struct PreparedOfficeRuntime {
    shared: serve::Shared,
    session_manager: Arc<org_office_sessions::SessionManager>,
    channel_watcher: Arc<watcher::ChannelWatcher>,
    federation_rt: FederationRuntime,
    host_gone: Arc<AtomicBool>,
}

/// HQ's inbox over `office`, with every source registered here, in the composition root: the
/// organization (the inbox's own), the mailbox and the budget. `inbox` imports neither
/// `mailbox_store` nor `cost_calls`; both implement its `Source` contract.
pub(crate) fn hq_inbox(office: Arc<Office>) -> std::io::Result<inbox::Hq> {
    let mailbox = crate::mailbox_store::MailboxInboxSource::new(Arc::clone(&office));
    let budget = crate::cost_calls::BudgetInboxSource::new(Arc::clone(&office));
    inbox::Hq::new(office, vec![Arc::new(mailbox), Arc::new(budget)])
}

fn prepare_office_runtime(
    control: &mut Control,
    office: &Arc<Office>,
    info: &org_office::OfficeInfo,
    socket: &Path,
    auth: serve::LaunchAuth,
    from_host: Lifetime,
    runtimes: org_office_sessions::RuntimeRegistry,
) -> Option<PreparedOfficeRuntime> {
    let wake = Arc::new(org_feed::FeedWake::new());
    let live_wake = Arc::clone(&wake);
    let live = Arc::new(live::LiveEvents::for_owner(
        Box::new(move || live_wake.notify()),
        office.owner_member_id().to_owned(),
    ));
    let inbox = match hq_inbox(Arc::clone(office)) {
        Ok(inbox) => inbox,
        Err(error) => {
            eprintln!("agentic-org-office: inbox clock initialization failed: {error}");
            log_causes(&error);
            let _ = std::fs::remove_file(socket);
            let _ = fail(
                control,
                FailCode::StorageError,
                "cannot initialize inbox clock",
            );
            return None;
        }
    };
    let session_manager = start_session_manager_with_runtimes(Arc::clone(office), socket, runtimes);
    let channel_watcher = start_channel_watcher(Arc::clone(office), &session_manager);
    let sat_sync_wake = Arc::new(SatSyncWake::default());
    let hq_fed_slot: HqFederationSlot = Arc::default();
    let sync_wake_fn =
        build_sync_wake_fn(office, Arc::clone(&sat_sync_wake), Arc::clone(&hq_fed_slot));
    let messaging = build_messaging_ports(
        office,
        &live,
        &wake,
        &session_manager,
        &channel_watcher,
        Arc::clone(&sync_wake_fn),
    );
    let delivery_notifier = build_federation_delivery_notifier(
        office.owner_member_id().to_owned(),
        &messaging.chat_svc,
        &messaging.mailbox,
        &live,
        &channel_watcher,
    );
    let hq_federation_listening = Arc::new(AtomicBool::new(false));
    let federation_rt = match FederationRuntime::start(
        office,
        &session_manager,
        &messaging.chat_svc,
        delivery_notifier,
        sat_sync_wake,
        Arc::clone(&wake),
        &hq_federation_listening,
    ) {
        Ok(rt) => rt,
        Err(reason) => {
            eprintln!("agentic-org-office: federation listener startup failed: {reason}");
            let _ = std::fs::remove_file(socket);
            let _ = fail(
                control,
                FailCode::StorageError,
                "HQ federation listener failed to bind",
            );
            return None;
        }
    };
    if let Some(handle) = federation_rt.hq_handle() {
        let _ = hq_fed_slot.set(handle);
    }
    let ports = build_office_ports(
        Arc::clone(office),
        Arc::clone(&session_manager),
        Arc::clone(&channel_watcher),
        live,
        &wake,
        messaging,
        Arc::clone(&hq_federation_listening),
        hq_fed_slot,
        sync_wake_fn,
    );
    let mut shared = serve::Shared::new(info, socket, ports, auth);
    let directory_office = Arc::clone(office);
    shared.watch_directory_changes(move || {
        directory_office
            .org_snapshot()
            .map_or(u64::MAX, |snapshot| snapshot.position.seq)
    });
    shared.register_inbox(inbox, office.is_hq());
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
        federation_rt.stop();
        let _ = std::fs::remove_file(socket);
        let _ = fail(
            control,
            FailCode::StorageError,
            "cannot watch the control pipe",
        );
        return None;
    }
    Some(PreparedOfficeRuntime {
        shared,
        session_manager,
        channel_watcher,
        federation_rt,
        host_gone,
    })
}

pub(crate) struct ServeOfficeRequest<'a> {
    pub(crate) control: Control,
    pub(crate) transfer: Option<startup::Transfer>,
    pub(crate) from_host: Lifetime,
    pub(crate) runtime: &'a Path,
    pub(crate) office: Office,
    pub(crate) auth: serve::LaunchAuth,
    pub(crate) instance: String,
    pub(crate) runtimes: org_office_sessions::RuntimeRegistry,
}

#[cfg(test)]
pub(crate) fn serve_office(
    control: Control,
    transfer: Option<startup::Transfer>,
    from_host: Lifetime,
    runtime: &Path,
    office: Office,
    auth: serve::LaunchAuth,
    instance: String,
) -> i32 {
    serve_office_with_runtimes(ServeOfficeRequest {
        control,
        transfer,
        from_host,
        runtime,
        office,
        auth,
        instance,
        runtimes: org_office_sessions::RuntimeRegistry::production(),
    })
}

pub(crate) fn serve_office_with_runtimes(req: ServeOfficeRequest<'_>) -> i32 {
    let ServeOfficeRequest {
        mut control,
        transfer,
        from_host,
        runtime,
        office,
        auth,
        instance,
        runtimes,
    } = req;
    let info = office.info().clone();
    let office = Arc::new(office);
    let socket = match socket_path(runtime, &info.office_id) {
        Ok(socket) => socket,
        Err(message) => return fail(&mut control, FailCode::InvalidRequest, message),
    };
    if let Err(error) = prepare_socket_dir(&socket) {
        eprintln!("agentic-org-office: socket folder refused: {error}");
        return fail(
            &mut control,
            FailCode::StorageError,
            "cannot prepare the socket folder",
        );
    }
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
    signals::install();
    let Some(mut prepared) = prepare_office_runtime(
        &mut control,
        &office,
        &info,
        &socket,
        auth,
        from_host,
        runtimes,
    ) else {
        return 1;
    };
    announce_ready(&mut control, &socket, &mut prepared.shared);
    let shared = Arc::new(prepared.shared);
    let transfers = match start_transfers(transfer, &shared, &office, instance, &mut control) {
        Ok(service) => service,
        Err(_) => {
            prepared.federation_rt.stop();
            let _ = std::fs::remove_file(&socket);
            eprintln!("agentic-org-office: cannot start transfer service");
            return 1;
        }
    };
    let stopped = Arc::new(AtomicBool::new(false));
    let stop = || {
        if let Some(fatal) = office.fatal() {
            if !stopped.swap(true, Ordering::SeqCst) {
                eprintln!("agentic-org-office: {fatal}");
            }
            return true;
        }
        prepared.host_gone.load(Ordering::SeqCst) || signals::stop_requested()
    };
    run_serve_loop(
        &office,
        &listener,
        &shared,
        &prepared.session_manager,
        &prepared.channel_watcher,
        &stop,
    );
    prepared.federation_rt.stop();
    drop(transfers);
    drop(listener);
    let _ = std::fs::remove_file(&socket);
    drop(shared);
    drop(prepared.channel_watcher);
    drop(prepared.session_manager);
    if stopped.load(Ordering::SeqCst) {
        return 1;
    }
    close(office)
}

/// The orderly stop: the store closes (flushed, confirmed, written) and leaves its clean-stop
/// marker. An office something still holds is dropped instead: a stop without the marker.
fn run_serve_loop(
    office: &Arc<Office>,
    listener: &std::os::unix::net::UnixListener,
    shared: &Arc<serve::Shared>,
    session_manager: &Arc<org_office_sessions::SessionManager>,
    channel_watcher: &Arc<watcher::ChannelWatcher>,
    stop: &dyn Fn() -> bool,
) {
    let wake_stop = Arc::new(AtomicBool::new(false));
    let wake_thread = org_office_sessions::spawn_wake_retry_loop(
        Arc::clone(session_manager),
        Arc::clone(&wake_stop),
        std::time::Duration::from_secs(2),
    );
    let watch_thread = watcher::spawn_watcher_loop(
        Arc::clone(channel_watcher),
        Arc::clone(&wake_stop),
        std::time::Duration::from_secs(2),
    );
    let readiness_thread = provider_refresh::spawn_readiness_refresh_loop(
        Arc::clone(office),
        Arc::clone(&wake_stop),
        session_manager.runtimes().clone(),
    );
    let snooze_thread = shared.mailbox().map(|mailbox| {
        let stop_flag = Arc::clone(&wake_stop);
        std::thread::Builder::new()
            .name("snooze-resurfacer".into())
            .spawn(move || {
                while !stop_flag.load(Ordering::SeqCst) {
                    if let Err(err) = mailbox.release_all_due_snoozes() {
                        eprintln!("agentic-org-office: snooze resurfacer error: {err}");
                    }
                    std::thread::sleep(std::time::Duration::from_millis(250));
                }
            })
            .ok()
    });
    serve::run(listener, shared, stop);
    wake_stop.store(true, Ordering::SeqCst);
    if let Some(t) = wake_thread {
        let _ = t.join();
    }
    let _ = watch_thread.join();
    if let Some(t) = readiness_thread {
        let _ = t.join();
    }
    if let Some(Some(t)) = snooze_thread {
        let _ = t.join();
    }
    session_manager.shutdown("office shutdown");
}

fn start_transfers(
    transfer: Option<startup::Transfer>,
    shared: &Arc<serve::Shared>,
    office: &Arc<Office>,
    instance: String,
    control: &mut Control,
) -> std::io::Result<Option<transfer::Service>> {
    let revocations = Arc::new(std::sync::Mutex::new(revocations::Revocations::default()));
    transfer
        .map(|transport| {
            let mut channels = transfer::Channels::new(
                Arc::clone(shared),
                Arc::clone(office),
                instance,
                control.writer().expect("launch rendezvous writer"),
                Arc::clone(&revocations),
            );
            transfer::Service::start(transport, revocations, move |channel| {
                channels.open(channel)
            })
        })
        .transpose()
}

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
pub(crate) fn announce_ready(control: &mut Control, socket: &Path, shared: &mut serve::Shared) {
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

/// Initializes the production C3 `SessionManager` and runs startup recovery scan.
/// Wires the per-channel question watcher to `session_manager` using a weak notice callback so
/// neither side creates a strong reference cycle at office shutdown.
fn start_channel_watcher(
    office: Arc<Office>,
    session_manager: &Arc<org_office_sessions::SessionManager>,
) -> Arc<watcher::ChannelWatcher> {
    let channel_watcher = Arc::new(watcher::ChannelWatcher::production(
        office,
        Some(Arc::clone(session_manager)),
    ));
    let notices_watcher = Arc::downgrade(&channel_watcher);
    session_manager.set_watch_notice_provider(Arc::new(move |emp_id| {
        notices_watcher
            .upgrade()
            .map(|w| w.drain_watch_end_notices(emp_id))
            .unwrap_or_default()
    }));
    channel_watcher
}

/// Federation-backed [`org_office_sessions::TurnAttachmentResolver`] for satellite offices:
/// authorizes reads locally and pulls missing shared blobs from HQ via [`federation::HqBlobRemote`].
struct SatelliteAttachmentResolver {
    office: Arc<Office>,
}

impl SatelliteAttachmentResolver {
    fn new(office: Arc<Office>) -> Self {
        Self { office }
    }
}

impl org_office_sessions::TurnAttachmentResolver for SatelliteAttachmentResolver {
    fn resolve_for_turn(
        &self,
        employee_id: &str,
        delivery_scope: &org_office::attachments::DeliveryScope,
        attachment_id: &str,
        remaining_staging_budget: u64,
    ) -> Result<
        Result<org_office::attachments::DeliveryAttachmentPayload, org_proto::ApiError>,
        org_office::OfficeError,
    > {
        let remote = federation::HqBlobRemote::new(Arc::clone(&self.office));
        let pull = org_office::attachments::remote::BlobPull {
            remote: &remote,
            cache_max_bytes: federation::blob_cache_max_bytes(),
        };
        self.office.attachment_read_for_delivery_with(
            employee_id,
            attachment_id,
            delivery_scope,
            remaining_staging_budget,
            Some(pull),
        )
    }
}

#[cfg(test)]
pub(crate) fn start_session_manager(
    office: Arc<Office>,
    socket: &Path,
) -> Arc<org_office_sessions::SessionManager> {
    start_session_manager_with_runtimes(
        office,
        socket,
        org_office_sessions::RuntimeRegistry::production(),
    )
}

pub(crate) fn start_session_manager_with_runtimes(
    office: Arc<Office>,
    socket: &Path,
    runtimes: org_office_sessions::RuntimeRegistry,
) -> Arc<org_office_sessions::SessionManager> {
    let mut cfg = org_office_sessions::SessionManagerConfig::production(socket.to_path_buf());
    cfg.runtimes = runtimes;
    if office.info().role == org_office::OfficeRole::Satellite {
        cfg.hq_permits = Some(Arc::new(federation::SatelliteHqPermitCoordinator::new(
            Arc::clone(&office),
        )));
        cfg.attachment_resolver = Some(Arc::new(SatelliteAttachmentResolver::new(Arc::clone(
            &office,
        ))));
    }
    let needs_snapshot = office
        .sync_status()
        .is_ok_and(|s| s.initial_snapshot_required);
    let session_manager = Arc::new(org_office_sessions::SessionManager::new(office, cfg));
    org_office_sessions::SessionManager::attach_self(&session_manager);
    // Defer session recovery when a satellite is awaiting a complete HQ snapshot so a restart
    // mid-snapshot never recovers agents against partially replaced state (`TASK-52` P8-#4).
    if !needs_snapshot {
        match session_manager.recover_on_startup() {
            Ok(report) if !report.actions.is_empty() => {
                eprintln!(
                    "agentic-org-office: startup session recovery recorded {} action(s)",
                    report.actions.len()
                );
            }
            Ok(_) => {}
            Err(error) => {
                eprintln!("agentic-org-office: startup session recovery scan failed: {error}");
            }
        }
    }
    session_manager
}

/// Makes the office sockets folder (`<run>/o`) and this office's own socket folder inside it
/// (`<run>/o/<handle>`, the socket's parent; `org_proto::location`) owner-only before binding,
/// in that order: each is created 0700 when missing (a host-prepared runtime folder already has
/// `o/`; a standalone or test one may not), tightened to exactly 0700 when it exists, and refused
/// when it is a link or not this account's. Only those two are created: a missing runtime folder
/// fails here.
pub(crate) fn prepare_socket_dir(socket: &Path) -> std::io::Result<()> {
    let socket_dir = socket
        .parent()
        .ok_or_else(|| std::io::Error::other("the socket has no folder"))?;
    let sockets_dir = socket_dir
        .parent()
        .ok_or_else(|| std::io::Error::other("the socket folder has no parent"))?;
    for dir in [sockets_dir, socket_dir] {
        match org_platform::fs::create_owner_only_dir(dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        org_platform::fs::set_owner_only_dir(dir)?;
    }
    Ok(())
}

/// The office's socket path, or why it cannot be one.
pub(crate) fn socket_path(
    runtime: &Path,
    office_id: &str,
) -> Result<std::path::PathBuf, &'static str> {
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
