//! Shared core/transport values. No framing, TLS, storage or I/O.
//! Values validate at construction; transport decoding must use these constructors.
#![deny(unsafe_code)]

mod authority;
mod digest;
mod values;
mod work_digest;

pub use authority::authority_record_preimage_v1;
pub use digest::event_digest_v1;
pub use values::{
    Codec, Digest, Epoch, InvalidValue, OfficeId, OrgId, OriginIncarnation, Position, SourceSeq,
};

/// X-05 maximum encoded federation frame, before allocating its body.
pub const MAX_ENCODED_FRAME_BYTES: usize = 1024 * 1024;

pub use work_digest::{work_record_digest_v1, work_source_digest_v1};
