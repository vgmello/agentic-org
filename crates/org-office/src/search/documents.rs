//! The search document of each entity, built from its domain record the same way for every
//! engine (`ENG-1`): the SQLite adapter indexes exactly these documents, and the memory engine
//! scans them.
//!
//! Field weights ([`super::records::FIELD_WEIGHTS`]): the key or reference, then the title and
//! file paths, then labels, tags and headings, then the body (with a decision's why and
//! alternatives), then comments and checklist steps.

use org_proto::private_notes::PrivateNoteKind;
use org_proto::search::SearchEntityType;
use org_store::{EngineError, Id};

use crate::messaging::records::MessageRecord;
use crate::organization::records::{Employee, Role, Team};
use crate::private_notes::{derive_title, PrivateNoteRecord};
use crate::processes::records::StructureDocument;
use crate::projects::records::Project;
use crate::tasks::ports::TasksRead;
use crate::tasks::records::{ChecklistStepRecord, TaskCommentRecord, TaskRecord};

use super::records::{DocMeta, SearchDoc};

/// What a document holds, as a number: bump it on any change to a builder below, so an index
/// built the old way is recognized and rebuilt.
pub const FORMAT: u32 = 1;

/// The non-empty parts, one per line.
fn joined<'a>(parts: impl IntoIterator<Item = &'a str>) -> String {
    let mut out = String::new();
    for part in parts.into_iter().map(str::trim).filter(|p| !p.is_empty()) {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(part);
    }
    out
}

/// The Markdown headings of `body`, without their `#`s.
fn headings(body: &str) -> impl Iterator<Item = &str> {
    body.lines()
        .map(str::trim_start)
        .filter(|line| line.starts_with('#'))
        .map(|line| line.trim_start_matches('#').trim())
}

fn text(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or("")
}

fn id_text(id: &Option<Id>) -> Option<String> {
    id.as_ref().map(Id::to_string)
}

fn doc(kind: SearchEntityType, id: String, meta: DocMeta, fields: [String; 5]) -> SearchDoc {
    SearchDoc {
        entity_type: kind,
        entity_id: id,
        meta,
        fields,
    }
}

/// A task: its key; title and file paths; labels and headings; body and details; public
/// comments and checklist steps.
pub fn task(
    task: &TaskRecord,
    comments: &[TaskCommentRecord],
    steps: &[ChecklistStepRecord],
) -> SearchDoc {
    let key = task.key();
    let files = task.reference_files.iter().chain(&task.modified_files);
    let title = joined(std::iter::once(task.title.as_str()).chain(files.map(String::as_str)));
    let tags = joined(
        task.labels
            .iter()
            .map(String::as_str)
            .chain(headings(&task.body_markdown)),
    );
    let mut body_parts = vec![task.body_markdown.as_str()];
    body_parts.extend([&task.blocked_reason, &task.resolution].map(text));
    if let Some(details) = &task.details {
        body_parts.extend(
            [
                &details.definition_of_done,
                &details.what_happened,
                &details.expected,
                &details.steps,
                &details.environment,
                &details.closed_reason,
                &details.request_text,
                &details.route_reason,
            ]
            .map(text),
        );
    }
    let extra = joined(
        comments
            .iter()
            .map(|c| c.body_markdown.as_str())
            .chain(steps.iter().map(|s| s.text.as_str())),
    );
    let meta = DocMeta {
        reference: Some(key.clone()),
        title: Some(task.title.clone()),
        status: Some(task.status.as_str().to_owned()),
        project_id: id_text(&task.project_id),
        assignee_id: id_text(&task.assignee_id),
        updated_at: task
            .meta
            .updated_at
            .clone()
            .or_else(|| task.meta.created_at.clone())
            .unwrap_or_default(),
        labels: task.labels.clone(),
        modified_files: task.modified_files.clone(),
        ..DocMeta::default()
    };
    let fields = [key, title, tags, joined(body_parts), extra];
    doc(SearchEntityType::Task, task.id.to_string(), meta, fields)
}

/// A project: its key and slug, and its name.
pub fn project(project: &Project) -> SearchDoc {
    let key = project.key();
    let meta = DocMeta {
        reference: Some(key.clone()),
        title: Some(project.name.clone()),
        status: Some(
            if project.archived {
                "archived"
            } else {
                "active"
            }
            .to_owned(),
        ),
        project_id: Some(project.id.to_string()),
        updated_at: project.meta.updated_at.clone().unwrap_or_default(),
        ..DocMeta::default()
    };
    let fields = [
        joined([key.as_str(), project.slug.as_str()]),
        project.name.clone(),
        String::new(),
        String::new(),
        String::new(),
    ];
    doc(
        SearchEntityType::Project,
        project.id.to_string(),
        meta,
        fields,
    )
}

/// A member: handle; name; titles, role text and kind; focus and profile.
pub fn member(employee: &Employee) -> SearchDoc {
    let meta = DocMeta {
        reference: Some(employee.handle.clone()),
        title: Some(employee.name.clone()),
        status: Some(employee.status.as_str().to_owned()),
        updated_at: employee.meta.updated_at.clone().unwrap_or_default(),
        ..DocMeta::default()
    };
    let tags = joined([
        text(&employee.title),
        text(&employee.title_in_team),
        text(&employee.role_text),
        employee.kind.as_str(),
    ]);
    let body = joined([text(&employee.focus), employee.body.as_str()]);
    let fields = [
        employee.handle.clone(),
        employee.name.clone(),
        tags,
        body,
        String::new(),
    ];
    doc(
        SearchEntityType::Member,
        employee.id.to_string(),
        meta,
        fields,
    )
}

/// A team: its key and name.
pub fn team(team: &Team) -> SearchDoc {
    let key = crate::organization::records::team_key(team.number);
    let meta = DocMeta {
        reference: Some(key.clone()),
        title: Some(team.name.clone()),
        status: Some(team.status.as_str().to_owned()),
        updated_at: team.meta.updated_at.clone().unwrap_or_default(),
        ..DocMeta::default()
    };
    let fields = [
        key,
        team.name.clone(),
        String::new(),
        String::new(),
        String::new(),
    ];
    doc(SearchEntityType::Team, team.id.to_string(), meta, fields)
}

/// A role: its key, name, headings and description.
pub fn role(role: &Role) -> SearchDoc {
    let meta = DocMeta {
        reference: role.key.clone(),
        title: Some(role.name.clone()),
        updated_at: role.meta.updated_at.clone().unwrap_or_default(),
        ..DocMeta::default()
    };
    let fields = [
        text(&role.key).to_owned(),
        role.name.clone(),
        joined(headings(&role.body)),
        role.body.clone(),
        String::new(),
    ];
    doc(SearchEntityType::Role, role.id.to_string(), meta, fields)
}

/// An organization process or standard: its key, title, kind and headings, and body.
pub fn process(document: &StructureDocument) -> SearchDoc {
    let meta = DocMeta {
        reference: document.key.clone(),
        title: Some(document.title.clone()),
        status: Some(document.status.as_str().to_owned()),
        project_id: id_text(&document.project_id),
        updated_at: document.meta.updated_at.clone().unwrap_or_default(),
        ..DocMeta::default()
    };
    let tags =
        joined(std::iter::once(document.kind.as_str()).chain(headings(&document.body_markdown)));
    let fields = [
        text(&document.key).to_owned(),
        document.title.clone(),
        tags,
        document.body_markdown.clone(),
        String::new(),
    ];
    doc(
        SearchEntityType::Process,
        document.id.to_string(),
        meta,
        fields,
    )
}

/// A chat message: its subject, body and sender's name.
pub fn message(message: &MessageRecord) -> SearchDoc {
    let meta = DocMeta {
        title: message.subject.clone(),
        author_id: id_text(&message.sender_employee_id),
        conversation_id: Some(message.conversation_id.to_string()),
        updated_at: message.created_at.clone(),
        ..DocMeta::default()
    };
    let fields = [
        String::new(),
        text(&message.subject).to_owned(),
        String::new(),
        message.body_markdown.clone(),
        message.sender_display_name.clone(),
    ];
    doc(SearchEntityType::Message, message.id.clone(), meta, fields)
}

/// A private note, plan or decision: its title, tags, why, alternatives and body, and kind.
/// `None` for a tombstone, which nothing finds.
pub fn private_note(note: &PrivateNoteRecord) -> Option<SearchDoc> {
    if note.deleted_at.is_some() {
        return None;
    }
    let body = text(&note.body_markdown);
    let kind = if note.kind == PrivateNoteKind::Decision {
        SearchEntityType::PrivateDecision
    } else {
        SearchEntityType::PrivateNote
    };
    let meta = DocMeta {
        title: Some(note.title.clone().unwrap_or_else(|| derive_title(body))),
        author_id: Some(note.author_id.clone()),
        project_id: note.project_id.clone(),
        task_id: note.task_id.clone(),
        updated_at: note.updated_at.clone(),
        labels: note.tags.clone(),
        ..DocMeta::default()
    };
    let fields = [
        String::new(),
        text(&note.title).to_owned(),
        joined(note.tags.iter().map(String::as_str)),
        joined([text(&note.why), text(&note.alternatives), body]),
        note.kind.as_str().to_owned(),
    ];
    Some(doc(kind, note.id.clone(), meta, fields))
}

/// The shared document of entity `id` of type `kind` as `read` holds it; `None` when the
/// entity is gone or `kind` is not a shared type.
pub fn shared_doc<R: TasksRead>(
    read: &R,
    kind: SearchEntityType,
    id: &str,
) -> Result<Option<SearchDoc>, EngineError> {
    let Ok(id) = Id::parse(id) else {
        return Ok(None);
    };
    Ok(match kind {
        SearchEntityType::Task => match read.task(&id)? {
            Some(record) => Some(task(
                &record,
                &read.task_comments(&id)?,
                &read.checklist_steps(&id)?,
            )),
            None => None,
        },
        SearchEntityType::Project => read.project(&id)?.as_ref().map(project),
        SearchEntityType::Member => read.employee(&id)?.as_ref().map(member),
        SearchEntityType::Team => read.team(&id)?.as_ref().map(team),
        SearchEntityType::Role => read.role(&id)?.as_ref().map(role),
        SearchEntityType::Process => read.structure_document(&id)?.as_ref().map(process),
        _ => None,
    })
}

/// Every shared document `read` holds.
pub fn shared_docs<R: TasksRead>(read: &R) -> Result<Vec<SearchDoc>, EngineError> {
    let mut docs = Vec::new();
    for record in read.tasks()? {
        let comments = read.task_comments(&record.id)?;
        let steps = read.checklist_steps(&record.id)?;
        docs.push(task(&record, &comments, &steps));
    }
    docs.extend(read.projects()?.iter().map(project));
    docs.extend(read.employees()?.iter().map(member));
    docs.extend(read.teams()?.iter().map(team));
    docs.extend(read.roles()?.iter().map(role));
    docs.extend(read.structure_documents()?.iter().map(process));
    Ok(docs)
}
