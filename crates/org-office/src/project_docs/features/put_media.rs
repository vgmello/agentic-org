//! `docs.put_media`: one image asset under `docs/media/`, its metadata sidecar and the
//! regenerated index, in one entry.
//!
//! The bytes go through the attachments' media choke-point
//! (`crate::attachments::media::validate`): only PNG, JPEG, WebP and GIF whose magic bytes
//! match, decoded and re-encoded so EXIF, GPS and every other metadata block is dropped. SVG,
//! HTML and anything else is refused. [`prepare`] does that work before the write is
//! submitted, so the writer only decides.

use base64::Engine as _;
use org_proto::docs::{
    DocMediaKindWire, DocsPutMediaParams, DocsPutMediaResult, DOC_MEDIA_MAX_BASE64_CHARS,
    DOC_MEDIA_MAX_BYTES,
};
use org_proto::ErrorCode;
use org_store::{Change, Id, Sha256, Timestamp};

use super::change::TreeEdit;
use super::{fields, reply, wire};
use crate::attachments::media::{self, MediaType};
use crate::organization::records::{time_text, By, Meta};
use crate::project_docs::access::{authorize, refused, Denied, DocsAccess};
use crate::project_docs::paths::{MediaFile, MEDIA_EXTENSIONS};
use crate::project_docs::ports::ProjectDocsRead;
use crate::project_docs::records::{media_type_of, DocMedia, MediaKind, MEDIA_RECORD_MAX_BYTES};

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

fn too_large() -> Denied {
    refused(ErrorCode::TooLarge, "content_base64: at most 360 KiB")
}

/// Checks the file name, the type and the bytes of `params`, and sanitizes the image.
pub fn prepare(params: &DocsPutMediaParams) -> Result<PreparedMedia, Denied> {
    let file = MediaFile::parse(params.file_name.trim())
        .map_err(|e| fields::path_error("file_name", &e))?;
    let kind = match params.kind {
        DocMediaKindWire::Image => MediaKind::Image,
        DocMediaKindWire::Diagram => MediaKind::Diagram,
        DocMediaKindWire::Unknown => return Err(fields::invalid("type", "one of: image diagram")),
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
        .map_err(|_| fields::invalid("content_base64", "standard base64"))?;
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
    let title = fields::title(&params.title)?;
    let summary = fields::summary(params.summary.as_deref())?.unwrap_or_default();
    let mut edit = TreeEdit::load(view, &cleared.project.id)?;
    let by = By::Employee(cleared.caller.id.clone());
    let (id, meta) = match (edit.media_at(&prepared.file), params.expected_revision) {
        (Some(existing), Some(expected)) => {
            if existing.meta.revision != expected {
                return Err(refused(
                    ErrorCode::Conflict,
                    format!(
                        "expected revision {expected}, but the asset is at revision {}",
                        existing.meta.revision
                    ),
                ));
            }
            let mut meta = existing.meta.clone();
            meta.revision += 1;
            meta.updated_at = Some(time_text(now));
            meta.updated_by = by;
            (existing.id.clone(), meta)
        }
        (Some(existing), None) => {
            return Err(refused(
                ErrorCode::AlreadyExists,
                format!(
                    "file_name: an asset is already here at revision {}; pass expected_revision to replace it",
                    existing.meta.revision
                ),
            ))
        }
        (None, Some(_)) => return Err(refused(ErrorCode::NotFound, "media asset not found")),
        (None, None) => (id, Meta::created(&by, now)),
    };
    let record = DocMedia {
        id,
        project_id: cleared.project.id,
        file: prepared.file.clone(),
        kind: prepared.kind,
        media_type: prepared.media_type.to_owned(),
        bytes: prepared.bytes.len() as u64,
        sha256: Sha256::of(&prepared.bytes).to_hex(),
        title,
        summary,
        meta,
    };
    let answer = DocsPutMediaResult {
        media: wire::media(&record),
    };
    edit.put_media(record, prepared.bytes.clone())?;
    edit.finish(reply(&answer))
}
