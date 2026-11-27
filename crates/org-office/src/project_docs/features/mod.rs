//! Operations of the documentation tree: the reads (`docs.index`, `docs.list`,
//! `docs.inspect`) and the decisions of the writes (`docs.create`, `docs.update`, `docs.move`,
//! `docs.put_media`). Each is a pure function of one view: the store layer
//! (`store/open/project_doc_ops.rs`) runs the reads on a snapshot and submits the decisions.

pub mod change;
pub mod create;
pub mod fields;
pub mod put_media;
pub mod read;
pub mod update;
pub mod wire;

use org_proto::{ApiError, ErrorCode};
use org_store::{Id, Refusal, Sha256};
use serde::Serialize;

use super::access::Denied;

/// A write's reply, as the receipt keeps it for a retry.
pub(crate) fn reply<T: Serialize>(answer: &T) -> Vec<u8> {
    serde_json::to_vec(answer).expect("a documentation reply encodes")
}

/// What the writer is told when a decision stops: a refusal is recorded (a retry gets the same
/// answer), a storage failure is not.
pub fn refusal(denied: Denied) -> Refusal {
    match denied {
        Denied::Refused(error) => Refusal::rejected(reply(&error)),
        Denied::Storage(error) => Refusal::from(error),
    }
}

/// The refusal a rejected write's reply holds.
pub fn decode_rejection(bytes: &[u8]) -> ApiError {
    serde_json::from_slice(bytes)
        .unwrap_or_else(|_| ApiError::new(ErrorCode::InvalidRequest, "documentation write refused"))
}

/// The ID of a record a write creates: a UUIDv7 whose time is the writer's clock and whose
/// random bits come from `(namespace, command_id)`, so every run of the same decision mints the
/// same ID.
pub fn derive_id(namespace: &str, command_id: &str, now_ms: u64) -> Id {
    let digest = Sha256::of_parts(&[namespace.as_bytes(), b"\0", command_id.as_bytes()]);
    let hash = digest.as_bytes();
    let mut b = [0u8; 16];
    b[0..6].copy_from_slice(&now_ms.to_be_bytes()[2..8]);
    b[6] = (hash[6] & 0x0f) | 0x70;
    b[7] = hash[7];
    b[8] = (hash[8] & 0x3f) | 0x80;
    b[9..16].copy_from_slice(&hash[9..16]);
    let hex: String = b.iter().map(|byte| format!("{byte:02x}")).collect();
    let text = format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    );
    Id::parse(&text).expect("a derived UUIDv7 parses")
}
