//! Durable records and YAML codecs for the projects family (`state/projects/<id>/project.yaml`,
//! `state/identity/counters/organization/<org-id>/project.yaml`, and
//! `state/identity/allocations/organization/<org-id>/project/<number>.yaml`).

use org_store::{DurablePath, Id, StoreError};
use serde_json::{json, Value};

use crate::organization::records::{
    self, is_line, is_slug, is_stable_id, path_number,
    yaml::{self, Fields, Reader},
    Meta, RecordError,
};

fn durable_path(text: &str) -> DurablePath {
    DurablePath::parse(text).expect("project durable path is canonical")
}

fn stable_id(text: &str) -> Option<Id> {
    is_stable_id(text).then(|| Id::parse(text).ok()).flatten()
}

fn read_id(reader: &mut Reader, key: &str) -> Result<Id, RecordError> {
    let text = reader.text(key)?;
    stable_id(&text).ok_or_else(|| reader.error(key, "not a lowercase UUIDv7"))
}

fn read_optional_id(reader: &mut Reader, key: &str) -> Result<Option<Id>, RecordError> {
    let Some(text) = reader.optional_text(key)? else {
        return Ok(None);
    };
    stable_id(&text)
        .map(Some)
        .ok_or_else(|| reader.error(key, "not a lowercase UUIDv7"))
}

fn read_time(reader: &mut Reader, key: &str) -> Result<String, RecordError> {
    let text = reader.text(key)?;
    crate::time::parse_rfc3339_ms(&text)
        .ok_or_else(|| reader.error(key, "not an RFC 3339 timestamp"))?;
    Ok(text)
}

/// Derives a canonical lowercase project slug from `name` (ASCII alphanumeric runs separated by
/// single hyphens, falling back to `"project"` if `name` has no ASCII alphanumeric characters).
pub fn derive_slug(name: &str) -> String {
    let mut out = String::new();
    let mut prev_hyphen = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_hyphen = false;
        } else if !out.is_empty() && !prev_hyphen {
            out.push('-');
            prev_hyphen = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.len() > 56 {
        out.truncate(56);
        while out.ends_with('-') {
            out.pop();
        }
    }
    if out.is_empty() {
        "project".to_owned()
    } else {
        out
    }
}

/// One project-to-repository link (`project_repositories`, `doc-35`).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct ProjectRepositoryLink {
    pub repository_id: Id,
    pub integration_branch: String,
}

/// A project record (`state/projects/<project-id>/project.yaml`, data contract document 02).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Project {
    pub id: Id,
    pub number: u64,
    pub slug: String,
    pub name: String,
    pub owner_team_id: Id,
    pub archived: bool,
    pub development_process_id: Option<Id>,
    pub contributing_team_ids: Vec<Id>,
    pub repositories: Vec<ProjectRepositoryLink>,
    pub meta: Meta,
}

impl Project {
    /// Human-readable organization reference `P-<number>`.
    pub fn key(&self) -> String {
        format!("P-{}", self.number)
    }

    pub fn to_wire(&self) -> org_proto::projects::ProjectRecord {
        org_proto::projects::ProjectRecord {
            id: self.id.to_string(),
            number: self.number,
            key: self.key(),
            slug: self.slug.clone(),
            name: self.name.clone(),
            owner_team_id: self.owner_team_id.to_string(),
            contributing_team_ids: self
                .contributing_team_ids
                .iter()
                .map(Id::to_string)
                .collect(),
            archived: self.archived,
            revision: self.meta.revision,
            created_at: self.meta.created_at.clone(),
            updated_at: self.meta.updated_at.clone(),
        }
    }
}

pub fn project_path(project: &Id) -> DurablePath {
    durable_path(&format!("state/projects/{project}/project.yaml"))
}

pub fn encode_project(record: &Project) -> Vec<u8> {
    let mut fields = Fields::new();
    record.meta.write(&mut fields);
    fields.insert("id".into(), json!(record.id.as_str()));
    fields.insert("number".into(), json!(record.number));
    fields.insert("slug".into(), json!(record.slug));
    fields.insert("name".into(), json!(record.name));
    fields.insert("owner_team_id".into(), json!(record.owner_team_id.as_str()));
    fields.insert("archived".into(), json!(record.archived));
    fields.insert(
        "development_process_id".into(),
        json!(record.development_process_id.as_ref().map(Id::as_str)),
    );
    let contributing: Vec<&str> = record
        .contributing_team_ids
        .iter()
        .map(Id::as_str)
        .collect();
    fields.insert("contributing_team_ids".into(), json!(contributing));
    let mut sorted_repos = record.repositories.clone();
    sorted_repos.sort_by(|a, b| a.repository_id.cmp(&b.repository_id));
    let repo_entries: Vec<Value> = sorted_repos
        .into_iter()
        .map(|link| {
            json!({
                "repository_id": link.repository_id.as_str(),
                "integration_branch": link.integration_branch,
            })
        })
        .collect();
    fields.insert("repositories".into(), Value::Array(repo_entries));
    yaml::encode(&fields)
}

pub fn decode_project(bytes: &[u8]) -> Result<Project, RecordError> {
    let mut reader = yaml::decode(bytes)?;
    let meta = Meta::read(&mut reader)?;
    let id = read_id(&mut reader, "id")?;
    let number = reader.number("number")?;
    if number == 0 {
        return Err(reader.error("number", "below 1"));
    }
    let slug = reader.text("slug")?;
    if !is_slug(&slug) {
        return Err(reader.error("slug", "not a canonical lowercase slug"));
    }
    let name = reader.text("name")?;
    if !is_line(&name) {
        return Err(reader.error("name", "not one line of visible text"));
    }
    let owner_team_id = read_id(&mut reader, "owner_team_id")?;
    let archived = reader.flag("archived")?;
    let development_process_id = read_optional_id(&mut reader, "development_process_id")?;
    let contributing_raw = reader.strings("contributing_team_ids")?;
    let mut contributing_team_ids = Vec::with_capacity(contributing_raw.len());
    for (idx, text) in contributing_raw.into_iter().enumerate() {
        let Some(team_id) = stable_id(&text) else {
            return Err(reader.error(
                &format!("contributing_team_ids[{idx}]"),
                "not a lowercase UUIDv7",
            ));
        };
        contributing_team_ids.push(team_id);
    }
    let repos_raw = reader.list("repositories")?;
    let mut repositories = Vec::with_capacity(repos_raw.len());
    for mut entry in repos_raw {
        let repository_id = read_id(&mut entry, "repository_id")?;
        let integration_branch = entry.text("integration_branch")?;
        if !records::is_branch(&integration_branch) {
            return Err(entry.error("integration_branch", "not a valid branch name"));
        }
        entry.finish()?;
        repositories.push(ProjectRepositoryLink {
            repository_id,
            integration_branch,
        });
    }
    reader.finish()?;
    Ok(Project {
        id,
        number,
        slug,
        name,
        owner_team_id,
        archived,
        development_process_id,
        contributing_team_ids,
        repositories,
        meta,
    })
}

// ---- identity/counters/organization/<org-id>/project.yaml ----

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ProjectCounter {
    pub organization_id: Id,
    pub last_number: u64,
}

impl ProjectCounter {
    pub fn advance(&self) -> Result<Self, StoreError> {
        if self.last_number >= i64::MAX as u64 {
            return Err(StoreError::Exhausted("project numbers"));
        }
        Ok(Self {
            organization_id: self.organization_id.clone(),
            last_number: self.last_number + 1,
        })
    }
}

pub fn project_counter_path(organization: &Id) -> DurablePath {
    durable_path(&format!(
        "state/identity/counters/organization/{organization}/project.yaml"
    ))
}

pub fn encode_project_counter(record: &ProjectCounter) -> Vec<u8> {
    let fields = json!({
        "scope_kind": "organization",
        "scope_id": record.organization_id.as_str(),
        "entity_kind": "project",
        "last_number": record.last_number,
    });
    yaml::encode(fields.as_object().expect("an object"))
}

fn project_scope(reader: &mut Reader) -> Result<Id, RecordError> {
    if reader.text("scope_kind")? != "organization" {
        return Err(reader.error("scope_kind", "not organization"));
    }
    let organization = read_id(reader, "scope_id")?;
    if reader.text("entity_kind")? != "project" {
        return Err(reader.error("entity_kind", "not project"));
    }
    Ok(organization)
}

pub fn decode_project_counter(bytes: &[u8]) -> Result<ProjectCounter, RecordError> {
    let mut reader = yaml::decode(bytes)?;
    let record = ProjectCounter {
        organization_id: project_scope(&mut reader)?,
        last_number: reader.number("last_number")?,
    };
    reader.finish()?;
    Ok(record)
}

// ---- identity/allocations/organization/<org-id>/project/<number>.yaml ----

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ProjectAllocation {
    pub organization_id: Id,
    pub number: u64,
    pub project_id: Id,
    pub allocated_at: String,
}

pub fn project_allocation_path(organization: &Id, number: u64) -> DurablePath {
    durable_path(&format!(
        "state/identity/allocations/organization/{organization}/project/{number}.yaml"
    ))
}

pub fn encode_project_allocation(record: &ProjectAllocation) -> Vec<u8> {
    let fields = json!({
        "scope_kind": "organization",
        "scope_id": record.organization_id.as_str(),
        "entity_kind": "project",
        "number": record.number,
        "entity_id": record.project_id.as_str(),
        "allocated_at": record.allocated_at,
    });
    yaml::encode(fields.as_object().expect("an object"))
}

pub fn decode_project_allocation(bytes: &[u8]) -> Result<ProjectAllocation, RecordError> {
    let mut reader = yaml::decode(bytes)?;
    let record = ProjectAllocation {
        organization_id: project_scope(&mut reader)?,
        number: reader.number("number")?,
        project_id: read_id(&mut reader, "entity_id")?,
        allocated_at: read_time(&mut reader, "allocated_at")?,
    };
    if record.number == 0 {
        return Err(reader.error("number", "below 1"));
    }
    reader.finish()?;
    Ok(record)
}

// ---- path ownership ----

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Kind {
    Project(Id),
    ProjectCounter(Id),
    ProjectAllocation(Id, u64),
}

pub fn kind_of(path: &DurablePath) -> Option<Kind> {
    let parts: Vec<&str> = path.components().collect();
    match parts.as_slice() {
        ["state", "projects", id, "project.yaml"] => stable_id(id).map(Kind::Project),
        ["state", "identity", "counters", "organization", scope, "project.yaml"] => {
            stable_id(scope).map(Kind::ProjectCounter)
        }
        ["state", "identity", "allocations", "organization", scope, "project", name] => {
            let number = name.strip_suffix(".yaml").and_then(path_number)?;
            Some(Kind::ProjectAllocation(stable_id(scope)?, number))
        }
        _ => None,
    }
}

pub(crate) fn time_text(at: org_store::Timestamp) -> String {
    records::time_text(at)
}
