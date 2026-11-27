//! Canonical durable JSON envelopes, path layout, and `canonical-json/2` content hashing
//! for the messaging family (`doc-90`, `TASK-10`, `ADR-0024`).
//!
//! Path layout under `<data-root>/<organization-id>/durable/messaging/`:
//! - `messaging/conversations/<conversation-id>/conversation.json`
//! - `messaging/conversations/<conversation-id>/messages/<message-id>.json`
//! - `messaging/conversations/<conversation-id>/participants/<employee-id>/state.json`
//! - `messaging/conversations/<conversation-id>/participants/<employee-id>/received/snapshot.json`
//! - `messaging/conversations/<conversation-id>/participants/<employee-id>/received/segments/<first-seq>.jsonl`
//! - `messaging/conversations/<conversation-id>/participants/<employee-id>/reactions/<message-id>.json`
//! - `messaging/conversations/<conversation-id>/participants/<employee-id>/answers/<message-id>.json`

use std::collections::BTreeMap;

use org_proto::attachments::ArtifactStatus;
use org_proto::chat::{AutoScope, ConversationKind, DeliveredBecause, MuteMode, ReactionKind};
use org_proto::msg::{MessageLifecycleState, MessagePriority, Meta};
use org_store::{DurablePath, Id, Sha256};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::organization::records::yaml::{canonical_json, RecordError};

/// Schema version written to all durable messaging JSON envelopes (`doc-90`).
pub const SCHEMA_VERSION: u32 = 1;

/// Sender kind for an immutable message record (`doc-90`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SenderKind {
    Employee,
    Office,
}

impl SenderKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Employee => "employee",
            Self::Office => "office",
        }
    }
}

pub fn conversation_kind_str(kind: ConversationKind) -> &'static str {
    match kind {
        ConversationKind::Dm => "dm",
        ConversationKind::GroupDm => "group_dm",
        ConversationKind::Channel => "channel",
        _ => "channel",
    }
}

pub fn parse_conversation_kind(raw: &str) -> Result<ConversationKind, RecordError> {
    match raw {
        "dm" => Ok(ConversationKind::Dm),
        "group_dm" => Ok(ConversationKind::GroupDm),
        "channel" => Ok(ConversationKind::Channel),
        other => Err(RecordError::new(
            "kind",
            format!("invalid conversation kind {other:?}"),
        )),
    }
}

pub fn auto_scope_str(scope: AutoScope) -> &'static str {
    match scope {
        AutoScope::Organization => "organization",
        AutoScope::Team => "team",
        AutoScope::Project => "project",
        _ => "organization",
    }
}

pub fn parse_auto_scope(raw: &str) -> Result<AutoScope, RecordError> {
    match raw {
        "organization" => Ok(AutoScope::Organization),
        "team" => Ok(AutoScope::Team),
        "project" => Ok(AutoScope::Project),
        other => Err(RecordError::new(
            "auto_scope",
            format!("invalid auto_scope {other:?}"),
        )),
    }
}

pub fn priority_str(priority: MessagePriority) -> &'static str {
    match priority {
        MessagePriority::Normal => "normal",
        MessagePriority::High => "high",
        MessagePriority::Urgent => "urgent",
        _ => "normal",
    }
}

pub fn parse_priority(raw: &str) -> Result<MessagePriority, RecordError> {
    match raw {
        "normal" => Ok(MessagePriority::Normal),
        "high" => Ok(MessagePriority::High),
        "urgent" => Ok(MessagePriority::Urgent),
        other => Err(RecordError::new(
            "priority",
            format!("invalid priority {other:?}"),
        )),
    }
}

pub fn lifecycle_str(state: MessageLifecycleState) -> &'static str {
    match state {
        MessageLifecycleState::Unread => "unread",
        MessageLifecycleState::Snoozed => "snoozed",
        MessageLifecycleState::Read => "read",
        MessageLifecycleState::Done => "done",
        MessageLifecycleState::Expired => "expired",
        MessageLifecycleState::DeadLetter => "dead_letter",
        _ => "unread",
    }
}

pub fn parse_lifecycle(raw: &str) -> Result<MessageLifecycleState, RecordError> {
    match raw {
        "unread" => Ok(MessageLifecycleState::Unread),
        "snoozed" => Ok(MessageLifecycleState::Snoozed),
        "read" => Ok(MessageLifecycleState::Read),
        "done" => Ok(MessageLifecycleState::Done),
        "expired" => Ok(MessageLifecycleState::Expired),
        "dead_letter" => Ok(MessageLifecycleState::DeadLetter),
        other => Err(RecordError::new(
            "state",
            format!("invalid lifecycle state {other:?}"),
        )),
    }
}

pub fn delivered_because_str(because: DeliveredBecause) -> &'static str {
    match because {
        DeliveredBecause::Addressed => "addressed",
        DeliveredBecause::ThreadReply => "thread_reply",
        DeliveredBecause::Mention => "mention",
        DeliveredBecause::AnswerCandidate => "answer_candidate",
        _ => "addressed",
    }
}

pub fn parse_delivered_because(raw: &str) -> Result<DeliveredBecause, RecordError> {
    match raw {
        "addressed" => Ok(DeliveredBecause::Addressed),
        "thread_reply" => Ok(DeliveredBecause::ThreadReply),
        "mention" => Ok(DeliveredBecause::Mention),
        "answer_candidate" => Ok(DeliveredBecause::AnswerCandidate),
        other => Err(RecordError::new(
            "delivered_because",
            format!("invalid delivered_because {other:?}"),
        )),
    }
}

pub fn mute_mode_str(mode: MuteMode) -> &'static str {
    match mode {
        MuteMode::None => "none",
        MuteMode::Muted => "muted",
        MuteMode::Digest => "digest",
        _ => "none",
    }
}

pub fn parse_mute_mode(raw: &str) -> Result<MuteMode, RecordError> {
    match raw {
        "none" => Ok(MuteMode::None),
        "muted" => Ok(MuteMode::Muted),
        "digest" => Ok(MuteMode::Digest),
        other => Err(RecordError::new(
            "mute_mode",
            format!("invalid mute_mode {other:?}"),
        )),
    }
}

pub fn reaction_str(kind: ReactionKind) -> &'static str {
    match kind {
        ReactionKind::Seen => "seen",
        ReactionKind::Looking => "looking",
        _ => "seen",
    }
}

pub fn parse_reaction(raw: &str) -> Result<ReactionKind, RecordError> {
    match raw {
        "seen" => Ok(ReactionKind::Seen),
        "looking" => Ok(ReactionKind::Looking),
        other => Err(RecordError::new(
            "reaction",
            format!("invalid reaction {other:?}"),
        )),
    }
}

/// Embedded membership entry in `conversation.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationMembership {
    pub employee_id: Id,
    pub joined_at: String,
    #[serde(default)]
    pub left_at: Option<String>,
}

/// Embedded pin entry in `conversation.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationPin {
    pub message_id: String,
    pub pinned_by: Id,
    pub pinned_at: String,
}

/// Durable `conversation.json` envelope (`doc-90`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationRecord {
    pub schema_version: u32,
    pub revision: u64,
    pub organization_id: Id,
    pub id: Id,
    pub kind: ConversationKind,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub auto_scope: Option<AutoScope>,
    #[serde(default)]
    pub team_id: Option<Id>,
    #[serde(default)]
    pub project_id: Option<Id>,
    pub archived: bool,
    pub created_at: String,
    #[serde(default)]
    pub memberships: Vec<ConversationMembership>,
    #[serde(default)]
    pub pins: Vec<ConversationPin>,
}

/// Immutable attachment metadata embedded in a message file, or in a participant's answer
/// file (`doc-90`, `TASK-98.2`). The bytes are the shared blob `content_ref` names
/// (`blobs/sha256/<aa>/<bb>/<sha256>`); `id` is the binding's own attachment ID.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageAttachmentRecord {
    pub id: String,
    pub content_ref: String,
    pub media_type: String,
    pub byte_count: u64,
    pub content_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub artifact_status: ArtifactStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uploaded_by_employee_id: Option<Id>,
    /// When the file was bound; a message's files share the message's `created_at`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

/// Immutable `messages/<message-id>.json` envelope (`doc-90`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageRecord {
    pub schema_version: u32,
    pub organization_id: Id,
    /// Message ID: canonical lowercase UUIDv7 (or legacy ULID in conformance tests).
    pub id: String,
    pub conversation_id: Id,
    pub sender_kind: SenderKind,
    #[serde(default)]
    pub sender_employee_id: Option<Id>,
    #[serde(default)]
    pub sender_office_id: Option<Id>,
    pub sender_display_name: String,
    pub kind: String,
    pub priority: MessagePriority,
    #[serde(default)]
    pub subject: Option<String>,
    pub body_markdown: String,
    #[serde(default)]
    pub metadata: Meta,
    #[serde(default)]
    pub thread_root_id: Option<String>,
    #[serde(default)]
    pub reply_to_message_id: Option<String>,
    #[serde(default)]
    pub also_in_channel: bool,
    pub created_at: String,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub ttl_ms: Option<u64>,
    pub origin_office_id: Id,
    pub origin_incarnation: String,
    pub origin_sequence: u64,
    pub org_epoch: u64,
    pub org_sequence: u64,
    pub content_sha256: String,
    #[serde(default)]
    pub attachments: Vec<MessageAttachmentRecord>,
    /// Legacy `org-mailbox` content hash stored alongside `content_sha256` for exact retry checks.
    #[serde(default)]
    pub mailbox_content_hash: Option<String>,
}

impl MessageRecord {
    /// Computes the `doc-90` `canonical-json/2` SHA-256 hex digest over the 23 immutable
    /// message columns (including nullable keys as `null`).
    pub fn compute_content_sha256(&self) -> String {
        let metadata_obj = Value::Object(
            self.metadata
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        );
        let canonical = json!({
            "organization_id": self.organization_id.as_str(),
            "id": self.id,
            "conversation_id": self.conversation_id.as_str(),
            "sender_kind": self.sender_kind.as_str(),
            "sender_employee_id": self.sender_employee_id.as_ref().map(Id::as_str),
            "sender_office_id": self.sender_office_id.as_ref().map(Id::as_str),
            "sender_display_name": self.sender_display_name,
            "kind": self.kind,
            "priority": priority_str(self.priority),
            "subject": self.subject,
            "body_markdown": self.body_markdown,
            "metadata_json": metadata_obj,
            "thread_root_id": self.thread_root_id,
            "reply_to_message_id": self.reply_to_message_id,
            "also_in_channel": self.also_in_channel,
            "created_at": self.created_at,
            "expires_at": self.expires_at,
            "ttl_ms": self.ttl_ms,
            "origin_office_id": self.origin_office_id.as_str(),
            "origin_incarnation": self.origin_incarnation,
            "origin_sequence": self.origin_sequence,
            "org_epoch": self.org_epoch,
            "org_sequence": self.org_sequence,
        });
        Sha256::of(canonical_json(&canonical).as_bytes()).to_hex()
    }
}

/// One per-employee recipient lifecycle row (`doc-90` `message_recipient_states`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipientStateRecord {
    pub organization_id: Id,
    pub message_id: String,
    pub conversation_id: Id,
    pub employee_id: Id,
    #[serde(default)]
    pub recipient_display_name: Option<String>,
    pub state: MessageLifecycleState,
    pub triage_generation: u64,
    #[serde(default)]
    pub snoozed_until: Option<String>,
    #[serde(default)]
    pub snooze_after_task: bool,
    #[serde(default)]
    pub snooze_note: Option<String>,
    #[serde(default)]
    pub triaged_at: Option<String>,
    #[serde(default)]
    pub read_at: Option<String>,
    pub updated_at: String,
    #[serde(default)]
    pub reason: Option<String>,
    pub delivered_because: DeliveredBecause,
    #[serde(default)]
    pub related_message_ids: Vec<String>,
    #[serde(default)]
    pub expired_waiting_person: bool,
    pub owner_office_id: Id,
    pub origin_incarnation: String,
    pub origin_sequence: u64,
    pub org_epoch: u64,
    pub org_sequence: u64,
}

/// Compacted participant recipient-state snapshot (`received/snapshot.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParticipantReceivedSnapshot {
    pub schema_version: u32,
    pub revision: u64,
    pub organization_id: Id,
    pub conversation_id: Id,
    pub employee_id: Id,
    pub states: BTreeMap<String, RecipientStateRecord>,
}

/// One mute preference in `participants/<employee-id>/state.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationPreferenceRecord {
    pub muted_employee_id: Id,
    pub mute_mode: MuteMode,
}

/// Read marker in `participants/<employee-id>/state.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationReadMarkerRecord {
    pub organization_id: Id,
    pub conversation_id: Id,
    pub viewer_employee_id: Id,
    #[serde(default)]
    pub read_at: Option<String>,
    #[serde(default)]
    pub last_read_message_id: Option<String>,
}

/// Per-participant preferences & read markers (`participants/<employee-id>/state.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParticipantStateFile {
    pub schema_version: u32,
    pub revision: u64,
    pub organization_id: Id,
    pub conversation_id: Id,
    pub employee_id: Id,
    #[serde(default)]
    pub preferences: Vec<ConversationPreferenceRecord>,
    #[serde(default)]
    pub read_marker: Option<ConversationReadMarkerRecord>,
}

/// One reaction row (`doc-90` `message_reactions`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageReactionRecord {
    pub organization_id: Id,
    pub message_id: String,
    pub employee_id: Id,
    pub reaction: ReactionKind,
    pub created_at: String,
}

/// Per-employee reactions file (`participants/<employee-id>/reactions/<message-id>.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParticipantReactionsFile {
    pub schema_version: u32,
    pub revision: u64,
    pub organization_id: Id,
    pub conversation_id: Id,
    pub employee_id: Id,
    pub message_id: String,
    #[serde(default)]
    pub reactions: Vec<MessageReactionRecord>,
}

/// Per-employee answer/handoff acceptance file (`participants/<employee-id>/answers/<message-id>.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParticipantAnswerFile {
    pub schema_version: u32,
    pub revision: u64,
    pub organization_id: Id,
    pub conversation_id: Id,
    pub employee_id: Id,
    pub message_id: String,
    #[serde(default)]
    pub accepted_by: Option<Id>,
    #[serde(default)]
    pub accepted_at: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub answer: Option<Value>,
    /// The answer's files (`TASK-98.2`), projected into `message_answer_attachments`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<MessageAttachmentRecord>,
}

/// Classification of a durable path owned by `MessagingFiles`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Conversation(Id),
    Message(Id, String),
    ParticipantState(Id, Id),
    ParticipantReceivedSnapshot(Id, Id),
    ParticipantReceivedSegment(Id, Id, u64),
    ParticipantReactions(Id, Id, String),
    ParticipantAnswer(Id, Id, String),
}

pub fn conversation_path(conversation_id: &Id) -> DurablePath {
    DurablePath::parse(&format!(
        "messaging/conversations/{conversation_id}/conversation.json"
    ))
    .expect("valid conversation path")
}

pub fn message_path(conversation_id: &Id, message_id: &str) -> DurablePath {
    DurablePath::parse(&format!(
        "messaging/conversations/{conversation_id}/messages/{message_id}.json"
    ))
    .expect("valid message path")
}

pub fn participant_state_path(conversation_id: &Id, employee_id: &Id) -> DurablePath {
    DurablePath::parse(&format!(
        "messaging/conversations/{conversation_id}/participants/{employee_id}/state.json"
    ))
    .expect("valid participant state path")
}

pub fn participant_received_snapshot_path(conversation_id: &Id, employee_id: &Id) -> DurablePath {
    DurablePath::parse(&format!(
        "messaging/conversations/{conversation_id}/participants/{employee_id}/received/snapshot.json"
    ))
    .expect("valid participant received snapshot path")
}

pub fn participant_received_segment_path(
    conversation_id: &Id,
    employee_id: &Id,
    first_seq: u64,
) -> DurablePath {
    DurablePath::parse(&format!(
        "messaging/conversations/{conversation_id}/participants/{employee_id}/received/segments/{first_seq}.jsonl"
    ))
    .expect("valid participant received segment path")
}

pub fn participant_reactions_path(
    conversation_id: &Id,
    employee_id: &Id,
    message_id: &str,
) -> DurablePath {
    DurablePath::parse(&format!(
        "messaging/conversations/{conversation_id}/participants/{employee_id}/reactions/{message_id}.json"
    ))
    .expect("valid participant reactions path")
}

pub fn participant_answer_path(
    conversation_id: &Id,
    employee_id: &Id,
    message_id: &str,
) -> DurablePath {
    DurablePath::parse(&format!(
        "messaging/conversations/{conversation_id}/participants/{employee_id}/answers/{message_id}.json"
    ))
    .expect("valid participant answer path")
}

/// Identifies a path owned by the messaging projector (`durable/messaging/...`).
pub fn kind_of(path: &DurablePath) -> Option<Kind> {
    let parts: Vec<&str> = path.as_str().split('/').collect();
    if parts.len() < 4 || parts[0] != "messaging" || parts[1] != "conversations" {
        return None;
    }
    let conversation_id = Id::parse(parts[2]).ok()?;
    match parts.as_slice() {
        [_, _, _, "conversation.json"] => Some(Kind::Conversation(conversation_id)),
        [_, _, _, "messages", file] => {
            let msg_id = file.strip_suffix(".json")?;
            (!msg_id.is_empty()).then(|| Kind::Message(conversation_id, msg_id.to_owned()))
        }
        [_, _, _, "participants", emp, "state.json"] => {
            let emp_id = Id::parse(emp).ok()?;
            Some(Kind::ParticipantState(conversation_id, emp_id))
        }
        [_, _, _, "participants", emp, "received", "snapshot.json"] => {
            let emp_id = Id::parse(emp).ok()?;
            Some(Kind::ParticipantReceivedSnapshot(conversation_id, emp_id))
        }
        [_, _, _, "participants", emp, "received", "segments", file] => {
            let emp_id = Id::parse(emp).ok()?;
            let first = file.strip_suffix(".jsonl")?.parse::<u64>().ok()?;
            Some(Kind::ParticipantReceivedSegment(
                conversation_id,
                emp_id,
                first,
            ))
        }
        [_, _, _, "participants", emp, "reactions", file] => {
            let emp_id = Id::parse(emp).ok()?;
            let msg_id = file.strip_suffix(".json")?;
            (!msg_id.is_empty())
                .then(|| Kind::ParticipantReactions(conversation_id, emp_id, msg_id.to_owned()))
        }
        [_, _, _, "participants", emp, "answers", file] => {
            let emp_id = Id::parse(emp).ok()?;
            let msg_id = file.strip_suffix(".json")?;
            (!msg_id.is_empty())
                .then(|| Kind::ParticipantAnswer(conversation_id, emp_id, msg_id.to_owned()))
        }
        _ => None,
    }
}

/// Deterministic canonical lowercase UUIDv7 for a 2-party DM or auto-scoped channel so
/// disconnected offices or lazy auto-creation converge on the exact same conversation ID.
pub fn derive_conversation_id(domain: &str, key: &str) -> Id {
    let hash = Sha256::of_parts(&[
        b"agentic-org/messaging/v1\0",
        domain.as_bytes(),
        b"\0",
        key.as_bytes(),
    ]);
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hash.as_bytes()[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let text = format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11],
        bytes[12], bytes[13], bytes[14], bytes[15],
    );
    Id::parse(&text).expect("derived UUIDv7 is valid")
}

/// Deterministic DM conversation ID for an unordered pair of employees within an organization.
pub fn dm_conversation_id(organization_id: &Id, a: &Id, b: &Id) -> Id {
    let (lo, hi) = if a.as_str() <= b.as_str() {
        (a.as_str(), b.as_str())
    } else {
        (b.as_str(), a.as_str())
    };
    derive_conversation_id("dm", &format!("{organization_id}:{lo}:{hi}"))
}

pub fn encode_json<T: Serialize>(value: &T) -> Vec<u8> {
    let val = serde_json::to_value(value).expect("messaging record serializes");
    let mut out = canonical_json(&val);
    out.push('\n');
    out.into_bytes()
}

pub fn decode_json<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, RecordError> {
    serde_json::from_slice(bytes)
        .map_err(|err| RecordError::new("", format!("invalid messaging JSON: {err}")))
}

pub fn encode_jsonl(records: &[RecipientStateRecord]) -> Vec<u8> {
    let mut out = String::new();
    for rec in records {
        let val = serde_json::to_value(rec).expect("recipient state serializes");
        out.push_str(&canonical_json(&val));
        out.push('\n');
    }
    out.into_bytes()
}

pub fn decode_jsonl(bytes: &[u8]) -> Result<Vec<RecipientStateRecord>, RecordError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| RecordError::new("", "recipient segment is not UTF-8"))?;
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let rec: RecipientStateRecord = serde_json::from_str(trimmed)
            .map_err(|err| RecordError::new("", format!("invalid recipient state line: {err}")))?;
        out.push(rec);
    }
    Ok(out)
}
