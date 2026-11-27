//! Journaled `tls.set` replacement handler for [`WebTls`].

use org_host_control::ops::{TlsRefusal, TlsSetParams};
use org_host_control::HostOpResult;
use org_proto::{ApiError, ErrorCode};

use super::store::{wall_ms, WebTls};
use super::{check, Source};
use crate::host_log::log;
use crate::hostdb::journal::Attempt;
use crate::hostdb::web_tls::{TlsLookup, TlsStored};

impl WebTls {
    /// `tls.set` (X-02 §6.2) from the administrator `account` at `now` (Unix seconds): the
    /// journal lookup, then the pair's checks in the contract's order, then the pair is
    /// installed as supplied. `Ok(true)` with a result that installed a pair: the caller emits
    /// `tls.renew`. Refusals other than `tls_key_mismatch` are answered and not stored.
    pub(crate) fn set(
        &self,
        params: &TlsSetParams,
        account: &str,
        now: i64,
    ) -> Result<(HostOpResult<org_proto::Empty, TlsRefusal>, bool), ApiError> {
        let storage =
            |_| ApiError::new(ErrorCode::StorageError, "the TLS pair could not be stored");
        let rejected = |refusal| HostOpResult::Rejected { refusal };
        let Ok(fingerprint) = org_host_control::journal::fingerprint(params) else {
            return Ok((rejected(TlsRefusal::TlsInvalid), false));
        };
        let op = Attempt {
            id: &params.operation_id,
            kind: <TlsSetParams as org_host_control::journal::Journaled>::METHOD,
            fingerprint,
            account,
        };
        let wall =
            wall_ms().ok_or_else(|| ApiError::new(ErrorCode::Unavailable, "clock unavailable"))?;
        let _writing = crate::sync::lock(&self.writing);
        if let TlsLookup::Replay(stored) = self.db.tls_set_lookup(&op, wall).map_err(journal)? {
            return Ok((replayed(stored)?, false));
        }
        let checked = match check(
            &params.cert,
            params.key.expose(),
            &[&self.origin],
            Some(now),
        ) {
            Ok(checked) => checked,
            Err(TlsRefusal::TlsKeyMismatch) => {
                let stored: TlsStored = HostOpResult::Rejected {
                    refusal: TlsRefusal::TlsKeyMismatch,
                };
                let answer = self
                    .db
                    .tls_set_write(&op, wall, &stored, None)
                    .map_err(journal)?
                    .unwrap_or(stored);
                return Ok((replayed(answer)?, false));
            }
            Err(refusal) => return Ok((rejected(refusal), false)),
        };
        let previous = self.db.web_tls_pair().map_err(storage)?;
        let pair = self
            .stored(&checked, Source::Supplied)
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "clock unavailable"))?;
        self.add_key(&checked).map_err(|_| {
            ApiError::new(ErrorCode::StorageError, "the TLS key could not be stored")
        })?;
        let completed: TlsStored = HostOpResult::Completed {
            result: serde_json::json!({}),
        };
        let written = self.db.tls_set_write(&op, wall, &completed, Some(&pair));
        if written.is_err() {
            self.drop_unnamed(&checked);
        }
        match written.map_err(journal)? {
            None => {
                self.forget(previous.as_ref(), &pair);
                log(&format!(
                    "the TLS certificate for {} was replaced: {}",
                    self.origin,
                    checked.summary(Source::Supplied)
                ));
                Ok((replayed(completed)?, true))
            }
            Some(existing) => Ok((replayed(existing)?, false)),
        }
    }
}

fn replayed(stored: TlsStored) -> Result<HostOpResult<org_proto::Empty, TlsRefusal>, ApiError> {
    match stored {
        HostOpResult::Completed { .. } => Ok(HostOpResult::Completed {
            result: org_proto::Empty {},
        }),
        HostOpResult::Rejected { refusal } => Ok(HostOpResult::Rejected { refusal }),
        _ => Err(ApiError::new(
            ErrorCode::StorageError,
            "the stored result could not be read",
        )),
    }
}

fn journal(error: crate::hostdb::Error) -> ApiError {
    use crate::hostdb::Error;
    match error {
        Error::OperationReused => ApiError::new(
            ErrorCode::OperationReused,
            "the operation ID was used for another operation",
        ),
        Error::Expired => ApiError::new(ErrorCode::Expired, "the operation ID has expired"),
        Error::ClockSkew => {
            ApiError::new(ErrorCode::ClockSkew, "the operation ID is from the future")
        }
        Error::InvalidId => ApiError::new(ErrorCode::InvalidRequest, "invalid operation ID"),
        _ => ApiError::new(ErrorCode::StorageError, "the host registry is unavailable"),
    }
}
