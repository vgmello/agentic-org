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

use super::records::{AttachmentRecord, MetaPatch};

/// A private file with the fingerprint of the request whose command created it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PrivateOperation {
    /// Lowercase hex SHA-256 of the canonical request ([`request_fingerprint`]).
    pub fingerprint: String,
    pub record: AttachmentRecord,
}

impl PrivateOperation {
    /// The answer to the owner's command arriving again: the first record when it carries the
    /// same request, `operation_reused` when it carries another.
    pub fn replay(self, fingerprint: &str) -> Result<AttachmentRecord, ApiError> {
        if self.fingerprint == fingerprint {
            Ok(self.record)
        } else {
            Err(operation_reused())
        }
    }
}

/// The fingerprint a private file keeps of the request that created it.
pub fn request_fingerprint(request: &[u8]) -> String {
    super::blobs::ContentHash::of(request).to_string()
}

/// The port of the private space's metadata. Command IDs are scoped to the private space's
/// owner: one person's IDs never collide with, or reveal, another's.
pub trait UserSpaceStore: Send + Sync {
    /// Whether this office holds private spaces (HQ) at all.
    fn available(&self) -> bool;

    /// Records a new private file for its owner (`record.target_id`) under the owner's
    /// `operation_id`. The owner's retry with the same `fingerprint` returns the first record;
    /// the owner's reuse of `operation_id` with another one is `operation_reused`, and nothing
    /// is written.
    fn insert(
        &self,
        org_id: &str,
        operation_id: &str,
        fingerprint: &str,
        record: &AttachmentRecord,
    ) -> Result<Result<AttachmentRecord, ApiError>, OfficeError>;

    fn get(&self, org_id: &str, id: &str) -> Result<Option<AttachmentRecord>, OfficeError>;

    /// The file `owner`'s command `operation_id` created, to answer its retry.
    fn by_operation(
        &self,
        org_id: &str,
        owner: &str,
        operation_id: &str,
    ) -> Result<Option<PrivateOperation>, OfficeError>;

    /// The owner's files in ascending ID order, those after `after` only, at most `limit`.
    fn list(
        &self,
        org_id: &str,
        owner: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<AttachmentRecord>, OfficeError>;

    /// Applies `patch` to `owner`'s file `id` as it is when the write is decided (one
    /// transaction), and returns the result; `None` when `owner` has no such file.
    fn update_meta(
        &self,
        org_id: &str,
        owner: &str,
        id: &str,
        patch: &MetaPatch,
    ) -> Result<Option<AttachmentRecord>, OfficeError>;
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
        _fingerprint: &str,
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
        _owner: &str,
        _operation_id: &str,
    ) -> Result<Option<PrivateOperation>, OfficeError> {
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

    fn update_meta(
        &self,
        _org_id: &str,
        _owner: &str,
        _id: &str,
        _patch: &MetaPatch,
    ) -> Result<Option<AttachmentRecord>, OfficeError> {
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

    use super::{AttachmentRecord, MetaPatch, OfficeError, PrivateOperation, UserSpaceStore};

    /// `(organization, id)` to `(operation_id, the file and its request fingerprint)`.
    type Rows = BTreeMap<(String, String), (String, PrivateOperation)>;

    #[derive(Default)]
    pub struct MemoryUserSpaceStore {
        rows: Mutex<Rows>,
    }

    fn owned_by(record: &AttachmentRecord, owner: &str) -> bool {
        record
            .target_id
            .as_ref()
            .is_some_and(|o| o.as_str() == owner)
    }

    fn find(
        rows: &Rows,
        org_id: &str,
        owner: &str,
        operation_id: &str,
    ) -> Option<PrivateOperation> {
        rows.iter()
            .find(|((org, _), (op, row))| {
                org == org_id && op == operation_id && owned_by(&row.record, owner)
            })
            .map(|(_, (_, row))| row.clone())
    }

    impl MemoryUserSpaceStore {
        pub fn new() -> Self {
            Self::default()
        }

        fn lock(&self) -> std::sync::MutexGuard<'_, Rows> {
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
            fingerprint: &str,
            record: &AttachmentRecord,
        ) -> Result<Result<AttachmentRecord, ApiError>, OfficeError> {
            let Some(owner) = record.target_id.as_ref() else {
                return Err(OfficeError::Invalid("a private file names its owner"));
            };
            let mut rows = self.lock();
            if let Some(earlier) = find(&rows, org_id, owner.as_str(), operation_id) {
                return Ok(earlier.replay(fingerprint));
            }
            let key = (org_id.to_owned(), record.id.to_string());
            if rows.contains_key(&key) {
                return Err(OfficeError::Invalid("a private file breaks its contract"));
            }
            let row = PrivateOperation {
                fingerprint: fingerprint.to_owned(),
                record: record.clone(),
            };
            rows.insert(key, (operation_id.to_owned(), row));
            Ok(Ok(record.clone()))
        }

        fn get(&self, org_id: &str, id: &str) -> Result<Option<AttachmentRecord>, OfficeError> {
            Ok(self
                .lock()
                .get(&(org_id.to_owned(), id.to_owned()))
                .map(|(_, row)| row.record.clone()))
        }

        fn by_operation(
            &self,
            org_id: &str,
            owner: &str,
            operation_id: &str,
        ) -> Result<Option<PrivateOperation>, OfficeError> {
            Ok(find(&self.lock(), org_id, owner, operation_id))
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
                .filter(|((org, id), (_, row))| {
                    org == org_id
                        && owned_by(&row.record, owner)
                        && after.is_none_or(|after| id.as_str() > after)
                })
                .take(limit)
                .map(|(_, (_, row))| row.record.clone())
                .collect())
        }

        fn update_meta(
            &self,
            org_id: &str,
            owner: &str,
            id: &str,
            patch: &MetaPatch,
        ) -> Result<Option<AttachmentRecord>, OfficeError> {
            let mut rows = self.lock();
            let Some((_, row)) = rows.get_mut(&(org_id.to_owned(), id.to_owned())) else {
                return Ok(None);
            };
            if !owned_by(&row.record, owner) {
                return Ok(None);
            }
            patch.apply(&mut row.record);
            Ok(Some(row.record.clone()))
        }
    }
}
