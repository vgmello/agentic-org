//! Text attachments (`text/markdown`, `text/plain`, `text/csv`, `application/json`): valid
//! UTF-8 with no NUL byte, and for JSON one syntactically valid document. The bytes are kept
//! as uploaded.

use org_proto::ApiError;

use super::{corrupt, MediaType};

/// Refuses text that is not UTF-8, holds a NUL byte, or (for JSON) does not parse.
pub fn check(media: MediaType, bytes: &[u8]) -> Result<(), ApiError> {
    if bytes.contains(&0) {
        return Err(corrupt("a text attachment cannot contain NUL bytes"));
    }
    let text =
        std::str::from_utf8(bytes).map_err(|_| corrupt("a text attachment must be valid UTF-8"))?;
    if media == MediaType::Json {
        // Skipping a value (`IgnoredAny`) walks it without recursion and builds nothing, so
        // neither a hostile depth nor a hostile size can exhaust the stack or the heap.
        let body = text.trim_start_matches('\u{feff}');
        serde_json::from_str::<serde::de::IgnoredAny>(body)
            .map_err(|error| corrupt(format!("invalid JSON at line {}", error.line())))?;
    }
    Ok(())
}
