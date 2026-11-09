//! The office's plain values. No storage or wire types: the engine and the contract both
//! map to and from these.

/// What an office is in the organization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfficeRole {
    /// The first office, which holds the organization's authoritative data.
    Hq,
}

/// What the host tells a new office server to create (never on a command line).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewOffice {
    /// A plain id (ASCII letters and digits, 1 to 64), made by the host. The office folder and
    /// host's key for it are named by it.
    pub office_id: String,
    pub office_name: String,
    pub role: OfficeRole,
    pub org_id: String,
    pub org_name: String,
}

/// An office as `office.json` records it. No secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfficeInfo {
    pub office_id: String,
    pub office_name: String,
    pub role: OfficeRole,
    pub org_id: String,
    pub org_name: String,
    /// When the organization was created: RFC 3339 UTC with milliseconds.
    pub created_at: String,
}
