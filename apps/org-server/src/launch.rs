//! Normalize launch versions at the process boundary; office workflows use one input shape.
use crate::{serve::LaunchAuth, startup::Request};
use org_office::{Key, NewOffice};
use org_office_launch::{
    v3::{Handoff, HostFirstLine},
    HostMessage, LaunchError,
};
use org_proto::{OfficeRole, RecoveryCode};

#[cfg_attr(not(test), expect(dead_code, reason = "activated by X-02 2a.3.2b.2b"))]
pub(super) struct Create {
    pub office: NewOffice,
    pub role: OfficeRole,
    pub key: Key,
    pub auth: LaunchAuth,
    pub instance: Option<String>,
    // The remaining field stays alive through create AND the blocking serve call.
    pub _recovery: CreationRecovery,
}

#[cfg_attr(not(test), expect(dead_code, reason = "activated by X-02 2a.3.2b.2b"))]
pub(super) enum CreationRecovery {
    Legacy,
    V3 { _code: RecoveryCode },
}

#[cfg_attr(not(test), expect(dead_code, reason = "activated by X-02 2a.3.2b.2b"))]
pub(super) enum Launch {
    Create(Create),
    Start {
        key: Key,
        auth: LaunchAuth,
        instance: Option<String>,
    },
    Discard(Option<Key>),
}

#[cfg_attr(not(test), expect(dead_code, reason = "activated by X-02 2a.3.2b.2b"))]
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
                _recovery: CreationRecovery::Legacy,
            }),
            Request::V2(HostMessage::Start(s)) => Self::Start {
                key: Key::from_db_key(s.db_key),
                auth: s.app_token.into(),
                instance: None,
            },
            Request::V2(HostMessage::Discard(d)) => Self::Discard(d.db_key.map(Key::from_db_key)),
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
                instance: Some(s.instance_id),
            },
            Request::V3 {
                line: HostFirstLine::Discard(_),
                handoff: Handoff::Discard { db_key, .. },
            } => Self::Discard(db_key.map(Key::from_db_key)),
            _ => return Err(LaunchError::Invalid("unknown launch request")),
        })
    }
}

#[cfg(test)]
mod tests;
