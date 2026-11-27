//! Office channel grants: opening an office channel for a signed-in session and signing the
//! grant the office checks.

use std::sync::Arc;
use std::time::{Duration, Instant};

use org_host_control::SessionHandle;
use org_proto::ApiError;
use sha2::Sha256;

use crate::control_listener::Connection;
use crate::hostdb::accounts::Account;

use super::{denied, digest, unavailable, wall_ms, Authentication, Channel, ABSOLUTE};

impl Authentication {
    pub(crate) fn open_channel(
        &self,
        connection: &Connection,
        handle: &SessionHandle,
        endpoint: Arc<crate::supervisor::channels::Endpoint>,
        organization: &str,
    ) -> Result<
        (
            String,
            std::os::unix::net::UnixStream,
            crate::supervisor::channels::ChannelSlot,
        ),
        ApiError,
    > {
        self.open_channel_inner(connection, handle, endpoint, organization, false)
    }

    /// Opens a local `host.sock` channel to a joined satellite office for an instance
    /// administrator who has no local `account_org_links` row for that organization. The
    /// resulting grant carries `member = office_id`, which the satellite office admits solely for
    /// read-only `office.sync_status` checks.
    pub(crate) fn open_satellite_operator_channel(
        &self,
        connection: &Connection,
        handle: &SessionHandle,
        endpoint: Arc<crate::supervisor::channels::Endpoint>,
        organization: &str,
    ) -> Result<
        (
            String,
            std::os::unix::net::UnixStream,
            crate::supervisor::channels::ChannelSlot,
        ),
        ApiError,
    > {
        self.open_channel_inner(connection, handle, endpoint, organization, true)
    }

    fn open_channel_inner(
        &self,
        connection: &Connection,
        handle: &SessionHandle,
        endpoint: Arc<crate::supervisor::channels::Endpoint>,
        organization: &str,
        satellite_operator: bool,
    ) -> Result<
        (
            String,
            std::os::unix::net::UnixStream,
            crate::supervisor::channels::ChannelSlot,
        ),
        ApiError,
    > {
        let may_open = |account: &Account| {
            account
                .links()
                .iter()
                .any(|link| link.org_id() == organization)
                || (satellite_operator
                    && account.admin()
                    && connection.channel_kind() == org_office_launch::ChannelKind::Local)
        };
        let account = self.authorize(connection, handle)?;
        if !may_open(&account) {
            return Err(denied());
        }
        // Authorization is checked before contacting the worker and repeated
        // after its descriptor receipt, because logout or authority changes may race that handoff.
        let (id, socket, slot) = endpoint
            .open(connection.channel_kind())
            .map_err(|_| unavailable())?;
        let refreshed = self.authorize(connection, handle)?;
        if !may_open(&refreshed) {
            return Err(denied());
        }
        let mut sessions = self.sessions.lock().map_err(|_| unavailable())?;
        let session = sessions.get_mut(&digest(handle)).ok_or_else(denied)?;
        session
            .channels
            .retain(|_, channel| channel._slot.is_live());
        if !session.live(Instant::now()) || !slot.is_live() || session.channels.len() >= 64 {
            return Err(unavailable());
        }
        session.channels.insert(
            id.clone(),
            Channel {
                endpoint: Arc::downgrade(&endpoint),
                organization: organization.into(),
                satellite_operator,
                _slot: slot.clone(),
            },
        );
        Ok((id, socket, slot))
    }

    pub(crate) fn grant(
        &self,
        connection: &Connection,
        handle: &SessionHandle,
        office: &str,
        channel: &str,
    ) -> Result<org_host_control::ops::SessionGrantResult, ApiError> {
        if !connection.channel_received(channel) {
            return Err(denied());
        }
        let account = self.authorize(connection, handle)?;
        let sessions = self.sessions.lock().map_err(|_| unavailable())?;
        let session = sessions
            .get(&digest(handle))
            .filter(|s| s.live(Instant::now()))
            .ok_or_else(denied)?;
        let bound = session.channels.get(channel).ok_or_else(denied)?;
        let endpoint = bound.endpoint.upgrade().ok_or_else(unavailable)?;
        if endpoint.office != office || !endpoint.is_live() || !bound._slot.is_live() {
            return Err(denied());
        }
        let member = if let Some(link) = account
            .links()
            .iter()
            .find(|link| link.org_id() == bound.organization)
        {
            link.member_id()
        } else if bound.satellite_operator
            && account.admin()
            && connection.channel_kind() == org_office_launch::ChannelKind::Local
        {
            office
        } else {
            return Err(denied());
        };
        let remaining = ABSOLUTE
            .checked_sub(Instant::now().duration_since(session.created))
            .ok_or_else(denied)?;
        sign_grant(
            &endpoint,
            GrantInput {
                account: account.account_id(),
                member,
                org_id: &bound.organization,
                office_id: office,
                channel_id: channel,
                session_id: &session.id,
                ttl: remaining.min(Duration::from_secs(300)),
            },
        )
    }
}

pub(crate) struct GrantInput<'a> {
    pub(crate) account: &'a str,
    pub(crate) member: &'a str,
    pub(crate) org_id: &'a str,
    pub(crate) office_id: &'a str,
    pub(crate) channel_id: &'a str,
    pub(crate) session_id: &'a str,
    pub(crate) ttl: Duration,
}

pub(crate) fn sign_grant(
    endpoint: &crate::supervisor::channels::Endpoint,
    input: GrantInput<'_>,
) -> Result<org_host_control::ops::SessionGrantResult, ApiError> {
    use hmac::{Hmac, Mac};
    let ttl_ms = u64::try_from(input.ttl.as_millis()).map_err(|_| unavailable())?;
    if ttl_ms == 0 {
        return Err(denied());
    }
    let exp = wall_ms()?.checked_add(ttl_ms).ok_or_else(unavailable)?;
    let claims = org_proto::GrantClaims {
        account: input.account.into(),
        member: input.member.into(),
        org_id: input.org_id.into(),
        office_id: input.office_id.into(),
        channel_id: input.channel_id.into(),
        session_id: input.session_id.into(),
        exp,
    };
    let mut mac =
        Hmac::<Sha256>::new_from_slice(endpoint.key.expose_bytes()).map_err(|_| unavailable())?;
    mac.update(&org_proto::grant_mac_input(&claims).ok_or_else(unavailable)?);
    let mut bytes = mac.finalize().into_bytes();
    let mut tag: [u8; 32] = bytes[..].try_into().map_err(|_| unavailable())?;
    let signature = org_proto::GrantMac::from_bytes(tag);
    crate::secrets::zero(&mut tag);
    bytes.fill(0);
    Ok(org_host_control::ops::SessionGrantResult {
        grant: org_proto::Grant {
            claims,
            mac: signature,
        },
    })
}
