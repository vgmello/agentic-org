//! Run a replayable sync attempt through the office's existing SQLite writer.

use std::error::Error;

use org_sync::error::{StoreError, SyncError, MAX_STORE_RETRIES};
use rusqlite::{Connection, ErrorCode};

use super::Store;
use crate::error::OfficeError;

fn sqlite_busy_or_locked(cause: &(dyn Error + 'static)) -> bool {
    let mut current = Some(cause);
    while let Some(error) = current {
        if let Some(rusqlite::Error::SqliteFailure(failure, _)) = error.downcast_ref() {
            return matches!(
                failure.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            );
        }
        current = error.source();
    }
    false
}

pub(super) fn storage_error(cause: impl Error + Send + Sync + 'static) -> StoreError {
    if sqlite_busy_or_locked(&cause) {
        StoreError::busy_or_locked(cause)
    } else {
        StoreError::unavailable(cause)
    }
}

fn aborts_batch(error: &OfficeError) -> bool {
    error
        .source()
        .and_then(|cause| cause.downcast_ref::<SyncError>())
        .is_some_and(SyncError::aborts_batch)
}

fn retryable(error: &OfficeError) -> bool {
    match error
        .source()
        .and_then(|cause| cause.downcast_ref::<SyncError>())
    {
        Some(SyncError::Storage(cause)) => cause.is_retryable(),
        Some(SyncError::Refused(_)) => false,
        None => sqlite_busy_or_locked(error),
    }
}

pub(super) fn sync_error(error: OfficeError) -> SyncError {
    match error {
        OfficeError::Storage(cause) => match cause.downcast::<SyncError>() {
            Ok(error) => *error,
            Err(cause) => storage_error(OfficeError::Storage(cause)).into(),
        },
        error => storage_error(error).into(),
    }
}

impl Store {
    pub(super) fn sync_write<R: Send + 'static>(
        &self,
        attempt: impl Fn(&Connection) -> Result<R, SyncError> + Send + std::marker::Sync + 'static,
    ) -> Result<R, SyncError> {
        self.write_replayable(move |db| attempt(db).map_err(OfficeError::storage))
            .map_err(sync_error)
    }

    /// Retry an entire deterministic writer request, retaining its original error and output.
    pub(crate) fn write_replayable<R: Send + 'static>(
        &self,
        attempt: impl Fn(&Connection) -> Result<R, OfficeError> + Send + std::marker::Sync + 'static,
    ) -> Result<R, OfficeError> {
        let attempt = std::sync::Arc::new(attempt);
        for retry in 0..=MAX_STORE_RETRIES {
            let attempt = std::sync::Arc::clone(&attempt);
            let result = self.write_with_batch_abort(
                move |db| attempt(db).map(|value| (value, None)),
                aborts_batch,
            );
            match result {
                Err(ref error) if retry < MAX_STORE_RETRIES && retryable(error) => {}
                result => return result,
            }
        }
        unreachable!("the final attempt always returns")
    }
}
