//! The office's plain values. No storage or wire types: the engine and the contract both
//! map to and from these.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Organization {
    /// Opaque ULID made by the office. Directory names use it, never the display name.
    pub id: String,
    pub name: String,
    /// RFC 3339 UTC with milliseconds.
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfficeInfo {
    pub id: String,
    pub name: String,
}
