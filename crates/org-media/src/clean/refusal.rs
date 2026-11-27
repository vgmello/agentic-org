//! The two refusals of a clean check: an image that carries something besides its pixels, and
//! an image that is damaged.

use org_proto::ApiError;

use super::super::{corrupt, MediaType};

/// The refusal for an image that is not as an office keeps it.
pub(super) fn not_clean(what: &str) -> ApiError {
    corrupt(format!(
        "the image carries {what}: only an image as an office keeps it (pixels alone) is accepted"
    ))
}

pub(super) fn damaged(media: MediaType) -> ApiError {
    corrupt(format!("the `{}` image is damaged", media.mime()))
}
