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

mod handoff;
mod rendezvous;
mod transfer;

pub use handoff::{
    decode_handoff, encode_create_handoff, encode_discard_handoff, encode_handoff,
    encode_join_handoff, encode_start_handoff, Handoff, HandoffFd,
};
pub use rendezvous::{
    read_server_event, write_rendezvous_event, write_server_event, ReadyForSecrets, RefusalReason,
    RendezvousEvent,
};
pub use transfer::{decode_transfer_record, encode_transfer_record, ChannelKind, TransferRecord};

/// The version of launch pipe lines and rendezvous lines.
pub const LAUNCH_VERSION: u32 = 1;

/// Matched host/worker channel ownership and closure contract (CH-LIFE-1).
pub const CHANNEL_PROFILE: &str = "office-channels/1";
/// Maximum ordinary physical channels, including pending transfers and public recovery (`TASK-140`).
pub const MAX_CHANNELS: usize = 32;
/// The web process's allowance leaves dedicated ordinary slots available for local callers.
pub const MAX_WEB_CHANNELS: usize = 24;

/// The largest transfer-record header, in bytes (a contract choice, X-02 §5.2).
pub const TRANSFER_HEADER_MAX_BYTES: usize = 1024;

/// The largest `--join` handoff header, in bytes: holds the office secrets key, grant key,
/// and one-time invitation token (`InvitationSecret`, up to 16 KiB including pinned CA DER).
pub const JOIN_HANDOFF_HEADER_MAX_BYTES: usize = 20 * 1024;

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

#[cfg(test)]
mod tests;
