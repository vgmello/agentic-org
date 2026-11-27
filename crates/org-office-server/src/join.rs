//! The join launch mode: satellite enrollment with HQ, the pending joining key, the initial sync,
//! and opening or provisioning the joined satellite office.

use std::path::Path;
use std::sync::Arc;

use crate::failure::{emit, fail, fail_with};
use crate::launch::{self, Launch};
use crate::office_runtime::{
    admit, first_line, serve_office_with_runtimes, socket_path, ServeOfficeRequest,
};
use crate::startup::Control;
use crate::{federation, transfer};
use org_office::{Office, OfficeError};
use org_office_launch::{office_dir, FailCode, FirstLine, OfficeFile, ServerEvent};
use org_office_sessions::RuntimeRegistry;
use org_proto::OfficeRole;

pub(crate) fn join_and_serve(
    mut control: Control,
    home: &Path,
    runtime: &Path,
    runtimes: RuntimeRegistry,
) -> i32 {
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
    serve_office_with_runtimes(ServeOfficeRequest {
        control,
        transfer,
        from_host,
        runtime,
        office,
        auth: join.auth,
        instance: join.instance,
        runtimes,
    })
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
    org_office_sqlite::install();
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
