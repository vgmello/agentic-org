//! Retained org command answers: lookup, principal matching and the durable clock.

use super::*;
use org_proto::OperationOutcome;

/// A trusted connection identity, supplied by the later grant/binding adapter.
/// `None` is today's implicit owner connection; never derive this from request JSON.
#[derive(Clone, Copy)]
pub(super) struct Principal<'a> {
    pub account: Option<&'a str>,
    pub member: Option<&'a str>,
    pub kind: &'a str,
}

pub(super) struct Retained {
    pub position: Position,
    expires_at: String,
    result: Option<String>,
    account: Option<String>,
    member: Option<String>,
    kind: Option<String>,
}

impl Retained {
    pub fn matches(&self, principal: Option<Principal<'_>>) -> bool {
        // Legacy rows match any principal; until grants, connections act as the owner.
        let Some(kind) = self.kind.as_deref() else {
            return true;
        };
        let Some(principal) = principal else {
            return true;
        };
        kind == principal.kind
            && self.account.as_deref() == principal.account
            && self.member.as_deref() == principal.member
    }

    pub fn answer(&self) -> Result<Answer, OfficeError> {
        self.result
            .as_deref()
            .and_then(|text| serde_json::from_str(text).ok())
            .ok_or(OfficeError::Invalid(
                "a stored command answer is not readable",
            ))
    }
}

pub(super) fn retained(
    db: &Connection,
    id: &str,
    load_principal: bool,
) -> Result<Option<Retained>, OfficeError> {
    // An implicit owner needs only the legacy replay projection. Bound replay and
    // lookup read the principal columns; neither path rewrites retained answers.
    let query = if load_principal {
        "SELECT epoch, seq, expires_at, result, principal_account, principal_member, actor_kind
         FROM commands WHERE command_id = ?1"
    } else {
        "SELECT epoch, seq, expires_at, result, NULL, NULL, NULL
         FROM commands WHERE command_id = ?1"
    };
    db.query_row(query, [id], |row| {
        Ok(Retained {
            position: Position {
                epoch: row.get::<_, i64>(0)? as u64,
                seq: row.get::<_, i64>(1)? as u64,
            },
            expires_at: row.get(2)?,
            result: row.get(3)?,
            account: row.get(4)?,
            member: row.get(5)?,
            kind: row.get(6)?,
        })
    })
    .optional()
    .map_err(OfficeError::storage)
}

pub(super) fn clock_high(db: &Connection) -> Result<u64, OfficeError> {
    match db
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            [CLOCK_HIGH_KEY],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(OfficeError::storage)?
    {
        None => Ok(0),
        Some(value) => value
            .parse()
            .map_err(|_| OfficeError::Invalid("the clock high-water mark is not readable")),
    }
}

pub(super) fn reused() -> ApiError {
    ApiError::new(
        ErrorCode::OperationReused,
        "command id reused by another operation",
    )
}

pub(super) fn lookup(
    db: &Connection,
    clock_ms: u64,
    id: &str,
    principal: Option<Principal<'_>>,
) -> Result<Result<OperationOutcome, ApiError>, OfficeError> {
    if ulid::time_ms(id).is_none() {
        return Ok(Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "operation id must be a ULID",
        )));
    }
    let missing = || ApiError::new(ErrorCode::NotFound, "operation not found");
    let Some(stored) = retained(db, id, true)? else {
        return Ok(Err(missing()));
    };
    if !stored.matches(principal) {
        return Ok(Err(missing()));
    }
    let expires = time::parse_rfc3339_ms(&stored.expires_at).ok_or(OfficeError::Invalid(
        "a stored command expiry is not readable",
    ))?;
    if expires <= clock_ms.max(clock_high(db)?) {
        return Ok(Err(ApiError::new(
            ErrorCode::Expired,
            "operation has expired",
        )));
    }
    let answer = stored.answer()?;
    Ok(Ok(match answer.refusal {
        Some(refusal) => OperationOutcome::Rejected { refusal },
        None => OperationOutcome::Committed {
            result: serde_json::to_value(committed(stored.position))
                .map_err(OfficeError::storage)?,
        },
    }))
}

#[cfg(test)]
mod tests;
