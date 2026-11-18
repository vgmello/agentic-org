//! Turn the launch request into the office workflow's input at the process boundary.
use crate::{serve::LaunchAuth, startup::Request};
use org_office::{Key, NewOffice};
use org_office_launch::{Handoff, HostFirstLine, LaunchError};
use org_proto::{OfficeRole, RecoveryCode};

pub(super) struct Create {
    pub office: NewOffice,
    pub role: OfficeRole,
    pub key: Key,
    pub auth: LaunchAuth,
    pub instance: String,
    pub owner_member_id: String,
    /// Consumed by HQ creation-v1 during preparation, before serving.
    pub recovery: RecoveryCode,
    /// A create without it is refused: HQ creation-v1 is the only create (G2).
    pub hq_creation: Option<org_office_launch::HqCreationV1>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct OfficeIdentity {
    pub office_id: String,
    pub instance_id: String,
}

pub(super) enum Launch {
    Create(Create),
    Start {
        key: Key,
        auth: LaunchAuth,
        office: OfficeIdentity,
    },
    /// Discard needs no key: the databases are plain SQLite (ADR-0016). A key the handoff
    /// carries is ignored.
    Discard {
        office: OfficeIdentity,
    },
}

impl Launch {
    pub fn from_request(request: Request) -> Result<Self, LaunchError> {
        Ok(match request {
            Request::Launch {
                line: HostFirstLine::Create(c),
                handoff:
                    Handoff::Create {
                        db_key,
                        grant_key,
                        recovery_code,
                        ..
                    },
            } => Self::Create(Create {
                office: NewOffice {
                    office_id: c.office.office_id,
                    office_name: c.office_name,
                    role: org_office::OfficeRole::Hq,
                    org_id: c.org_id,
                    org_name: c.org_name,
                },
                role: c.role,
                key: Key::from_db_key(db_key),
                auth: grant_key.into(),
                instance: c.office.instance_id,
                owner_member_id: c.owner_member_id,
                hq_creation: c.hq_creation,
                recovery: recovery_code,
            }),
            Request::Launch {
                line: HostFirstLine::Start(s),
                handoff:
                    Handoff::Start {
                        db_key, grant_key, ..
                    },
            } => Self::Start {
                key: Key::from_db_key(db_key),
                auth: grant_key.into(),
                office: OfficeIdentity {
                    office_id: s.office_id,
                    instance_id: s.instance_id,
                },
            },
            Request::Launch {
                line: HostFirstLine::Discard(d),
                handoff: Handoff::Discard { .. },
            } => Self::Discard {
                office: OfficeIdentity {
                    office_id: d.office_id,
                    instance_id: d.instance_id,
                },
            },
            _ => return Err(LaunchError::Invalid("unknown launch request")),
        })
    }
}

#[cfg(test)]
mod tests;
