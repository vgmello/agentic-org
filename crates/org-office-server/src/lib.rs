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
mod chat_calls;
mod cost_calls;
mod directory;
pub mod fdswap;
pub mod federation;
mod federation_calls;
mod follow;
mod git_calls;
mod grant;
mod hq_creation;
pub mod inbox;
mod launch;
mod live;
mod mailbox_store;
mod msg_calls;
mod org_feed;
mod process_calls;
mod provider_calls;
mod revocations;
mod serve;
mod signals;
mod snapshot_transfer;
mod startup;
mod task_calls;
#[cfg(any(test, all(feature = "test-support", debug_assertions)))]
pub mod test_support;
mod transfer;
mod wake_bridge;
mod watcher;
mod watcher_store;
mod zeroed;

#[cfg(test)]
mod a2a_live_three_runtimes_tests;
#[cfg(test)]
mod a2a_messaging_tests;
#[cfg(test)]
mod test_teardown;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use launch::Launch;
use org_office::Office;
use org_office::OfficeError;
use org_office_launch::{
    office_dir, FailCode, FirstLine, InspectImportPerson, InspectImportPreview,
    InspectImportRequest, InspectImportResponse, LaunchError, OfficeFile, ServerEvent, CREATE_FLAG,
    DISCARD_FLAG, INSPECT_IMPORT_FLAG, JOIN_FLAG, OFFICE_FLAG,
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
    Join,
    Serve(String),
    Discard(String),
    InspectImport,
}

fn parse_args(args: &[String]) -> Result<Mode, &'static str> {
    match args {
        [flag] if flag == CREATE_FLAG => Ok(Mode::Create),
        [flag] if flag == JOIN_FLAG => Ok(Mode::Join),
        [flag, id] if flag == OFFICE_FLAG => Ok(Mode::Serve(id.clone())),
        [flag, id] if flag == DISCARD_FLAG => Ok(Mode::Discard(id.clone())),
        [flag] if flag == INSPECT_IMPORT_FLAG => Ok(Mode::InspectImport),
        _ => Err(
            "usage: agentic-org-office --create | --join | --office <id> | --discard <id> | --inspect-import",
        ),
    }
}

/// Runs the server and returns its exit code.
pub fn run(args: &[String], control: ControlPipe) -> i32 {
    if matches!(parse_args(args), Ok(Mode::InspectImport)) {
        return inspect_import(control);
    }
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
        Mode::Join => join_and_serve(control, &home, &runtime),
        Mode::Serve(id) => open_and_serve(control, &home, &runtime, &id),
        Mode::InspectImport => unreachable!(),
    }
}

/// `--inspect-import`: reads one JSON request line from `control.from_host`, clones and
/// validates the organization's Git state branch read-only via `org_office::import`, and writes
/// one JSON [`InspectImportResponse`] line to `control.to_host`.
fn inspect_import(mut control: ControlPipe) -> i32 {
    use std::io::{BufRead, BufReader, Read, Write};
    let mut line = String::new();
    let read_res = BufReader::new(&mut control.from_host)
        .take(16 * 1024)
        .read_line(&mut line);
    let response = match read_res
        .ok()
        .and_then(|n| (n > 0).then(|| serde_json::from_str::<InspectImportRequest>(&line).ok()))
        .flatten()
    {
        None => InspectImportResponse::Error {
            code: "invalid_request".into(),
            path: None,
            reason: "invalid inspect-import request".into(),
        },
        Some(req) => match org_office::import::clone_and_validate_state(
            &req.repository_remote_url,
            &req.state_branch,
        ) {
            Ok(imported) => InspectImportResponse::Ok {
                preview: Box::new(InspectImportPreview {
                    organization_id: imported.summary.organization_id.as_str().to_owned(),
                    name: imported.summary.name,
                    slug: imported.summary.slug,
                    repository_remote_url: imported.repository_remote_url,
                    canonical_remote_url: imported.summary.canonical_remote_url,
                    state_branch: imported.state_branch,
                    commit_oid: imported.commit_oid,
                    teams_count: imported.summary.teams_count,
                    projects_count: imported.summary.projects_count,
                    agents_count: imported.summary.agents_count,
                    people: imported
                        .summary
                        .people
                        .into_iter()
                        .map(|p| InspectImportPerson {
                            id: p.id,
                            name: p.name,
                            handle: p.handle,
                            home_team_id: p.home_team_id,
                        })
                        .collect(),
                    reserved_handles: imported.summary.reserved_handles,
                    reserved_entity_ids: imported.summary.reserved_entity_ids,
                    has_active_root_team: imported.summary.has_active_root_team,
                    local_only_not_restored: imported.local_only_not_restored,
                }),
            },
            Err(org_office::import::ImportError::InvalidFile { path, reason }) => {
                InspectImportResponse::Error {
                    code: "invalid_file".into(),
                    path: Some(path),
                    reason,
                }
            }
            Err(org_office::import::ImportError::GitFailed(reason)) => {
                InspectImportResponse::Error {
                    code: "git_failed".into(),
                    path: None,
                    reason,
                }
            }
            Err(org_office::import::ImportError::InvalidInput(reason)) => {
                InspectImportResponse::Error {
                    code: "invalid_request".into(),
                    path: None,
                    reason,
                }
            }
            Err(org_office::import::ImportError::Storage(reason)) => InspectImportResponse::Error {
                code: "storage_error".into(),
                path: None,
                reason,
            },
        },
    };
    if let Ok(json) = serde_json::to_string(&response) {
        let _ = writeln!(control.to_host, "{json}");
        let _ = control.to_host.flush();
    }
    0
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
    let context = create.hq_creation;
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
    let import_source = context.import_source;
    let binding = org_office::hq_creation::Binding {
        operation_id: context.operation_id,
        owner_member_id: create.owner_member_id,
        bootstrap: *context.bootstrap,
    };
    let created = hq_creation::prepare(
        &folder,
        &create.office,
        Some(create.key),
        &binding,
        admitted.roots,
        admitted.custody,
        recovery,
        import_source.as_deref(),
    );
    let office = match created {
        Ok(office) => office,
        Err(e) => return fail_with(&mut control, &e),
    };
    let info = office.info();
    // The office is created with UUIDv7 ids only, so its `office.json` is version 2.
    let file = match OfficeFile::selected(
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

fn join_and_serve(mut control: Control, home: &Path, runtime: &Path) -> i32 {
    let (message, from_host) = match first_line(&mut control, FirstLine::Join, None) {
        Ok(first) => first,
        Err(code) => return code,
    };
    let Launch::Join(join) = message else {
        return fail(&mut control, FailCode::InvalidRequest, "not a join message");
    };
    let Some((folder, token)) = validate_join_launch(&mut control, home, runtime, &join) else {
        return 1;
    };
    let mut transfer = control.transfer();
    let admitted = match admit(&mut control, &mut transfer, &join.office_id, &join.instance) {
        Ok(admitted) => admitted,
        Err(code) => return code,
    };
    let Some(office) = run_join_enrollment_and_sync(&mut control, &folder, &join, &token, admitted)
    else {
        return 1;
    };
    serve_office(
        control,
        transfer,
        from_host,
        runtime,
        office,
        join.auth,
        join.instance,
    )
}

fn validate_join_launch(
    control: &mut Control,
    home: &Path,
    runtime: &Path,
    join: &launch::Join,
) -> Option<(std::path::PathBuf, org_pki::invitation::InvitationToken)> {
    let Some(folder) = office_dir(home, &join.office_id) else {
        let _ = fail(control, FailCode::InvalidRequest, "not a plain office id");
        return None;
    };
    if let Err(message) = socket_path(runtime, &join.office_id) {
        let _ = fail(control, FailCode::InvalidRequest, message);
        return None;
    }
    let Ok(token) = org_pki::invitation::InvitationToken::decode(join.invitation.expose()) else {
        let _ = fail(
            control,
            FailCode::InvalidRequest,
            "invalid satellite invitation token",
        );
        return None;
    };
    if token.office_id != join.office_id {
        let _ = fail(
            control,
            FailCode::InvalidRequest,
            "invitation office_id does not match join request",
        );
        return None;
    }
    Some((folder, token))
}

fn run_join_enrollment_and_sync(
    control: &mut Control,
    folder: &Path,
    join: &launch::Join,
    token: &org_pki::invitation::InvitationToken,
    admitted: crate::transfer::Admitted,
) -> Option<Office> {
    let (req_id, local_key) =
        match load_or_create_pending_joining_key(folder, &token.org_id, &join.office_id, &join.key)
        {
            Ok(pair) => pair,
            Err(msg) => {
                let _ = fail(control, FailCode::StorageError, &msg);
                return None;
            }
        };
    let enrolled = match federation::enroll_satellite_with_hq(token, &local_key, |check_code| {
        emit(
            control,
            &ServerEvent::JoinPending {
                request_id: req_id.clone(),
                check_code: check_code.to_owned(),
            },
        );
    }) {
        Ok(bundle) => bundle,
        Err(reason) => {
            let _ = fail(control, FailCode::InvalidRequest, &reason);
            return None;
        }
    };
    let office = match open_or_provision_joined_satellite(
        folder, join, token, enrolled, &local_key, admitted,
    ) {
        Ok(o) => Arc::new(o),
        Err(e) => {
            let _ = fail_with(control, &e);
            return None;
        }
    };
    finalize_joined_satellite_sync(control, folder, join.re_enroll, office)
}

fn finalize_joined_satellite_sync(
    control: &mut Control,
    folder: &Path,
    re_enroll: bool,
    office_arc: Arc<Office>,
) -> Option<Office> {
    // Gate `office.json`, pending key removal, `Joined`, and readiness on durable initial
    // snapshot installation so a failed first snapshot remains cleanly resumable (`TASK-52` H-4).
    let mut session = match federation::SatelliteSyncSession::connect(Arc::clone(&office_arc)) {
        Ok(s) => s,
        Err(reason) => {
            let _ = fail(control, FailCode::StorageError, &reason);
            return None;
        }
    };
    let op_id = office_arc.next_uuid_v7();
    if let Err(reason) = session.sync_initial_snapshot(&op_id) {
        let _ = fail(control, FailCode::StorageError, &reason);
        return None;
    }
    drop(session);
    if !re_enroll {
        if let Err(e) = office_arc.publish_satellite_office_file() {
            let _ = fail_with(control, &e);
            return None;
        }
    }
    let _ = std::fs::remove_file(folder.join("pending-joining-key.v1.json"));
    let info = office_arc.info();
    let office_file = match OfficeFile::selected(
        &info.office_id,
        &info.office_name,
        OfficeRole::Satellite,
        &info.org_id,
        &info.org_name,
        &info.created_at,
    ) {
        Ok(f) => f,
        Err(e) => {
            let _ = fail(
                control,
                FailCode::StorageError,
                &format!("joined satellite office file invalid: {e}"),
            );
            return None;
        }
    };
    emit(
        control,
        &ServerEvent::Joined {
            office: office_file,
        },
    );
    match Arc::try_unwrap(office_arc) {
        Ok(o) => Some(o),
        Err(_) => {
            let _ = fail(
                control,
                FailCode::StorageError,
                "satellite office still referenced after initial sync",
            );
            None
        }
    }
}

fn open_or_provision_joined_satellite(
    folder: &Path,
    join: &launch::Join,
    token: &org_pki::invitation::InvitationToken,
    enrolled: federation::EnrolledCertificateBundle,
    local_key: &org_pki::SigningKey,
    admitted: transfer::Admitted,
) -> Result<Office, OfficeError> {
    if join.re_enroll {
        let opened = Office::open_as_owner(
            folder,
            Some(join.key.clone()),
            &join.instance,
            admitted.roots,
            admitted.custody,
            &admitted.admission.owner_member,
        )?;
        opened.re_enroll_satellite(org_office::SatelliteReEnrollSpec {
            org_id: token.org_id.clone(),
            new_hq_office_id: enrolled.hq_office_id,
            new_hq_addresses: token.hq_addresses.clone(),
            new_epoch: enrolled.epoch,
            new_ca_cert_der: enrolled.ca_cert_der,
            new_office_cert_der: enrolled.cert_der,
            new_office_key_der: org_pki::Zeroizing::new(local_key.local_der().to_vec()),
        })?;
        Ok(opened)
    } else {
        Office::provision_satellite_pending(
            folder,
            org_office::SatelliteProvisionSpec {
                new: org_office::NewOffice {
                    office_id: join.office_id.clone(),
                    office_name: join.office_name.clone(),
                    role: org_office::OfficeRole::Satellite,
                    org_id: token.org_id.clone(),
                    org_name: token.org_name.clone(),
                },
                operation_id: join.operation_id.clone(),
                hq_office_id: enrolled.hq_office_id,
                hq_addresses: token.hq_addresses.clone(),
                epoch: enrolled.epoch,
                ca_cert_der: enrolled.ca_cert_der,
                office_cert_der: enrolled.cert_der,
                office_key_der: org_pki::Zeroizing::new(local_key.local_der().to_vec()),
            },
            join.key.clone(),
            admitted.roots,
            admitted.custody,
        )
    }
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
    // The office checks the admission's organization and roots against its creation intent,
    // and binds the host-authoritative current organization owner (which may have been handed over).
    let opened = Office::open_as_owner(
        folder,
        key,
        &instance,
        admitted.roots,
        admitted.custody,
        &admitted.admission.owner_member,
    );
    let office = match opened {
        Ok(office) => office,
        Err(e) => return fail_with(&mut control, &e),
    };
    // Verify writer custody against the opened office's role (`TASK-52` #1):
    // - HQ requires the admitted `owner_member` to match the canonical organization owner.
    // - Satellites verify custody identity (`office_id`, `organization_id`, `owner_member == office_id`).
    let custody_ok = match office.info().role {
        org_office::OfficeRole::Satellite => Ok(admitted.admission.owner_member
            == office.info().office_id
            && admitted.admission.identity.office_id == office.info().office_id
            && admitted.admission.identity.organization_id == office.info().org_id),
        _ => office.org_snapshot().map(|snapshot| {
            snapshot
                .members
                .into_iter()
                .find(|member| member.owner)
                .is_some_and(|owner| owner.id == admitted.admission.owner_member)
        }),
    };
    match custody_ok {
        Ok(true) => {}
        Ok(false) => {
            // Nothing was appended: the orderly stop puts back the clean-stop marker the
            // opening consumed.
            if let Err(error) = office.close() {
                eprintln!("agentic-org-office: closing the refused office failed: {error}");
            }
            return fail(
                &mut control,
                FailCode::StorageError,
                "protected writer admission requires recovery",
            );
        }
        Err(e) => return fail_with(&mut control, &e),
    }
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

#[derive(serde::Serialize, serde::Deserialize)]
struct PendingJoiningKeyFile {
    v: u32,
    request_id: String,
    wrapped_key_b64: String,
}

fn try_load_pending_joining_key(
    path: &Path,
    secrets_key: &org_office::Key,
    binding: &org_pki::wrap::WrapBinding<'_>,
) -> Option<(String, org_pki::SigningKey)> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let bytes = std::fs::read(path).ok()?;
    let saved: PendingJoiningKeyFile = serde_json::from_slice(&bytes).ok()?;
    let wrapped = STANDARD.decode(&saved.wrapped_key_b64).ok()?;
    let raw = org_pki::wrap::unwrap(secrets_key.expose_bytes(), binding, &wrapped).ok()?;
    let key = org_pki::SigningKey::from_local_der(&raw).ok()?;
    Some((saved.request_id, key))
}

/// Persists a wrapped satellite joining TLS key + request_id before first contact with HQ so
/// worker restarts during check-code approval reuse the exact same key & check code (`TASK-52` P2-4).
fn load_or_create_pending_joining_key(
    folder: &Path,
    org_id: &str,
    office_id: &str,
    secrets_key: &org_office::Key,
) -> Result<(String, org_pki::SigningKey), String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use org_pki::wrap::{self, WrapBinding, WrapPurpose};
    let path = folder.join("pending-joining-key.v1.json");
    let binding = WrapBinding {
        organization_id: org_id,
        office_id,
        purpose: WrapPurpose::OfficeTlsKey,
    };
    if let Some(existing) = try_load_pending_joining_key(&path, secrets_key, &binding) {
        return Ok(existing);
    }
    let key = org_pki::SigningKey::generate()
        .map_err(|_| "failed generating satellite TLS key".to_owned())?;
    let wrapped = wrap::wrap(secrets_key.expose_bytes(), &binding, key.local_der())
        .map_err(|_| "failed wrapping pending joining key".to_owned())?;
    let request_id = format!("req-{office_id}");
    let payload = serde_json::to_vec_pretty(&PendingJoiningKeyFile {
        v: 1,
        request_id: request_id.clone(),
        wrapped_key_b64: STANDARD.encode(wrapped),
    })
    .map_err(|e| e.to_string())?;
    let _ = std::fs::create_dir_all(folder);
    org_store::publish_one(folder, "pending-joining-key.v1.json", &payload, true)
        .map_err(|e| format!("failed persisting pending joining key: {e}"))?;
    Ok((request_id, key))
}

/// Condvar wake signal for the satellite background sync loop so outbound cross-office
/// messages/answers/withdrawals/accepts flush immediately instead of waiting for the idle poll.
#[derive(Default)]
struct SatSyncWake {
    gen: std::sync::Mutex<u64>,
    cv: std::sync::Condvar,
}

impl SatSyncWake {
    fn notify(&self) {
        let mut g = self.gen.lock().unwrap_or_else(|p| p.into_inner());
        *g = g.saturating_add(1);
        self.cv.notify_all();
    }

    fn generation(&self) -> u64 {
        *self.gen.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn wait_or_stop(&self, seen: u64, timeout: std::time::Duration, stop: &AtomicBool) -> u64 {
        let deadline = std::time::Instant::now() + timeout;
        let mut g = self.gen.lock().unwrap_or_else(|p| p.into_inner());
        while *g == seen && !stop.load(Ordering::SeqCst) {
            let now = std::time::Instant::now();
            if now >= deadline {
                break;
            }
            let step = deadline
                .saturating_duration_since(now)
                .min(std::time::Duration::from_millis(100));
            let (next, _) = self
                .cv
                .wait_timeout(g, step)
                .unwrap_or_else(|p| p.into_inner());
            g = next;
        }
        *g
    }
}

struct FederationRuntime {
    hq: Option<federation::HqFederationServer>,
    sat_stop: Arc<AtomicBool>,
    sat_wake: Arc<SatSyncWake>,
    sat_thread: Option<std::thread::JoinHandle<()>>,
}

impl FederationRuntime {
    fn start(
        office: &Arc<Office>,
        session_manager: &Arc<org_office_sessions::SessionManager>,
        delivery_notifier: federation::InboundDeliveryNotifier,
        sat_wake: Arc<SatSyncWake>,
        hq_listening: &Arc<AtomicBool>,
    ) -> Result<Self, String> {
        let sat_stop = Arc::new(AtomicBool::new(false));
        let wake_handler = build_federation_wake_handler(session_manager);
        let hq = start_optional_hq_federation(
            office,
            Some(Arc::clone(&wake_handler)),
            Some(Arc::clone(&delivery_notifier)),
        )?;
        hq_listening.store(hq.is_some(), Ordering::SeqCst);
        let sat_thread = start_optional_satellite_sync(
            office,
            &sat_stop,
            Arc::clone(&sat_wake),
            wake_handler,
            delivery_notifier,
            Arc::downgrade(session_manager),
        );
        Ok(Self {
            hq,
            sat_thread,
            sat_stop,
            sat_wake,
        })
    }

    fn stop(self) {
        if let Some(fed) = self.hq {
            fed.stop();
        }
        self.sat_stop.store(true, Ordering::SeqCst);
        self.sat_wake.notify();
        if let Some(handle) = self.sat_thread {
            let _ = handle.join();
        }
    }
}

fn build_federation_delivery_notifier(
    owner_id: String,
    chat_svc: &Arc<chat_calls::ChatService>,
    mailbox: &Arc<org_mailbox::Mailbox>,
    live: &Arc<live::LiveEvents>,
    watcher: &Arc<watcher::ChannelWatcher>,
) -> federation::InboundDeliveryNotifier {
    let chat_svc = Arc::downgrade(chat_svc);
    let mailbox = Arc::downgrade(mailbox);
    let live = Arc::downgrade(live);
    let watcher = Arc::downgrade(watcher);
    Arc::new(move |outcome: &org_office::RelayedMessageOutcome| {
        if let Some(ask_id) = outcome.closed_ask_id.as_deref() {
            if let Some(chat) = chat_svc.upgrade() {
                chat.notify_ask_waiters();
            }
            if let Some(w) = watcher.upgrade() {
                let reason = outcome.closed_ask_reason.as_deref().unwrap_or("answered");
                w.on_question_closed(ask_id, reason);
            }
            if let Some(lv) = live.upgrade() {
                lv.record_mailbox_change();
            }
        }
        let mut owner_touched = outcome.handoff_accepted;
        for header in &outcome.local_delivered_headers {
            if let Some(mb) = mailbox.upgrade() {
                mb.notify_member(&header.to_member_id);
            }
            if header.to_member_id == owner_id {
                owner_touched = true;
            }
            if outcome.inserted {
                if let Some(lv) = live.upgrade() {
                    lv.publish_live(live::Live::Received(Box::new(
                        org_proto::chat::header_only_received(header.clone()),
                    )));
                }
            }
        }
        if owner_touched {
            if let Some(lv) = live.upgrade() {
                lv.record_mailbox_change();
            }
        }
        if outcome.inserted {
            if let (Some(msg), Some(conv_kind), Some(w)) = (
                outcome.delivered_message.as_ref(),
                outcome.conversation_kind,
                watcher.upgrade(),
            ) {
                w.on_channel_message_posted(msg, conv_kind);
            }
        }
    })
}

fn build_federation_wake_handler(
    session_manager: &Arc<org_office_sessions::SessionManager>,
) -> federation::WakeRelayHandler {
    let sessions = Arc::downgrade(session_manager);
    Arc::new(move |wake: federation::RelayedWake| {
        let Some(sessions) = sessions.upgrade() else {
            return Err("session manager shut down".to_owned());
        };
        if let Some(w) = wake.ask_withdrawal {
            return sessions.on_ask_withdrawn_durable(&w.asker_id, &w.ask_id);
        }
        if let Some(ans) = wake.ask_answer {
            // Dispatch structured answer transition and propagate persistence errors so
            // federation only commits `delivered: true` after resume intent is durable (`TASK-52` P6-#2 / P7-#4).
            return sessions
                .on_ask_answered_durable(
                    &ans.asker_id,
                    &ans.ask_id,
                    &ans.question_summary,
                    &ans.answer_summary,
                    (&wake.sender_id, &wake.sender_name, wake.sender_is_person),
                    false,
                )
                .map(|_| ());
        }
        let kind = match wake.kind.as_str() {
            "mention" => org_office_sessions::WakeKind::Mention,
            "handoff" => org_office_sessions::WakeKind::Handoff,
            "ask" => org_office_sessions::WakeKind::Ask,
            "ask_answer" => org_office_sessions::WakeKind::AskAnswer,
            "task" => org_office_sessions::WakeKind::TaskAssignment,
            _ => org_office_sessions::WakeKind::DirectMessage,
        };
        let item = org_office_sessions::PendingWakeItem {
            item_id: format!("relay:{}:{}", wake.source_id, wake.employee_id),
            kind,
            sender_id: wake.sender_id,
            sender_name: wake.sender_name,
            sender_is_person: wake.sender_is_person,
            conversation_id: wake.conversation_id,
            source_id: wake.source_id,
            summary: wake.summary,
            enqueued_at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        };
        sessions
            .enqueue_wake_durable(&wake.employee_id, item)
            .map(|_| ())
    })
}

fn start_optional_hq_federation(
    office: &Arc<Office>,
    wake_handler: Option<federation::WakeRelayHandler>,
    delivery_notifier: Option<federation::InboundDeliveryNotifier>,
) -> Result<Option<federation::HqFederationServer>, String> {
    if office.info().role != org_office::OfficeRole::Hq {
        return Ok(None);
    }
    let Ok(bind_raw) = std::env::var("AGENTIC_ORG_FEDERATION_BIND") else {
        return Ok(None);
    };
    let bind_raw = bind_raw.trim();
    if bind_raw.is_empty() {
        return Ok(None);
    }
    let addr: std::net::SocketAddr = bind_raw
        .parse()
        .map_err(|e| format!("invalid AGENTIC_ORG_FEDERATION_BIND `{bind_raw}`: {e}"))?;
    let mut san_hosts = vec![addr.ip().to_string(), "localhost".to_owned()];
    // Configure advertised endpoints separately from bind address so `0.0.0.0` binds cover DNS SANs (P2-16).
    if let Ok(adv) = std::env::var("AGENTIC_ORG_FEDERATION_ADVERTISE") {
        for item in adv.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            san_hosts.push(item.to_owned());
        }
    }
    federation::HqFederationServer::start_with_notifier(
        Arc::clone(office),
        addr,
        &san_hosts,
        wake_handler,
        delivery_notifier,
    )
    .map(Some)
}

fn run_satellite_sync_tick(
    office: &Arc<Office>,
    stop: &Arc<AtomicBool>,
    wake_handler: &federation::WakeRelayHandler,
    delivery_notifier: &federation::InboundDeliveryNotifier,
    sessions_for_recovery: &std::sync::Weak<org_office_sessions::SessionManager>,
) {
    let Ok(mut session) =
        federation::SatelliteSyncSession::connect_cancellable(Arc::clone(office), Some(stop))
    else {
        return;
    };
    if office
        .sync_status()
        .is_ok_and(|s| s.initial_snapshot_required)
    {
        let op_id = office.next_uuid_v7();
        if session.sync_initial_snapshot(&op_id).is_err() {
            return;
        }
        // Run deferred session recovery once the full snapshot generation is durably installed (`TASK-52` P8-#4).
        if let Some(sm) = sessions_for_recovery.upgrade() {
            let _ = sm.recover_on_startup();
        }
    }
    let _ = session.pull_org_events();
    let _ = session.flush_work_outbox();
    let _ = session.flush_message_outbox();
    let _ =
        session.receive_inbound_relays_with_notifier(Some(wake_handler), Some(delivery_notifier));
}

fn start_optional_satellite_sync(
    office: &Arc<Office>,
    stop: &Arc<AtomicBool>,
    sat_wake: Arc<SatSyncWake>,
    wake_handler: federation::WakeRelayHandler,
    delivery_notifier: federation::InboundDeliveryNotifier,
    sessions_for_recovery: std::sync::Weak<org_office_sessions::SessionManager>,
) -> Option<std::thread::JoinHandle<()>> {
    if office.info().role != org_office::OfficeRole::Satellite {
        return None;
    }
    let office = Arc::clone(office);
    let stop = Arc::clone(stop);
    std::thread::Builder::new()
        .name("sat-federation-sync".into())
        .spawn(move || {
            let mut seen = sat_wake.generation();
            while !stop.load(Ordering::SeqCst) {
                run_satellite_sync_tick(
                    &office,
                    &stop,
                    &wake_handler,
                    &delivery_notifier,
                    &sessions_for_recovery,
                );
                seen = sat_wake.wait_or_stop(seen, std::time::Duration::from_millis(500), &stop);
            }
        })
        .ok()
}

/// Starts the host's writer control and its admission, before the office is created or
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
struct PreparedOfficeRuntime {
    shared: serve::Shared,
    session_manager: Arc<org_office_sessions::SessionManager>,
    channel_watcher: Arc<watcher::ChannelWatcher>,
    federation_rt: FederationRuntime,
    host_gone: Arc<AtomicBool>,
}

fn prepare_office_runtime(
    control: &mut Control,
    office: &Arc<Office>,
    info: &org_office::OfficeInfo,
    socket: &Path,
    auth: serve::LaunchAuth,
    from_host: Lifetime,
) -> Option<PreparedOfficeRuntime> {
    let wake = Arc::new(org_feed::FeedWake::new());
    let live_wake = Arc::clone(&wake);
    let live = Arc::new(live::LiveEvents::for_owner(
        Box::new(move || live_wake.notify()),
        office.owner_member_id().to_owned(),
    ));
    let inbox = match inbox::Hq::for_office(Arc::clone(office)) {
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
    let session_manager = start_session_manager(Arc::clone(office), socket);
    let channel_watcher = start_channel_watcher(Arc::clone(office), &session_manager);
    let sat_sync_wake = Arc::new(SatSyncWake::default());
    let messaging = build_messaging_ports(
        office,
        &live,
        &wake,
        &session_manager,
        &channel_watcher,
        Arc::clone(&sat_sync_wake),
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
        delivery_notifier,
        sat_sync_wake,
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
    let ports = build_office_ports(
        Arc::clone(office),
        Arc::clone(&session_manager),
        Arc::clone(&channel_watcher),
        live,
        &wake,
        messaging,
        Arc::clone(&hq_federation_listening),
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
    signals::install();
    let Some(mut prepared) =
        prepare_office_runtime(&mut control, &office, &info, &socket, auth, from_host)
    else {
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

fn start_session_manager(
    office: Arc<Office>,
    socket: &Path,
) -> Arc<org_office_sessions::SessionManager> {
    let mut cfg = org_office_sessions::SessionManagerConfig::production(socket.to_path_buf());
    if office.info().role == org_office::OfficeRole::Satellite {
        cfg.hq_permits = Some(Arc::new(federation::SatelliteHqPermitCoordinator::new(
            Arc::clone(&office),
        )));
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

struct MessagingPorts {
    directory: Arc<dyn org_mailbox::MemberDirectory + Send + Sync>,
    mailbox: Arc<org_mailbox::Mailbox>,
    chat_svc: Arc<chat_calls::ChatService>,
    chat: serve::ChatFn,
    feed: Box<dyn follow::OrgFeed>,
}

fn build_messaging_ports(
    office: &Arc<Office>,
    live: &Arc<live::LiveEvents>,
    wake: &Arc<org_feed::FeedWake>,
    session_manager: &Arc<org_office_sessions::SessionManager>,
    channel_watcher: &Arc<watcher::ChannelWatcher>,
    sat_sync_wake: Arc<SatSyncWake>,
) -> MessagingPorts {
    let directory: Arc<dyn org_mailbox::MemberDirectory + Send + Sync> =
        Arc::new(directory::OfficeDirectory::of(Arc::clone(office)));
    let make_id = || org_mailbox::OfficeIdentity {
        office_id: office.info().office_id.clone(),
        incarnation: office.journal_incarnation().to_owned(),
        is_hq: matches!(office.is_hq(), Ok(true)),
    };
    let store = Arc::new(mailbox_store::OfficeMailboxStore::new(Arc::clone(office)));
    let mailbox = match org_mailbox::Mailbox::with_triage(
        Arc::clone(&store) as Arc<dyn org_mailbox::MessageStore + Send + Sync>,
        store as Arc<dyn org_mailbox::TriageStore + Send + Sync>,
        Arc::clone(&directory),
        make_id(),
        Arc::new(org_mailbox::SystemClock),
        Arc::clone(live) as Arc<dyn org_mailbox::MessageEvents>,
    ) {
        Ok(mb) => Arc::new(mb),
        Err(error) => {
            eprintln!("agentic-org-office: snooze timer seed failed: {error}");
            Arc::new(org_mailbox::Mailbox::new(
                Arc::new(mailbox_store::OfficeMailboxStore::new(Arc::clone(office))),
                Arc::clone(&directory),
                make_id(),
                Arc::new(org_mailbox::SystemClock),
                Arc::clone(live) as Arc<dyn org_mailbox::MessageEvents>,
            ))
        }
    };
    let chat_svc = Arc::new(
        chat_calls::ChatService::with_watcher(
            Arc::clone(office),
            Arc::clone(live),
            Arc::clone(&mailbox),
            Some(Arc::clone(session_manager)),
            Arc::clone(channel_watcher),
        )
        .with_sat_sync_wake(Arc::new(move || sat_sync_wake.notify())),
    );
    let chat_for_dispatch = Arc::clone(&chat_svc);
    let chat: serve::ChatFn = Box::new(move |member, via_grant, stopping, request| {
        chat_for_dispatch.dispatch(member, via_grant, stopping, request)
    });
    let feed: Box<dyn follow::OrgFeed> = Box::new(org_feed::OfficeOrgFeed::new(
        Arc::clone(office),
        Arc::clone(wake),
    ));
    MessagingPorts {
        directory,
        mailbox,
        chat_svc,
        chat,
        feed,
    }
}

struct SessionHooks {
    sessions: serve::SessionsFn,
    run_binding_verify: serve::RunBindingVerifyFn,
    run_binding_active: serve::RunBindingActiveFn,
    record_agent_command: serve::RecordCommandFn,
    on_agent_stop: serve::AgentLifecycleHookFn,
}

fn build_session_hooks(session_manager: &Arc<org_office_sessions::SessionManager>) -> SessionHooks {
    SessionHooks {
        sessions: {
            let sessions = Arc::clone(session_manager);
            Box::new(move |member, request| sessions.dispatch_rpc(member, request))
        },
        run_binding_verify: {
            let sessions = Arc::clone(session_manager);
            Box::new(move |member_id, token, peer_pid| {
                sessions
                    .bindings()
                    .verify_peer(member_id, token, peer_pid)
                    .is_ok()
            })
        },
        run_binding_active: {
            let sessions = Arc::clone(session_manager);
            Box::new(move |member_id| sessions.bindings().has_active(member_id))
        },
        record_agent_command: {
            let sessions = Arc::clone(session_manager);
            Box::new(move |member_id, method| sessions.record_accepted_command(member_id, method))
        },
        on_agent_stop: {
            let sessions = Arc::clone(session_manager);
            Box::new(move |employee_id, reason| {
                let _ = sessions.stop_session(employee_id, reason);
            })
        },
    }
}

fn snapshot_pending(office: &Office) -> bool {
    office
        .sync_status()
        .is_ok_and(|s| s.initial_snapshot_required)
}

fn snapshot_unavailable_api_error() -> org_proto::ApiError {
    org_proto::ApiError::new(
        org_proto::ErrorCode::Unavailable,
        "organization snapshot is not available yet",
    )
}

fn gate_rpc_port<F>(office: &Arc<Office>, inner: F) -> serve::HiringFn
where
    F: Fn(Option<&str>, &org_proto::Request) -> Option<org_proto::Response> + Send + Sync + 'static,
{
    let office = Arc::clone(office);
    Box::new(move |member, request| {
        if snapshot_pending(&office) {
            return Some(org_proto::Response::error(
                request.id,
                snapshot_unavailable_api_error(),
            ));
        }
        inner(member, request)
    })
}

fn build_hiring_port(
    office: &Arc<Office>,
    wake: &Arc<org_feed::FeedWake>,
    session_manager: &Arc<org_office_sessions::SessionManager>,
    channel_watcher: &Arc<watcher::ChannelWatcher>,
) -> serve::HiringFn {
    let office = Arc::clone(office);
    let wake = Arc::clone(wake);
    let sessions = Arc::clone(session_manager);
    let watcher = Arc::clone(channel_watcher);
    Box::new(move |member, request| {
        if snapshot_pending(&office) {
            return Some(org_proto::Response::error(
                request.id,
                snapshot_unavailable_api_error(),
            ));
        }
        let response = office.dispatch_hiring_rpc(member, request);
        if matches!(
            response.as_ref().map(|r| &r.outcome),
            Some(org_proto::Outcome::Result(_))
        ) {
            wake.notify();
            notify_agent_lifecycle(&sessions, &watcher, request);
        }
        response
    })
}

fn notify_agent_lifecycle(
    sessions: &org_office_sessions::SessionManager,
    watcher: &watcher::ChannelWatcher,
    request: &org_proto::Request,
) {
    if request.is::<org_proto::hiring::AgentSetStatus>() {
        if let Ok(params) = request.params::<org_proto::hiring::AgentSetStatus>() {
            if !params.paused {
                sessions.on_agent_resumed(&params.member_id);
            }
        }
    } else if request.is::<org_proto::hiring::AgentOffboard>() {
        if let Ok(params) = request.params::<org_proto::hiring::AgentOffboard>() {
            sessions.on_agent_offboarded(&params.member_id);
            watcher.on_agent_offboarded(&params.member_id);
        }
    } else if request.is::<org_proto::hiring::AgentMove>() {
        if let Ok(params) = request.params::<org_proto::hiring::AgentMove>() {
            watcher.on_agent_team_moved(&params.member_id);
        }
    }
}

fn notify_org_command_lifecycle(
    sessions: &org_office_sessions::SessionManager,
    watcher: &watcher::ChannelWatcher,
    command: &org_office::rules::OrgCommand,
) {
    match command {
        org_office::rules::OrgCommand::MemberSetAgentStatus(params)
            if params.status == org_proto::AgentStatus::Active =>
        {
            sessions.on_agent_resumed(&params.member_id);
        }
        org_office::rules::OrgCommand::MemberOffboard(params) => {
            sessions.on_agent_offboarded(&params.member_id);
            watcher.on_agent_offboarded(&params.member_id);
        }
        _ => {}
    }
}

fn build_submit_port(
    office: &Arc<Office>,
    wake: &Arc<org_feed::FeedWake>,
    session_manager: &Arc<org_office_sessions::SessionManager>,
    channel_watcher: &Arc<watcher::ChannelWatcher>,
) -> serve::SubmitFn {
    let office = Arc::clone(office);
    let wake = Arc::clone(wake);
    let sessions = Arc::clone(session_manager);
    let watcher = Arc::clone(channel_watcher);
    Box::new(move |member, command, dry_run| {
        let answer = if dry_run {
            office.check_org(member, command)
        } else {
            let lifecycle_cmd = match &command {
                org_office::rules::OrgCommand::MemberSetAgentStatus(p) => Some(
                    org_office::rules::OrgCommand::MemberSetAgentStatus(p.clone()),
                ),
                org_office::rules::OrgCommand::MemberOffboard(p) => {
                    Some(org_office::rules::OrgCommand::MemberOffboard(p.clone()))
                }
                _ => None,
            };
            let res = office.submit_org(member, command);
            if matches!(res, Ok(Ok(_))) {
                wake.notify();
                if let Some(cmd) = lifecycle_cmd.as_ref() {
                    notify_org_command_lifecycle(&sessions, &watcher, cmd);
                }
            }
            res
        };
        answer.map_err(|e| logged("command", &e))
    })
}

struct ReadPorts {
    snapshot: serve::SnapshotFn,
    projects_list: serve::ProjectsListFn,
    project_inspect: serve::ProjectInspectFn,
    who: serve::WhoFn,
    status: serve::StatusFn,
}

fn build_read_ports(
    office: &Arc<Office>,
    session_manager: &Arc<org_office_sessions::SessionManager>,
    live: &Arc<live::LiveEvents>,
) -> ReadPorts {
    let o_snap = Arc::clone(office);
    let o_list = Arc::clone(office);
    let o_insp = Arc::clone(office);
    let o_who = Arc::clone(office);
    let s_who = Arc::clone(session_manager);
    let o_stat = Arc::clone(office);
    let l_stat = Arc::clone(live);
    ReadPorts {
        snapshot: Box::new(move || {
            if snapshot_pending(&o_snap) {
                return Err(serve::SnapshotFailure::Unavailable);
            }
            o_snap.org_snapshot().map_err(|e| snapshot_failure(&e))
        }),
        projects_list: Box::new(move |params| {
            if snapshot_pending(&o_list) {
                return Ok(Err(snapshot_unavailable_api_error()));
            }
            o_list
                .list_projects(params.include_archived)
                .map(Ok)
                .map_err(|e| logged("project.list", &e))
        }),
        project_inspect: Box::new(move |params| {
            if snapshot_pending(&o_insp) {
                return Ok(Err(snapshot_unavailable_api_error()));
            }
            o_insp
                .inspect_project(&params.project)
                .map_err(|e| logged("project.inspect", &e))
        }),
        who: Box::new(move |params| {
            if snapshot_pending(&o_who) {
                return Ok(Err(snapshot_unavailable_api_error()));
            }
            let overlay = wake_bridge::SessionRuntimeOverlay::new(Arc::clone(&s_who));
            o_who
                .who_with_runtime(params, &overlay)
                .map_err(|e| logged("who", &e))
        }),
        status: Box::new(
            move |member, params| match o_stat.set_status(member, params) {
                Ok((result, changed)) => {
                    l_stat.publish_presence(changed);
                    Ok(Ok(result))
                }
                Err(refusal) => Ok(Err(refusal)),
            },
        ),
    }
}

/// Composes the office RPC ports passed to `serve::Shared`.
fn build_office_ports(
    office: Arc<Office>,
    session_manager: Arc<org_office_sessions::SessionManager>,
    channel_watcher: Arc<watcher::ChannelWatcher>,
    live: Arc<live::LiveEvents>,
    wake: &Arc<org_feed::FeedWake>,
    messaging: MessagingPorts,
    hq_federation_listening: Arc<AtomicBool>,
) -> serve::OfficePorts {
    let session_hooks = build_session_hooks(&session_manager);
    let hiring = build_hiring_port(&office, wake, &session_manager, &channel_watcher);
    let submit = build_submit_port(&office, wake, &session_manager, &channel_watcher);
    let reads = build_read_ports(&office, &session_manager, &live);
    let task_svc = Arc::new(task_calls::TaskService::new(
        Arc::clone(&office),
        Arc::clone(&live),
        Some(Arc::clone(&session_manager)),
    ));
    let git_svc = Arc::new(git_calls::GitService::new(
        Arc::clone(&office),
        Arc::clone(&live),
    ));
    serve::OfficePorts {
        work: Box::new(|_, _| Ok(Err(unsupported("work", "B4")))),
        operation_get: {
            let office = Arc::clone(&office);
            Box::new(move |member, id| {
                office
                    .operation(member, id)
                    .map_err(|e| logged("operation.get", &e))
            })
        },
        snapshot: reads.snapshot,
        projects_list: reads.projects_list,
        project_inspect: reads.project_inspect,
        hiring: Some(hiring),
        sessions: Some(session_hooks.sessions),
        run_binding_verify: Some(session_hooks.run_binding_verify),
        run_binding_active: Some(session_hooks.run_binding_active),
        record_agent_command: Some(session_hooks.record_agent_command),
        on_agent_stop: Some(session_hooks.on_agent_stop),
        hr_message_guard: {
            let office = Arc::clone(&office);
            Some(Box::new(move |sender, target| {
                office
                    .check_hr_message_allowed(sender, target)
                    .map_err(|e| logged("msg.send", &e))
            }))
        },
        who: reads.who,
        status: reads.status,
        live: Arc::clone(&live),
        feed: Some(messaging.feed),
        mailbox: Some(messaging.mailbox),
        chat: Some(messaging.chat),
        tasks: Some(Box::new(move |member, request| {
            task_svc.dispatch(member, request)
        })),
        git: Some(gate_rpc_port(&office, move |member, request| {
            git_svc.dispatch(member, request)
        })),
        processes: {
            let o = Arc::clone(&office);
            Some(gate_rpc_port(&office, move |member, request| {
                process_calls::dispatch_process_rpc(&o, member, request)
            }))
        },
        costs: {
            let o = Arc::clone(&office);
            let sessions = Arc::clone(&session_manager);
            let live = Arc::clone(&live);
            Some(gate_rpc_port(&office, move |member, request| {
                cost_calls::dispatch_cost_budget_rpc(&o, Some(&sessions), &live, member, request)
            }))
        },
        providers: {
            let o = Arc::clone(&office);
            let sessions = Arc::clone(&session_manager);
            let wake = Arc::clone(wake);
            Some(gate_rpc_port(&office, move |member, request| {
                provider_calls::dispatch_provider_pool_rpc(
                    &o,
                    Some(&sessions),
                    Some(&wake),
                    member,
                    request,
                )
            }))
        },
        federation: {
            let office = Arc::clone(&office);
            let hq_listening = Arc::clone(&hq_federation_listening);
            Some(Box::new(move |member, request| {
                federation_calls::dispatch_federation_rpc(
                    &office,
                    member,
                    request,
                    hq_listening.load(Ordering::Acquire),
                )
            }))
        },
        directory: messaging.directory,
        submit,
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
