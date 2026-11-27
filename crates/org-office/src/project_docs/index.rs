//! The office-maintained `docs/index.yaml` of one project: rendered from the project's documents
//! and media, deterministically (entries in path order, keys in byte order, canonical JSON
//! values), so the same tree always gives the same bytes. Every write of the tree puts the
//! rendered index in the same journal entry as the files it changes
//! (`features::change`), and a rebuild checks the stored index against a fresh rendering
//! (`projector`). Nobody else writes it.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::records::{DocEntry, DocMedia};
use crate::organization::records::yaml::{self, Fields};
use org_store::Id;

const HEADER: &str =
    "# Maintained by the office: regenerated on every documentation change. Do not edit.\n";
const INDEX_SCHEMA_VERSION: u64 = 1;

fn doc_entry(doc: &DocEntry) -> Value {
    json!({
        "id": doc.id.as_str(),
        "path": doc.path.as_str(),
        "section": doc.path.section().as_str(),
        "kind": doc.kind,
        "title": doc.title,
        "status": doc.status.as_str(),
        "summary": doc.summary,
        "owner_employee_id": doc.owner_employee_id.as_ref().map(Id::as_str),
        "revision": doc.meta.revision,
    })
}

fn media_entry(media: &DocMedia) -> Value {
    json!({
        "id": media.id.as_str(),
        "path": media.file.rel_path(),
        "type": media.kind.as_str(),
        "media_type": media.media_type,
        "bytes": media.bytes,
        "sha256": media.sha256,
        "title": media.title,
        "revision": media.meta.revision,
    })
}

/// The index file of `project` for exactly these documents and media, in any order.
pub fn render(project: &Id, docs: &[DocEntry], media: &[DocMedia]) -> Vec<u8> {
    let mut by_path: Vec<&DocEntry> = docs.iter().collect();
    by_path.sort_by(|a, b| a.path.cmp(&b.path));
    let mut media_by_path: Vec<&DocMedia> = media.iter().collect();
    media_by_path.sort_by(|a, b| a.file.cmp(&b.file));
    let mut sections: BTreeMap<&str, u64> = BTreeMap::new();
    for doc in &by_path {
        let section = doc.path.as_str().rsplit_once('/').map_or("", |(s, _)| s);
        *sections.entry(section).or_default() += 1;
    }
    let mut fields = Fields::new();
    fields.insert("schema_version".into(), json!(INDEX_SCHEMA_VERSION));
    fields.insert("project_id".into(), json!(project.as_str()));
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
        Value::Array(by_path.into_iter().map(doc_entry).collect()),
    );
    fields.insert(
        "media".into(),
        Value::Array(media_by_path.into_iter().map(media_entry).collect()),
    );
    let mut out = HEADER.as_bytes().to_vec();
    out.extend(yaml::encode(&fields));
    out
}
