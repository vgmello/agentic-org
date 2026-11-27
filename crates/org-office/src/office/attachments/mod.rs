//! The office's attachment facade (`TASK-98.1`, ADR-0037): the nine `attachment.*` calls
//! (`upload.rs`, `attach.rs`, `read.rs`) over the stores in [`crate::attachments`], and the
//! files of messages, asks and answers (`messages.rs`, `TASK-98.2`), each decided by
//! [`AuthorizationPolicy`] and nothing else.
//!
//! Across offices (`TASK-98.6`): a satellite's new shared files wait in its outbox and ship to
//! HQ (`shipping.rs`); HQ checks, stores and binds them and serves shared bytes to satellites
//! (`custody.rs`); a satellite reads its outbox, its cache, or pulls from HQ (`local_blobs.rs`).
//!
//! Every call names the caller by employee ID (the connection's binding, never the wire), and
//! answers `Ok(Err(..))` for a caller's mistake or refusal and `Err(..)` only for storage.

mod attach;
mod custody;
mod local_blobs;
mod messages;
mod read;
mod shipping;
mod upload;

use std::sync::atomic::{AtomicU64, Ordering};

use org_proto::attachments::AttachmentTargetKind;
use org_proto::{ApiError, ErrorCode};
use org_store::{Id, Sha256};

use super::{storage, Office};
use crate::attachments::records::AttachmentRecord;
use crate::authz::{
    AuthorizationPolicy, Denied, ParentFacts, PrincipalContext, ResourceFacts, ResourceRef,
};
use crate::error::OfficeError;
use crate::model::OfficeRole;
use crate::store::open::attachment_ops::{AttachmentById, PrincipalRead, ResourceFactsRead};

/// A call's outcome: the answer, the caller's refusal, or a storage failure.
type Answer<T> = Result<Result<T, ApiError>, OfficeError>;

static ID_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// The office's snapshot as the policy's source of facts.
struct OfficeFacts<'a>(&'a Office);

impl ResourceFacts for OfficeFacts<'_> {
    fn facts(&self, resource: &ResourceRef) -> Result<ParentFacts, OfficeError> {
        let read = ResourceFactsRead {
            resource: resource.clone(),
            owner: self.0.effective_owner_for_ops(),
        };
        Ok(self.0.store.read(read).map_err(storage)?.0)
    }
}

fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::InvalidRequest, message)
}

fn not_found() -> ApiError {
    ApiError::new(ErrorCode::NotFound, "attachment not found")
}

/// An edit of a file's details on a satellite: HQ owns every shared record once it holds it,
/// so edits are made there.
fn hq_only() -> ApiError {
    ApiError::new(
        ErrorCode::Unavailable,
        "a file's details are edited at HQ; this satellite office uploads, attaches and reads files",
    )
}

/// The policy's resource for what a record is bound to.
fn target_ref(kind: AttachmentTargetKind, target: Option<&Id>, uploader: &Id) -> ResourceRef {
    let id = || target.map(Id::to_string).unwrap_or_default();
    match kind {
        AttachmentTargetKind::Organization => ResourceRef::Organization,
        AttachmentTargetKind::Project => ResourceRef::Project(id()),
        AttachmentTargetKind::Task => ResourceRef::Task(id()),
        // A comment is resolved under its task; no record names one yet in this phase.
        AttachmentTargetKind::TaskComment => ResourceRef::TaskComment {
            task_id: String::new(),
            comment_id: id(),
        },
        AttachmentTargetKind::Message => ResourceRef::Message(id()),
        AttachmentTargetKind::AskAnswer => ResourceRef::AskAnswer {
            ask_message_id: id(),
            answerer_id: uploader.to_string(),
        },
        AttachmentTargetKind::ReviewFinding => ResourceRef::ReviewFinding(id()),
        AttachmentTargetKind::ReviewFindingReply => ResourceRef::ReviewFindingReply(id()),
        AttachmentTargetKind::UserPrivate => ResourceRef::UserPrivate {
            owner_employee_id: id(),
        },
    }
}

/// The policy's resource for a bound record.
fn record_ref(record: &AttachmentRecord) -> ResourceRef {
    ResourceRef::Attachment {
        id: record.id.to_string(),
        target: Box::new(target_ref(
            record.target_kind,
            record.target_id.as_ref(),
            &record.uploaded_by_employee_id,
        )),
        uploaded_by: record.uploaded_by_employee_id.to_string(),
    }
}

impl Office {
    fn attachment_now_ms(&self) -> u64 {
        self.monotonic_now_ms()
    }

    /// A new canonical UUIDv7 for an upload or an attachment. Not a capability: every use is
    /// authorized by the policy, so the ID only has to be unique.
    fn mint_attachment_id(&self) -> String {
        let now = self.attachment_now_ms();
        let seq = ID_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let entropy = {
            use std::hash::{BuildHasher, Hasher};
            let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
            hasher.write_u64(seq);
            hasher.write_u64(now);
            hasher.finish()
        };
        let seed = format!("{}:{now}:{seq}:{entropy}", self.info.office_id);
        let hash = Sha256::of(seed.as_bytes());
        let mut bytes = [0u8; 16];
        bytes[0..6].copy_from_slice(&now.to_be_bytes()[2..8]);
        bytes[6..16].copy_from_slice(&hash.as_bytes()[0..10]);
        bytes[6] = 0x70 | (bytes[6] & 0x0f);
        bytes[8] = 0x80 | (bytes[8] & 0x3f);
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        format!(
            "{}-{}-{}-{}-{}",
            &hex[0..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..32]
        )
    }

    fn is_satellite(&self) -> bool {
        self.info.role == OfficeRole::Satellite
    }

    /// The caller as the policy sees them, or the refusal for someone who is not a member.
    fn attachment_caller(&self, actor: &str) -> Answer<PrincipalContext> {
        let read = PrincipalRead {
            employee_id: actor.to_owned(),
            owner_id: self.effective_owner_for_ops().to_string(),
        };
        Ok(match self.store.read(read).map_err(storage)?.0 {
            Some(caller) if caller.active => Ok(caller),
            Some(_) => Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "inactive or paused employees cannot use attachments",
            )),
            None => Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "caller is not a member of this organization",
            )),
        })
    }

    /// Runs a policy decision: `Ok(Ok(()))` allowed, `Ok(Err(..))` refused, `Err` storage.
    fn decide(
        &self,
        check: impl FnOnce(&AuthorizationPolicy<'_, OfficeFacts<'_>>) -> Result<(), Denied>,
    ) -> Answer<()> {
        let facts = OfficeFacts(self);
        let policy = AuthorizationPolicy::new(&facts);
        match check(&policy) {
            Ok(()) => Ok(Ok(())),
            Err(denied) => denied.into_api().map(Err),
        }
    }

    /// A bound attachment by ID, shared or private, whoever may see it.
    fn find_attachment(
        &self,
        attachment_id: &str,
    ) -> Result<Option<AttachmentRecord>, OfficeError> {
        if !org_proto::is_uuid_v7(attachment_id) {
            return Ok(None);
        }
        let Ok(id) = Id::parse(attachment_id) else {
            return Ok(None);
        };
        if let Some(record) = self.store.read(AttachmentById(id)).map_err(storage)?.0 {
            return Ok(Some(record));
        }
        if let Some(record) = self.message_file_record(attachment_id)? {
            return Ok(Some(record));
        }
        self.attachments
            .user_space
            .get(&self.info.org_id, attachment_id)
    }

    /// Removes every expired staged upload now; returns how many went.
    pub fn attachment_gc_staged(&self) -> Result<usize, OfficeError> {
        self.attachments.staging.gc(self.attachment_now_ms())
    }

    /// [`Self::attachment_gc_staged`] as if the clock read `now_ms` (tests).
    #[cfg(any(test, feature = "testkit"))]
    pub fn attachment_gc_staged_at(&self, now_ms: u64) -> Result<usize, OfficeError> {
        self.attachments.staging.gc(now_ms)
    }

    /// How many shared and private blobs this office holds (tests and diagnostics).
    #[cfg(any(test, feature = "testkit"))]
    pub fn attachment_blob_counts(&self) -> Result<(usize, usize), OfficeError> {
        let private = match &self.attachments.private {
            Some(store) => store.count()?,
            None => 0,
        };
        Ok((self.attachments.shared.count()?, private))
    }

    /// The staging folder (tests and diagnostics).
    #[cfg(any(test, feature = "testkit"))]
    pub fn attachment_staging_root(&self) -> std::path::PathBuf {
        self.attachments.staging.root().to_path_buf()
    }

    /// The satellite's outbox folder (tests and diagnostics).
    #[cfg(any(test, feature = "testkit"))]
    pub fn attachment_outbox_root(&self) -> std::path::PathBuf {
        self.outbox().root().to_path_buf()
    }

    /// The satellite's blob cache (tests and diagnostics).
    #[cfg(any(test, feature = "testkit"))]
    pub fn attachment_blob_cache(&self) -> crate::attachments::cache::BlobCache {
        self.blob_cache()
    }
}

#[cfg(test)]
mod tests;
