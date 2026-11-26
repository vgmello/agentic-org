//! `task.md`: a task's YAML front matter and Markdown body (`doc-77`, `doc-79`), read through
//! `org_docs::SourceDoc` within the `selected-task-resources/1` bounds (front matter at most
//! 64 KiB, body at most 8 MiB). The front matter holds every field of the task, null when
//! unknown, so a task has one field set; its typed links are in it too.

use std::collections::BTreeMap;

use org_docs::{MAX_FRONT_MATTER_BYTES, MAX_SOURCE_BODY_BYTES};
use org_proto::tasks::{TaskPriority, TaskStatus, WorkType};
use org_store::Id;
use serde_json::{json, Map, Value};

use super::{
    is_valid_office_code, normalize_file_paths, normalize_labels, TaskDetailsRecord,
    TaskLinkRecord, TaskMeta, TaskRecord, TaskRepositoryRecord, SCHEMA_VERSION,
};
use crate::organization::records::{
    is_stable_id,
    yaml::{self, Fields, Reader},
    RecordError,
};

/// A decoded `task.md`: the task and its typed links.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TaskDoc {
    pub task: TaskRecord,
    pub links: Vec<TaskLinkRecord>,
}

fn id_value(id: Option<&Id>) -> Value {
    id.map_or(Value::Null, |id| json!(id.as_str()))
}

fn text_value(text: Option<&str>) -> Value {
    text.map_or(Value::Null, |text| json!(text))
}

/// Links in one order whatever order an engine returns them in, so every engine writes the
/// same bytes.
fn sorted_links(links: &[TaskLinkRecord]) -> Vec<&TaskLinkRecord> {
    let mut sorted: Vec<_> = links.iter().collect();
    sorted.sort_by(|a, b| {
        (&a.kind, &a.target_kind, &a.target_id, &a.id).cmp(&(
            &b.kind,
            &b.target_kind,
            &b.target_id,
            &b.id,
        ))
    });
    sorted
}

fn details_fields(details: &TaskDetailsRecord) -> Value {
    let text = |t: &Option<String>| text_value(t.as_deref());
    let id = |i: &Option<Id>| id_value(i.as_ref());
    json!({
        "definition_of_done": text(&details.definition_of_done),
        "what_happened": text(&details.what_happened),
        "expected": text(&details.expected),
        "steps": text(&details.steps),
        "environment": text(&details.environment),
        "severity_confirmed": details.severity_confirmed,
        "reopen_count": details.reopen_count,
        "duplicate_of_task_id": id(&details.duplicate_of_task_id),
        "closed_reason": text(&details.closed_reason),
        "affects_feature_id": id(&details.affects_feature_id),
        "request_kind": text(&details.request_kind),
        "request_text": text(&details.request_text),
        "route_reason": text(&details.route_reason),
        "routing_source": text(&details.routing_source),
        "routed_by_employee_id": id(&details.routed_by_employee_id),
        "merged_into_task_id": id(&details.merged_into_task_id),
        "suggested_triager_id": id(&details.suggested_triager_id),
        "suggested_verifier_id": id(&details.suggested_verifier_id),
    })
}

fn repository_fields(task: &TaskRecord, fields: &mut Fields) {
    let mut repositories: Vec<&TaskRepositoryRecord> = task.repositories.iter().collect();
    repositories.sort_by(|a, b| a.repository_id.cmp(&b.repository_id));
    let ids: Vec<&str> = repositories
        .iter()
        .map(|r| r.repository_id.as_str())
        .collect();
    let mut branches = Map::new();
    let mut commits = Map::new();
    for repo in repositories {
        if let Some(branch) = &repo.branch {
            branches.insert(repo.repository_id.as_str().to_owned(), json!(branch));
        }
        if let Some(commit) = &repo.integrated_commit {
            commits.insert(repo.repository_id.as_str().to_owned(), json!(commit));
        }
    }
    fields.insert("repository_ids".into(), json!(ids));
    fields.insert("branches".into(), Value::Object(branches));
    fields.insert("integrated_commits".into(), Value::Object(commits));
}

/// The front matter of `task.md`: also the `known_facts` of a task's creation event.
pub fn task_fields(task: &TaskRecord, links: &[TaskLinkRecord]) -> Fields {
    let meta = &task.meta;
    let mut fields = Fields::new();
    let mut put = |key: &str, value: Value| {
        fields.insert(key.to_owned(), value);
    };
    put("schema_version", json!(SCHEMA_VERSION));
    put("id", json!(task.id.as_str()));
    put("office_code", json!(task.office_code));
    put("number", json!(task.number));
    put("type", json!(task.task_type));
    put(
        "work_type",
        text_value(task.work_type.map(WorkType::as_str)),
    );
    put("team_id", json!(task.team_id.as_str()));
    put("assigned_team_id", id_value(task.assigned_team_id.as_ref()));
    put("revision", json!(meta.revision));
    put("project_id", id_value(task.project_id.as_ref()));
    put("title", json!(task.title));
    put("status", json!(task.status.as_str()));
    put("assignee_id", id_value(task.assignee_id.as_ref()));
    put("created_at", text_value(meta.created_at.as_deref()));
    put("updated_at", text_value(meta.updated_at.as_deref()));
    put("created_by_kind", json!(meta.created_by_kind));
    put("created_by_id", id_value(meta.created_by_id.as_ref()));
    put("updated_by_kind", json!(meta.updated_by_kind));
    put("updated_by_id", id_value(meta.updated_by_id.as_ref()));
    put("feature_id", id_value(task.feature_id.as_ref()));
    put("milestone_id", id_value(task.milestone_id.as_ref()));
    put("priority", json!(task.priority.as_str()));
    put("effort", task.effort.map_or(Value::Null, |n| json!(n)));
    put("severity", text_value(task.severity.as_deref()));
    put("resolution", text_value(task.resolution.as_deref()));
    put("blocked_reason", text_value(task.blocked_reason.as_deref()));
    put(
        "rank_order",
        task.rank_order.map_or(Value::Null, |n| json!(n)),
    );
    put("labels", json!(task.labels));
    put("reference_files", json!(task.reference_files));
    put("modified_files", json!(task.modified_files));
    let links: Vec<Value> = sorted_links(links)
        .into_iter()
        .map(|l| {
            json!({
                "id": l.id.as_str(),
                "kind": l.kind,
                "target_kind": l.target_kind,
                "target_id": l.target_id.as_str(),
            })
        })
        .collect();
    put("links", Value::Array(links));
    put(
        "details",
        task.details.as_ref().map_or(Value::Null, details_fields),
    );
    // Reserved for task attachments (`TASK-98.4`): always written, empty until supported.
    put("attachments", json!([]));
    repository_fields(task, &mut fields);
    fields
}

/// The bytes of `task.md`, or why they would not fit the `selected-task-resources/1` bounds.
pub fn encode_task_md(task: &TaskRecord, links: &[TaskLinkRecord]) -> Result<Vec<u8>, String> {
    let fields = task_fields(task, links);
    let front = yaml::encode(&fields);
    if front.len() > MAX_FRONT_MATTER_BYTES {
        return Err(format!(
            "the task's front matter would be {} bytes, over the {MAX_FRONT_MATTER_BYTES}-byte limit",
            front.len()
        ));
    }
    if task.body_markdown.len() > MAX_SOURCE_BODY_BYTES {
        return Err(format!(
            "the task's body is over the {MAX_SOURCE_BODY_BYTES}-byte limit"
        ));
    }
    Ok(yaml::encode_document(&fields, &task.body_markdown))
}

fn stable(reader: &mut Reader, key: &str) -> Result<Id, RecordError> {
    let text = reader.text(key)?;
    stable_text(reader, key, &text)
}

fn stable_text(reader: &Reader, key: &str, text: &str) -> Result<Id, RecordError> {
    if !is_stable_id(text) {
        return Err(reader.error(key, "not a lowercase UUIDv7"));
    }
    Id::parse(text).map_err(|_| reader.error(key, "not an ID"))
}

fn optional_stable(reader: &mut Reader, key: &str) -> Result<Option<Id>, RecordError> {
    match reader.optional_text(key)? {
        None => Ok(None),
        Some(text) => stable_text(reader, key, &text).map(Some),
    }
}

fn by(reader: &mut Reader, prefix: &str) -> Result<(String, Option<Id>), RecordError> {
    let kind_key = format!("{prefix}_by_kind");
    let id_key = format!("{prefix}_by_id");
    let kind = reader.text(&kind_key)?;
    let id = optional_stable(reader, &id_key)?;
    match (kind.as_str(), &id) {
        ("employee", Some(_)) | ("system" | "unknown", None) => Ok((kind, id)),
        _ => Err(reader.error(&kind_key, "does not agree with its ID")),
    }
}

fn read_meta(reader: &mut Reader) -> Result<TaskMeta, RecordError> {
    let revision = reader.number("revision")?;
    if revision == 0 {
        return Err(reader.error("revision", "below 1"));
    }
    let created_at = reader.optional_text("created_at")?;
    let updated_at = reader.optional_text("updated_at")?;
    let (created_by_kind, created_by_id) = by(reader, "created")?;
    let (updated_by_kind, updated_by_id) = by(reader, "updated")?;
    Ok(TaskMeta {
        revision,
        created_at,
        created_by_kind,
        created_by_id,
        updated_at,
        updated_by_kind,
        updated_by_id,
    })
}

fn read_details(reader: &mut Reader) -> Result<Option<TaskDetailsRecord>, RecordError> {
    let Some(mut d) = reader.optional_mapping("details")? else {
        return Ok(None);
    };
    let details = TaskDetailsRecord {
        definition_of_done: d.optional_text("definition_of_done")?,
        what_happened: d.optional_text("what_happened")?,
        expected: d.optional_text("expected")?,
        steps: d.optional_text("steps")?,
        environment: d.optional_text("environment")?,
        severity_confirmed: d.flag("severity_confirmed")?,
        reopen_count: d.number("reopen_count")?,
        duplicate_of_task_id: optional_stable(&mut d, "duplicate_of_task_id")?,
        closed_reason: d.optional_text("closed_reason")?,
        affects_feature_id: optional_stable(&mut d, "affects_feature_id")?,
        request_kind: d.optional_text("request_kind")?,
        request_text: d.optional_text("request_text")?,
        route_reason: d.optional_text("route_reason")?,
        routing_source: d.optional_text("routing_source")?,
        routed_by_employee_id: optional_stable(&mut d, "routed_by_employee_id")?,
        merged_into_task_id: optional_stable(&mut d, "merged_into_task_id")?,
        suggested_triager_id: optional_stable(&mut d, "suggested_triager_id")?,
        suggested_verifier_id: optional_stable(&mut d, "suggested_verifier_id")?,
    };
    d.finish()?;
    Ok(Some(details))
}

fn read_links(reader: &mut Reader, task_id: &Id) -> Result<Vec<TaskLinkRecord>, RecordError> {
    let mut links = Vec::new();
    for mut item in reader.list("links")? {
        let link = TaskLinkRecord {
            source_id: task_id.clone(),
            id: stable(&mut item, "id")?,
            kind: item.text("kind")?,
            target_kind: item.text("target_kind")?,
            target_id: stable(&mut item, "target_id")?,
        };
        if !matches!(link.kind.as_str(), "depends_on" | "relates_to" | "fixes") {
            return Err(item.error("kind", "not depends_on, relates_to or fixes"));
        }
        item.finish()?;
        links.push(link);
    }
    Ok(links)
}

fn text_map(reader: &mut Reader, key: &str) -> Result<BTreeMap<String, String>, RecordError> {
    let Value::Object(map) = reader.object(key)? else {
        return Err(reader.error(key, "not a mapping"));
    };
    let mut out = BTreeMap::new();
    for (repo, value) in map {
        let Value::String(text) = value else {
            return Err(reader.error(&format!("{key}.{repo}"), "not a string"));
        };
        out.insert(repo, text);
    }
    Ok(out)
}

fn read_repositories(reader: &mut Reader) -> Result<Vec<TaskRepositoryRecord>, RecordError> {
    let ids = reader.strings("repository_ids")?;
    let mut branches = text_map(reader, "branches")?;
    let mut commits = text_map(reader, "integrated_commits")?;
    let mut repositories = Vec::with_capacity(ids.len());
    for text in ids {
        let repository_id = stable_text(reader, "repository_ids", &text)?;
        repositories.push(TaskRepositoryRecord {
            branch: branches.remove(&text),
            integrated_commit: commits.remove(&text),
            repository_id,
        });
    }
    if let Some(repo) = branches.keys().chain(commits.keys()).next() {
        let reason = format!("names repository `{repo}` that is not in repository_ids");
        return Err(reader.error("branches", reason));
    }
    Ok(repositories)
}

fn normalized(
    reader: &Reader,
    key: &str,
    list: Vec<String>,
    normalize: impl Fn(&[String]) -> Result<Vec<String>, org_proto::ApiError>,
) -> Result<Vec<String>, RecordError> {
    match normalize(&list) {
        Ok(norm) if norm == list => Ok(list),
        _ => Err(reader.error(key, "not a normalized, sorted list without repeats")),
    }
}

fn parse_with<T>(
    reader: &Reader,
    key: &str,
    text: &str,
    parse: fn(&str) -> Option<T>,
) -> Result<T, RecordError> {
    parse(text).ok_or_else(|| reader.error(key, format!("`{text}` is not a known value")))
}

/// The task and links `task.md` holds, strictly: every field once, none unknown.
pub fn decode_task_md(bytes: &[u8]) -> Result<TaskDoc, RecordError> {
    let (mut r, body) = yaml::decode_source_document(bytes)?;
    if r.number("schema_version")? != SCHEMA_VERSION {
        return Err(r.error("schema_version", "not 1"));
    }
    let id = stable(&mut r, "id")?;
    let office_code = r.text("office_code")?;
    if !is_valid_office_code(&office_code) {
        return Err(r.error("office_code", "not an office code"));
    }
    let number = r.number("number")?;
    if number == 0 {
        return Err(r.error("number", "below 1"));
    }
    let task_type = r.text("type")?;
    if task_type.is_empty() {
        return Err(r.error("type", "empty"));
    }
    let work_type = match r.optional_text("work_type")? {
        None => None,
        Some(text) => Some(parse_with(&r, "work_type", &text, WorkType::parse)?),
    };
    let status = r.text("status")?;
    let priority = r.text("priority")?;
    let labels = r.strings("labels")?;
    let reference_files = r.strings("reference_files")?;
    let modified_files = r.strings("modified_files")?;
    let task = TaskRecord {
        team_id: stable(&mut r, "team_id")?,
        assigned_team_id: optional_stable(&mut r, "assigned_team_id")?,
        project_id: optional_stable(&mut r, "project_id")?,
        title: r.text("title")?,
        status: parse_with(&r, "status", &status, TaskStatus::parse)?,
        assignee_id: optional_stable(&mut r, "assignee_id")?,
        body_markdown: body,
        feature_id: optional_stable(&mut r, "feature_id")?,
        milestone_id: optional_stable(&mut r, "milestone_id")?,
        priority: parse_with(&r, "priority", &priority, TaskPriority::parse)?,
        effort: r.optional_number("effort")?,
        severity: r.optional_text("severity")?,
        resolution: r.optional_text("resolution")?,
        blocked_reason: r.optional_text("blocked_reason")?,
        rank_order: r.optional_number("rank_order")?,
        labels: normalized(&r, "labels", labels, normalize_labels)?,
        reference_files: normalized(&r, "reference_files", reference_files, |l| {
            normalize_file_paths(l, "reference")
        })?,
        modified_files: normalized(&r, "modified_files", modified_files, |l| {
            normalize_file_paths(l, "modified")
        })?,
        repositories: read_repositories(&mut r)?,
        details: read_details(&mut r)?,
        meta: read_meta(&mut r)?,
        id,
        office_code,
        number,
        task_type,
        work_type,
    };
    let links = read_links(&mut r, &task.id)?;
    if !r.strings("attachments").is_ok_and(|a| a.is_empty()) {
        let reason = "task attachments are not supported by this office yet";
        return Err(RecordError::new("attachments", reason));
    }
    r.finish()?;
    Ok(TaskDoc { task, links })
}

#[cfg(test)]
#[path = "task_md_tests.rs"]
mod tests;
