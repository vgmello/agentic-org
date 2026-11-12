//! Normalize launch versions at the process boundary; office workflows use one input shape.
use crate::{serve::LaunchAuth, startup::Request};
use org_office::{Key, NewOffice};
use org_office_launch::{
    v3::{Handoff, HostFirstLine},
    HostMessage, LaunchError,
};
use org_proto::{OfficeRole, RecoveryCode};

pub(super) struct Create {
    pub office: NewOffice,
    pub role: OfficeRole,
    pub key: Key,
    pub auth: LaunchAuth,
    pub instance: Option<String>,
    pub owner_member_id: Option<String>,
    // Retains the recovery code through create and the blocking serve call.
    pub _recovery: CreationRecovery,
}

pub(super) enum CreationRecovery {
    Legacy,
    V3 { _code: RecoveryCode },
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
        office: Option<OfficeIdentity>,
    },
    Discard {
        key: Option<Key>,
        office: Option<OfficeIdentity>,
    },
}

impl Launch {
    pub fn from_request(request: Request) -> Result<Self, LaunchError> {
        Ok(match request {
            Request::V2(HostMessage::Create(c)) => Self::Create(Create {
                office: NewOffice {
                    office_id: c.office_id,
                    office_name: c.office_name,
                    role: org_office::OfficeRole::Hq,
                    org_id: c.org_id,
                    org_name: c.org_name,
                },
                role: c.role,
                key: Key::from_db_key(c.db_key),
                auth: c.app_token.into(),
                instance: None,
                owner_member_id: None,
                _recovery: CreationRecovery::Legacy,
            }),
            Request::V2(HostMessage::Start(s)) => Self::Start {
                key: Key::from_db_key(s.db_key),
                auth: s.app_token.into(),
                office: None,
            },
            Request::V2(HostMessage::Discard(d)) => Self::Discard {
                key: d.db_key.map(Key::from_db_key),
                office: None,
            },
            Request::V3 {
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
                auth: LaunchAuth::Grants { _key: grant_key },
                instance: Some(c.office.instance_id),
                owner_member_id: Some(c.owner_member_id),
                _recovery: CreationRecovery::V3 {
                    _code: recovery_code,
                },
            }),
            Request::V3 {
                line: HostFirstLine::Start(s),
                handoff:
                    Handoff::Start {
                        db_key, grant_key, ..
                    },
            } => Self::Start {
                key: Key::from_db_key(db_key),
                auth: LaunchAuth::Grants { _key: grant_key },
                office: Some(OfficeIdentity {
                    office_id: s.office_id,
                    instance_id: s.instance_id,
                }),
            },
            Request::V3 {
                line: HostFirstLine::Discard(d),
                handoff: Handoff::Discard { db_key, .. },
            } => Self::Discard {
                key: db_key.map(Key::from_db_key),
                office: Some(OfficeIdentity {
                    office_id: d.office_id,
                    instance_id: d.instance_id,
                }),
            },
            _ => return Err(LaunchError::Invalid("unknown launch request")),
        })
    }
}

#[cfg(test)]
mod tests;
