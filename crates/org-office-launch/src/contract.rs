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
    read_secret_line, zero, ByteCounter, FirstLine, LaunchError, OfficeSecretsKey, ServerEvent,
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
    /// With [`crate::JOIN_FLAG`]: non-secret satellite enrollment metadata; the secret invitation
    /// token travels ONLY over the verified rendezvous [`Handoff::Join`].
    Join(JoinLine),
    /// With [`crate::OFFICE_FLAG`].
    Start(OfficeLine),
    /// With [`crate::DISCARD_FLAG`].
    Discard(OfficeLine),
    /// An op this build doesn't know: refused as a first line. Never written.
    #[serde(other, skip_serializing)]
    Unknown,
}

/// Non-secret satellite join/re-enroll first line (`doc-67` §Provisioning a satellite, D25).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawJoinLine")]
pub struct JoinLine {
    #[serde(flatten)]
    pub office: OfficeLine,
    pub invitation_id: String,
    pub org_id: String,
    pub org_name: String,
    pub office_name: String,
    pub hq_office_id: String,
    pub hq_addresses: Vec<String>,
    #[serde(default)]
    pub re_enroll: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawJoinLine {
    #[serde(default, rename = "v")]
    _version: Option<IgnoredAny>,
    instance_id: String,
    office_id: String,
    rendezvous: String,
    invitation_id: String,
    org_id: String,
    org_name: String,
    office_name: String,
    hq_office_id: String,
    hq_addresses: Vec<String>,
    #[serde(default)]
    re_enroll: bool,
}

impl TryFrom<RawJoinLine> for JoinLine {
    type Error = &'static str;

    fn try_from(raw: RawJoinLine) -> Result<Self, Self::Error> {
        let line = JoinLine {
            office: OfficeLine {
                instance_id: raw.instance_id,
                office_id: raw.office_id,
                rendezvous: raw.rendezvous,
            },
            invitation_id: raw.invitation_id,
            org_id: raw.org_id,
            org_name: raw.org_name,
            office_name: raw.office_name,
            hq_office_id: raw.hq_office_id,
            hq_addresses: raw.hq_addresses,
            re_enroll: raw.re_enroll,
        };
        line.check()?;
        Ok(line)
    }
}

impl JoinLine {
    fn check(&self) -> Result<(), &'static str> {
        self.office.check()?;
        if !org_proto::is_uuid_v7(&self.invitation_id)
            || !crate::is_selected_entity_id(&self.org_id)
            || !crate::is_selected_entity_id(&self.hq_office_id)
            || self.hq_addresses.is_empty()
        {
            return Err("join line holds invalid organization, invitation, or HQ metadata");
        }
        if org_proto::OrganizationName::parse(&self.org_name).is_err()
            || org_proto::OrganizationName::parse(&self.office_name).is_err()
        {
            return Err("join line holds an invalid organization or office name");
        }
        Ok(())
    }
}

/// Redacted, non-`Clone` satellite invitation token secret carried ONLY in [`Handoff::Join`].
pub struct InvitationSecret(String);

impl std::fmt::Debug for InvitationSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InvitationSecret(<redacted>)")
    }
}

impl Drop for InvitationSecret {
    fn drop(&mut self) {
        // SAFETY-free volatile zeroing via crate::zero on the UTF-8 buffer before drop.
        let bytes = std::mem::take(&mut self.0).into_bytes();
        let mut buf = bytes;
        zero(&mut buf);
    }
}

impl InvitationSecret {
    pub fn new(token: String) -> Result<Self, &'static str> {
        let trimmed = token.trim();
        if trimmed.is_empty() || trimmed.len() > 16 * 1024 {
            return Err("invalid invitation secret length");
        }
        Ok(Self(trimmed.to_owned()))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl Serialize for InvitationSecret {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}

impl<'de> Deserialize<'de> for InvitationSecret {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Self::new(raw).map_err(serde::de::Error::custom)
    }
}

/// What every launch first line names: the instance, the office and the rendezvous socket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawOfficeLine")]
pub struct OfficeLine {
    /// The instance the office belongs to: canonical lowercase UUIDv7 text (a ULID is refused).
    /// The server stores it at create and refuses a `start` naming another.
    pub instance_id: String,
    /// Canonical lowercase UUIDv7 text.
    pub office_id: String,
    /// The absolute path of the per-spawn rendezvous socket: `<runtime>/r/<16 hex>`.
    pub rendezvous: String,
}

/// A first line refuses a field it doesn't define (a v2 key such as `db_key` included); `v` is the
/// line's own, checked before the decode.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOfficeLine {
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
        if !org_proto::is_uuid_v7(&self.instance_id) {
            return Err("the first line's instance id isn't a canonical lowercase UUIDv7");
        }
        if !crate::is_selected_entity_id(&self.office_id) {
            return Err("the first line's office id isn't a canonical lowercase UUIDv7");
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
    /// The protected HQ creation (REC-ESC-1) with its bootstrap: every create carries it.
    pub hq_creation: HqCreationV1,
}

/// Canonical summary of local-only data that never lives in organization Git (`doc-91` Loss
/// Boundaries) and therefore does not come back when opening an organization from Git alone.
pub const LOCAL_ONLY_NOT_RESTORED: &[&str] = &[
    "Browser accounts and sign-in sessions (other people must be invited again)",
    "Chat conversations, channels, and direct messages (protected local messaging store)",
    "Usage and cost accounting ledger (protected local accounting store)",
    "Budget policies, periods, and holds (protected local budget store)",
    "Private task notes (HQ-only private store)",
    "Operation receipts and office acceptance journals (protected local recovery store)",
    "Previous HQ certificate authority private key (a new HQ CA was generated; other offices must re-enroll)",
];

/// Optional Git source for opening an existing organization (`TASK-39` / R1) into a new HQ.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportSourceV1 {
    pub repository_remote_url: String,
    pub state_branch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_oid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_owner_person: Option<org_proto::selected_bootstrap::CreatorV1>,
}

/// Request written on stdin for `agentic-org-office --inspect-import`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InspectImportRequest {
    pub repository_remote_url: String,
    pub state_branch: String,
}

/// Active person summary returned by `agentic-org-office --inspect-import`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InspectImportPerson {
    pub id: String,
    pub name: String,
    pub handle: String,
    pub home_team_id: String,
}

/// Preview payload returned when `--inspect-import` validates an organization repository cleanly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InspectImportPreview {
    pub organization_id: String,
    pub name: String,
    pub slug: String,
    pub repository_remote_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_remote_url: Option<String>,
    pub state_branch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_oid: Option<String>,
    pub teams_count: usize,
    pub projects_count: usize,
    pub agents_count: usize,
    pub people: Vec<InspectImportPerson>,
    #[serde(default)]
    pub reserved_handles: Vec<String>,
    #[serde(default)]
    pub reserved_entity_ids: Vec<String>,
    #[serde(default)]
    pub has_active_root_team: bool,
    pub local_only_not_restored: Vec<String>,
}

/// One-line JSON response printed on stdout by `agentic-org-office --inspect-import`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum InspectImportResponse {
    Ok {
        preview: Box<InspectImportPreview>,
    },
    Error {
        code: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        reason: String,
    },
}

/// The protected creation context of an HQ create, version 2. It names no path: the
/// organization's data roots travel only in the writer admission
/// ([`crate::writer::Admission`]), which the host sends before the worker creates anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HqCreationV1 {
    pub version: u32,
    pub operation_id: String,
    pub escrow_profile: u32,
    /// The prepared organization: every creation carries it.
    pub bootstrap: Box<org_proto::selected_bootstrap::PreparedV1>,
    /// Present only when opening an existing organization from Git files (`TASK-39` / R1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import_source: Option<Box<ImportSourceV1>>,
}

impl HqCreationV1 {
    /// The only version: 2 (version 1 carried a public root).
    pub const VERSION: u32 = 2;

    fn check(&self) -> Result<(), &'static str> {
        if !self.bootstrap.valid()
            || self.version != Self::VERSION
            || self.escrow_profile != 1
            || !org_proto::is_uuid_v7(&self.operation_id)
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
    hq_creation: HqCreationV1,
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
        if !crate::is_selected_entity_id(&self.org_id) {
            return Err("the create line's organization id isn't a canonical lowercase UUIDv7");
        }
        if !org_proto::is_valid_member_id(&self.owner_member_id) {
            return Err("the create line's owner_member_id isn't a member id");
        }
        if self.role != OfficeRole::Hq {
            return Err("this version creates only HQ offices");
        }
        Ok(())
    }

    /// Every create is a protected HQ creation with its bootstrap (the only create the host
    /// reserves writer custody for), and the bootstrap names this line's office, organization
    /// and owner.
    fn check_bootstrap_context(&self) -> Result<(), &'static str> {
        let context = &self.hq_creation;
        context.check()?;
        let b = &context.bootstrap;
        if b.organization_id != self.org_id
            || b.office_id != self.office.office_id
            || b.plan.name != self.org_name
            || b.creator.employee_id != self.owner_member_id
        {
            return Err("selected bootstrap scope mismatch");
        }
        Ok(())
    }
}

impl HostFirstLine {
    fn check(&self) -> Result<(), LaunchError> {
        match self {
            HostFirstLine::Create(create) => create.check(),
            HostFirstLine::Join(join) => join.check(),
            HostFirstLine::Start(line) | HostFirstLine::Discard(line) => line.check(),
            HostFirstLine::Unknown => Err("can't send an unknown op"),
        }
        .map_err(LaunchError::Invalid)
    }

    /// The office this line names.
    pub fn office(&self) -> Option<&OfficeLine> {
        match self {
            HostFirstLine::Create(create) => Some(&create.office),
            HostFirstLine::Join(join) => Some(&join.office),
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
        | (HostFirstLine::Join(_), FirstLine::Join)
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
    fn into_event(self) -> RendezvousEvent {
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
    /// After a join line (satellite): the office's secrets key, grant key, and one-time
    /// invitation secret (never in argv or first line). `fds` is `["transfer"]`.
    Join {
        office_secrets_key: OfficeSecretsKey,
        grant_key: GrantKey,
        invitation: InvitationSecret,
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
    Join,
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
    #[serde(default, deserialize_with = "present")]
    invitation: Option<Option<InvitationSecret>>,
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
            raw.invitation,
        ) {
            (
                HandoffOp::Create,
                Some(Some(office_secrets_key)),
                Some(Some(grant_key)),
                recovery_code @ (None | Some(Some(_))),
                None,
            ) => Ok(Handoff::Create {
                office_secrets_key,
                grant_key,
                recovery_code: recovery_code.flatten(),
                fds,
            }),
            (
                HandoffOp::Join,
                Some(Some(office_secrets_key)),
                Some(Some(grant_key)),
                None,
                Some(Some(invitation)),
            ) => Ok(Handoff::Join {
                office_secrets_key,
                grant_key,
                invitation,
                fds,
            }),
            (HandoffOp::Start, Some(office_secrets_key), Some(Some(grant_key)), None, None) => {
                Ok(Handoff::Start {
                    office_secrets_key,
                    grant_key,
                    fds,
                })
            }
            (HandoffOp::Discard, None, None, None, None) => Ok(Handoff::Discard { fds }),
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
            Handoff::Create { .. } | Handoff::Join { .. } | Handoff::Start { .. } => {
                &[HandoffFd::Transfer]
            }
            Handoff::Discard { .. } => &[],
        }
    }

    /// The descriptors it names.
    pub fn fds(&self) -> &[HandoffFd] {
        match self {
            Handoff::Create { fds, .. }
            | Handoff::Join { fds, .. }
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

/// Borrow host-held secrets for a satellite join handoff; caller zeroes the returned line.
pub fn encode_join_handoff(
    office_secrets_key: &OfficeSecretsKey,
    grant_key: &GrantKey,
    invitation: &InvitationSecret,
) -> Result<Vec<u8>, LaunchError> {
    #[derive(Serialize)]
    struct Join<'a> {
        op: &'static str,
        office_secrets_key: &'a OfficeSecretsKey,
        grant_key: &'a GrantKey,
        invitation: &'a InvitationSecret,
        fds: [HandoffFd; 1],
    }
    encode_secret(&Join {
        op: "join",
        office_secrets_key,
        grant_key,
        invitation,
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
