//! Store `Refusal` <-> `ApiError` encoding helpers shared by review validation and mutations.

use std::fmt;

use org_proto::{ApiError, ErrorCode};
use org_store::{Id, Refusal};

pub(crate) fn api_refusal(code: ErrorCode, message: impl Into<String>) -> Refusal {
    let payload = ApiError::new(code, message);
    Refusal::rejected(serde_json::to_vec(&payload).expect("ApiError serializes"))
}

pub(crate) fn decode_api_refusal(refusal: &Refusal, fallback: &str) -> ApiError {
    match refusal {
        Refusal::Rejected(bytes) => serde_json::from_slice::<ApiError>(bytes)
            .unwrap_or_else(|_| ApiError::new(ErrorCode::InvalidRequest, fallback)),
        Refusal::Invalid(err) => ApiError::new(ErrorCode::InvalidRequest, err.to_string()),
    }
}

pub(crate) fn storage_api_refusal(context: &str, err: impl fmt::Display) -> Refusal {
    api_refusal(ErrorCode::StorageError, format!("{context}: {err}"))
}

pub(crate) fn parse_uuid_v7_refusal(raw: &str, field: &str) -> Result<Id, Refusal> {
    if !org_proto::is_uuid_v7(raw) {
        return Err(api_refusal(
            ErrorCode::InvalidRequest,
            format!("{field} must be a canonical lowercase UUIDv7"),
        ));
    }
    Id::parse(raw).map_err(|_| api_refusal(ErrorCode::InvalidRequest, format!("invalid {field}")))
}
