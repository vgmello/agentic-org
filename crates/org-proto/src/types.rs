use serde::{Deserialize, Serialize};

/// The longest organization name, in Unicode scalar values, after trimming.
///
/// The server enforces the name rules and is the source of truth. Clients may use this
/// constant to pre-check input, but must still handle [`crate::ErrorCode::InvalidName`].
/// Rules: trimmed, 1..=64 chars, no control characters; unique case-insensitively.
pub const ORGANIZATION_NAME_MAX_CHARS: usize = 64;

/// The office a server runs, as reported by `hello`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeInfo {
    /// Opaque, server-made identifier (a ULID today; don't parse it).
    pub id: String,
    pub name: String,
    /// Where the office runs, for display, e.g. "This Mac".
    pub host: String,
}

/// An organization hosted by an office server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Organization {
    /// Opaque, server-made identifier (a ULID today; don't parse it).
    pub id: String,
    pub name: String,
    /// RFC 3339 UTC with milliseconds, e.g. `2026-11-08T02:10:00.000Z`.
    pub created_at: String,
}
