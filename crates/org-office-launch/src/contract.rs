//! The launch contract (X-02 §5.1, §5.2, §5.4), version 1, the only launch version. The
//! retired numbered versions 2 and 3 were removed in A1.5 and A1.6.
//!
//! Its parts:
//! - **The first line is non-secret** ([`HostFirstLine`]): the instance, the office and the path
//!   of a per-spawn **rendezvous** socket the host made after checking the server's image, plus,
//!   for a create, the organization, the names, the role and the owner's member id. No keys.
//! - **The keys travel over the rendezvous connection**, after the host's post-accept checks: the
//!   server connects, sends [`ReadyForSecrets`], and the host answers with one JSON line, a
//!   [`Handoff`] header carrying the keys and naming the descriptors that follow, then one
//!   `sendmsg` carrying a 1-byte [`FD_MARKER`] and exactly those descriptors. A `--discard` run
//!   gets no transfer socket: its header names none, and no marker follows. The server reads
//!   that connection only with `recvmsg` and a control buffer sized for the count; `MSG_CTRUNC`,
//!   `MSG_TRUNC` or another count is fatal.
//! - **Worker events** (`created`, `ready`, `discarded`, `failed`: [`crate::ServerEvent`]) also
//!   travel over the rendezvous connection, as launch lines ([`write_server_event`],
//!   [`read_server_event`]); [`RendezvousEvent`] adds authenticated descriptor receipts.
//!   The host treats stdout as untrusted.
//! - **The transfer socket** (an `AF_UNIX SOCK_DGRAM` socket pair, host → server only) carries
//!   [`TransferRecord`]s: a JSON header of at most [`TRANSFER_HEADER_MAX_BYTES`] plus zero or one
//!   descriptor. This is the one definition, for the worker's transfer socket and the web process's.
//!   `org-host-control` doesn't re-export it, which keeps that crate's dependencies to
//!   `org-proto`, serde, `serde_json` and `sha2` (X-02 §5.2, §11): the web process imports it from
//!   here.
//! - Selected REC-DEL-1 adds a mandatory-known `recovery_public_read` record with
//!   exactly one connected descriptor. Its one-shot, EOF-terminated 16 KiB
//!   public exchange is defined by [`crate::recovery`]; it is not writer control
//!   and cannot carry private CA material.
//!
//! The "first line only, then drain unparsed" rule stays for the pipe. A launch line is
//! `{"v": 1, ...}` ([`LAUNCH_VERSION`]); [`read_first_line`] refuses a line of any retired
//! version (2, which carried keys, or 3) by its version, without decoding it.

use std::io::{BufRead, Write};

use org_proto::frame::{self, FrameError};
use org_proto::{GrantKey, OfficeRole};
use serde::de::IgnoredAny;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    is_plain_id, read_secret_line, zero, ByteCounter, FirstLine, LaunchError, OfficeSecretsKey,
    ServerEvent,
};

/// An organization's recovery code (X-02 §6.4): staged by the host at `reserved` and sent on
/// every attempt of an HQ create, so the server creates the CA and the escrow with it. A secret:
/// redacted `Debug`, not `Clone`, zeroed on drop. Defined once in `org-proto`, shared with
/// `org-host-control`'s `recovery.release`.
pub use org_proto::RecoveryCode;

/// The version of launch pipe lines and rendezvous lines.
pub const LAUNCH_VERSION: u32 = 1;

/// Matched host/worker channel ownership and closure contract (CH-LIFE-1).
pub const CHANNEL_PROFILE: &str = "office-channels/1";
/// Maximum ordinary physical channels, including pending transfers and public recovery.
pub const MAX_CHANNELS: usize = 8;
/// The web process's allowance leaves one ordinary slot available for a local caller.
pub const MAX_WEB_CHANNELS: usize = 7;

/// The largest transfer-record header, in bytes (a contract choice, X-02 §5.2).
pub const TRANSFER_HEADER_MAX_BYTES: usize = 1024;

/// The send and receive buffer both ends set on a transfer socket, in bytes.
pub const TRANSFER_SOCKET_BUFFER_BYTES: usize = 64 * 1024;

/// The one byte sent with the descriptors that follow a [`Handoff`] header.
pub const FD_MARKER: u8 = 0;

/// A server's first stdin line, matching its flag ([`FirstLine`]). It holds **no secret**:
/// it's `Clone`, which no secret type is. Write it with [`write_first_line`], read it with
/// [`read_first_line`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
#[non_exhaustive]
pub enum HostFirstLine {
    /// With [`crate::CREATE_FLAG`].
    Create(CreateLine),
    /// With [`crate::OFFICE_FLAG`].
    Start(OfficeLine),
    /// With [`crate::DISCARD_FLAG`].
    Discard(OfficeLine),
    /// An op this build doesn't know: refused as a first line. Never written.
    #[serde(other, skip_serializing)]
    Unknown,
}

/// What every launch first line names: the instance, the office and the rendezvous socket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawOfficeLine")]
pub struct OfficeLine {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_profile: Option<IdProfile>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub selected_writer_required: bool,
    /// The instance the office belongs to: canonical lowercase UUIDv7 text (a ULID is refused).
    /// The server stores it at create and refuses a `start` naming another; an office created
    /// before instance ids adopts it on its first `start`.
    pub instance_id: String,
    pub office_id: String,
    /// The absolute path of the per-spawn rendezvous socket: `<runtime>/r/<16 hex>`.
    pub rendezvous: String,
}

/// Explicit additive identity profile; omission retains legacy launch rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdProfile {
    SelectedV1,
}

/// A first line refuses a field it doesn't define (a v2 key such as `db_key` included); `v` is the
/// line's own, checked before the decode.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOfficeLine {
    #[serde(default)]
    id_profile: Option<IdProfile>,
    #[serde(default)]
    selected_writer_required: bool,
    #[serde(default, rename = "v")]
    _version: Option<IgnoredAny>,
    instance_id: String,
    office_id: String,
    rendezvous: String,
}

impl TryFrom<RawOfficeLine> for OfficeLine {
    type Error = &'static str;

    fn try_from(raw: RawOfficeLine) -> Result<Self, Self::Error> {
        let line = OfficeLine {
            id_profile: raw.id_profile,
            selected_writer_required: raw.selected_writer_required,
            instance_id: raw.instance_id,
            office_id: raw.office_id,
            rendezvous: raw.rendezvous,
        };
        line.check()?;
        Ok(line)
    }
}

impl OfficeLine {
    fn check(&self) -> Result<(), &'static str> {
        let office_valid = match self.id_profile {
            None => is_plain_id(&self.office_id),
            Some(IdProfile::SelectedV1) => crate::is_selected_entity_id(&self.office_id),
        };
        if !org_proto::is_uuid_v7(&self.instance_id) {
            return Err("the first line's instance id isn't a canonical lowercase UUIDv7");
        }
        if !office_valid {
            return Err("the first line holds an id that isn't plain");
        }
        if !self.rendezvous.starts_with('/') || self.rendezvous.contains('\0') {
            return Err("the rendezvous path must be absolute, without NUL");
        }
        Ok(())
    }
}

/// A create. As in the retired version 2, this version creates only HQ offices, and both ids and the
/// owner's member id are checked whenever it's decoded or encoded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawCreateLine")]
pub struct CreateLine {
    #[serde(flatten)]
    pub office: OfficeLine,
    pub org_id: String,
    /// In [`org_proto::OrganizationName::as_str`] form, as in v2.
    pub org_name: String,
    pub office_name: String,
    pub role: OfficeRole,
    /// The organization's owner member, chosen by the host so a retried create uses the same
    /// one ([`org_proto::is_valid_member_id`]).
    pub owner_member_id: String,
    /// Explicit REC-ESC-1 preparation; absence preserves legacy create behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hq_creation: Option<HqCreationV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HqCreationV1 {
    pub version: u32,
    pub operation_id: String,
    pub escrow_profile: u32,
    /// Host-resolved canonical state root; never supplied by a browser request.
    pub public_root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bootstrap: Option<Box<org_proto::selected_bootstrap::PreparedV1>>,
}

impl HqCreationV1 {
    fn check(&self) -> Result<(), &'static str> {
        if self.bootstrap.as_ref().is_some_and(|b| !b.valid())
            || self.version != 1
            || self.escrow_profile != 1
            || org_proto::ulid_time_ms(&self.operation_id).is_none()
            || !std::path::Path::new(&self.public_root).is_absolute()
            || self.public_root.contains('\0')
            || std::path::Path::new(&self.public_root).parent().is_none()
            || std::path::Path::new(&self.public_root)
                .components()
                .any(|c| {
                    !matches!(
                        c,
                        std::path::Component::RootDir | std::path::Component::Normal(_)
                    )
                })
        {
            return Err("unsupported HQ creation context");
        }
        Ok(())
    }
}

/// As [`RawOfficeLine`]: no field the line doesn't define.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCreateLine {
    #[serde(default)]
    id_profile: Option<IdProfile>,
    #[serde(default)]
    selected_writer_required: bool,
    #[serde(default, rename = "v")]
    _version: Option<IgnoredAny>,
    instance_id: String,
    office_id: String,
    rendezvous: String,
    org_id: String,
    org_name: String,
    office_name: String,
    #[serde(default)]
    role: OfficeRole,
    owner_member_id: String,
    #[serde(default)]
    hq_creation: Option<HqCreationV1>,
}

impl TryFrom<RawCreateLine> for CreateLine {
    type Error = &'static str;

    fn try_from(raw: RawCreateLine) -> Result<Self, Self::Error> {
        let line = CreateLine {
            office: OfficeLine {
                id_profile: raw.id_profile,
                selected_writer_required: raw.selected_writer_required,
                instance_id: raw.instance_id,
                office_id: raw.office_id,
                rendezvous: raw.rendezvous,
            },
            org_id: raw.org_id,
            org_name: raw.org_name,
            office_name: raw.office_name,
            role: raw.role,
            owner_member_id: raw.owner_member_id,
            hq_creation: raw.hq_creation,
        };
        line.check()?;
        Ok(line)
    }
}

impl CreateLine {
    fn check(&self) -> Result<(), &'static str> {
        self.office.check()?;
        self.check_bootstrap_context()?;
        let org_valid = match self.office.id_profile {
            None => is_plain_id(&self.org_id),
            Some(IdProfile::SelectedV1) => {
                crate::is_selected_entity_id(&self.org_id) && self.hq_creation.is_some()
            }
        };
        if !org_valid {
            return Err("the create line holds an id that isn't plain");
        }
        if !org_proto::is_valid_member_id(&self.owner_member_id) {
            return Err("the create line's owner_member_id isn't a member id");
        }
        if self.role != OfficeRole::Hq {
            return Err("this version creates only HQ offices");
        }
        Ok(())
    }

    fn check_bootstrap_context(&self) -> Result<(), &'static str> {
        if self.office.selected_writer_required
            && self
                .hq_creation
                .as_ref()
                .and_then(|c| c.bootstrap.as_ref())
                .is_none()
        {
            return Err("selected writer admission requires the explicit bootstrap profile");
        }
        if let Some(context) = &self.hq_creation {
            context.check()?;
            if let Some(b) = &context.bootstrap {
                if b.organization_id != self.org_id
                    || b.office_id != self.office.office_id
                    || b.plan.name != self.org_name
                    || self.office.id_profile != Some(IdProfile::SelectedV1)
                    || !self.office.selected_writer_required
                {
                    return Err("selected bootstrap scope mismatch");
                }
            }
        }
        Ok(())
    }
}

impl HostFirstLine {
    fn check(&self) -> Result<(), LaunchError> {
        match self {
            HostFirstLine::Create(create) => create.check(),
            HostFirstLine::Start(line) | HostFirstLine::Discard(line) => line.check(),
            HostFirstLine::Unknown => Err("can't send an unknown op"),
        }
        .map_err(LaunchError::Invalid)
    }

    /// The office this line names.
    pub fn office(&self) -> Option<&OfficeLine> {
        match self {
            HostFirstLine::Create(create) => Some(&create.office),
            HostFirstLine::Start(line) | HostFirstLine::Discard(line) => Some(line),
            HostFirstLine::Unknown => None,
        }
    }
}

#[derive(Serialize)]
struct Line<'a, T> {
    v: u32,
    #[serde(flatten)]
    body: &'a T,
}

#[derive(Deserialize)]
struct Versioned {
    v: u32,
}

fn check_version(line: &[u8]) -> Result<(), LaunchError> {
    let version: Versioned = serde_json::from_slice(line).map_err(LaunchError::Json)?;
    if version.v == LAUNCH_VERSION {
        Ok(())
    } else {
        Err(LaunchError::UnsupportedVersion {
            found: version.v,
            supported: &[LAUNCH_VERSION],
        })
    }
}

/// Write a first line, then flush. An unknown op, or a line that breaks its rules, is
/// refused and nothing is written.
pub fn write_first_line(w: &mut impl Write, line: &HostFirstLine) -> Result<(), LaunchError> {
    line.check()?;
    let body = Line {
        v: LAUNCH_VERSION,
        body: line,
    };
    frame::write_message(w, &body).map_err(LaunchError::Frame)
}

/// Read a server's first line and check it matches the flag. `Ok(None)` means EOF before it
/// (the host is gone). Another `v` is [`LaunchError::UnsupportedVersion`]; an unknown op, or
/// another flag's op, is [`LaunchError::Invalid`]. The line is read through a buffer reserved
/// up front and zeroed afterwards, since a stray v2 host line carries keys.
pub fn read_first_line(
    r: &mut impl BufRead,
    expected: FirstLine,
) -> Result<Option<HostFirstLine>, LaunchError> {
    let Some(mut line) = read_secret_line(r)? else {
        return Ok(None);
    };
    // The version first, so a line of any other version (a v2 line with its keys included) is
    // refused as a version without being decoded.
    let decoded = check_version(&line)
        .and_then(|()| serde_json::from_slice::<HostFirstLine>(&line).map_err(LaunchError::Json));
    zero(&mut line);
    let message = decoded?;
    match (&message, expected) {
        (HostFirstLine::Create(_), FirstLine::Create)
        | (HostFirstLine::Start(_), FirstLine::Start)
        | (HostFirstLine::Discard(_), FirstLine::Discard) => Ok(Some(message)),
        _ => Err(LaunchError::Invalid(
            "the first line isn't the op this server's flag expects",
        )),
    }
}

/// Write a worker event on the rendezvous connection, as a launch line (`{"v": 1, "event": ...}`).
/// [`ServerEvent::Unknown`] and a created office that [`crate::OfficeFile::to_vec`] would refuse
/// are refused, and nothing is written. In the launch contract the host acts on `created` and `discarded` only
/// when they arrive this way; stdout is untrusted.
pub fn write_server_event(w: &mut impl Write, event: &ServerEvent) -> Result<(), LaunchError> {
    match event {
        ServerEvent::Created { office } => drop(office.to_vec()?),
        ServerEvent::Unknown => return Err(LaunchError::Invalid("can't send an unknown event")),
        ServerEvent::Ready { .. } | ServerEvent::Discarded | ServerEvent::Failed { .. } => {}
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
    if let RendezvousEvent::Server(ServerEvent::Created { office }) = &event {
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
enum RawServerEvent {
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
        #[allow(dead_code)]
        v: u32,
        office: crate::OfficeFile,
    },
    Ready {
        #[allow(dead_code)]
        v: u32,
        socket: String,
        pid: u32,
    },
    Discarded {
        #[allow(dead_code)]
        v: u32,
    },
    Failed {
        #[allow(dead_code)]
        v: u32,
        code: crate::FailCode,
        message: String,
    },
    #[serde(other)]
    Unknown,
}

impl RawServerEvent {
    fn into_event(self) -> RendezvousEvent {
        let event = match self {
            Self::FdsReceived { of, .. } => return RendezvousEvent::FdsReceived { of },
            Self::FdsRefused { of, reason, .. } => {
                return RendezvousEvent::FdsRefused { of, reason };
            }
            Self::ChannelClosed { of, .. } => return RendezvousEvent::ChannelClosed { of },
            Self::Created { office, .. } => ServerEvent::Created { office },
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

const READY_FOR_SECRETS: &str = "ready_for_secrets";

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

/// A descriptor named by a [`Handoff`] header, in the order they arrive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffFd {
    /// The server's end of its transfer socket.
    Transfer,
}

/// The host's answer to [`ReadyForSecrets`] on the rendezvous connection: one JSON line with the
/// secrets, naming the descriptors the following `sendmsg` carries. Its op matches the first
/// line's. Not `Clone`: it holds the keys. Encode it with [`encode_handoff`] and zero the bytes
/// after sending them.
///
/// It decodes as one flat object, not through serde's internally tagged enums, so each secret
/// goes straight from the JSON into its secret type without an intermediate buffer. A field the
/// op doesn't carry is refused, even as `null`.
#[derive(Debug, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Handoff {
    /// After a create line (HQ): the new office's secrets key (required), the grant key and,
    /// when the create has one, the recovery code (absent otherwise, never `null`). `fds` is
    /// `["transfer"]`.
    Create {
        office_secrets_key: OfficeSecretsKey,
        grant_key: GrantKey,
        #[serde(skip_serializing_if = "Option::is_none")]
        recovery_code: Option<RecoveryCode>,
        fds: Vec<HandoffFd>,
    },
    /// After a start line: the office secrets key, or `null` when the host has none for that
    /// office (present either way), and the grant key. A start without a key proceeds: the
    /// office serves its data and only what the key unwraps is unavailable. `fds` is
    /// `["transfer"]`.
    Start {
        office_secrets_key: Option<OfficeSecretsKey>,
        grant_key: GrantKey,
        fds: Vec<HandoffFd>,
    },
    /// After a discard line: no secret at all (a plain office opens nothing that needs a key).
    /// `fds` is `[]`: no transfer socket, and no marker follows.
    Discard { fds: Vec<HandoffFd> },
}

/// A present key, `null` included, as `Some`: so a key the op doesn't carry is refused even as
/// `null`, and an absent one stays `None` through `#[serde(default)]`.
fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum HandoffOp {
    Create,
    Start,
    Discard,
}

/// A handoff as read: every field at the top level, each secret decoded in place. A field it
/// doesn't define (a retired `app_token` or `db_key` included) is refused; `v` is checked before
/// the decode.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawHandoff {
    #[serde(default, rename = "v")]
    _version: Option<IgnoredAny>,
    op: HandoffOp,
    #[serde(default, deserialize_with = "present")]
    office_secrets_key: Option<Option<OfficeSecretsKey>>,
    #[serde(default, deserialize_with = "present")]
    grant_key: Option<Option<GrantKey>>,
    #[serde(default, deserialize_with = "present")]
    recovery_code: Option<Option<RecoveryCode>>,
    fds: Vec<HandoffFd>,
}

impl<'de> Deserialize<'de> for Handoff {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let raw = RawHandoff::deserialize(d)?;
        let fds = raw.fds;
        match (
            raw.op,
            raw.office_secrets_key,
            raw.grant_key,
            raw.recovery_code,
        ) {
            (
                HandoffOp::Create,
                Some(Some(office_secrets_key)),
                Some(Some(grant_key)),
                recovery_code @ (None | Some(Some(_))),
            ) => Ok(Handoff::Create {
                office_secrets_key,
                grant_key,
                recovery_code: recovery_code.flatten(),
                fds,
            }),
            (HandoffOp::Start, Some(office_secrets_key), Some(Some(grant_key)), None) => {
                Ok(Handoff::Start {
                    office_secrets_key,
                    grant_key,
                    fds,
                })
            }
            (HandoffOp::Discard, None, None, None) => Ok(Handoff::Discard { fds }),
            _ => Err(D::Error::custom(
                "a handoff must carry exactly the fields of its op",
            )),
        }
    }
}

impl Handoff {
    /// The descriptors this header must name, in order.
    pub fn expected_fds(&self) -> &'static [HandoffFd] {
        match self {
            Handoff::Create { .. } | Handoff::Start { .. } => &[HandoffFd::Transfer],
            Handoff::Discard { .. } => &[],
        }
    }

    /// The descriptors it names.
    pub fn fds(&self) -> &[HandoffFd] {
        match self {
            Handoff::Create { fds, .. }
            | Handoff::Start { fds, .. }
            | Handoff::Discard { fds, .. } => fds,
        }
    }

    fn check(&self) -> Result<(), LaunchError> {
        if self.fds() == self.expected_fds() {
            Ok(())
        } else {
            Err(LaunchError::Invalid(
                "the handoff names other descriptors than its op carries",
            ))
        }
    }
}

/// Encode a handoff as one line with its `v` and `\n`. A header naming the wrong descriptors is
/// refused. The bytes hold the keys: zero them after sending.
pub fn encode_handoff(handoff: &Handoff) -> Result<Vec<u8>, LaunchError> {
    handoff.check()?;
    encode_secret(handoff)
}

/// Borrow host-held secrets for a start without creating another owned key copy. The office
/// secrets key is `None` when the host has none for the office: it goes out as `null` and the
/// start proceeds. The returned line is secret and must be zeroed by the sender on every path.
pub fn encode_start_handoff(
    office_secrets_key: Option<&OfficeSecretsKey>,
    grant_key: &GrantKey,
) -> Result<Vec<u8>, LaunchError> {
    #[derive(Serialize)]
    struct Start<'a> {
        op: &'static str,
        office_secrets_key: Option<&'a OfficeSecretsKey>,
        grant_key: &'a GrantKey,
        fds: [HandoffFd; 1],
    }
    encode_secret(&Start {
        op: "start",
        office_secrets_key,
        grant_key,
        fds: [HandoffFd::Transfer],
    })
}

/// Borrow the frozen host-held creation material. No owned secret copies and no
/// alternative framing to the existing create handoff; caller zeroes every line.
pub fn encode_create_handoff(
    office_secrets_key: &OfficeSecretsKey,
    grant_key: &GrantKey,
    recovery_code: &RecoveryCode,
) -> Result<Vec<u8>, LaunchError> {
    #[derive(Serialize)]
    struct Create<'a> {
        op: &'static str,
        office_secrets_key: &'a OfficeSecretsKey,
        grant_key: &'a GrantKey,
        recovery_code: &'a RecoveryCode,
        fds: [HandoffFd; 1],
    }
    encode_secret(&Create {
        op: "create",
        office_secrets_key,
        grant_key,
        recovery_code,
        fds: [HandoffFd::Transfer],
    })
}

/// The discard handoff: no secret, and `fds` is `[]` (no transfer socket, no marker follows).
/// It still goes only to the authenticated reader after `ready_for_secrets`, keeping the
/// handshake order of every op.
pub fn encode_discard_handoff() -> Result<Vec<u8>, LaunchError> {
    #[derive(Serialize)]
    struct Discard {
        op: &'static str,
        fds: [HandoffFd; 0],
    }
    encode_secret(&Discard {
        op: "discard",
        fds: [],
    })
}

fn encode_secret(handoff: &impl Serialize) -> Result<Vec<u8>, LaunchError> {
    let body = Line {
        v: LAUNCH_VERSION,
        body: handoff,
    };
    // Measured first, so the buffer never reallocates and leaves a copy of a key behind.
    let mut counter = ByteCounter(0);
    serde_json::to_writer(&mut counter, &body).map_err(LaunchError::Json)?;
    let mut line = Vec::with_capacity(counter.0 + 1);
    if let Err(e) = serde_json::to_writer(&mut line, &body) {
        zero(&mut line);
        return Err(LaunchError::Json(e));
    }
    line.push(b'\n');
    Ok(line)
}

/// Decode a handoff line (without its `\n`). Another `v` is [`LaunchError::UnsupportedVersion`],
/// and a header naming the wrong descriptors is [`LaunchError::Invalid`]. The caller zeroes the
/// line afterwards.
pub fn decode_handoff(line: &[u8]) -> Result<Handoff, LaunchError> {
    check_version(line)?;
    let handoff: Handoff = serde_json::from_slice(line).map_err(LaunchError::Json)?;
    handoff.check()?;
    Ok(handoff)
}

/// Which client a channel's descriptor serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    /// For the web process, opened with `office.open_channel` on its control connection.
    Web,
    /// For `agentic-org` on `host.sock`.
    Local,
}

/// One record on a transfer socket (X-02 §5.2): a JSON header of at most
/// [`TRANSFER_HEADER_MAX_BYTES`], with exactly [`TransferRecord::fd_count`] descriptors. A record
/// of another kind, or with another descriptor count, is fatal: the receiver closes the transfer
/// socket (a worker exits and the host restarts it with a new key).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TransferRecord {
    /// One-shot public committed HQ escrow read, on a separate connected FD.
    RecoveryPublicRead {
        version: u32,
        office_id: String,
        channel_id: String,
    },
    /// Private writer admission/checkpoint channel, never a human or agent task
    /// channel. Sent only for a previously provisioned selected office.
    WriterControl {
        office_id: String,
        channel_id: String,
    },
    /// A new host-made channel to this office: one `SOCK_STREAM` descriptor.
    Channel {
        kind: ChannelKind,
        office_id: String,
        channel_id: String,
    },
    /// Sessions that ended: end their subscriptions, release their leases and refuse their grants
    /// until they expire. No descriptor.
    Revoked { session_ids: Vec<String> },
}

impl TransferRecord {
    /// How many descriptors arrive with this record.
    pub fn fd_count(&self) -> usize {
        match self {
            TransferRecord::Channel { .. }
            | TransferRecord::WriterControl { .. }
            | TransferRecord::RecoveryPublicRead { .. } => 1,
            TransferRecord::Revoked { .. } => 0,
        }
    }
}

/// Encode a record's header. Over [`TRANSFER_HEADER_MAX_BYTES`] is refused
/// ([`FrameError::TooLargeToSend`]): split a long revocation into several records.
pub fn encode_transfer_record(record: &TransferRecord) -> Result<Vec<u8>, LaunchError> {
    if matches!(record, TransferRecord::RecoveryPublicRead { version, .. } if *version != 1) {
        return Err(LaunchError::Invalid(
            "unsupported recovery transfer version",
        ));
    }
    let bytes = serde_json::to_vec(record).map_err(LaunchError::Json)?;
    if bytes.len() > TRANSFER_HEADER_MAX_BYTES {
        return Err(LaunchError::Frame(FrameError::TooLargeToSend));
    }
    Ok(bytes)
}

/// Decode a record's header, as received in one datagram. Over [`TRANSFER_HEADER_MAX_BYTES`] is
/// refused ([`FrameError::TooLarge`]); so is anything that isn't a known record.
pub fn decode_transfer_record(bytes: &[u8]) -> Result<TransferRecord, LaunchError> {
    if bytes.len() > TRANSFER_HEADER_MAX_BYTES {
        return Err(LaunchError::Frame(FrameError::TooLarge));
    }
    let record = serde_json::from_slice(bytes).map_err(LaunchError::Json)?;
    if matches!(record, TransferRecord::RecoveryPublicRead { version, .. } if version != 1) {
        return Err(LaunchError::Invalid(
            "unsupported recovery transfer version",
        ));
    }
    Ok(record)
}

#[cfg(test)]
mod tests;
