//! Chunked attachment blob transfer between offices (`TASK-98.6`, ADR-0037): the bounds of a
//! transfer, the splitter that turns a blob into [`SyncFrame::BlobChunk`]s, and the
//! [`BlobAssembler`] that reassembles them in order and verifies the SHA-256 before anyone
//! may keep the bytes. Pure: no storage and no I/O.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use sha2::{Digest as _, Sha256};

use crate::frame::SyncFrame;

/// The most raw bytes one [`SyncFrame::BlobChunk`] carries. Base64 and the frame's other
/// fields keep the encoded frame well inside [`crate::MAX_ENCODED_FRAME_BYTES`].
pub const BLOB_CHUNK_MAX_BYTES: usize = 512 * 1024;

/// The largest blob any transfer carries: the allowlisted document limit (images are at most
/// 10 MiB; the receiver applies the per-type limit as well).
pub const BLOB_TRANSFER_MAX_BYTES: u64 = 25 * 1024 * 1024;

/// Why a chunked transfer was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlobTransferError {
    /// The digest is not 64 lowercase hex digits.
    InvalidDigest,
    /// The declared size is zero, or over the receiver's bound.
    InvalidSize,
    /// A chunk names another blob or another size than the transfer began with.
    Mismatch,
    /// A chunk does not start where the bytes received so far end.
    OutOfOrder,
    /// A chunk is empty, over [`BLOB_CHUNK_MAX_BYTES`], or runs past the declared size.
    BadChunk,
    /// A chunk's data is not standard base64.
    Base64,
    /// All the bytes arrived and their SHA-256 is not the declared one.
    DigestMismatch,
}

impl std::fmt::Display for BlobTransferError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidDigest => "the blob digest is not 64 lowercase hex digits",
            Self::InvalidSize => "the blob size is zero or over the transfer bound",
            Self::Mismatch => "a chunk names another blob or size",
            Self::OutOfOrder => "a chunk is out of order",
            Self::BadChunk => "a chunk is empty, oversized, or past the end",
            Self::Base64 => "a chunk is not standard base64",
            Self::DigestMismatch => "the reassembled blob does not match its SHA-256",
        })
    }
}

impl std::error::Error for BlobTransferError {}

/// Whether `text` is a lowercase hex SHA-256.
pub fn is_blob_digest(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The lowercase hex SHA-256 of `bytes`.
pub fn blob_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `bytes` as in-order [`SyncFrame::BlobChunk`]s of at most [`BLOB_CHUNK_MAX_BYTES`].
pub fn blob_chunk_frames<'a>(
    request_id: &'a str,
    content_sha256: &'a str,
    bytes: &'a [u8],
) -> impl Iterator<Item = SyncFrame> + 'a {
    let total_bytes = bytes.len() as u64;
    bytes
        .chunks(BLOB_CHUNK_MAX_BYTES)
        .enumerate()
        .map(move |(index, piece)| SyncFrame::BlobChunk {
            request_id: request_id.to_owned(),
            content_sha256: content_sha256.to_owned(),
            offset: (index * BLOB_CHUNK_MAX_BYTES) as u64,
            total_bytes,
            data_b64: STANDARD.encode(piece),
        })
}

/// Reassembles one blob from its chunks, strictly in order and within its declared size, and
/// verifies its SHA-256 once the last byte arrives.
pub struct BlobAssembler {
    content_sha256: String,
    total_bytes: u64,
    bytes: Vec<u8>,
    hasher: Sha256,
    verified: bool,
}

impl std::fmt::Debug for BlobAssembler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlobAssembler")
            .field("content_sha256", &self.content_sha256)
            .field("total_bytes", &self.total_bytes)
            .field("received", &self.bytes.len())
            .finish()
    }
}

impl BlobAssembler {
    /// A transfer of `total_bytes` (1 to `max_bytes`, never over
    /// [`BLOB_TRANSFER_MAX_BYTES`]) whose SHA-256 must be `content_sha256`.
    pub fn new(
        content_sha256: &str,
        total_bytes: u64,
        max_bytes: u64,
    ) -> Result<Self, BlobTransferError> {
        if !is_blob_digest(content_sha256) {
            return Err(BlobTransferError::InvalidDigest);
        }
        if total_bytes == 0 || total_bytes > max_bytes.min(BLOB_TRANSFER_MAX_BYTES) {
            return Err(BlobTransferError::InvalidSize);
        }
        Ok(Self {
            content_sha256: content_sha256.to_owned(),
            total_bytes,
            bytes: Vec::new(),
            hasher: Sha256::new(),
            verified: false,
        })
    }

    /// How many bytes have arrived.
    pub fn received(&self) -> u64 {
        self.bytes.len() as u64
    }

    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    pub fn content_sha256(&self) -> &str {
        &self.content_sha256
    }

    /// Takes the next chunk. `Ok(true)` once every byte arrived and the digest matched.
    pub fn push(
        &mut self,
        content_sha256: &str,
        offset: u64,
        total_bytes: u64,
        data_b64: &str,
    ) -> Result<bool, BlobTransferError> {
        // A finished transfer takes no more chunks: anything after the last byte is out of order.
        if self.verified {
            return Err(BlobTransferError::OutOfOrder);
        }
        if content_sha256 != self.content_sha256 || total_bytes != self.total_bytes {
            return Err(BlobTransferError::Mismatch);
        }
        if offset != self.received() {
            return Err(BlobTransferError::OutOfOrder);
        }
        // Bound the encoded text before decoding it: 4 characters per 3 bytes, padded.
        if data_b64.is_empty() || data_b64.len() > BLOB_CHUNK_MAX_BYTES.div_ceil(3) * 4 {
            return Err(BlobTransferError::BadChunk);
        }
        let data = STANDARD
            .decode(data_b64.as_bytes())
            .map_err(|_| BlobTransferError::Base64)?;
        let end = offset.saturating_add(data.len() as u64);
        if data.is_empty() || data.len() > BLOB_CHUNK_MAX_BYTES || end > self.total_bytes {
            return Err(BlobTransferError::BadChunk);
        }
        self.hasher.update(&data);
        self.bytes.extend_from_slice(&data);
        if end < self.total_bytes {
            return Ok(false);
        }
        let digest: String = self
            .hasher
            .clone()
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if digest != self.content_sha256 {
            return Err(BlobTransferError::DigestMismatch);
        }
        self.verified = true;
        Ok(true)
    }

    /// The reassembled bytes once every one arrived and their SHA-256 matched; `None` before.
    pub fn into_verified_bytes(self) -> Option<Vec<u8>> {
        self.verified.then_some(self.bytes)
    }
}

#[cfg(test)]
mod tests;
