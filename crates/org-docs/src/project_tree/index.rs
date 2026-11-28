//! Deterministic JSON/YAML encoding and canonical `docs/index.yaml` rendering.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use super::{DocPath, DocStatus, MediaFile, MediaKind};

const INDEX_HEADER: &str =
    "# Maintained by the office: regenerated on every documentation change. Do not edit.\n";
const INDEX_SCHEMA_VERSION: u64 = 1;

fn write_canonical_value(out: &mut String, value: &Value) {
    match value {
        Value::Object(fields) => {
            out.push('{');
            let mut entries: Vec<_> = fields.iter().collect();
            entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
            for (i, (key, val)) in entries.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::from(key.as_str()).to_string());
                out.push(':');
                write_canonical_value(out, val);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical_value(out, item);
            }
            out.push(']');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}

/// Deterministic compact JSON with object keys sorted in byte order.
pub fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_canonical_value(&mut out, value);
    out
}

/// Encodes a flat mapping as deterministic YAML (`key: <canonical JSON>\n`, keys in byte order).
pub fn yaml_encode(fields: &Map<String, Value>) -> Vec<u8> {
    let mut entries: Vec<_> = fields.iter().collect();
    entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let mut out = String::new();
    for (key, value) in entries {
        out.push_str(key);
        out.push_str(": ");
        write_canonical_value(&mut out, value);
        out.push('\n');
    }
    out.into_bytes()
}

/// Encodes front matter plus Markdown body between `---` fences.
pub fn yaml_encode_document(fields: &Map<String, Value>, body: &str) -> Vec<u8> {
    let mut out = b"---\n".to_vec();
    out.extend(yaml_encode(fields));
    out.extend(b"---\n");
    out.extend(body.as_bytes());
    out
}

/// Borrowed document metadata row for deterministic `docs/index.yaml` rendering.
#[derive(Debug, Clone, Copy)]
pub struct IndexDocRow<'a> {
    /// Document UUIDv7.
    pub id: &'a str,
    /// Validated path relative to `docs/`.
    pub path: &'a DocPath,
    /// Document kind slug.
    pub kind: &'a str,
    /// Document title.
    pub title: &'a str,
    /// Document lifecycle status.
    pub status: DocStatus,
    /// Optional summary.
    pub summary: Option<&'a str>,
    /// Optional owner employee UUIDv7.
    pub owner_employee_id: Option<&'a str>,
    /// Current revision.
    pub revision: u64,
}

/// Borrowed media metadata row for deterministic `docs/index.yaml` rendering.
#[derive(Debug, Clone, Copy)]
pub struct IndexMediaRow<'a> {
    /// Media UUIDv7.
    pub id: &'a str,
    /// Validated media file name.
    pub file: &'a MediaFile,
    /// Media kind (`image` or `diagram`).
    pub kind: MediaKind,
    /// MIME type.
    pub media_type: &'a str,
    /// Byte length.
    pub bytes: u64,
    /// Lowercase SHA-256 hex digest.
    pub sha256: &'a str,
    /// Media title.
    pub title: &'a str,
    /// Current revision.
    pub revision: u64,
}

/// Deterministically renders `docs/index.yaml` for a project's documents and media.
pub fn render_project_index(
    project_id: &str,
    docs: &[IndexDocRow<'_>],
    media: &[IndexMediaRow<'_>],
) -> Vec<u8> {
    let mut by_path: Vec<&IndexDocRow<'_>> = docs.iter().collect();
    by_path.sort_by(|a, b| a.path.cmp(b.path));
    let mut media_by_path: Vec<&IndexMediaRow<'_>> = media.iter().collect();
    media_by_path.sort_by(|a, b| a.file.cmp(b.file));
    let mut sections: BTreeMap<&str, u64> = BTreeMap::new();
    for doc in &by_path {
        let section = doc.path.as_str().rsplit_once('/').map_or("", |(s, _)| s);
        *sections.entry(section).or_default() += 1;
    }
    let mut fields = Map::new();
    fields.insert("schema_version".into(), json!(INDEX_SCHEMA_VERSION));
    fields.insert("project_id".into(), json!(project_id));
    fields.insert(
        "sections".into(),
        Value::Array(
            sections
                .into_iter()
                .map(|(path, count)| json!({"path": path, "document_count": count}))
                .collect(),
        ),
    );
    fields.insert(
        "documents".into(),
        Value::Array(
            by_path
                .into_iter()
                .map(|doc| {
                    json!({
                        "id": doc.id,
                        "path": doc.path.as_str(),
                        "section": doc.path.section().as_str(),
                        "kind": doc.kind,
                        "title": doc.title,
                        "status": doc.status.as_str(),
                        "summary": doc.summary,
                        "owner_employee_id": doc.owner_employee_id,
                        "revision": doc.revision,
                    })
                })
                .collect(),
        ),
    );
    fields.insert(
        "media".into(),
        Value::Array(
            media_by_path
                .into_iter()
                .map(|m| {
                    json!({
                        "id": m.id,
                        "path": m.file.rel_path(),
                        "type": m.kind.as_str(),
                        "media_type": m.media_type,
                        "bytes": m.bytes,
                        "sha256": m.sha256,
                        "title": m.title,
                        "revision": m.revision,
                    })
                })
                .collect(),
        ),
    );
    let mut out = INDEX_HEADER.as_bytes().to_vec();
    out.extend(yaml_encode(&fields));
    out
}
