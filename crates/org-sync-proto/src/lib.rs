//! Shared core/transport values. No framing, TLS, storage or I/O.
//! Values validate at construction; transport decoding must use these constructors.
#![forbid(unsafe_code)]

mod authority;
mod blob;
mod digest;
pub mod frame;
mod values;
mod work_ack;
mod work_digest;

pub use authority::authority_record_preimage_v1;
pub use blob::{
    blob_chunk_frames, blob_digest, is_blob_digest, BlobAssembler, BlobTransferError,
    BLOB_CHUNK_MAX_BYTES, BLOB_TRANSFER_MAX_BYTES,
};
pub use digest::event_digest_v1;
pub use frame::{
    decode_sync_frame, encode_sync_frame, read_sync_frame, write_sync_frame, EnrollTerminalStatus,
    FrameCodecError, FrameReader, SyncFrame, WireOfficeRole, SYNC_WIRE_VERSION,
};
pub use values::{
    Codec, Digest, Epoch, InvalidValue, OfficeId, OrgId, OriginIncarnation, Position, SourceSeq,
};

/// X-05 maximum encoded federation frame, before allocating its body.
pub const MAX_ENCODED_FRAME_BYTES: usize = 1024 * 1024;

pub use work_ack::{
    work_ack_proof_digest_v1, work_rejection_code_valid, WorkAck, WorkRejection, WORK_CREDIT_WINDOW,
};
pub use work_digest::{work_record_digest_v1, work_source_digest_v1};
