//! The pending HQ recovery code a selected HQ preparation keeps in host custody until the HQ
//! is linked: its record, and the one reader that checks it against its reservation. Both the
//! provisioning flow and the recovery ledger read it, so it lives with the reservation it is
//! bound to.

use org_proto::{ApiError, ErrorCode, RecoveryCode};
use serde::{Deserialize, Serialize};

use super::provisioning::Reservation;
use crate::host_secrets::{HostSecrets, Name};

/// The custody record of a pending recovery code, bound to its reservation.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Pending {
    pub(crate) version: u32,
    pub(crate) reservation: Reservation,
    pub(crate) code: RecoveryCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) import_source: Option<org_office_launch::ImportSourceV1>,
}

/// Reads the pending recovery code for `r` from custody. Refuses a record not bound to exactly
/// this unlinked reservation, and a code not in its grouped hexadecimal form.
pub fn pending_recovery(
    custody: &dyn HostSecrets,
    r: &Reservation,
) -> Result<RecoveryCode, ApiError> {
    let bytes = custody
        .load(&Name::parse(&r.recovery_handle).map_err(|_| unavailable())?)
        .map_err(|_| unavailable())?
        .ok_or_else(unavailable)?;
    let p: Pending = serde_json::from_slice(bytes.expose()).map_err(|_| unavailable())?;
    if p.version != 1
        || p.reservation.operation_id != r.operation_id
        || p.reservation.account != r.account
        || p.reservation.fingerprint != r.fingerprint
        || p.reservation.prepared != r.prepared
        || p.reservation.owner_member != r.owner_member
        || p.reservation.recovery_handle != r.recovery_handle
        || p.reservation.key_fingerprint.is_some()
        || p.reservation.linked
    {
        return Err(unavailable());
    }
    let code = p.code.expose().as_bytes();
    if code.len() != 49
        || code.iter().enumerate().any(|(i, b)| {
            if i % 5 == 4 {
                *b != b'-'
            } else {
                !b.is_ascii_digit() && !(b'A'..=b'F').contains(b)
            }
        })
    {
        return Err(unavailable());
    }
    Ok(p.code)
}

/// HQ preparation has not resolved; the caller retries the original operation.
pub(crate) fn unavailable() -> ApiError {
    ApiError::new(
        ErrorCode::Unavailable,
        "HQ preparation is unresolved; retry the original operation",
    )
}
