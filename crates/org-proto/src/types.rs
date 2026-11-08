use serde::{Deserialize, Serialize};

/// The longest organization name, in Unicode scalar values, after trimming.
///
/// The server enforces the name rules and is the source of truth. Clients may use this
/// constant to pre-check input, but must still handle [`crate::ErrorCode::InvalidName`].
/// Rules: trimmed, 1..=64 chars, no control characters; unique case-insensitively among
/// the organizations on this machine.
///
/// "Trimmed" means leading and trailing characters with the Unicode `White_Space` property
/// are removed, exactly as Rust's `str::trim` does. JavaScript's `String.prototype.trim`
/// uses a different set (it strips U+FEFF and keeps U+0085), so a TypeScript pre-check must
/// trim the `White_Space` set itself.
pub const ORGANIZATION_NAME_MAX_CHARS: usize = 64;

/// An office: a node of one organization, run by its own `org-office-server` process.
/// The organization's first office is its HQ (see `docs/design/offices-hq-and-satellites.md`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeInfo {
    /// Opaque, made when the office is created (a ULID today; don't parse it).
    pub id: String,
    /// The organization this office belongs to.
    pub organization_id: String,
    pub name: String,
    /// Missing means unknown, never assumed HQ.
    #[serde(default)]
    pub role: OfficeRole,
    /// Where the office runs, for display, e.g. "This Mac" or a machine name.
    pub host: String,
    /// How to reach it from this machine: a local socket path, when it's on this machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// Missing means unknown, never assumed healthy.
    #[serde(default)]
    pub status: OfficeStatus,
}

/// An office's role in its organization. Unknown values decode as [`OfficeRole::Unknown`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum OfficeRole {
    /// The organization's first, authoritative office.
    Hq,
    /// Any other office; holds a copy and syncs through HQ.
    Satellite,
    /// Missing or not known to this build.
    #[default]
    #[serde(other)]
    Unknown,
}

/// Whether an office is running and reachable. Unknown values decode as
/// [`OfficeStatus::Unknown`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum OfficeStatus {
    Active,
    /// Its server is starting.
    Starting,
    /// Invited but not joined yet.
    Pending,
    /// Not reachable right now.
    Offline,
    /// Can't open its data (e.g. its key is missing).
    Unavailable,
    /// Runs a version too old for its organization.
    UpdateNeeded,
    /// Missing or not known to this build.
    #[default]
    #[serde(other)]
    Unknown,
}

/// An organization, with its offices known to the answering server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Organization {
    /// Opaque, made when the organization is created (a ULID today; don't parse it).
    pub id: String,
    pub name: String,
    /// RFC 3339 UTC with milliseconds, e.g. `2026-11-08T02:10:00.000Z`.
    pub created_at: String,
    /// Its offices. From this machine's host: its offices on this machine (HQ first).
    #[serde(default)]
    pub offices: Vec<OfficeInfo>,
}
