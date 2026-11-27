//! The discard, create and open launch modes: writer custody first, then the office, then serving.

use std::path::Path;
use std::sync::Arc;

use crate::failure::{emit, fail, fail_with};
use crate::hq_creation;
use crate::launch::Launch;
use crate::office_runtime::{
    admit, first_line, serve_office_with_runtimes, socket_path, ServeOfficeRequest,
};
use crate::startup::Control;
use org_office::{Office, OfficeStorageBackend};
use org_office_launch::{office_dir, FailCode, FirstLine, OfficeFile, ServerEvent};
use org_office_sessions::RuntimeRegistry;
use org_proto::OfficeRole;

/// `--discard`: needs no key, because the databases are plain SQLite (ADR-0016); a key the
/// handoff carries is dropped when the request is parsed.
pub(crate) fn discard(mut control: Control, home: &Path, id: &str) -> i32 {
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

pub(crate) fn create_and_serve(
    mut control: Control,
    home: &Path,
    runtime: &Path,
    runtimes: RuntimeRegistry,
    backend: Arc<dyn OfficeStorageBackend>,
) -> i32 {
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
        backend,
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
    serve_office_with_runtimes(ServeOfficeRequest {
        control,
        transfer,
        from_host,
        runtime,
        office,
        auth: create.auth,
        instance: create.instance,
        runtimes,
    })
}

pub(crate) fn open_and_serve(
    mut control: Control,
    home: &Path,
    runtime: &Path,
    id: &str,
    runtimes: RuntimeRegistry,
    backend: Arc<dyn OfficeStorageBackend>,
) -> i32 {
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
        backend,
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
    serve_office_with_runtimes(ServeOfficeRequest {
        control,
        transfer,
        from_host,
        runtime,
        office,
        auth,
        instance,
        runtimes,
    })
}
