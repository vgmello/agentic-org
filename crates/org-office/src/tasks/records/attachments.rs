//! Canonical attachment metadata embedded in `task.md` (`attachments: []`, max 20) and
//! `comments/<id>.md` (`attachments: []`, max 10) (`TASK-98.4`, ADR-0037).
//!
//! Each entry carries the full metadata needed to rebuild the `attachments` projection table
//! losslessly from durable Markdown front matter alone; the bytes live content-addressed in
//! `<durable>/blobs/sha256/`.

use std::collections::BTreeSet;

use org_proto::attachments::{ArtifactStatus, AttachmentTargetKind};
use org_store::Id;
use serde_json::{json, Map, Value};

use crate::attachments::blobs::ContentHash;
use crate::attachments::media::MediaType;
use crate::attachments::records::{is_description, is_filename, is_title, AttachmentRecord};
use crate::organization::records::{is_stable_id, yaml::Reader, RecordError};

/// Encodes `records` as a deterministic, ID-sorted JSON array for YAML front matter.
pub fn encode_attachments(records: &[AttachmentRecord]) -> Value {
    let mut sorted: Vec<&AttachmentRecord> = records.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    let items: Vec<Value> = sorted
        .into_iter()
        .map(|r| {
            let mut obj = Map::new();
            obj.insert("id".into(), json!(r.id.as_str()));
            obj.insert("content_sha256".into(), json!(r.content_sha256.as_str()));
            obj.insert("mime_type".into(), json!(r.mime_type));
            obj.insert("byte_count".into(), json!(r.byte_count));
            obj.insert("filename".into(), json!(r.filename));
            obj.insert("title".into(), json!(r.title));
            obj.insert("description".into(), json!(r.description));
            obj.insert("artifact_status".into(), json!(r.artifact_status.as_str()));
            if let Some(width) = r.width {
                obj.insert("width".into(), json!(width));
            }
            if let Some(height) = r.height {
                obj.insert("height".into(), json!(height));
            }
            if let Some(page_count) = r.page_count {
                obj.insert("page_count".into(), json!(page_count));
            }
            obj.insert(
                "uploaded_by_employee_id".into(),
                json!(r.uploaded_by_employee_id.as_str()),
            );
            obj.insert("created_at".into(), json!(r.created_at));
            obj.insert("updated_at".into(), json!(r.updated_at));
            Value::Object(obj)
        })
        .collect();
    Value::Array(items)
}

fn read_id(reader: &mut Reader, key: &str) -> Result<Id, RecordError> {
    let text = reader.text(key)?;
    if !is_stable_id(&text) {
        return Err(reader.error(key, "not a lowercase UUIDv7"));
    }
    Id::parse(&text).map_err(|_| reader.error(key, "not an ID"))
}

fn read_time(reader: &mut Reader, key: &str) -> Result<String, RecordError> {
    let text = reader.text(key)?;
    crate::time::parse_rfc3339_ms(&text)
        .ok_or_else(|| reader.error(key, "not an RFC 3339 timestamp"))?;
    Ok(text)
}

fn read_dimension(reader: &mut Reader, key: &str) -> Result<Option<u32>, RecordError> {
    if !reader.has(key) {
        return Ok(None);
    }
    let value = reader.number(key)?;
    match u32::try_from(value) {
        Ok(value) if value > 0 => Ok(Some(value)),
        _ => Err(reader.error(key, "not a positive 32-bit number")),
    }
}

/// Decodes `attachments` from `task.md` or `comments/<id>.md` front matter. Missing key reads as
/// empty for backwards compatibility with pre-`TASK-98.4` files; invalid or over-cap entries fail
/// closed.
pub fn read_attachments(
    reader: &mut Reader,
    target_kind: AttachmentTargetKind,
    target_id: &Id,
    max: usize,
) -> Result<Vec<AttachmentRecord>, RecordError> {
    if !reader.has("attachments") {
        return Ok(Vec::new());
    }
    let items = reader.list("attachments")?;
    if items.len() > max {
        return Err(RecordError::new(
            "attachments",
            format!("exceeds maximum of {max} attachments"),
        ));
    }
    let mut seen = BTreeSet::new();
    let mut records = Vec::with_capacity(items.len());
    for mut item in items {
        let id = read_id(&mut item, "id")?;
        if !seen.insert(id.clone()) {
            return Err(item.error("id", "duplicate attachment id"));
        }
        let sha_text = item.text("content_sha256")?;
        let content_sha256 = ContentHash::parse(&sha_text)
            .ok_or_else(|| item.error("content_sha256", "not 64 lowercase hex digits"))?;
        let mime_type = item.text("mime_type")?;
        if MediaType::from_mime(&mime_type).is_none() {
            return Err(item.error("mime_type", "not an accepted attachment type"));
        }
        let byte_count = item.number("byte_count")?;
        if byte_count == 0 {
            return Err(item.error("byte_count", "must be positive"));
        }
        let filename = item.text("filename")?;
        if !is_filename(&filename) {
            return Err(item.error("filename", "not one plain file name"));
        }
        let title = item.text("title")?;
        if !is_title(&title) {
            return Err(item.error("title", "not 1 to 160 characters of one line"));
        }
        let description = item.optional_text("description")?;
        if description.as_deref().is_some_and(|d| !is_description(d)) {
            return Err(item.error("description", "longer than 280 characters"));
        }
        let status_text = item.text("artifact_status")?;
        let artifact_status = ArtifactStatus::parse(&status_text)
            .ok_or_else(|| item.error("artifact_status", "not a known status"))?;
        let width = read_dimension(&mut item, "width")?;
        let height = read_dimension(&mut item, "height")?;
        let page_count = read_dimension(&mut item, "page_count")?;
        let uploaded_by_employee_id = read_id(&mut item, "uploaded_by_employee_id")?;
        let created_at = read_time(&mut item, "created_at")?;
        let updated_at = read_time(&mut item, "updated_at")?;
        item.finish()?;
        records.push(AttachmentRecord {
            id,
            target_kind,
            target_id: Some(target_id.clone()),
            content_sha256,
            mime_type,
            byte_count,
            filename,
            title,
            description,
            artifact_status,
            width,
            height,
            page_count,
            uploaded_by_employee_id,
            created_at,
            updated_at,
        });
    }
    records.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(records)
}
