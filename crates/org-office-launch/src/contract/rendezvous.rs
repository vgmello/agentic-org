//! Worker events and launch rendezvous events on the rendezvous connection, and the server's
//! ready-for-secrets line.

use std::io::{BufRead, Write};

use org_proto::frame;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{check_version, Line, CHANNEL_PROFILE, LAUNCH_VERSION};
use crate::{LaunchError, ServerEvent};

/// Write a worker event on the rendezvous connection, as a launch line (`{"v": 1, "event": ...}`).
/// [`ServerEvent::Unknown`] and a created office that [`crate::OfficeFile::to_vec`] would refuse
/// are refused, and nothing is written. In the launch contract the host acts on `created` and `discarded` only
/// when they arrive this way; stdout is untrusted.
pub fn write_server_event(w: &mut impl Write, event: &ServerEvent) -> Result<(), LaunchError> {
    match event {
        ServerEvent::Created { office } | ServerEvent::Joined { office } => drop(office.to_vec()?),
        ServerEvent::Unknown => return Err(LaunchError::Invalid("can't send an unknown event")),
        ServerEvent::JoinPending { .. }
        | ServerEvent::Ready { .. }
        | ServerEvent::Discarded
        | ServerEvent::Failed { .. } => {}
    }
    let body = Line {
        v: LAUNCH_VERSION,
        body: event,
    };
    frame::write_message(w, &body).map_err(LaunchError::Frame)
}

/// Events authenticated by the launch rendezvous, including descriptor receipts (X-02 rev15 §5.2).
/// [`ServerEvent`] (a worker's own event) deliberately has no receipt variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
#[non_exhaustive]
pub enum RendezvousEvent {
    /// Sent after taking ownership of a handoff's or channel's descriptors. `of` is its
    /// correlation value (`handoff` or a channel ID); the host checks its pending records.
    FdsReceived { of: String },
    /// The worker closed a rejected descriptor without starting a serving thread.
    FdsRefused { of: String, reason: RefusalReason },
    /// An acknowledged channel's serving owner and shutdown descriptors are gone.
    ChannelClosed { of: String },
    /// An ordinary worker event, carried on the authenticated connection.
    #[serde(untagged)]
    Server(ServerEvent),
}

/// The only ordinary transfer refusal; malformed or uncertain transfers remain fatal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalReason {
    /// The worker has no physical channel slot available.
    Capacity,
}

/// Write a launch rendezvous event, including a descriptor receipt, and flush.
/// Ordinary events retain [`write_server_event`]'s validation.
pub fn write_rendezvous_event(
    w: &mut impl Write,
    event: &RendezvousEvent,
) -> Result<(), LaunchError> {
    match event {
        RendezvousEvent::Server(event) => write_server_event(w, event),
        RendezvousEvent::FdsReceived { .. }
        | RendezvousEvent::FdsRefused { .. }
        | RendezvousEvent::ChannelClosed { .. } => frame::write_message(
            w,
            &Line {
                v: LAUNCH_VERSION,
                body: event,
            },
        )
        .map_err(LaunchError::Frame),
    }
}

/// Read one worker event from the rendezvous connection. `Ok(None)` means it closed. Another
/// `v` is [`LaunchError::UnsupportedVersion`]; an event this build doesn't know is
/// [`RendezvousEvent::Server`] containing [`ServerEvent::Unknown`], which the host ignores.
pub fn read_server_event(r: &mut impl BufRead) -> Result<Option<RendezvousEvent>, LaunchError> {
    let Some(line) = frame::read_line(r).map_err(LaunchError::Frame)? else {
        return Ok(None);
    };
    check_version(&line)?;
    let event = serde_json::from_slice::<RawServerEvent>(&line)
        .map_err(LaunchError::Json)?
        .into_event();
    if let RendezvousEvent::Server(
        ServerEvent::Created { office } | ServerEvent::Joined { office },
    ) = &event
    {
        office.to_vec()?;
    }
    Ok(Some(event))
}

/// A worker event as it arrives: [`ServerEvent`]'s variants plus `v`, refusing any field the
/// event doesn't define, like every other launch line.
/// The `office` in `created` is an `office.json` body and keeps that file's own rule: fields this
/// build doesn't know are kept, not refused. An event name this build doesn't know is
/// [`ServerEvent::Unknown`], whatever its fields.
#[derive(Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum RawServerEvent {
    FdsReceived {
        #[serde(rename = "v")]
        _version: u32,
        of: String,
    },
    FdsRefused {
        #[serde(rename = "v")]
        _version: u32,
        of: String,
        reason: RefusalReason,
    },
    ChannelClosed {
        #[serde(rename = "v")]
        _version: u32,
        of: String,
    },
    Created {
        #[serde(rename = "v")]
        _version: u32,
        office: crate::OfficeFile,
    },
    JoinPending {
        #[serde(rename = "v")]
        _version: u32,
        request_id: String,
        check_code: String,
    },
    Joined {
        #[serde(rename = "v")]
        _version: u32,
        office: crate::OfficeFile,
    },
    Ready {
        #[serde(rename = "v")]
        _version: u32,
        socket: String,
        pid: u32,
    },
    Discarded {
        #[serde(rename = "v")]
        _version: u32,
    },
    Failed {
        #[serde(rename = "v")]
        _version: u32,
        code: crate::FailCode,
        message: String,
    },
    #[serde(other)]
    Unknown,
}

impl RawServerEvent {
    pub(super) fn into_event(self) -> RendezvousEvent {
        let event = match self {
            Self::FdsReceived { of, .. } => return RendezvousEvent::FdsReceived { of },
            Self::FdsRefused { of, reason, .. } => {
                return RendezvousEvent::FdsRefused { of, reason };
            }
            Self::ChannelClosed { of, .. } => return RendezvousEvent::ChannelClosed { of },
            Self::Created { office, .. } => ServerEvent::Created { office },
            Self::JoinPending {
                request_id,
                check_code,
                ..
            } => ServerEvent::JoinPending {
                request_id,
                check_code,
            },
            Self::Joined { office, .. } => ServerEvent::Joined { office },
            Self::Ready { socket, pid, .. } => ServerEvent::Ready { socket, pid },
            Self::Discarded { .. } => ServerEvent::Discarded,
            Self::Failed { code, message, .. } => ServerEvent::Failed { code, message },
            Self::Unknown => ServerEvent::Unknown,
        };
        RendezvousEvent::Server(event)
    }
}

/// The server's first line on the rendezvous connection: `{"v": 1, "event":
/// "ready_for_secrets", "channel_profile":"office-channels/1"}`. The host accepts it only after its post-accept checks (it starts after
/// anything buffered at accept, nothing follows it, and the peer's token is still the admitted
/// one), then sends the [`Handoff`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReadyForSecrets;

pub(super) const READY_FOR_SECRETS: &str = "ready_for_secrets";

impl Serialize for ReadyForSecrets {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire {
            v: u32,
            event: &'static str,
            channel_profile: &'static str,
        }
        Wire {
            v: LAUNCH_VERSION,
            event: READY_FOR_SECRETS,
            channel_profile: CHANNEL_PROFILE,
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for ReadyForSecrets {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            v: u32,
            event: String,
            channel_profile: String,
        }
        let wire = Wire::deserialize(d)?;
        if wire.v != LAUNCH_VERSION
            || wire.event != READY_FOR_SECRETS
            || wire.channel_profile != CHANNEL_PROFILE
        {
            return Err(serde::de::Error::custom("not a launch ready_for_secrets"));
        }
        Ok(ReadyForSecrets)
    }
}
