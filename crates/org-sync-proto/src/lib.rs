//! Shared core/transport values. No framing, TLS, storage or I/O.
//! Values validate at construction; transport decoding must use these constructors.
#![deny(unsafe_code)]

mod digest;
mod values;

pub use digest::event_digest_v1;
pub use values::{Codec, Digest, Epoch, InvalidValue, OfficeId, OrgId, Position};
