//! `docs.put_media`: image sanitization via `crate::attachments::media` plus tree write.

use base64::Engine as _;
use org_docs::project_tree::codecs::{DocActor, DocMeta, RawDocMedia};
use org_docs::project_tree::ops::{validate_summary, validate_title, wire_doc_media, RawTreeEdit};
use org_proto::docs::{
    DocMediaKindWire, DocsPutMediaParams, DocsPutMediaResult, DOC_MEDIA_MAX_BASE64_CHARS,
    DOC_MEDIA_MAX_BYTES,
};
use org_proto::ErrorCode;
use org_store::{Change, Id, Sha256, Timestamp};

use super::{map_tree_err, to_change, TreeViewAdapter};
use crate::attachments::media::{self, MediaType};
use crate::organization::records::time_text;
use crate::project_docs::access::{authorize, refused, Denied, DocsAccess};
use crate::project_docs::paths::{MediaFile, MEDIA_EXTENSIONS};
use crate::project_docs::ports::ProjectDocsRead;
use crate::project_docs::records::{media_type_of, MediaKind, MEDIA_RECORD_MAX_BYTES};

/// An asset checked and sanitized, ready to be decided on.
pub struct PreparedMedia {
    file: MediaFile,
    kind: MediaKind,
    media_type: &'static str,
    bytes: Vec<u8>,
}

impl std::fmt::Debug for PreparedMedia {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedMedia")
            .field("file", &self.file)
            .field("bytes", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

fn invalid(field: &str, rule: impl std::fmt::Display) -> Denied {
    refused(ErrorCode::InvalidRequest, format!("{field}: {rule}"))
}

fn too_large() -> Denied {
    refused(ErrorCode::TooLarge, "content_base64: at most 360 KiB")
}

/// Checks the file name, type and bytes of `params`, and sanitizes the image.
pub fn prepare(params: &DocsPutMediaParams) -> Result<PreparedMedia, Denied> {
    let file = MediaFile::parse(params.file_name.trim()).map_err(|e| invalid("file_name", e))?;
    let kind = match params.kind {
        DocMediaKindWire::Image => MediaKind::Image,
        DocMediaKindWire::Diagram => MediaKind::Diagram,
        DocMediaKindWire::Unknown => return Err(invalid("type", "one of: image diagram")),
    };
    let declared = MediaType::parse_declared(&params.media_type)?;
    let media_type = media_type_of(&file);
    if !declared.is_image() || declared.mime() != media_type {
        return Err(refused(
            ErrorCode::Unsupported,
            format!(
                "media_type: an asset is one of {} and matches its file name's extension",
                MEDIA_EXTENSIONS.join(", ")
            ),
        ));
    }
    if params.content_base64.len() > DOC_MEDIA_MAX_BASE64_CHARS {
        return Err(too_large());
    }
    let raw = base64::engine::general_purpose::STANDARD
        .decode(params.content_base64.as_bytes())
        .map_err(|_| invalid("content_base64", "standard base64"))?;
    if raw.len() > DOC_MEDIA_MAX_BYTES {
        return Err(too_large());
    }
    let kept = media::validate(declared, raw)?;
    if kept.media != declared {
        return Err(refused(
            ErrorCode::Unsupported,
            "an animated WebP is kept as a GIF: upload it as a .gif",
        ));
    }
    if kept.bytes.is_empty() || kept.bytes.len() as u64 > MEDIA_RECORD_MAX_BYTES {
        return Err(refused(
            ErrorCode::TooLarge,
            "the sanitized asset is over 360 KiB",
        ));
    }
    Ok(PreparedMedia {
        file,
        kind,
        media_type,
        bytes: kept.bytes,
    })
}

/// Decides `docs.put_media` for `caller` on `view`; `id` is a new asset's ID.
pub fn decide<R: ProjectDocsRead>(
    view: &R,
    now: Timestamp,
    caller: &str,
    params: &DocsPutMediaParams,
    prepared: &PreparedMedia,
    id: Id,
) -> Result<Change, Denied> {
    let cleared = authorize(view, caller, &params.project, DocsAccess::Write)?;
    let title = validate_title(&params.title).map_err(map_tree_err)?;
    let summary = validate_summary(params.summary.as_deref())
        .map_err(map_tree_err)?
        .unwrap_or_default();
    let mut edit = RawTreeEdit::load(&TreeViewAdapter(view), cleared.project.id.as_str())
        .map_err(map_tree_err)?;
    let actor = DocActor::Employee(cleared.caller.id.to_string());
    let ts = Some(time_text(now));
    let (id, meta) = match (edit.media_at(&prepared.file), params.expected_revision) {
        (Some(existing), Some(expected)) => {
            if existing.meta.revision != expected {
                return Err(refused(
                    ErrorCode::Conflict,
                    format!("expected revision {expected}, but the asset is at revision {}", existing.meta.revision),
                ));
            }
            let mut meta = existing.meta.clone();
            meta.revision += 1;
            meta.updated_at = ts;
            meta.updated_by = actor;
            (existing.id.clone(), meta)
        }
        (Some(existing), None) => {
            return Err(refused(
                ErrorCode::AlreadyExists,
                format!("file_name: an asset is already here at revision {}; pass expected_revision to replace it", existing.meta.revision),
            ))
        }
        (None, Some(_)) => return Err(refused(ErrorCode::NotFound, "media asset not found")),
        (None, None) => (
            id.to_string(),
            DocMeta {
                revision: 1,
                created_at: ts.clone(),
                created_by: actor.clone(),
                updated_at: ts,
                updated_by: actor,
            },
        ),
    };
    let record = RawDocMedia {
        id,
        project_id: cleared.project.id.to_string(),
        file: prepared.file.clone(),
        kind: prepared.kind,
        media_type: prepared.media_type.to_owned(),
        bytes: prepared.bytes.len() as u64,
        sha256: Sha256::of(&prepared.bytes).to_hex(),
        title,
        summary,
        meta,
    };
    let reply = serde_json::to_vec(&DocsPutMediaResult {
        media: wire_doc_media(&record),
    })
    .expect("a documentation reply encodes");
    edit.put_media(record, prepared.bytes.clone())
        .map_err(map_tree_err)?;
    to_change(edit.finish(reply).map_err(map_tree_err)?)
}
