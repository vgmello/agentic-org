//! Run a replayable sync attempt through the office's existing SQLite writer.

use std::error::Error;

use org_sync::error::{StoreError, SyncError, MAX_STORE_RETRIES};
use rusqlite::{Connection, ErrorCode};

use super::Store;
use crate::error::OfficeError;

pub(super) fn storage_error(cause: impl Error + Send + Sync + 'static) -> StoreError {
    let mut current: Option<&(dyn Error + 'static)> = Some(&cause);
    while let Some(error) = current {
        if let Some(rusqlite::Error::SqliteFailure(failure, _)) = error.downcast_ref() {
            if matches!(
                failure.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            ) {
                return StoreError::busy_or_locked(cause);
            }
            break;
        }
        current = error.source();
    }
    StoreError::unavailable(cause)
}

pub(crate) fn aborts_batch(error: &OfficeError) -> bool {
    error
        .source()
        .and_then(|cause| cause.downcast_ref::<SyncError>())
        .is_some_and(SyncError::aborts_batch)
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
        let attempt = std::sync::Arc::new(attempt);
        for retry in 0..=MAX_STORE_RETRIES {
            let attempt = std::sync::Arc::clone(&attempt);
            let result = self
                .write_with_batch_abort(
                    move |db| {
                        attempt(db)
                            .map(|value| (value, None))
                            .map_err(OfficeError::storage)
                    },
                    aborts_batch,
                )
                .map_err(sync_error);
            match result {
                Err(SyncError::Storage(error))
                    if error.is_retryable() && retry < MAX_STORE_RETRIES => {}
                result => return result,
            }
        }
        unreachable!("the final attempt always returns")
    }
}
