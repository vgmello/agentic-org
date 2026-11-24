//! Durable `structure_documents` record and YAML+Markdown codec (`doc-75`, `doc-34` §2.1-2.2).

use org_process::{DocumentKind, DocumentStatus, ProcessKey, Scope, StructureDocInput};
use org_store::{DurablePath, Id, Sha256};
use serde_json::{json, Value};

use crate::organization::records::{
    is_line, is_stable_id,
    yaml::{self, Fields, Reader},
    Meta, RecordError,
};

/// One durable organization process or standard document (`structure_documents`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StructureDocument {
    pub id: Id,
    pub key: Option<String>,
    pub kind: DocumentKind,
    pub title: String,
    pub status: DocumentStatus,
    pub owner_employee_id: Option<Id>,
    pub team_id: Option<Id>,
    pub project_id: Option<Id>,
    pub overridable: bool,
    pub body_markdown: String,
    pub meta: Meta,
}

impl StructureDocument {
    /// Scope enum matching `(team_id, project_id)`.
    pub fn scope(&self) -> Scope {
        if let Some(tid) = &self.team_id {
            Scope::Team(tid.as_str().to_owned())
        } else if let Some(pid) = &self.project_id {
            Scope::Project(pid.as_str().to_owned())
        } else {
            Scope::Organization
        }
    }

    /// Converts to the pure [`StructureDocInput`] consumed by `org_process::resolve_single_chain`.
    pub fn to_process_input(&self, path: String) -> StructureDocInput {
        let encoded = encode_structure_document(self);
        let blob_sha256 = Sha256::of(&encoded).to_hex();
        StructureDocInput {
            id: self.id.as_str().to_owned(),
            key: self.key.as_deref().and_then(ProcessKey::new),
            kind: self.kind,
            title: self.title.clone(),
            status: self.status,
            owner_employee_id: self
                .owner_employee_id
                .as_ref()
                .map(|id| id.as_str().to_owned()),
            scope: self.scope(),
            overridable: self.overridable,
            revision: self.meta.revision,
            body_markdown: self.body_markdown.clone(),
            path,
            blob_sha256,
            invalid_field: None,
        }
    }
}

/// Parsed canonical path for a `structure_documents` Markdown file under `state/`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum StructureDocPath {
    Organization(DocumentKind, Id),
    Team(Vec<Id>, DocumentKind, Id),
    Project(Id, DocumentKind, Id),
}

fn kind_folder(kind: DocumentKind) -> &'static str {
    match kind {
        DocumentKind::Process => "processes",
        DocumentKind::Standard => "standards",
    }
}

fn parse_kind_folder(folder: &str) -> Option<DocumentKind> {
    match folder {
        "processes" => Some(DocumentKind::Process),
        "standards" => Some(DocumentKind::Standard),
        _ => None,
    }
}

fn parse_md_id(file_name: &str) -> Option<Id> {
    let stem = file_name.strip_suffix(".md")?;
    is_stable_id(stem).then(|| Id::parse(stem).ok()).flatten()
}

/// Canonical durable path (`state/.../<id>.md`) for a process or standard document (`doc-75`).
pub fn structure_document_path(
    kind: DocumentKind,
    id: &Id,
    team_chain: Option<&[Id]>,
    project_id: Option<&Id>,
) -> Result<DurablePath, org_store::StoreError> {
    let folder = kind_folder(kind);
    let raw = if let Some(pid) = project_id {
        format!(
            "state/projects/{}/{folder}/{}.md",
            pid.as_str(),
            id.as_str()
        )
    } else if let Some(chain) = team_chain {
        let mut prefix = String::from("state");
        for tid in chain {
            prefix.push_str("/teams/");
            prefix.push_str(tid.as_str());
        }
        format!("{prefix}/{folder}/{}.md", id.as_str())
    } else {
        format!("state/{folder}/{}.md", id.as_str())
    };
    DurablePath::parse(&raw).map_err(|e| {
        org_store::StoreError::invalid(format!("invalid structure_document path: {e}"))
    })
}

/// Parses a durable path if it belongs to the processes/standards family.
pub fn parse_path(path: &DurablePath) -> Option<StructureDocPath> {
    let segs: Vec<&str> = path.as_str().split('/').collect();
    if segs.first() != Some(&"state") {
        return None;
    }
    let tail = &segs[1..];
    match tail {
        [folder, file] => {
            let kind = parse_kind_folder(folder)?;
            let id = parse_md_id(file)?;
            Some(StructureDocPath::Organization(kind, id))
        }
        ["projects", pid_str, folder, file] => {
            let pid = is_stable_id(pid_str)
                .then(|| Id::parse(pid_str).ok())
                .flatten()?;
            let kind = parse_kind_folder(folder)?;
            let id = parse_md_id(file)?;
            Some(StructureDocPath::Project(pid, kind, id))
        }
        _ if tail.len() >= 4 && tail[0] == "teams" => {
            let (file, rest) = tail.split_last()?;
            let (folder, team_segs) = rest.split_last()?;
            let kind = parse_kind_folder(folder)?;
            let id = parse_md_id(file)?;
            let (pairs, rem) = team_segs.as_chunks::<2>();
            if !rem.is_empty() {
                return None;
            }
            let mut chain = Vec::with_capacity(pairs.len());
            for [seg, team_id] in pairs {
                if *seg != "teams" || !is_stable_id(team_id) {
                    return None;
                }
                chain.push(Id::parse(team_id).ok()?);
            }
            Some(StructureDocPath::Team(chain, kind, id))
        }
        _ => None,
    }
}

/// Encodes a [`StructureDocument`] to deterministic YAML front matter + Markdown bytes.
pub fn encode_structure_document(doc: &StructureDocument) -> Vec<u8> {
    let mut fields = Fields::new();
    fields.insert("id".into(), json!(doc.id.as_str()));
    fields.insert(
        "key".into(),
        doc.key.as_deref().map_or(Value::Null, |k| json!(k)),
    );
    fields.insert("kind".into(), json!(doc.kind.as_str()));
    fields.insert("title".into(), json!(doc.title));
    fields.insert("status".into(), json!(doc.status.as_str()));
    fields.insert(
        "owner_employee_id".into(),
        doc.owner_employee_id
            .as_ref()
            .map_or(Value::Null, |id| json!(id.as_str())),
    );
    fields.insert(
        "team_id".into(),
        doc.team_id
            .as_ref()
            .map_or(Value::Null, |id| json!(id.as_str())),
    );
    fields.insert(
        "project_id".into(),
        doc.project_id
            .as_ref()
            .map_or(Value::Null, |id| json!(id.as_str())),
    );
    fields.insert("overridable".into(), json!(doc.overridable));
    doc.meta.write(&mut fields);
    yaml::encode_document(&fields, &doc.body_markdown)
}

fn read_id(reader: &mut Reader, key: &str) -> Result<Id, RecordError> {
    let text = reader.text(key)?;
    if !is_stable_id(&text) {
        return Err(reader.error(key, "must be a canonical lowercase UUIDv7"));
    }
    Id::parse(&text).map_err(|_| reader.error(key, "must be a canonical lowercase UUIDv7"))
}

fn read_opt_id(reader: &mut Reader, key: &str) -> Result<Option<Id>, RecordError> {
    let Some(text) = reader.optional_text(key)? else {
        return Ok(None);
    };
    if !is_stable_id(&text) {
        return Err(reader.error(key, "must be a canonical lowercase UUIDv7"));
    }
    Id::parse(&text)
        .map(Some)
        .map_err(|_| reader.error(key, "must be a canonical lowercase UUIDv7"))
}

/// Decodes a [`StructureDocument`] from its durable YAML+Markdown bytes.
pub fn decode_structure_document(bytes: &[u8]) -> Result<StructureDocument, RecordError> {
    let (mut reader, body_markdown) = yaml::decode_document(bytes)?;
    let id = read_id(&mut reader, "id")?;
    let key = match reader.optional_text("key")? {
        None => None,
        Some(raw) => {
            if ProcessKey::new(&raw).is_none() {
                return Err(reader.error("key", "invalid process key format"));
            }
            Some(raw)
        }
    };
    let kind_str = reader.text("kind")?;
    let kind = DocumentKind::parse(&kind_str)
        .ok_or_else(|| reader.error("kind", "must be process or standard"))?;
    let title = reader.text("title")?;
    if !is_line(&title) {
        return Err(reader.error("title", "must be a valid single line"));
    }
    let status_str = reader.text("status")?;
    let status = DocumentStatus::parse(&status_str)
        .ok_or_else(|| reader.error("status", "must be draft, current, or deprecated"))?;
    let owner_employee_id = read_opt_id(&mut reader, "owner_employee_id")?;
    if status == DocumentStatus::Current && owner_employee_id.is_none() {
        return Err(reader.error("owner_employee_id", "required for current document"));
    }
    let team_id = read_opt_id(&mut reader, "team_id")?;
    let project_id = read_opt_id(&mut reader, "project_id")?;
    if team_id.is_some() && project_id.is_some() {
        return Err(reader.error(
            "project_id",
            "team_id and project_id are mutually exclusive",
        ));
    }
    let overridable = reader.flag("overridable")?;
    let meta = Meta::read(&mut reader)?;
    reader.finish()?;
    Ok(StructureDocument {
        id,
        key,
        kind,
        title,
        status,
        owner_employee_id,
        team_id,
        project_id,
        overridable,
        body_markdown,
        meta,
    })
}
