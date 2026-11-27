//! Files on messages, asks and ask answers (`TASK-98.2`, ADR-0037).
//!
//! A `chat.send`, `msg.reply`, `chat.ask` or `chat.answer` names up to ten of the caller's
//! committed staged uploads. Before the journal commits, the office's attachment facade checks
//! each upload is the caller's (anyone else's is `not_found`), checks its size, and promotes its
//! sanitized bytes into the shared blob store ([`StagedFile`]). Then, inside the commit, the
//! decision binds them here: [`MessageFiles::bind`] asks [`AuthorizationPolicy`] whether the
//! caller may post each file into the conversation on the very snapshot the record commits on,
//! and records each under an attachment ID derived from the record and the upload, so an exact
//! retry (whose uploads are gone by then) is recognized by [`MessageFiles::bound_ids`].

use std::collections::BTreeSet;

use org_proto::attachments::{ArtifactStatus, AttachmentFilter, AttachmentMeta};
use org_proto::chat::ConversationKind;
use org_proto::ErrorCode;
use org_store::{Id, Refusal, Sha256};

use crate::authz::{
    AuthorizationPolicy, Denied, ParentFacts, PrincipalContext, ResourceFacts, ResourceRef,
};
use crate::error::{api_refusal as refusal, OfficeError};

use super::records::{ConversationRecord, MessageAttachmentRecord};

/// The scope a message's files are bound in.
pub const MESSAGE_SCOPE: &str = "message";
/// The scope an ask answer's files are bound in.
pub const ANSWER_SCOPE: &str = "ask_answer";

/// One committed staged upload of the caller's whose sanitized bytes are already in the shared
/// blob store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagedFile {
    pub upload_id: String,
    pub uploaded_by: String,
    pub content_sha256: String,
    pub media_type: String,
    pub byte_count: u64,
    pub filename: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub page_count: Option<u32>,
}

/// The files a message, ask or answer names, prepared before its commit.
#[derive(Clone, Debug, Default)]
pub struct MessageFiles {
    /// The caller as the policy sees them; set whenever a file is named.
    pub caller: Option<PrincipalContext>,
    /// The upload IDs the call named, in order.
    pub requested: Vec<String>,
    /// The promoted uploads in the same order; empty when some upload is not there for the
    /// caller (bound by an earlier commit of this very call, or never theirs).
    pub staged: Vec<StagedFile>,
}

/// Where a decision binds the files: one record, at one time, in one conversation.
pub struct Binding<'a> {
    pub organization_id: &'a Id,
    pub conversation: &'a ConversationRecord,
    /// Who may post into the conversation now (its effective members).
    pub members: &'a [Id],
    /// [`MESSAGE_SCOPE`] or [`ANSWER_SCOPE`].
    pub scope: &'static str,
    /// The message ID, or `<ask-id>:<answerer-id>` for an answer.
    pub key: &'a str,
    pub created_at: &'a str,
}

/// The conversation a record is being committed in, as the policy's only source of facts.
struct InFlight {
    conversation: String,
    members: BTreeSet<String>,
    open: bool,
}

impl ResourceFacts for InFlight {
    fn facts(&self, resource: &ResourceRef) -> Result<ParentFacts, OfficeError> {
        Ok(match resource {
            ResourceRef::Conversation(id) if *id == self.conversation => {
                ParentFacts::Conversation {
                    members: self.members.clone(),
                    open: self.open,
                }
            }
            _ => ParentFacts::Missing,
        })
    }
}

fn denied(denied: Denied) -> Refusal {
    match denied {
        Denied::NotFound => refusal(ErrorCode::NotFound, "attachment not found"),
        Denied::NotAuthorized(why) => refusal(ErrorCode::NotAuthorized, why),
        Denied::Storage(_) => refusal(ErrorCode::StorageError, "failed to check file access"),
    }
}

/// The attachment ID a named upload gets on a record: UUIDv7-shaped, its time the record's,
/// then the upload's position, so a record's files sort in the order they were named.
fn derive_id(
    organization_id: &Id,
    binding: (&str, &str),
    upload_id: &str,
    at_ms: u64,
    index: usize,
) -> String {
    let (scope, key) = binding;
    let hash = Sha256::of_parts(&[
        b"agentic-org/attachments/v1\0",
        organization_id.as_str().as_bytes(),
        b"\0",
        scope.as_bytes(),
        b"\0",
        key.as_bytes(),
        b"\0",
        upload_id.as_bytes(),
    ]);
    let mut bytes = [0u8; 16];
    bytes[0..6].copy_from_slice(&at_ms.to_be_bytes()[2..8]);
    bytes[6] = 0x70 | (index as u8 & 0x0f);
    bytes[7..16].copy_from_slice(&hash.as_bytes()[0..9]);
    bytes[8] = 0x80 | (bytes[8] & 0x3f);
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// Where the shared blob store files a digest.
fn content_ref(sha256: &str) -> String {
    format!("blobs/sha256/{}/{}/{sha256}", &sha256[0..2], &sha256[2..4])
}

/// A file's title when it is given none: its name, at most 160 characters.
fn default_title(filename: &str) -> String {
    filename
        .chars()
        .take(160)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// A message says something (text, files or both), names at most ten distinct uploads, and,
/// with files, has a canonical UUIDv7 ID their attachment IDs derive from (`TASK-98.2`).
pub fn check_message_files(id: &str, body: &str, attachment_ids: &[String]) -> Result<(), Refusal> {
    if !org_proto::attachments::has_content(body, attachment_ids) {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            "a message needs a body or at least one attachment",
        ));
    }
    org_proto::attachments::check_attachment_ids(attachment_ids)
        .map_err(|error| refusal(error.code, error.message))?;
    if !attachment_ids.is_empty() && !org_proto::is_uuid_v7(id) {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            "a message with attachments needs a canonical lowercase UUIDv7 id",
        ));
    }
    Ok(())
}

impl MessageFiles {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.requested.is_empty()
    }

    /// The attachment IDs the named uploads get on the record `key` committed at `created_at`.
    pub fn bound_ids(
        &self,
        organization_id: &Id,
        scope: &str,
        key: &str,
        created_at: &str,
    ) -> Vec<String> {
        let at_ms = crate::time::parse_rfc3339_ms(created_at).unwrap_or(0);
        self.requested
            .iter()
            .enumerate()
            .map(|(index, upload)| derive_id(organization_id, (scope, key), upload, at_ms, index))
            .collect()
    }

    /// Whether `existing` (a record this call may be retrying) carries exactly the files this
    /// call names.
    pub fn matches(
        &self,
        existing: &[MessageAttachmentRecord],
        organization_id: &Id,
        scope: &str,
        key: &str,
        created_at: &str,
    ) -> bool {
        let mut stored: Vec<&str> = existing.iter().map(|a| a.id.as_str()).collect();
        let mut expected = self.bound_ids(organization_id, scope, key, created_at);
        stored.sort_unstable();
        expected.sort_unstable();
        stored == expected
    }

    /// Binds every named upload into the record, as the policy allows the caller to post it
    /// into the conversation; an upload that is not the caller's is `not_found`.
    pub fn bind(&self, at: &Binding<'_>) -> Result<Vec<MessageAttachmentRecord>, Refusal> {
        if self.is_empty() {
            return Ok(Vec::new());
        }
        let Some(caller) = &self.caller else {
            return Err(refusal(ErrorCode::NotFound, "attachment not found"));
        };
        if self.staged.len() != self.requested.len() {
            return Err(refusal(ErrorCode::NotFound, "attachment not found"));
        }
        let facts = InFlight {
            conversation: at.conversation.id.to_string(),
            members: at.members.iter().map(Id::to_string).collect(),
            open: at.conversation.kind == ConversationKind::Channel,
        };
        let policy = AuthorizationPolicy::new(&facts);
        let target = ResourceRef::Conversation(facts.conversation.clone());
        let ids = self.bound_ids(at.organization_id, at.scope, at.key, at.created_at);
        let mut bound = Vec::with_capacity(self.staged.len());
        for (file, id) in self.staged.iter().zip(ids) {
            let source = ResourceRef::StagedUpload {
                id: file.upload_id.clone(),
                uploaded_by: file.uploaded_by.clone(),
            };
            policy
                .can_attach(caller, &source, &target)
                .map_err(denied)?;
            bound.push(MessageAttachmentRecord {
                id,
                content_ref: content_ref(&file.content_sha256),
                media_type: file.media_type.clone(),
                byte_count: file.byte_count,
                content_sha256: file.content_sha256.clone(),
                filename: Some(file.filename.clone()),
                title: Some(default_title(&file.filename)),
                description: None,
                artifact_status: ArtifactStatus::Final,
                width: file.width,
                height: file.height,
                page_count: file.page_count,
                uploaded_by_employee_id: Id::parse(&file.uploaded_by).ok(),
                created_at: Some(at.created_at.to_owned()),
            });
        }
        Ok(bound)
    }
}

/// What a reader of the record sees of one of its files. `created_at` and `uploaded_by` fall
/// back to the record's own when a file does not name them.
pub fn to_meta(
    file: &MessageAttachmentRecord,
    created_at: &str,
    uploaded_by: Option<&Id>,
) -> AttachmentMeta {
    let filename = file.filename.clone().unwrap_or_else(|| file.id.clone());
    AttachmentMeta {
        id: file.id.clone(),
        title: file
            .title
            .clone()
            .unwrap_or_else(|| default_title(&filename)),
        filename,
        description: file.description.clone(),
        artifact_status: file.artifact_status,
        mime_type: file.media_type.clone(),
        byte_count: file.byte_count,
        width: file.width,
        height: file.height,
        page_count: file.page_count,
        uploaded_by_employee_id: file
            .uploaded_by_employee_id
            .as_ref()
            .or(uploaded_by)
            .map(Id::to_string)
            .unwrap_or_default(),
        created_at: file
            .created_at
            .clone()
            .unwrap_or_else(|| created_at.to_owned()),
    }
}

/// Every file of a record as its readers see them.
pub fn metas(
    files: &[MessageAttachmentRecord],
    created_at: &str,
    uploaded_by: Option<&Id>,
) -> Vec<AttachmentMeta> {
    files
        .iter()
        .map(|f| to_meta(f, created_at, uploaded_by))
        .collect()
}

/// Whether a record with these files passes a `has:image` / `has:file` filter.
pub fn admits(filter: AttachmentFilter, files: &[MessageAttachmentRecord]) -> bool {
    filter.admits(files.iter().map(|f| f.media_type.as_str()))
}

#[cfg(test)]
mod tests;
