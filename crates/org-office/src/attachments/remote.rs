//! Cross-office blob transfer (`TASK-98.6`, ADR-0037): the values the federation adapter
//! moves between a satellite's outbox and HQ, and the port a satellite pulls shared blobs from
//! HQ through. The office decides who may read a file before it ever asks; the adapter (the
//! federation client in `org-office-server`) asks HQ over Organization-CA mTLS for one shared
//! attachment by `(org_id, attachment_id)` and HQ checks the satellite and that the file is
//! shared. The office verifies the bytes against the record's SHA-256 before it caches or
//! serves them.

use org_proto::{ApiError, ErrorCode};

use super::blobs::ContentHash;

/// How long a caller is told to wait before retrying a read whose bytes are at HQ.
pub const PULL_RETRY_AFTER_MS: u64 = 5_000;

/// Why a pull from HQ delivered nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PullFailure {
    /// No session with HQ, or it broke mid-transfer.
    Unreachable(String),
    /// HQ answered and refused, e.g. it does not hold the attachment (yet).
    Refused { code: String, message: String },
    /// HQ's bytes did not reassemble or verify.
    Corrupt(String),
}

/// Fetches one shared attachment's bytes from HQ.
pub trait BlobRemote: Send + Sync {
    /// The bytes of shared attachment `attachment_id`, which should be `byte_count` bytes
    /// hashing to `content_sha256` (the caller checks again).
    fn fetch(
        &self,
        attachment_id: &str,
        content_sha256: &ContentHash,
        byte_count: u64,
    ) -> Result<Vec<u8>, PullFailure>;
}

/// What a read may pull through when the bytes are not on this office.
#[derive(Clone, Copy)]
pub struct BlobPull<'a> {
    pub remote: &'a dyn BlobRemote,
    /// The satellite cache's cap ([`super::cache::DEFAULT_BLOB_CACHE_MAX_BYTES`] by default).
    pub cache_max_bytes: u64,
}

/// The answer to a read whose bytes are not on this office right now: `unavailable`, worth
/// retrying (`retry_after_ms`; HTTP 503 at the web edge).
pub fn bytes_unavailable(message: impl Into<String>) -> ApiError {
    let mut error = ApiError::new(ErrorCode::Unavailable, message);
    error.retry_after_ms = Some(PULL_RETRY_AFTER_MS);
    error
}

/// One outbox entry as a satellite ships it (`Office::attachment_outbox_pending`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OutboxShipment {
    pub attachment_id: String,
    pub content_sha256: String,
    pub byte_count: u64,
    /// The attachment's canonical YAML record, as HQ is sent it.
    pub record_yaml: Vec<u8>,
    /// The per-type size limit of the file (10 MiB images, 25 MiB documents).
    pub max_bytes: u64,
}

/// A shared attachment a satellite announced, as HQ checked it before any byte arrives
/// (`Office::attachment_ingest_check`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ShippedAttachment {
    pub(crate) record: super::records::AttachmentRecord,
    pub(crate) media: super::media::MediaType,
    /// The satellite that announced it: its uploader is decided again for it on commit.
    pub(crate) origin_office: String,
    /// Whether the satellite may skip sending the bytes: only for a retry of a file HQ has
    /// already bound and holds. A new attachment always sends them, so HQ checks every byte.
    pub present: bool,
}

impl ShippedAttachment {
    pub fn attachment_id(&self) -> String {
        self.record.id.to_string()
    }

    pub fn content_sha256(&self) -> String {
        self.record.content_sha256.to_string()
    }

    pub fn byte_count(&self) -> u64 {
        self.record.byte_count
    }

    /// The per-type size limit the bytes must keep to.
    pub fn max_bytes(&self) -> u64 {
        self.media.max_bytes()
    }
}

/// A shared attachment HQ serves a satellite's fetch from (`Office::attachment_peer_fetch`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PeerBlob {
    pub attachment_id: String,
    pub content_sha256: ContentHash,
    pub byte_count: u64,
}
