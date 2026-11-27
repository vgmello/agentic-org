//! Where a bound file's bytes are on this office (`TASK-98.6`, ADR-0037). HQ reads its shared
//! store (`<durable>/blobs/`) or, for a private file, its private store. A satellite reads a
//! shared file from its outbox (its own files HQ has not acked yet), then its LRU cache, then
//! its shared store; it holds no private bytes at all. Only after the policy allowed a read may
//! a satellite pull a shared file's bytes from HQ ([`Office::pull_shared_blob`]).

use org_proto::attachments::AttachmentScope;

use super::{Answer, Office};
use crate::attachments::blobs::ContentHash;
use crate::attachments::cache::BlobCache;
use crate::attachments::outbox::BlobOutbox;
use crate::attachments::records::AttachmentRecord;
use crate::attachments::remote::{bytes_unavailable, BlobPull, PullFailure};
use crate::error::OfficeError;

/// How a local read takes bytes from one store.
enum Want {
    Range { offset: u64, max: usize },
    All { limit: u64 },
}

impl Office {
    /// This satellite's outbox (`durable/local/offices/<office-id>/blob-outbox/`).
    pub(super) fn outbox(&self) -> BlobOutbox {
        BlobOutbox::new(&self.durable, &self.info.office_id)
    }

    /// This satellite's cache (`<runtime>/<office-id>/protected/blob-cache/`).
    pub(super) fn blob_cache(&self) -> BlobCache {
        BlobCache::new(&self.runtime_root, &self.info.office_id)
    }

    fn local_blob(
        &self,
        record: &AttachmentRecord,
        want: &Want,
    ) -> Result<Option<Vec<u8>>, OfficeError> {
        let hash = &record.content_sha256;
        if record.scope() == AttachmentScope::UserPrivate {
            let Some(private) = &self.attachments.private else {
                return Ok(None);
            };
            return match *want {
                Want::Range { offset, max } => private.read_range(hash, offset, max),
                Want::All { limit } => private.read_all(hash, limit),
            };
        }
        if self.is_satellite() {
            let outbox = self.outbox();
            let found = match *want {
                Want::Range { offset, max } => outbox.blobs().read_range(hash, offset, max)?,
                Want::All { limit } => outbox.blobs().read_all(hash, limit)?,
            };
            if found.is_some() {
                return Ok(found);
            }
            let cache = self.blob_cache();
            let found = match *want {
                Want::Range { offset, max } => cache.read_range(hash, offset, max)?,
                Want::All { limit } => cache.read_all(hash, limit)?,
            };
            if found.is_some() {
                return Ok(found);
            }
        }
        match *want {
            Want::Range { offset, max } => self.attachments.shared.read_range(hash, offset, max),
            Want::All { limit } => self.attachments.shared.read_all(hash, limit),
        }
    }

    /// Up to `max` bytes of `record`'s file from `offset`, if this office holds them.
    pub(super) fn local_blob_range(
        &self,
        record: &AttachmentRecord,
        offset: u64,
        max: usize,
    ) -> Result<Option<Vec<u8>>, OfficeError> {
        self.local_blob(record, &Want::Range { offset, max })
    }

    /// The whole of `record`'s file when it is at most `limit` bytes, if this office holds it.
    pub(super) fn local_blob_all(
        &self,
        record: &AttachmentRecord,
        limit: u64,
    ) -> Result<Option<Vec<u8>>, OfficeError> {
        self.local_blob(record, &Want::All { limit })
    }

    /// Whether this office could ever pull `record`'s bytes from HQ: a satellite, for a shared
    /// file. A private file is never asked of HQ by a satellite.
    pub(super) fn may_pull(&self, record: &AttachmentRecord) -> bool {
        self.is_satellite() && record.scope() == AttachmentScope::Shared
    }

    /// Pulls a shared file's bytes from HQ, checks them against the record, and keeps them in
    /// the cache. The caller has passed the policy for `record`. Anything that keeps the bytes
    /// away is `unavailable` and worth retrying.
    pub(super) fn pull_shared_blob(
        &self,
        record: &AttachmentRecord,
        pull: BlobPull<'_>,
    ) -> Answer<Vec<u8>> {
        if !self.may_pull(record) {
            return Ok(Err(bytes_unavailable(
                "the attachment's bytes are not on this office",
            )));
        }
        let fetched = pull.remote.fetch(
            record.id.as_str(),
            &record.content_sha256,
            record.byte_count,
        );
        let bytes = match fetched {
            Ok(bytes) => bytes,
            Err(PullFailure::Unreachable(_)) => {
                return Ok(Err(bytes_unavailable(
                    "HQ is unreachable and this file's bytes are not on this office yet; try again later",
                )))
            }
            Err(PullFailure::Refused { .. }) => {
                return Ok(Err(bytes_unavailable(
                    "HQ does not hold this file's bytes yet; try again later",
                )))
            }
            Err(PullFailure::Corrupt(_)) => {
                return Ok(Err(bytes_unavailable(
                    "this file's bytes did not arrive intact from HQ; try again",
                )))
            }
        };
        if bytes.len() as u64 != record.byte_count
            || ContentHash::of(&bytes) != record.content_sha256
        {
            return Ok(Err(bytes_unavailable(
                "this file's bytes did not arrive intact from HQ; try again",
            )));
        }
        self.blob_cache()
            .insert(&record.content_sha256, &bytes, pull.cache_max_bytes)?;
        Ok(Ok(bytes))
    }
}
