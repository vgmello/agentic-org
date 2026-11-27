//! Standalone files (`project`, `organization`, `user_private`): commit a staged upload (or
//! re-attach a readable file) with `attachment.attach`, list with `attachment.list`, and
//! change a file's title, description or status with `attachment.update_meta`.
//!
//! Bodies and queries are decoded strictly here (unknown fields, repeated or escaped query
//! values are refused); every rule about the target and the caller is the office's.

use hyper::StatusCode;
use org_proto::attachments::{
    ArtifactStatus, AttachmentAttach, AttachmentAttachParams, AttachmentList, AttachmentListParams,
    AttachmentScope, AttachmentTargetKind, AttachmentUpdateMeta, AttachmentUpdateMetaParams,
    ATTACHMENT_LIST_MAX_LIMIT,
};
use org_proto::web::{organizations, OperationId, ReadResponse};
use serde::Deserialize;

use super::super::{bad_input, invalid, result};
use super::office::{office_reply, AttachmentOffice, OFFICE_CALL};
use crate::service::ApiReply;

/// What a file list's page keeps, on top of the office's own filters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileListQuery {
    pub(super) params: AttachmentListParams,
    pub(super) mime_category: Option<MimeCategory>,
    pub(super) artifact_status: Option<ArtifactStatus>,
}

/// A coarse file type a list can be narrowed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MimeCategory {
    /// `image/*`.
    Image,
    /// Every other accepted type.
    Document,
}

/// The list query: `scope`, `target_kind`, `target_id`, `mime_category`, `artifact_status`,
/// `limit` and `cursor`, each at most once. Values are ids and names, so an escape is
/// refused rather than decoded.
pub(super) fn list_query(query: Option<&str>) -> Result<FileListQuery, ApiReply> {
    let mut out = FileListQuery {
        params: AttachmentListParams {
            target_kind: None,
            target_id: None,
            scope: None,
            limit: None,
            cursor: None,
        },
        mime_category: None,
        artifact_status: None,
    };
    let Some(query) = query.filter(|q| !q.is_empty()) else {
        return Ok(out);
    };
    if query.contains(['%', '+']) {
        return Err(invalid());
    }
    let mut seen = Vec::new();
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').ok_or_else(invalid)?;
        if value.is_empty() || seen.contains(&key) {
            return Err(invalid());
        }
        seen.push(key);
        match key {
            "scope" => {
                out.params.scope = Some(match value {
                    "shared" => AttachmentScope::Shared,
                    "user_private" => AttachmentScope::UserPrivate,
                    _ => return Err(invalid()),
                });
            }
            "target_kind" => {
                out.params.target_kind =
                    Some(AttachmentTargetKind::parse(value).ok_or_else(invalid)?);
            }
            "target_id" => out.params.target_id = Some(bounded(value)?),
            "cursor" => out.params.cursor = Some(bounded(value)?),
            "limit" => {
                let limit: u32 = value.parse().map_err(|_| invalid())?;
                if limit == 0 || limit > ATTACHMENT_LIST_MAX_LIMIT {
                    return Err(invalid());
                }
                out.params.limit = Some(limit);
            }
            "mime_category" => {
                out.mime_category = Some(match value {
                    "image" => MimeCategory::Image,
                    "document" => MimeCategory::Document,
                    _ => return Err(invalid()),
                });
            }
            "artifact_status" => {
                out.artifact_status = Some(ArtifactStatus::parse(value).ok_or_else(invalid)?);
            }
            _ => return Err(invalid()),
        }
    }
    Ok(out)
}

fn bounded(value: &str) -> Result<String, ApiReply> {
    if value.len() > 128 {
        return Err(invalid());
    }
    Ok(value.to_owned())
}

/// `attachment.list`, then the page narrowed to the asked type and status. A narrowed page
/// may hold fewer than `limit` files; `next_cursor` still continues after the office's page.
pub(super) async fn list<O: AttachmentOffice>(office: &O, query: FileListQuery) -> ApiReply {
    let mut page = match office
        .call::<AttachmentList>(query.params, true, OFFICE_CALL)
        .await
    {
        Ok(page) => page,
        Err(error) => return office_reply(error),
    };
    page.attachments.retain(|file| {
        let image = file.mime_type.starts_with("image/");
        let kind = match query.mime_category {
            None => true,
            Some(MimeCategory::Image) => image,
            Some(MimeCategory::Document) => !image,
        };
        kind && query
            .artifact_status
            .is_none_or(|status| status == file.artifact_status)
    });
    result(page)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileCommitBody {
    command_id: String,
    #[serde(default)]
    upload_id: Option<String>,
    #[serde(default)]
    source_attachment_id: Option<String>,
    target_kind: AttachmentTargetKind,
    #[serde(default)]
    target_id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    artifact_status: Option<ArtifactStatus>,
}

/// `POST /files`: one staged upload or one readable file, bound to a standalone target.
pub(super) fn commit_params(
    bytes: &[u8],
) -> Result<(OperationId, AttachmentAttachParams), ApiReply> {
    let body: FileCommitBody = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    let id = organizations::host_operation_id(&body.command_id).map_err(bad_input)?;
    let standalone = matches!(
        body.target_kind,
        AttachmentTargetKind::Project
            | AttachmentTargetKind::Organization
            | AttachmentTargetKind::UserPrivate
    );
    if !standalone || body.upload_id.is_some() == body.source_attachment_id.is_some() {
        return Err(invalid());
    }
    // Every id the office would look up is a canonical UUIDv7 before any IPC.
    let ids = [&body.upload_id, &body.source_attachment_id, &body.target_id];
    if ids
        .into_iter()
        .flatten()
        .any(|id| !org_proto::is_uuid_v7(id))
    {
        return Err(invalid());
    }
    Ok((
        id,
        AttachmentAttachParams {
            command_id: body.command_id,
            upload_id: body.upload_id,
            source_attachment_id: body.source_attachment_id,
            target_kind: body.target_kind,
            target_id: body.target_id,
            title: body.title,
            description: body.description,
            artifact_status: body.artifact_status,
        },
    ))
}

/// `attachment.attach`: `201` with the new file.
pub(super) async fn commit<O: AttachmentOffice>(
    office: &O,
    params: AttachmentAttachParams,
) -> ApiReply {
    match office
        .call::<AttachmentAttach>(params, false, OFFICE_CALL)
        .await
    {
        Ok(attached) => ApiReply::json(
            StatusCode::CREATED,
            serde_json::to_value(ReadResponse::Result(attached))
                .expect("attachment result serializes"),
        ),
        Err(error) => office_reply(error),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileUpdateBody {
    command_id: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    artifact_status: Option<ArtifactStatus>,
}

/// `PATCH /files/{id}`: at least one of `title`, `description` and `artifact_status`.
pub(super) fn update_params(
    attachment_id: String,
    bytes: &[u8],
) -> Result<(OperationId, AttachmentUpdateMetaParams), ApiReply> {
    let body: FileUpdateBody = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    let id = organizations::host_operation_id(&body.command_id).map_err(bad_input)?;
    if body.title.is_none() && body.description.is_none() && body.artifact_status.is_none() {
        return Err(invalid());
    }
    Ok((
        id,
        AttachmentUpdateMetaParams {
            command_id: body.command_id,
            attachment_id,
            title: body.title,
            description: body.description,
            artifact_status: body.artifact_status,
        },
    ))
}

/// `attachment.update_meta`: `200` with the changed file.
pub(super) async fn update<O: AttachmentOffice>(
    office: &O,
    params: AttachmentUpdateMetaParams,
) -> ApiReply {
    match office
        .call::<AttachmentUpdateMeta>(params, false, OFFICE_CALL)
        .await
    {
        Ok(updated) => result(updated),
        Err(error) => office_reply(error),
    }
}
