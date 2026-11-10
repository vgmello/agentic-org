use serde::{Deserialize, Serialize};

/// The longest organization name, in Unicode scalar values, after trimming.
///
/// The server enforces the name rules and is the source of truth. Clients may use this
/// constant to pre-check input, but must still handle [`crate::ErrorCode::InvalidName`].
/// Rules: trimmed, 1..=64 chars, no control characters, no format characters (Cf) or
/// default-ignorable code points, plus U+2028/U+2029, except U+200C, U+200D, U+FE0E and U+FE0F;
/// unique case-insensitively among the organizations on this machine.
///
/// "Trimmed" means leading and trailing characters with the Unicode `White_Space` property
/// are removed, exactly as Rust's `str::trim` does. JavaScript's `String.prototype.trim`
/// uses a different set (it strips U+FEFF and keeps U+0085), so a TypeScript pre-check must
/// trim the `White_Space` set itself.
///
/// Servers check the rule with [`crate::OrganizationName::parse`], and compare names for
/// uniqueness by [`crate::OrganizationName::key`].
pub const ORGANIZATION_NAME_MAX_CHARS: usize = 64;

/// An office: a node of one organization, run by its own `org-server` process.
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
    /// Why the office is `offline` or `unavailable`, when the host knows. Present only with
    /// those two statuses; absent with any other, and absent from a server that predates it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_reason: Option<OfficeStatusReason>,
    /// When the host will next try to start the office: RFC 3339 UTC with milliseconds, e.g.
    /// `2026-11-08T02:10:01.000Z`. Present only while the office is `offline` with a restart
    /// pending; absent otherwise (including when `unavailable`, where no restart is pending).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_restart_at: Option<String>,
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
    /// Not reachable right now. Its `status_reason` says why, and its `next_restart_at` when
    /// the host will try again (absent if no restart is pending).
    Offline,
    /// The host won't start it again on its own: it can't open its data (e.g. its key is
    /// missing), or it kept failing. Its `status_reason` says why.
    Unavailable,
    /// Runs a version too old for its organization.
    UpdateNeeded,
    /// Missing or not known to this build.
    #[default]
    #[serde(other)]
    Unknown,
}

/// Why an office is `offline` or `unavailable` ([`OfficeInfo::status_reason`]). The host sets
/// it from how the office server's start ended (the launch `failed` codes) or from how it
/// supervises it. Unknown values decode as [`OfficeStatusReason::Unknown`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum OfficeStatusReason {
    /// The office's data exists but its encryption key is gone (e.g. removed from the
    /// Keychain), so it can't be opened. Fatal until restored: the host doesn't retry it on
    /// its own, but `office.restart` retries it (e.g. after the owner restores the key). If
    /// the key is still missing, the office goes back to `unavailable` with this reason, with
    /// no backoff retries.
    KeyMissing,
    /// Another process still held the office's lock when its server started (e.g. an old
    /// server that hadn't exited yet). The host retries.
    InUse,
    /// The office server couldn't read or write its storage (disk, Keychain or database).
    /// The host retries.
    StorageError,
    /// The host started the office server in a way it didn't understand: a bug. Fatal:
    /// `office.restart` refuses it.
    InvalidRequest,
    /// The office server doesn't speak the host's version of the start protocol (they come
    /// from different builds). Fatal: `office.restart` refuses it.
    UnsupportedVersion,
    /// The office server stopped unexpectedly. The host retries.
    Crashed,
    /// The office server didn't become ready in time, so the host stopped it. The host
    /// retries.
    StartTimeout,
    /// The office kept failing, so the host stopped retrying (status `unavailable`).
    /// `office.restart` tries again, with the failure count back at 0.
    RestartsExhausted,
    /// The owner stopped the office on purpose (status `offline`, no `next_restart_at`). No
    /// call stops an office yet; one may come later. `office.restart` starts it again.
    StoppedByOwner,
    /// Not known to this build (sent by a newer host). A missing reason reads as `None`.
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
