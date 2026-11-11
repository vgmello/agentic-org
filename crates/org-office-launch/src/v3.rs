//! Launch contract v3 (X-02 §5.1, §5.2, §5.4): the types only. Version 2 (the crate root) is
//! unchanged and stays what the host and the server speak until the migration's moves 2a and 2b
//! switch to these.
//!
//! What changes from v2:
//! - **The first line is non-secret** ([`HostFirstLine`]): the instance, the office and the path
//!   of a per-spawn **rendezvous** socket the host made after checking the server's image, plus,
//!   for a create, the organization, the names, the role and the owner's member id. No app token
//!   (retired) and no keys.
//! - **The keys travel over the rendezvous connection**, after the host's post-accept checks: the
//!   server connects, sends [`ReadyForSecrets`], and the host answers with one JSON line, a
//!   [`Handoff`] header carrying the keys and naming the descriptors that follow, then one
//!   `sendmsg` carrying a 1-byte [`FD_MARKER`] and exactly those descriptors. A `--discard` run
//!   gets no transfer socket: its header names none, and no marker follows. The server reads
//!   that connection only with `recvmsg` and a control buffer sized for the count; `MSG_CTRUNC`,
//!   `MSG_TRUNC` or another count is fatal.
//! - **Worker events** (`created`, `ready`, `discarded`, `failed`: [`crate::ServerEvent`]) also
//!   travel over the rendezvous connection, as v3 lines ([`write_server_event`],
//!   [`read_server_event`]); the host treats stdout as untrusted.
//! - **The transfer socket** (an `AF_UNIX SOCK_DGRAM` socket pair, host → server only) carries
//!   [`TransferRecord`]s: a JSON header of at most [`TRANSFER_HEADER_MAX_BYTES`] plus zero or one
//!   descriptor. This is the one definition, for the worker's transfer socket and the gateway's.
//!   `org-host-control` doesn't re-export it, which keeps that crate's dependencies to
//!   `org-proto` and serde (X-02 §5.2, §11): the gateway imports it from here.
//!
//! The "first line only, then drain unparsed" rule of v2 stays for the pipe. A v3 line is
//! `{"v": 3, ...}`; a v2 build refuses it as another version, and [`read_first_line`] refuses a
//! v2 line the same way. Move 2a's server reads both with [`read_first_line_any`] until move 2b's
//! host lands.

use std::io::{BufRead, Write};

use org_proto::frame::{self, FrameError};
use org_proto::{GrantKey, OfficeRole};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    is_plain_id, read_secret_line, zero, ByteCounter, DbKey, FirstLine, HostMessage, LaunchError,
    ServerEvent,
};

/// An organization's recovery code (X-02 §6.4): staged by the host at `reserved` and sent on
/// every attempt of an HQ create, so the server creates the CA and the escrow with it. A secret:
/// redacted `Debug`, not `Clone`, zeroed on drop. Defined once in `org-proto`, shared with
/// `org-host-control`'s `recovery.release`.
pub use org_proto::RecoveryCode;

/// The version of v3 pipe lines and rendezvous lines.
pub const LAUNCH_VERSION: u32 = 3;

/// The largest transfer-record header, in bytes (a contract choice, X-02 §5.2).
pub const TRANSFER_HEADER_MAX_BYTES: usize = 1024;

/// The send and receive buffer both ends set on a transfer socket, in bytes.
pub const TRANSFER_SOCKET_BUFFER_BYTES: usize = 64 * 1024;

/// The one byte sent with the descriptors that follow a [`Handoff`] header.
pub const FD_MARKER: u8 = 0;

/// A server's first stdin line in v3, matching its flag ([`FirstLine`]). It holds **no secret**:
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

/// What every v3 first line names: the instance, the office and the rendezvous socket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawOfficeLine")]
pub struct OfficeLine {
    /// The instance the office belongs to (a ULID). The server stores it at create and refuses a
    /// `start` naming another; an office created before v3 adopts it on its first v3 `start`.
    pub instance_id: String,
    pub office_id: String,
    /// The absolute path of the per-spawn rendezvous socket: `<runtime>/r/<16 hex>`.
    pub rendezvous: String,
}

#[derive(Deserialize)]
struct RawOfficeLine {
    instance_id: String,
    office_id: String,
    rendezvous: String,
}

impl TryFrom<RawOfficeLine> for OfficeLine {
    type Error = &'static str;

    fn try_from(raw: RawOfficeLine) -> Result<Self, Self::Error> {
        let line = OfficeLine {
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
        if !is_plain_id(&self.instance_id) || !is_plain_id(&self.office_id) {
            return Err("the first line holds an id that isn't plain");
        }
        if !self.rendezvous.starts_with('/') || self.rendezvous.contains('\0') {
            return Err("the rendezvous path must be absolute, without NUL");
        }
        Ok(())
    }
}

/// A v3 create. As in v2, this version creates only HQ offices, and both ids and the
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
}

#[derive(Deserialize)]
struct RawCreateLine {
    instance_id: String,
    office_id: String,
    rendezvous: String,
    org_id: String,
    org_name: String,
    office_name: String,
    #[serde(default)]
    role: OfficeRole,
    owner_member_id: String,
}

impl TryFrom<RawCreateLine> for CreateLine {
    type Error = &'static str;

    fn try_from(raw: RawCreateLine) -> Result<Self, Self::Error> {
        let line = CreateLine {
            office: OfficeLine {
                instance_id: raw.instance_id,
                office_id: raw.office_id,
                rendezvous: raw.rendezvous,
            },
            org_id: raw.org_id,
            org_name: raw.org_name,
            office_name: raw.office_name,
            role: raw.role,
            owner_member_id: raw.owner_member_id,
        };
        line.check()?;
        Ok(line)
    }
}

impl CreateLine {
    fn check(&self) -> Result<(), &'static str> {
        self.office.check()?;
        if !is_plain_id(&self.org_id) {
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

fn check_v3(line: &[u8]) -> Result<(), LaunchError> {
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

/// Write a v3 first line, then flush. An unknown op, or a line that breaks its rules, is
/// refused and nothing is written.
pub fn write_first_line(w: &mut impl Write, line: &HostFirstLine) -> Result<(), LaunchError> {
    line.check()?;
    let body = Line {
        v: LAUNCH_VERSION,
        body: line,
    };
    frame::write_message(w, &body).map_err(LaunchError::Frame)
}

/// Read a server's v3 first line and check it matches the flag. `Ok(None)` means EOF before it
/// (the host is gone). Another `v` is [`LaunchError::UnsupportedVersion`]; an unknown op, or
/// another flag's op, is [`LaunchError::Invalid`]. The line is read like v2's secret lines and
/// zeroed afterwards, since a v2 host's line carries keys.
pub fn read_first_line(
    r: &mut impl BufRead,
    expected: FirstLine,
) -> Result<Option<HostFirstLine>, LaunchError> {
    let Some(mut line) = read_secret_line(r)? else {
        return Ok(None);
    };
    // The version first, so a line of any other version (a v2 line with its keys included) is
    // refused as a version without being decoded.
    let decoded = check_v3(&line)
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

/// A first line of either version, for the rollout (X-02 §5.4): move 2a's server accepts v2 and
/// v3 first lines until move 2b's host lands, and move 3 drops v2.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AnyFirstLine {
    /// A v2 line, with its app token and keys.
    V2(HostMessage),
    /// A v3 line, non-secret.
    V3(HostFirstLine),
}

/// Read a server's first line, v2 or v3, and check it matches the flag. One read of one line,
/// through a buffer reserved up front and zeroed on every path, as v2's reader does. Another `v`
/// is [`LaunchError::UnsupportedVersion`]; an unknown op, or another flag's op, is
/// [`LaunchError::Invalid`]. `Ok(None)` means EOF before it.
pub fn read_first_line_any(
    r: &mut impl BufRead,
    expected: FirstLine,
) -> Result<Option<AnyFirstLine>, LaunchError> {
    let Some(mut line) = read_secret_line(r)? else {
        return Ok(None);
    };
    let decoded = decode_any(&line);
    zero(&mut line);
    let message = decoded?;
    let matches = match &message {
        AnyFirstLine::V2(m) => matches!(
            (m, expected),
            (HostMessage::Create(_), FirstLine::Create)
                | (HostMessage::Start(_), FirstLine::Start)
                | (HostMessage::Discard(_), FirstLine::Discard)
        ),
        AnyFirstLine::V3(m) => matches!(
            (m, expected),
            (HostFirstLine::Create(_), FirstLine::Create)
                | (HostFirstLine::Start(_), FirstLine::Start)
                | (HostFirstLine::Discard(_), FirstLine::Discard)
        ),
    };
    if matches {
        Ok(Some(message))
    } else {
        Err(LaunchError::Invalid(
            "the first line isn't the op this server's flag expects",
        ))
    }
}

fn decode_any(line: &[u8]) -> Result<AnyFirstLine, LaunchError> {
    let version: Versioned = serde_json::from_slice(line).map_err(LaunchError::Json)?;
    match version.v {
        crate::LAUNCH_VERSION => serde_json::from_slice::<HostMessage>(line)
            .map(AnyFirstLine::V2)
            .map_err(LaunchError::Json),
        LAUNCH_VERSION => serde_json::from_slice::<HostFirstLine>(line)
            .map(AnyFirstLine::V3)
            .map_err(LaunchError::Json),
        other => Err(LaunchError::UnsupportedVersion {
            found: other,
            supported: &[crate::LAUNCH_VERSION, LAUNCH_VERSION],
        }),
    }
}

/// Write a worker event on the rendezvous connection, as a v3 line (`{"v": 3, "event": ...}`).
/// [`ServerEvent::Unknown`] and a created office that [`crate::OfficeFile::to_vec`] would refuse
/// are refused, and nothing is written. In v3 the host acts on `created` and `discarded` only
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

/// Read one v3 worker event from the rendezvous connection. `Ok(None)` means it closed. Another
/// `v` is [`LaunchError::UnsupportedVersion`]; an event this build doesn't know is
/// [`ServerEvent::Unknown`], which the host ignores.
pub fn read_server_event(r: &mut impl BufRead) -> Result<Option<ServerEvent>, LaunchError> {
    let Some(line) = frame::read_line(r).map_err(LaunchError::Frame)? else {
        return Ok(None);
    };
    check_v3(&line)?;
    let event: ServerEvent = serde_json::from_slice(&line).map_err(LaunchError::Json)?;
    if let ServerEvent::Created { office } = &event {
        office.to_vec()?;
    }
    Ok(Some(event))
}

/// The server's first line on the rendezvous connection: `{"v": 3, "event":
/// "ready_for_secrets"}`. The host accepts it only after its post-accept checks (it starts after
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
        }
        Wire {
            v: LAUNCH_VERSION,
            event: READY_FOR_SECRETS,
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for ReadyForSecrets {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            v: u32,
            event: String,
        }
        let wire = Wire::deserialize(d)?;
        if wire.v != LAUNCH_VERSION || wire.event != READY_FOR_SECRETS {
            return Err(serde::de::Error::custom("not a v3 ready_for_secrets"));
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
/// keys, naming the descriptors the following `sendmsg` carries. Its op matches the first
/// line's. Not `Clone`: it holds the keys. Encode it with [`encode_handoff`] and zero the bytes
/// after sending them.
///
/// It decodes as one flat object, not through serde's internally tagged enums, so each key goes
/// straight from the JSON into its secret type without an intermediate buffer. A key the op
/// doesn't carry is refused, even as `null`.
#[derive(Debug, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Handoff {
    /// After a create line (HQ): `fds` is `["transfer"]`.
    Create {
        db_key: DbKey,
        grant_key: GrantKey,
        recovery_code: RecoveryCode,
        fds: Vec<HandoffFd>,
    },
    /// After a start line: `fds` is `["transfer"]`.
    Start {
        db_key: DbKey,
        grant_key: GrantKey,
        fds: Vec<HandoffFd>,
    },
    /// After a discard line: the key, or `null` when the host has none (present either way, as
    /// in v2). `fds` is `[]`: no transfer socket, and no marker follows.
    Discard {
        db_key: Option<DbKey>,
        fds: Vec<HandoffFd>,
    },
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

/// A handoff as read: every field at the top level, each secret decoded in place.
#[derive(Deserialize)]
struct RawHandoff {
    op: HandoffOp,
    #[serde(default, deserialize_with = "present")]
    db_key: Option<Option<DbKey>>,
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
        match (raw.op, raw.db_key, raw.grant_key, raw.recovery_code) {
            (
                HandoffOp::Create,
                Some(Some(db_key)),
                Some(Some(grant_key)),
                Some(Some(recovery_code)),
            ) => Ok(Handoff::Create {
                db_key,
                grant_key,
                recovery_code,
                fds,
            }),
            (HandoffOp::Start, Some(Some(db_key)), Some(Some(grant_key)), None) => {
                Ok(Handoff::Start {
                    db_key,
                    grant_key,
                    fds,
                })
            }
            (HandoffOp::Discard, Some(db_key), None, None) => Ok(Handoff::Discard { db_key, fds }),
            _ => Err(D::Error::custom(
                "a handoff must carry exactly the keys of its op",
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
    check_v3(line)?;
    let handoff: Handoff = serde_json::from_slice(line).map_err(LaunchError::Json)?;
    handoff.check()?;
    Ok(handoff)
}

/// Which client a channel's descriptor serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    /// For the gateway, opened with `office.open_channel` on its control connection.
    Gateway,
    /// For `agentic-org` on `host.sock`.
    Local,
}

/// One record on a transfer socket (X-02 §5.2): a JSON header of at most
/// [`TRANSFER_HEADER_MAX_BYTES`], with exactly [`TransferRecord::fd_count`] descriptors. A record
/// of another kind, or with another descriptor count, is fatal: the receiver closes the transfer
/// socket (a worker exits and the host restarts it with a new key).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransferRecord {
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
            TransferRecord::Channel { .. } => 1,
            TransferRecord::Revoked { .. } => 0,
        }
    }
}

/// Encode a record's header. Over [`TRANSFER_HEADER_MAX_BYTES`] is refused
/// ([`FrameError::TooLargeToSend`]): split a long revocation into several records.
pub fn encode_transfer_record(record: &TransferRecord) -> Result<Vec<u8>, LaunchError> {
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
    serde_json::from_slice(bytes).map_err(LaunchError::Json)
}

#[cfg(test)]
mod tests;
