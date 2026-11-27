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

use super::browser_sessions::browser_now_ms;
use super::reply::ok;
use super::session_office::{authorize_session_first, journal_error};
use super::{Authentication, Service};
use crate::control_listener::Connection;
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
mod tests;
