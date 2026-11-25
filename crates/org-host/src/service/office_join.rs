//! Satellite office join, pending-join removal, and D25 re-enrollment handlers on `host.sock`
//! (`TASK-52` Slice 3, `doc-67` §The invitation and connection, D25).

use std::fs;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use org_host_control::{
    ops::{
        OfficeJoin, OfficeJoinParams, OfficeJoinResult, OfficeReEnroll, OfficeReEnrollParams,
        OfficeRemovePending, OfficeRemovePendingParams,
    },
    HostOpResult, InvitationTokenSecret,
};
use org_office_launch::{office_dir, OFFICE_FILE_NAME};
use org_pki::invitation::InvitationToken;
use org_proto::{ApiError, Empty, ErrorCode, OfficeRole, Request, Response};
use serde::{Deserialize, Serialize};

use super::{
    browser_now_ms,
    listener::Connection,
    office_ops::{authorize_session_first, journal_error},
    ok, Authentication, Service,
};
use crate::hostdb::{
    journal::{Attempt, Effect, Lookup, Stored, Write},
    HostDb, HostStore,
};

const PENDING_JOIN_FILE: &str = "pending-join.v1.json";
const LOCAL_BOOTSTRAP_ACTOR: &str = "local-bootstrap";

/// Non-secret pending join record stored under `<home>/offices/<office-id>/pending-join.v1.json`.
/// Never contains the invitation token secret or private key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PendingJoinRecord {
    v: u32,
    office_id: String,
    office_name: String,
    org_id: String,
    org_name: String,
    hq_office_id: String,
    hq_addresses: Vec<String>,
    invitation_id: String,
    ca_fingerprint_hex: String,
    re_enroll: bool,
    created_at_ms: i64,
    expires_at_ms: u64,
}

/// Enforces host join authority (`TASK-52` H-3):
/// - On a fresh satellite-only instance with NO administrator account, admits the verified local
///   `host.sock` peer (with explicit consent checked by the caller) under `local-bootstrap`.
/// - On any instance with accounts, requires an authenticated session belonging to an instance
///   administrator (`account.admin()`), refusing non-admin organization owners.
fn authorize_join_actor(
    db: &HostDb,
    authentication: &Authentication,
    request: &Request,
    connection: &Connection,
) -> Result<String, ApiError> {
    let has_admin = db.has_administrator().map_err(journal_error)?;
    if !has_admin {
        return Ok(LOCAL_BOOTSTRAP_ACTOR.to_owned());
    }
    let account = authorize_session_first(authentication, request, connection)?;
    if !account.admin() {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "only the instance administrator may join, remove-pending, or re-enroll an office",
        ));
    }
    Ok(account.account_id().to_owned())
}

fn decode_invitation_syntax(raw: &str) -> Result<InvitationToken, ApiError> {
    let token = InvitationToken::decode(raw).map_err(|_| {
        ApiError::new(
            ErrorCode::InvalidRequest,
            "invitation token is malformed or failed validation",
        )
    })?;
    if !org_proto::is_uuid_v7(&token.office_id)
        || !org_proto::is_uuid_v7(&token.org_id)
        || !org_proto::is_uuid_v7(&token.hq_office_id)
    {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "invitation token carries invalid canonical UUIDv7 identities",
        ));
    }
    Ok(token)
}

fn check_invitation_expiry(token: &InvitationToken, now_ms: i64) -> Result<(), ApiError> {
    if now_ms > 0 && (now_ms as u64) > token.expires_at_ms {
        return Err(ApiError::new(
            ErrorCode::Expired,
            "invitation token has expired",
        ));
    }
    Ok(())
}

fn checked_office_dir(home: &Path, office_id: &str) -> Result<PathBuf, ApiError> {
    office_dir(home, office_id)
        .ok_or_else(|| ApiError::new(ErrorCode::InvalidRequest, "invalid office_id"))
}

fn pending_join_path(home: &Path, office_id: &str) -> Result<PathBuf, ApiError> {
    Ok(checked_office_dir(home, office_id)?.join(PENDING_JOIN_FILE))
}

fn write_pending_join(home: &Path, record: &PendingJoinRecord) -> Result<(), ApiError> {
    let dir = checked_office_dir(home, &record.office_id)?;
    fs::create_dir_all(&dir)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "cannot create office folder"))?;
    let path = dir.join(PENDING_JOIN_FILE);
    let tmp = dir.join(format!(".pending-join.{}.tmp", std::process::id()));
    let bytes = serde_json::to_vec_pretty(record)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "cannot encode pending join"))?;
    let write_res = (|| -> std::io::Result<()> {
        use std::io::Write as _;
        let mut f = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        fs::rename(&tmp, &path)
    })();
    if write_res.is_err() {
        let _ = fs::remove_file(&tmp);
        return Err(ApiError::new(
            ErrorCode::StorageError,
            "cannot persist pending join record",
        ));
    }
    Ok(())
}

fn decode_join_stored(stored: Stored) -> Result<HostOpResult<OfficeJoinResult>, ApiError> {
    match stored {
        Stored::Completed { result } => {
            let parsed: OfficeJoinResult = serde_json::from_value(result).map_err(|_| {
                ApiError::new(ErrorCode::Unavailable, "unrecognized stored join result")
            })?;
            Ok(HostOpResult::Completed { result: parsed })
        }
        Stored::Rejected { refusal } => Ok(HostOpResult::Rejected { refusal }),
        _ => Err(ApiError::new(
            ErrorCode::Unavailable,
            "unrecognized stored join state",
        )),
    }
}

fn decode_empty_stored(stored: Stored) -> Result<HostOpResult<Empty>, ApiError> {
    match stored {
        Stored::Completed { .. } => Ok(HostOpResult::Completed { result: Empty {} }),
        Stored::Rejected { refusal } => Ok(HostOpResult::Rejected { refusal }),
        _ => Err(ApiError::new(
            ErrorCode::Unavailable,
            "unrecognized stored operation state",
        )),
    }
}

fn commit_join_result(
    db: &HostDb,
    attempt: &Attempt<'_>,
    now: i64,
    result: &OfficeJoinResult,
) -> Result<HostOpResult<OfficeJoinResult>, ApiError> {
    let val = serde_json::to_value(result)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "cannot serialize join result"))?;
    let write = db
        .write(attempt, now, |_| {
            Ok(Effect {
                stored: Stored::Completed { result: val },
                verifier: None,
            })
        })
        .map_err(journal_error)?;
    match write {
        Write::Committed(stored) | Write::Existing(Lookup::Replay(stored)) => {
            decode_join_stored(stored)
        }
        Write::Existing(_) => Err(ApiError::new(
            ErrorCode::OperationReused,
            "operation ID already used",
        )),
    }
}

fn try_resume_interrupted_join(
    service: &Service,
    actor_id: &str,
    operation_id: &str,
    invitation: &InvitationTokenSecret,
    token: &org_pki::invitation::InvitationToken,
    re_enroll: bool,
    replayed: HostOpResult<OfficeJoinResult>,
) -> Result<HostOpResult<OfficeJoinResult>, ApiError> {
    let HostOpResult::Completed { ref result } = replayed else {
        return Ok(replayed);
    };
    let already_enrolled = !re_enroll
        && service
            .offices
            .offices()
            .iter()
            .any(|o| o.id == result.office_id);
    let active = crate::offices::has_active_pending_join(&service.offices, &result.office_id);
    let pending_file = pending_join_path(service.offices.home(), &result.office_id)?;
    if result.state != org_host_control::ops::OfficeJoinState::JoinPending
        || already_enrolled
        || active
        || !pending_file.is_file()
    {
        return Ok(replayed);
    }
    let instance_id = service
        .instance_id
        .clone()
        .ok_or_else(|| ApiError::new(ErrorCode::Unavailable, "host instance_id unavailable"))?;
    let invitation_copy = InvitationTokenSecret::new(invitation.expose().to_owned())
        .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "empty invitation"))?;
    let resumed = service
        .offices
        .join_satellite(crate::offices::SatelliteJoinSpec {
            instance_id,
            operation_id: operation_id.to_owned(),
            invitation_id: token.invitation_id.clone(),
            org_id: token.org_id.clone(),
            org_name: token.org_name.clone(),
            office_id: token.office_id.clone(),
            office_name: token.office_name.clone(),
            hq_office_id: token.hq_office_id.clone(),
            hq_addresses: token.hq_addresses.clone(),
            invitation: invitation_copy,
            re_enroll,
            actor_account: actor_id.to_owned(),
            pending_file,
        })?;
    Ok(HostOpResult::Completed { result: resumed })
}

/// `office.join` on `host.sock`: admits verified local peer on a fresh satellite-only instance or
/// an instance administrator when accounts exist, launches `--join` over the verified rendezvous,
/// and journals the worker-confirmed pending/joined outcome (`TASK-52` H-1 + H-3).
pub(super) fn join_session_office(
    service: &Service,
    authentication: &Authentication,
    request: &Request,
    connection: &Connection,
) {
    let reply = (|| -> Result<HostOpResult<OfficeJoinResult>, ApiError> {
        let (db, _) = authentication.provisioning_storage();
        let actor_id = authorize_join_actor(&db, authentication, request, connection)?;
        let now = browser_now_ms()?;
        let p: OfficeJoinParams = request
            .params::<OfficeJoin>()
            .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "invalid office.join request"))?;
        if !p.consent {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "explicit destination consent (--yes) is required to join an organization",
            ));
        }
        let token = decode_invitation_syntax(p.invitation.expose())?;
        let attempt = Attempt::new(&p, &actor_id).map_err(journal_error)?;
        let _guard = crate::sync::lock(&service.office_restarting);
        match db.lookup(&attempt, now).map_err(journal_error)? {
            Lookup::Replay(stored) => {
                let replayed = decode_join_stored(stored)?;
                return try_resume_interrupted_join(
                    service,
                    &actor_id,
                    &p.operation_id,
                    &p.invitation,
                    &token,
                    false,
                    replayed,
                );
            }
            Lookup::NeedsVerification { .. } => {
                return Err(ApiError::new(
                    ErrorCode::OperationReused,
                    "operation ID already used",
                ));
            }
            Lookup::Vacant => {}
        }
        check_invitation_expiry(&token, now)?;
        if service
            .offices
            .offices()
            .iter()
            .any(|o| o.id == token.office_id)
        {
            return Err(ApiError::new(
                ErrorCode::AlreadyExists,
                "this office is already enrolled on this host",
            ));
        }
        let instance_id = service
            .instance_id
            .clone()
            .ok_or_else(|| ApiError::new(ErrorCode::Unavailable, "host instance_id unavailable"))?;
        let record = PendingJoinRecord {
            v: 1,
            office_id: token.office_id.clone(),
            office_name: token.office_name.clone(),
            org_id: token.org_id.clone(),
            org_name: token.org_name.clone(),
            hq_office_id: token.hq_office_id.clone(),
            hq_addresses: token.hq_addresses.clone(),
            invitation_id: token.invitation_id.clone(),
            ca_fingerprint_hex: token.ca_fingerprint_hex.clone(),
            re_enroll: false,
            created_at_ms: now,
            expires_at_ms: token.expires_at_ms,
        };
        write_pending_join(service.offices.home(), &record)?;
        let pending_file = pending_join_path(service.offices.home(), &token.office_id)?;
        let invitation_copy = InvitationTokenSecret::new(p.invitation.expose().to_owned())
            .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "empty invitation"))?;
        let res = service
            .offices
            .join_satellite(crate::offices::SatelliteJoinSpec {
                instance_id,
                operation_id: p.operation_id.clone(),
                invitation_id: token.invitation_id,
                org_id: token.org_id,
                org_name: token.org_name,
                office_id: token.office_id,
                office_name: token.office_name,
                hq_office_id: token.hq_office_id,
                hq_addresses: token.hq_addresses,
                invitation: invitation_copy,
                re_enroll: false,
                actor_account: actor_id.clone(),
                pending_file,
            })?;
        commit_join_result(&db, &attempt, now, &res)
    })();
    match reply {
        Ok(res) => connection.send_response(ok::<OfficeJoin>(request.id, &res)),
        Err(err) => connection.send_response(Response::error(request.id, err)),
    }
}

/// `office.remove_pending` on `host.sock`.
pub(super) fn remove_pending_session_office(
    service: &Service,
    authentication: &Authentication,
    request: &Request,
    connection: &Connection,
) {
    let reply = (|| -> Result<HostOpResult<Empty>, ApiError> {
        let (db, _) = authentication.provisioning_storage();
        let actor_id = authorize_join_actor(&db, authentication, request, connection)?;
        let now = browser_now_ms()?;
        let p: OfficeRemovePendingParams =
            request.params::<OfficeRemovePending>().map_err(|_| {
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    "invalid office.remove_pending request",
                )
            })?;
        if !org_proto::is_uuid_v7(&p.office_id) {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "office_id must be a canonical lowercase UUIDv7",
            ));
        }
        let attempt = Attempt::new(&p, &actor_id).map_err(journal_error)?;
        let _guard = crate::sync::lock(&service.office_restarting);
        match db.lookup(&attempt, now).map_err(journal_error)? {
            Lookup::Replay(stored) => return decode_empty_stored(stored),
            Lookup::NeedsVerification { .. } => {
                return Err(ApiError::new(
                    ErrorCode::OperationReused,
                    "operation ID already used",
                ));
            }
            Lookup::Vacant => {}
        }
        let dir = checked_office_dir(service.offices.home(), &p.office_id)?;
        let pending_file = pending_join_path(service.offices.home(), &p.office_id)?;
        let removing_file = dir.join("pending-join.removing.v1.json");
        // Stop and reap any in-flight `--join` worker BEFORE checking `office.json` or removing
        // pending state so a removed join can never complete or be adopted (`TASK-52` #3 / P4-#11).
        if !crate::offices::stop_pending_join_for_office(&service.offices, &p.office_id) {
            return Err(ApiError::new(
                ErrorCode::Unavailable,
                "pending join worker is still shutting down; retry remove-pending shortly",
            ));
        }
        if dir.join(OFFICE_FILE_NAME).exists() {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "office enrollment already completed; cannot remove pending join",
            ));
        }
        if pending_file.is_file() {
            fs::rename(&pending_file, &removing_file).map_err(|_| {
                ApiError::new(ErrorCode::StorageError, "cannot stage pending removal")
            })?;
        } else if !removing_file.is_file() {
            return Err(ApiError::new(
                ErrorCode::NotFound,
                "no pending join found for this office",
            ));
        }
        let write = db
            .write(&attempt, now, |_| {
                Ok(Effect {
                    stored: Stored::Completed {
                        result: serde_json::json!({}),
                    },
                    verifier: None,
                })
            })
            .map_err(journal_error)?;
        let _ = fs::remove_file(&removing_file);
        let _ = fs::remove_dir(&dir);
        match write {
            Write::Committed(stored) | Write::Existing(Lookup::Replay(stored)) => {
                decode_empty_stored(stored)
            }
            Write::Existing(_) => Err(ApiError::new(
                ErrorCode::OperationReused,
                "operation ID already used",
            )),
        }
    })();
    match reply {
        Ok(res) => connection.send_response(ok::<OfficeRemovePending>(request.id, &res)),
        Err(err) => connection.send_response(Response::error(request.id, err)),
    }
}

/// `office.re_enroll` on `host.sock` (D25): launches `--join` (`re_enroll: true`) through the
/// supervisor and verified rendezvous (`TASK-52` H-1 + H-3).
pub(super) fn re_enroll_session_office(
    service: &Service,
    authentication: &Authentication,
    request: &Request,
    connection: &Connection,
) {
    let reply = (|| -> Result<HostOpResult<OfficeJoinResult>, ApiError> {
        let (db, _) = authentication.provisioning_storage();
        let actor_id = authorize_join_actor(&db, authentication, request, connection)?;
        let now = browser_now_ms()?;
        let p: OfficeReEnrollParams = request.params::<OfficeReEnroll>().map_err(|_| {
            ApiError::new(
                ErrorCode::InvalidRequest,
                "invalid office.re_enroll request",
            )
        })?;
        if !p.consent {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "explicit destination consent (--yes) is required to re-enroll a satellite office",
            ));
        }
        if !org_proto::is_uuid_v7(&p.office_id) {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "office_id must be a canonical lowercase UUIDv7",
            ));
        }
        let token = decode_invitation_syntax(p.invitation.expose())?;
        if token.office_id != p.office_id {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "invitation office_id does not match the satellite office being re-enrolled",
            ));
        }
        let attempt = Attempt::new(&p, &actor_id).map_err(journal_error)?;
        let _guard = crate::sync::lock(&service.office_restarting);
        match db.lookup(&attempt, now).map_err(journal_error)? {
            Lookup::Replay(stored) => {
                let replayed = decode_join_stored(stored)?;
                return try_resume_interrupted_join(
                    service,
                    &actor_id,
                    &p.operation_id,
                    &p.invitation,
                    &token,
                    true,
                    replayed,
                );
            }
            Lookup::NeedsVerification { .. } => {
                return Err(ApiError::new(
                    ErrorCode::OperationReused,
                    "operation ID already used",
                ));
            }
            Lookup::Vacant => {}
        }
        check_invitation_expiry(&token, now)?;
        let existing = service
            .offices
            .offices()
            .into_iter()
            .find(|o| o.id == p.office_id)
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "satellite office not found"))?;
        if existing.role != OfficeRole::Satellite || existing.organization_id != token.org_id {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "only an enrolled satellite of the same organization may be re-enrolled",
            ));
        }
        let instance_id = service
            .instance_id
            .clone()
            .ok_or_else(|| ApiError::new(ErrorCode::Unavailable, "host instance_id unavailable"))?;
        let record = PendingJoinRecord {
            v: 1,
            office_id: token.office_id.clone(),
            office_name: token.office_name.clone(),
            org_id: token.org_id.clone(),
            org_name: token.org_name.clone(),
            hq_office_id: token.hq_office_id.clone(),
            hq_addresses: token.hq_addresses.clone(),
            invitation_id: token.invitation_id.clone(),
            ca_fingerprint_hex: token.ca_fingerprint_hex.clone(),
            re_enroll: true,
            created_at_ms: now,
            expires_at_ms: token.expires_at_ms,
        };
        write_pending_join(service.offices.home(), &record)?;
        let pending_file = pending_join_path(service.offices.home(), &token.office_id)?;
        let invitation_copy = InvitationTokenSecret::new(p.invitation.expose().to_owned())
            .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "empty invitation"))?;
        let res = service
            .offices
            .join_satellite(crate::offices::SatelliteJoinSpec {
                instance_id,
                operation_id: p.operation_id.clone(),
                invitation_id: token.invitation_id,
                org_id: token.org_id,
                org_name: token.org_name,
                office_id: token.office_id,
                office_name: token.office_name,
                hq_office_id: token.hq_office_id,
                hq_addresses: token.hq_addresses,
                invitation: invitation_copy,
                re_enroll: true,
                actor_account: actor_id.clone(),
                pending_file,
            })?;
        commit_join_result(&db, &attempt, now, &res)
    })();
    match reply {
        Ok(res) => connection.send_response(ok::<OfficeReEnroll>(request.id, &res)),
        Err(err) => connection.send_response(Response::error(request.id, err)),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;

    use super::{write_pending_join, PendingJoinRecord, Service};

    use org_host_control::{
        ops::{
            AccountVerify, AccountVerifyParams, OfficeJoin, OfficeJoinParams, OfficeReEnroll,
            OfficeReEnrollParams, OfficeRemovePending, OfficeRemovePendingParams,
        },
        HostOpResult, InvitationTokenSecret, Password, SessionHandle,
    };
    use org_office_launch::OfficeFile;
    use org_pki::{
        cert::build_org_ca_cert,
        invitation::{InvitationToken, NewInvitationParams},
        SigningKey,
    };
    use org_proto::{CallError, ErrorCode, Hello, OfficeRole};

    use crate::service::{authentication::hash_password, listener, tests as service_tests};

    fn make_invitation(org_id: &str, office_id: &str, hq_id: &str, expires_at_ms: u64) -> String {
        let ca_key = SigningKey::generate().unwrap();
        let ca = build_org_ca_cert(&ca_key, org_id, 1_760_000_000).unwrap();
        let inv_key = SigningKey::generate().unwrap();
        InvitationToken::new(
            NewInvitationParams {
                invitation_id: crate::ids::uuid_v7().unwrap(),
                org_id: org_id.into(),
                org_name: "Acme".into(),
                office_id: office_id.into(),
                office_name: "Lisbon".into(),
                hq_office_id: hq_id.into(),
                hq_addresses: vec!["127.0.0.1:9443".into()],
                ca_cert_der: ca.cert_der,
                expires_at_ms,
            },
            &inv_key,
        )
        .unwrap()
        .encode()
        .unwrap()
        .to_string()
    }

    const JOIN_WORKER_ENV: &str = "ORG_HOST_TEST_JOIN_WORKER";

    fn write_join_worker_script(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        let body = format!(
            "{JOIN_WORKER_ENV}=1 exec '{}' --exact service::office_join::tests::satellite_join_server \
             --ignored --test-threads=1 >/dev/null",
            std::env::current_exe().unwrap().display()
        );
        fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }

    #[test]
    #[ignore = "run only as the satellite --join worker of office_join tests"]
    fn satellite_join_server() {
        use org_office_launch::{HostFirstLine, OfficeFile, RendezvousEvent, ServerEvent};
        use std::os::unix::net::UnixStream;
        use std::time::{Duration, Instant};

        if std::env::var_os(JOIN_WORKER_ENV).is_none() {
            return;
        }
        let mut stdin = std::io::BufReader::new(std::io::stdin());
        let Ok(Some(HostFirstLine::Join(join))) =
            org_office_launch::read_first_line(&mut stdin, org_office_launch::FirstLine::Join)
        else {
            std::process::exit(2);
        };
        let mut stream = UnixStream::connect(&join.office.rendezvous).unwrap();
        org_proto::frame::write_message(&mut stream, &org_office_launch::ReadyForSecrets).unwrap();
        let fence = std::sync::Mutex::new(());
        let mut receiver = org_socket::rights::StreamReceiver::new(stream.try_clone().unwrap());
        let mut recv_step = |count: usize, marker: bool| {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let got = if marker {
                    receiver
                        .receive_marker(count, org_office_launch::FD_MARKER, &fence)
                        .map(|fds| (org_office_launch::FD_MARKER, fds))
                } else {
                    receiver.receive_byte(count, &fence)
                };
                match got {
                    Ok(v) => return v,
                    Err(org_socket::rights::ReceiveError::WouldBlock)
                        if Instant::now() < deadline =>
                    {
                        std::thread::yield_now()
                    }
                    Err(_) => std::process::exit(4),
                }
            }
        };
        let mut header = Vec::new();
        loop {
            let (b, _) = recv_step(0, false);
            if b == b'\n' {
                break;
            }
            header.push(b);
        }
        let Ok(org_office_launch::Handoff::Join { fds, .. }) =
            org_office_launch::decode_handoff(&header)
        else {
            std::process::exit(5);
        };
        let (_, mut transfer) = recv_step(fds.len(), true);
        drop(receiver);
        stream.set_nonblocking(false).unwrap();
        org_office_launch::write_rendezvous_event(
            &mut stream,
            &RendezvousEvent::FdsReceived {
                of: "handoff".into(),
            },
        )
        .unwrap();
        let transfer = transfer.pop().unwrap();
        let mut control = org_socket::rights::DatagramReceiver::new(
            std::os::unix::net::UnixDatagram::from(transfer),
        );
        let mut payload = [0; org_office_launch::TRANSFER_HEADER_MAX_BYTES];
        let deadline = Instant::now() + Duration::from_secs(5);
        let received = loop {
            match control.receive_up_to(&mut payload, 1, &fence) {
                Ok(r) => break r,
                Err(org_socket::rights::ReceiveError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(_) => std::process::exit(7),
            }
        };
        let Ok(org_office_launch::TransferRecord::WriterControl { channel_id, .. }) =
            org_office_launch::decode_transfer_record(&payload[..received.bytes])
        else {
            std::process::exit(8);
        };
        let writer = UnixStream::from(received.descriptors.into_iter().next().unwrap());
        writer.set_nonblocking(false).unwrap();
        let mut writer = std::io::BufReader::new(writer);
        let _ =
            org_office_launch::writer::read::<org_office_launch::writer::Admission>(&mut writer);
        org_office_launch::write_rendezvous_event(
            &mut stream,
            &RendezvousEvent::FdsReceived { of: channel_id },
        )
        .unwrap();
        org_office_launch::write_rendezvous_event(
            &mut stream,
            &RendezvousEvent::Server(ServerEvent::JoinPending {
                request_id: format!("req-{}", join.office.office_id),
                check_code: "ABCD-EFGH".into(),
            }),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(40));
        let office_file = OfficeFile::selected(
            &join.office.office_id,
            &join.office_name,
            OfficeRole::Satellite,
            &join.org_id,
            &join.org_name,
            "2026-11-23T00:00:00.000Z",
        )
        .unwrap();
        org_office_launch::write_rendezvous_event(
            &mut stream,
            &RendezvousEvent::Server(ServerEvent::Joined {
                office: office_file,
            }),
        )
        .unwrap();
        let mut sink = [0u8; 64];
        let _ = std::io::Read::read(&mut stdin, &mut sink);
    }

    struct JoinTestHarness {
        _home: tempfile::TempDir,
        _root: tempfile::TempDir,
        _local: listener::LocalConnection,
        service: Arc<Service>,
        peer: std::io::BufReader<std::os::unix::net::UnixStream>,
        session_token: Option<String>,
        now_ms: u64,
        org_id: String,
        hq_id: String,
        sat_id: String,
        enrolled_sat_id: String,
    }

    impl JoinTestHarness {
        fn start_with_mode(with_admin: bool, with_worker: bool) -> Self {
            use crate::broadcast::Broadcast;
            use crate::catalog::Catalog;
            use crate::keys::memory::MemoryKeys;
            use crate::offices::Offices;
            use crate::supervisor::{Supervisor, SupervisorConfig};
            use crate::timings::Timings;

            let home = tempfile::Builder::new()
                .prefix("aoj")
                .tempdir_in("/tmp")
                .unwrap();
            let runtime_dir = home.path().join("run");
            fs::create_dir_all(&runtime_dir).unwrap();
            let instance_id = crate::ids::uuid_v7().unwrap();
            let (root, authentication) = if with_admin {
                let (r, auth, _) = crate::service::authentication::tests::fixture();
                (r, auth)
            } else {
                crate::service::authentication::tests::fixture_without_accounts()
            };
            let (db, _) = authentication.provisioning_storage();
            let now = super::browser_now_ms().unwrap();
            let session_login = if with_admin {
                let verifier =
                    hash_password(&Password::new("synthetic test password".into()).unwrap())
                        .unwrap();
                let admin_acct = crate::ids::generate().unwrap();
                db.seed_account(&admin_acct, "Admin", &verifier, true, now)
                    .unwrap();
                Some("Admin")
            } else {
                None
            };

            let hq_id = crate::ids::uuid_v7().unwrap();
            let sat_id = crate::ids::uuid_v7().unwrap();
            let enrolled_sat_id = crate::ids::uuid_v7().unwrap();

            let program = if with_worker {
                let p = home.path().join("join-worker.sh");
                write_join_worker_script(&p);
                p
            } else {
                home.path().join("no-such-server")
            };
            let custody =
                crate::writer::TestCustody::reserve(home.path(), &instance_id, &[&enrolled_sat_id]);
            let org_id = custody.organization.clone();
            let supervisor = Supervisor::new(
                SupervisorConfig {
                    program,
                    home: home.path().to_path_buf(),
                    runtime_dir: Some(runtime_dir),
                    logs_dir: home.path().join("logs"),
                    log_max_bytes: 1024,
                    timings: Timings::default(),
                    keys: Arc::new(MemoryKeys::default()),
                    server_check: Arc::new(crate::server_check::ServerCheck::Skip),
                    sweep_orphans: false,
                    federation: None,
                },
                &instance_id,
            )
            .with_writers(custody.writers);
            let broadcast = Arc::new(Broadcast::default());
            let offices = Offices::start(
                home.path(),
                Catalog::default(),
                Vec::new(),
                supervisor,
                Arc::clone(&broadcast),
                &Timings::default(),
            );
            let service = Service::new(offices, broadcast, Some(instance_id))
                .with_authentication(Some(Arc::new(authentication)));
            service.offices.insert_catalog_for_test(OfficeFile::new(
                &enrolled_sat_id,
                "Existing Sat",
                OfficeRole::Satellite,
                &org_id,
                "Acme",
                "2026-11-21T00:00:00.000Z",
            ));
            let service = Arc::new(service);
            let (host_stream, peer_stream) = std::os::unix::net::UnixStream::pair().unwrap();
            let handler: Arc<dyn listener::Handler> = service.clone();
            let local = listener::LocalConnection::start(host_stream, handler).unwrap();
            peer_stream
                .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                .unwrap();
            let mut peer = std::io::BufReader::new(peer_stream);
            service_tests::exchange::<Hello>(&mut peer, 1, &Default::default())
                .unwrap()
                .into_result::<Hello>()
                .unwrap();
            let session_token = session_login.map(|login| {
                let verified = service_tests::exchange::<AccountVerify>(
                    &mut peer,
                    2,
                    &AccountVerifyParams {
                        login: login.into(),
                        password: Password::new("synthetic test password".into()).unwrap(),
                    },
                )
                .unwrap()
                .into_result::<AccountVerify>()
                .unwrap();
                verified.session.expose().into()
            });
            Self {
                _home: home,
                _root: root,
                _local: local,
                service,
                peer,
                session_token,
                now_ms: now as u64,
                org_id,
                hq_id,
                sat_id,
                enrolled_sat_id,
            }
        }

        fn session(&self) -> Option<SessionHandle> {
            self.session_token
                .as_ref()
                .map(|t| SessionHandle::new(t.clone()).unwrap())
        }
    }

    #[test]
    fn office_join_runs_supervisor_rendezvous_and_enforces_consent_and_replay() {
        let mut h = JoinTestHarness::start_with_mode(true, true);
        let token = make_invitation(&h.org_id, &h.sat_id, &h.hq_id, h.now_ms + 600_000);
        let s1 = h.session();
        let no_consent = service_tests::exchange::<OfficeJoin>(
            &mut h.peer,
            3,
            &OfficeJoinParams {
                session: s1,
                operation_id: crate::ids::uuid_v7().unwrap(),
                invitation: InvitationTokenSecret::new(token.clone()).unwrap(),
                consent: false,
            },
        )
        .unwrap()
        .into_result::<OfficeJoin>();
        assert!(matches!(
            no_consent,
            Err(CallError::Api(ref e)) if e.code == ErrorCode::InvalidRequest
        ));

        let join_op = crate::ids::uuid_v7().unwrap();
        let s2 = h.session();
        let joined = service_tests::exchange::<OfficeJoin>(
            &mut h.peer,
            4,
            &OfficeJoinParams {
                session: s2,
                operation_id: join_op.clone(),
                invitation: InvitationTokenSecret::new(token.clone()).unwrap(),
                consent: true,
            },
        )
        .unwrap()
        .into_result::<OfficeJoin>()
        .unwrap();
        match joined {
            HostOpResult::Completed { result } => {
                assert_eq!(result.office_id, h.sat_id);
                assert_eq!(
                    result.state,
                    org_host_control::ops::OfficeJoinState::JoinPending
                );
                assert_eq!(result.check_code.as_deref(), Some("ABCD-EFGH"));
            }
            other => panic!("expected Completed, got {other:?}"),
        }
        let s3 = h.session();
        let replay = service_tests::exchange::<OfficeJoin>(
            &mut h.peer,
            5,
            &OfficeJoinParams {
                session: s3,
                operation_id: join_op,
                invitation: InvitationTokenSecret::new(token).unwrap(),
                consent: true,
            },
        )
        .unwrap()
        .into_result::<OfficeJoin>()
        .unwrap();
        assert!(matches!(replay, HostOpResult::Completed { .. }));

        // Background join watcher adopts the worker into catalog once `Joined` arrives!
        let mut adopted = false;
        for _ in 0..40 {
            if h.service.offices.offices().iter().any(|o| o.id == h.sat_id) {
                adopted = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(
            adopted,
            "joined satellite must be adopted into host catalog"
        );
        h.service.offices.shutdown();
    }

    #[test]
    fn office_join_fresh_instance_admits_local_peer_and_accounts_require_admin() {
        // Fresh satellite-only instance (zero accounts): verified local peer with consent succeeds!
        let mut fresh = JoinTestHarness::start_with_mode(false, true);
        let token = make_invitation(
            &fresh.org_id,
            &fresh.sat_id,
            &fresh.hq_id,
            fresh.now_ms + 600_000,
        );
        let joined = service_tests::exchange::<OfficeJoin>(
            &mut fresh.peer,
            3,
            &OfficeJoinParams {
                session: None,
                operation_id: crate::ids::uuid_v7().unwrap(),
                invitation: InvitationTokenSecret::new(token).unwrap(),
                consent: true,
            },
        )
        .unwrap()
        .into_result::<OfficeJoin>()
        .unwrap();
        match joined {
            HostOpResult::Completed { result } => {
                assert_eq!(result.check_code.as_deref(), Some("ABCD-EFGH"));
            }
            other => panic!("expected Completed on fresh instance, got {other:?}"),
        }
        fresh.service.offices.shutdown();

        // Instance with an admin account refuses session-less join AND refuses non-admin owner!
        let mut with_acct = JoinTestHarness::start_with_mode(true, true);
        let (db, _) = with_acct
            .service
            .authentication
            .as_ref()
            .unwrap()
            .provisioning_storage();
        let verifier =
            hash_password(&Password::new("synthetic nonadmin password".into()).unwrap()).unwrap();
        let non_admin_acct = crate::ids::generate().unwrap();
        db.seed_account(
            &non_admin_acct,
            "NonAdminMember",
            &verifier,
            false,
            with_acct.now_ms as i64,
        )
        .unwrap();
        let verified_non_admin = service_tests::exchange::<AccountVerify>(
            &mut with_acct.peer,
            3,
            &AccountVerifyParams {
                login: "NonAdminMember".into(),
                password: Password::new("synthetic nonadmin password".into()).unwrap(),
            },
        )
        .unwrap()
        .into_result::<AccountVerify>()
        .unwrap();
        let non_admin_session =
            SessionHandle::new(verified_non_admin.session.expose().into()).unwrap();
        let tok2 = make_invitation(
            &with_acct.org_id,
            &with_acct.sat_id,
            &with_acct.hq_id,
            with_acct.now_ms + 600_000,
        );
        let refused = service_tests::exchange::<OfficeJoin>(
            &mut with_acct.peer,
            4,
            &OfficeJoinParams {
                session: Some(non_admin_session),
                operation_id: crate::ids::uuid_v7().unwrap(),
                invitation: InvitationTokenSecret::new(tok2).unwrap(),
                consent: true,
            },
        )
        .unwrap()
        .into_result::<OfficeJoin>();
        assert!(
            matches!(refused, Err(CallError::Api(ref e)) if e.code == ErrorCode::NotAuthorized),
            "non-admin account must be refused office.join: {refused:?}"
        );
        with_acct.service.offices.shutdown();
    }

    #[test]
    fn office_d25_re_enroll_and_remove_pending() {
        let mut h = JoinTestHarness::start_with_mode(true, true);
        let new_hq_id = crate::ids::uuid_v7().unwrap();
        let re_token = make_invitation(
            &h.org_id,
            &h.enrolled_sat_id,
            &new_hq_id,
            h.now_ms + 600_000,
        );
        let sat_id = h.enrolled_sat_id.clone();
        let s1 = h.session();
        let re_res = service_tests::exchange::<OfficeReEnroll>(
            &mut h.peer,
            7,
            &OfficeReEnrollParams {
                session: s1,
                operation_id: crate::ids::uuid_v7().unwrap(),
                office_id: sat_id,
                invitation: InvitationTokenSecret::new(re_token).unwrap(),
                consent: true,
            },
        )
        .unwrap()
        .into_result::<OfficeReEnroll>()
        .unwrap();
        match re_res {
            HostOpResult::Completed { result } => {
                assert_eq!(result.office_id, h.enrolled_sat_id);
                assert_eq!(result.hq_office_id, new_hq_id);
                assert_eq!(
                    result.state,
                    org_host_control::ops::OfficeJoinState::JoinPending
                );
                assert_eq!(result.check_code.as_deref(), Some("ABCD-EFGH"));
            }
            other => panic!("expected Completed, got {other:?}"),
        }
        // Write a pending-join file for an unjoined office and remove it via `office.remove_pending`.
        let pending_id = crate::ids::uuid_v7().unwrap();
        write_pending_join(
            h.service.offices.home(),
            &PendingJoinRecord {
                v: 1,
                office_id: pending_id.clone(),
                office_name: "Pending".into(),
                org_id: h.org_id.clone(),
                org_name: "Acme".into(),
                hq_office_id: h.hq_id.clone(),
                hq_addresses: vec!["127.0.0.1:9443".into()],
                invitation_id: crate::ids::uuid_v7().unwrap(),
                ca_fingerprint_hex: "00".repeat(32),
                re_enroll: false,
                created_at_ms: h.now_ms as i64,
                expires_at_ms: h.now_ms + 600_000,
            },
        )
        .unwrap();
        let s2 = h.session();
        let removed = service_tests::exchange::<OfficeRemovePending>(
            &mut h.peer,
            8,
            &OfficeRemovePendingParams {
                session: s2,
                operation_id: crate::ids::uuid_v7().unwrap(),
                office_id: pending_id,
            },
        )
        .unwrap()
        .into_result::<OfficeRemovePending>()
        .unwrap();
        assert!(matches!(removed, HostOpResult::Completed { .. }));
        h.service.offices.shutdown();
    }
}
