//! A person's HQ-only private file space (`TASK-98.1`, ADR-0037, `ARCH-02`): metadata in
//! `<durable>/private/user-space.db` (the SQLite adapter is `SqliteUserSpaceStore` in
//! `src/store/sqlite/user_space.rs`; [`memory::MemoryUserSpaceStore`] backs the memory engine)
//! and bytes in `<durable>/private/blobs/`.
//!
//! None of it ever enters `office.db`, `schema.sql`, `durable/state/`, the journal, Git or the
//! shared `durable/blobs/`. A satellite holds no private space at all: it never creates
//! `durable/private/` and answers every private call `unavailable`
//! ([`SatelliteRefusedUserSpace`]). Who may touch a record is decided by
//! [`crate::authz::AuthorizationPolicy`] before this store is asked.

use org_proto::{ApiError, ErrorCode};

use crate::error::OfficeError;

use super::records::AttachmentRecord;

/// The port of the private space's metadata.
pub trait UserSpaceStore: Send + Sync {
    /// Whether this office holds private spaces (HQ) at all.
    fn available(&self) -> bool;

    /// Records a new private file. Retrying `operation_id` for the same owner returns the
    /// first record; for anyone else it is `operation_reused`.
    fn insert(
        &self,
        org_id: &str,
        operation_id: &str,
        record: &AttachmentRecord,
    ) -> Result<Result<AttachmentRecord, ApiError>, OfficeError>;

    fn get(&self, org_id: &str, id: &str) -> Result<Option<AttachmentRecord>, OfficeError>;

    /// The record a command created, to answer its retry.
    fn by_operation(
        &self,
        org_id: &str,
        operation_id: &str,
    ) -> Result<Option<AttachmentRecord>, OfficeError>;

    /// The owner's files in ascending ID order, those after `after` only, at most `limit`.
    fn list(
        &self,
        org_id: &str,
        owner: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<AttachmentRecord>, OfficeError>;

    /// Replaces the title, description, status and `updated_at` of an existing record.
    fn update(&self, org_id: &str, record: &AttachmentRecord) -> Result<(), OfficeError>;
}

/// The refusal every private call gets on a satellite.
pub fn satellite_refusal() -> ApiError {
    ApiError::new(
        ErrorCode::Unavailable,
        "private files are kept only at HQ and are unavailable on a satellite office",
    )
}

/// The satellite's private space: there is none (`ARCH-02`). Nothing is created on disk.
#[derive(Debug, Default)]
pub struct SatelliteRefusedUserSpace;

impl UserSpaceStore for SatelliteRefusedUserSpace {
    fn available(&self) -> bool {
        false
    }

    fn insert(
        &self,
        _org_id: &str,
        _operation_id: &str,
        _record: &AttachmentRecord,
    ) -> Result<Result<AttachmentRecord, ApiError>, OfficeError> {
        Ok(Err(satellite_refusal()))
    }

    fn get(&self, _org_id: &str, _id: &str) -> Result<Option<AttachmentRecord>, OfficeError> {
        Ok(None)
    }

    fn by_operation(
        &self,
        _org_id: &str,
        _operation_id: &str,
    ) -> Result<Option<AttachmentRecord>, OfficeError> {
        Ok(None)
    }

    fn list(
        &self,
        _org_id: &str,
        _owner: &str,
        _after: Option<&str>,
        _limit: usize,
    ) -> Result<Vec<AttachmentRecord>, OfficeError> {
        Ok(Vec::new())
    }

    fn update(&self, _org_id: &str, _record: &AttachmentRecord) -> Result<(), OfficeError> {
        Err(OfficeError::Invalid(
            "a satellite office holds no private files",
        ))
    }
}

/// The refusal for an operation ID already used by another request.
pub fn operation_reused() -> ApiError {
    ApiError::new(
        ErrorCode::OperationReused,
        "command_id reused with a different request",
    )
}

#[cfg(any(test, feature = "testkit"))]
pub mod memory {
    //! The private space in memory, for the memory engine.

    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use org_proto::ApiError;

    use super::{operation_reused, AttachmentRecord, OfficeError, UserSpaceStore};

    #[derive(Default)]
    pub struct MemoryUserSpaceStore {
        /// `(organization, id)` to `(operation_id, record)`.
        rows: Mutex<BTreeMap<(String, String), (String, AttachmentRecord)>>,
    }

    impl MemoryUserSpaceStore {
        pub fn new() -> Self {
            Self::default()
        }

        fn lock(
            &self,
        ) -> std::sync::MutexGuard<'_, BTreeMap<(String, String), (String, AttachmentRecord)>>
        {
            self.rows.lock().unwrap_or_else(|p| p.into_inner())
        }
    }

    impl UserSpaceStore for MemoryUserSpaceStore {
        fn available(&self) -> bool {
            true
        }

        fn insert(
            &self,
            org_id: &str,
            operation_id: &str,
            record: &AttachmentRecord,
        ) -> Result<Result<AttachmentRecord, ApiError>, OfficeError> {
            let mut rows = self.lock();
            let earlier = rows
                .iter()
                .find(|((org, _), (op, _))| org == org_id && op == operation_id)
                .map(|(_, (_, earlier))| earlier.clone());
            if let Some(earlier) = earlier {
                return Ok(if earlier.target_id == record.target_id {
                    Ok(earlier)
                } else {
                    Err(operation_reused())
                });
            }
            rows.insert(
                (org_id.to_owned(), record.id.to_string()),
                (operation_id.to_owned(), record.clone()),
            );
            Ok(Ok(record.clone()))
        }

        fn get(&self, org_id: &str, id: &str) -> Result<Option<AttachmentRecord>, OfficeError> {
            Ok(self
                .lock()
                .get(&(org_id.to_owned(), id.to_owned()))
                .map(|(_, record)| record.clone()))
        }

        fn by_operation(
            &self,
            org_id: &str,
            operation_id: &str,
        ) -> Result<Option<AttachmentRecord>, OfficeError> {
            Ok(self
                .lock()
                .iter()
                .find(|((org, _), (op, _))| org == org_id && op == operation_id)
                .map(|(_, (_, record))| record.clone()))
        }

        fn list(
            &self,
            org_id: &str,
            owner: &str,
            after: Option<&str>,
            limit: usize,
        ) -> Result<Vec<AttachmentRecord>, OfficeError> {
            Ok(self
                .lock()
                .iter()
                .filter(|((org, id), (_, record))| {
                    org == org_id
                        && record
                            .target_id
                            .as_ref()
                            .is_some_and(|o| o.as_str() == owner)
                        && after.is_none_or(|after| id.as_str() > after)
                })
                .take(limit)
                .map(|(_, (_, record))| record.clone())
                .collect())
        }

        fn update(&self, org_id: &str, record: &AttachmentRecord) -> Result<(), OfficeError> {
            let mut rows = self.lock();
            let Some((_, row)) = rows.get_mut(&(org_id.to_owned(), record.id.to_string())) else {
                return Err(OfficeError::Invalid("no such private file"));
            };
            row.title = record.title.clone();
            row.description = record.description.clone();
            row.artifact_status = record.artifact_status;
            row.updated_at = record.updated_at.clone();
            Ok(())
        }
    }
}
