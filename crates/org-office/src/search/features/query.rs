//! `search.query` (`TASK-91`): validates a request, resolves its references in one consistent
//! snapshot, asks each source the caller may search, and merges one page.
//!
//! Private notes and decisions are searched only when asked for on their own, never beside
//! shared knowledge; the office facade asks the private index (HQ only) after the snapshot.

use std::collections::BTreeSet;

use org_proto::chat::ConversationKind;
use org_proto::search::{
    SearchCoverage, SearchEntityType, SearchHit, SearchHitField, SearchOmission,
    SearchOmissionReason, SearchQueryParams, SearchQueryResult, SearchSource, SEARCH_DEFAULT_LIMIT,
    SEARCH_MAX_LIMIT, SEARCH_MAX_PATH_BYTES, SEARCH_MAX_QUERY_BYTES, SEARCH_MAX_REF_BYTES,
};
use org_proto::tasks::TaskStatus;
use org_proto::{ApiError, ErrorCode};
use org_store::{EngineError, Id};

use crate::delivery::ports::DeliveryRead;
use crate::messaging::readable_conversations;
use crate::messaging::records::ConversationRecord;
use crate::organization::records::{Employee, EmployeeKind, EmployeeStatus};
use crate::private_notes::normalize_tag;
use crate::project_docs::access::may_see;
use crate::search::plan::{Filters, SearchPlan};
use crate::search::ports::{MessageScope, SearchRead};
use crate::search::rank::{excerpt, merge, Scored, SearchCursor, SourcePage};
use crate::search::text::query_terms;
use crate::tasks::ports::TasksRead;

/// The statuses a `status` filter may name: a task's, a member's, a team's, project's,
/// process's, document's, or feature's.
const STATUSES: [&str; 10] = [
    "active",
    "paused",
    "inactive",
    "archived",
    "draft",
    "current",
    "deprecated",
    "planned",
    "live",
    "retired",
];

/// The one refusal of a caller who may not search: unknown, inactive, paused or offboarded.
pub fn not_authorized() -> ApiError {
    ApiError::new(
        ErrorCode::NotAuthorized,
        "only an active member of this organization can search it",
    )
}

/// Whether `employee` may search, and whether it is a person: an active, never offboarded
/// member only. Every search checks the caller this way, and again in its snapshot.
pub fn searcher(employee: Option<&Employee>) -> Option<bool> {
    let employee = employee?;
    let active = employee.status == EmployeeStatus::Active && employee.offboarded_at.is_none();
    active.then_some(employee.kind == EmployeeKind::Person)
}

fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::InvalidRequest, message)
}

fn not_found(message: &str) -> ApiError {
    ApiError::new(ErrorCode::NotFound, message)
}

/// A validated `search.query`, its references not yet resolved.
#[derive(Clone, Debug)]
pub struct SearchRequest {
    plan: SearchPlan,
    project: Option<String>,
    channel: Option<String>,
    assignee: Option<String>,
    fields: BTreeSet<SearchHitField>,
}

/// A reference filter: present, trimmed, and at most [`SEARCH_MAX_REF_BYTES`] bytes.
fn reference(value: Option<String>, name: &str) -> Result<Option<String>, ApiError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > SEARCH_MAX_REF_BYTES || trimmed.contains('\0') {
        return Err(invalid(format!(
            "`{name}` is 1 to {SEARCH_MAX_REF_BYTES} bytes"
        )));
    }
    Ok(Some(trimmed.to_owned()))
}

fn types(requested: &[SearchEntityType]) -> Result<BTreeSet<SearchEntityType>, ApiError> {
    let mut types: BTreeSet<SearchEntityType> = requested.iter().copied().collect();
    if types.is_empty() {
        types.extend(SearchEntityType::SHARED);
        types.insert(SearchEntityType::Message);
    }
    let private = types.iter().filter(|kind| kind.is_private()).count();
    if private != 0 && private != types.len() {
        return Err(invalid(
            "private_note and private_decision are searched on their own, never beside shared \
             knowledge",
        ));
    }
    Ok(types)
}

fn status(value: Option<String>) -> Result<Option<String>, ApiError> {
    let Some(value) = reference(value, "status")? else {
        return Ok(None);
    };
    let known = TaskStatus::parse(&value).is_some() || STATUSES.contains(&value.as_str());
    if !known {
        return Err(invalid(
            "`status` is a task, member, team, project or process status",
        ));
    }
    Ok(Some(value))
}

/// A repository path as tasks record it: relative, without `..` or NUL.
fn modified_file(value: Option<String>) -> Result<Option<String>, ApiError> {
    let Some(path) = value else {
        return Ok(None);
    };
    if path.is_empty()
        || path.len() > SEARCH_MAX_PATH_BYTES
        || path.starts_with('/')
        || path.contains("..")
        || path.contains('\0')
    {
        return Err(invalid(
            "`modified_file` is a relative repository path without `..`",
        ));
    }
    Ok(Some(path))
}

/// Validates `params` without reading anything: every refusal comes before a store is read.
pub fn validate(params: SearchQueryParams) -> Result<SearchRequest, ApiError> {
    if params.q.len() > SEARCH_MAX_QUERY_BYTES {
        return Err(invalid(format!(
            "`q` is at most {SEARCH_MAX_QUERY_BYTES} bytes"
        )));
    }
    if params.q.trim().is_empty() {
        return Err(invalid("`q` is the words to search for"));
    }
    let limit = params.limit.unwrap_or(SEARCH_DEFAULT_LIMIT);
    if limit == 0 || limit > SEARCH_MAX_LIMIT {
        return Err(invalid(format!("`limit` is 1 to {SEARCH_MAX_LIMIT}")));
    }
    let label = reference(params.label, "label")?
        .map(|label| normalize_tag(&label))
        .transpose()
        .map_err(|_| invalid("`label` is a task label"))?;
    let author_id = params.author_id;
    if author_id
        .as_deref()
        .is_some_and(|id| !org_proto::is_uuid_v7(id))
    {
        return Err(invalid("`author_id` is a canonical lowercase UUIDv7"));
    }
    let plan = SearchPlan {
        terms: query_terms(&params.q),
        types: types(&params.types)?,
        filters: Filters {
            status: status(params.status)?,
            modified_file: modified_file(params.modified_file)?,
            label,
            author_id,
            ..Filters::default()
        },
        after: params
            .cursor
            .as_deref()
            .map(SearchCursor::parse)
            .transpose()?,
        limit: limit as usize,
        count_only: params.count_only,
        visible_projects: None,
    };
    Ok(SearchRequest {
        plan,
        project: reference(params.project, "project")?,
        channel: reference(params.channel, "channel")?,
        assignee: reference(params.assignee, "assignee")?,
        fields: params.fields.into_iter().collect(),
    })
}

/// A request resolved against one snapshot, with what that snapshot's sources answered.
#[derive(Debug)]
pub struct Resolved {
    /// Whether the caller is a person, as the snapshot holds it.
    caller_is_person: bool,
    plan: SearchPlan,
    pages: Vec<SourcePage>,
    coverage: SearchCoverage,
    fields: BTreeSet<SearchHitField>,
}

impl Resolved {
    pub fn plan(&self) -> &SearchPlan {
        &self.plan
    }

    /// Whether the caller is a person (who reads every author's private notes).
    pub fn caller_is_person(&self) -> bool {
        self.caller_is_person
    }

    /// Whether the private index has something to answer.
    pub fn searches_private(&self) -> bool {
        self.plan.wants_private()
    }

    /// Records the private index's answer: `None` when the query held no word to ask it.
    pub fn private_searched(&mut self, page: Option<SourcePage>) {
        self.coverage.searched.push(SearchSource::PrivateNotes);
        self.pages.extend(page);
    }

    /// Records that this office holds no private notes (a satellite).
    pub fn private_omitted(&mut self) {
        self.coverage.omitted.push(SearchOmission {
            source: SearchSource::PrivateNotes,
            reason: SearchOmissionReason::HqOnly,
        });
    }
}

/// The member a reference names: a UUIDv7 or a handle (with or without `@`).
fn find_member<R: TasksRead>(read: &R, reference: &str) -> Result<Option<String>, EngineError> {
    if let Ok(id) = Id::parse(reference) {
        if let Some(found) = read.employee(&id)? {
            return Ok(Some(found.id.to_string()));
        }
    }
    let handle = reference.trim_start_matches('@').to_ascii_lowercase();
    Ok(read
        .employees()?
        .into_iter()
        .find(|e| e.handle == handle)
        .map(|e| e.id.to_string()))
}

/// The readable conversation a reference names: a UUIDv7, `#name` or a channel's name.
fn find_conversation<'a>(
    readable: &'a [ConversationRecord],
    reference: &str,
) -> Option<&'a ConversationRecord> {
    if let Ok(id) = Id::parse(reference) {
        if let Some(found) = readable.iter().find(|c| c.id == id) {
            return Some(found);
        }
    }
    let name = reference.trim_start_matches('#').to_lowercase();
    readable.iter().find(|c| {
        c.kind == ConversationKind::Channel
            && c.name.as_deref().is_some_and(|n| n.to_lowercase() == name)
    })
}

/// The conversations a message search reads: all the caller may read, and those the channel
/// and project filters keep.
fn message_scope(readable: &[ConversationRecord], filters: &Filters) -> MessageScope {
    let kept = |conv: &&ConversationRecord| {
        let id = conv.id.to_string();
        filters.conversation_id.as_ref().is_none_or(|c| *c == id)
            && filters
                .project_id
                .as_ref()
                .is_none_or(|p| conv.project_id.as_ref().is_some_and(|q| q.as_str() == p))
    };
    MessageScope {
        visible: readable.iter().map(|c| c.id.to_string()).collect(),
        within: readable
            .iter()
            .filter(kept)
            .map(|c| c.id.to_string())
            .collect(),
    }
}

fn resolve_visible_projects<S: DeliveryRead>(
    read: &S,
    caller_is_person: bool,
    caller_emp: &Employee,
) -> Result<Option<BTreeSet<String>>, EngineError> {
    if caller_is_person {
        return Ok(None);
    }
    let mut allowed = BTreeSet::new();
    for p in read.projects()? {
        if may_see(read, caller_emp, &p)? {
            allowed.insert(p.id.to_string());
        }
    }
    Ok(Some(allowed))
}

fn apply_project_filter<S: DeliveryRead>(
    read: &S,
    plan: &mut SearchPlan,
    project: Option<String>,
) -> Result<Result<(), ApiError>, EngineError> {
    let Some(project) = project else {
        return Ok(Ok(()));
    };
    let Some(found) = read.resolve_project(&project)? else {
        return Ok(Err(not_found("project not found")));
    };
    let pid = found.id.to_string();
    let only_project_scoped = !plan.types.is_empty()
        && plan
            .types
            .iter()
            .all(|t| matches!(t, SearchEntityType::Document | SearchEntityType::Feature));
    if only_project_scoped
        && plan
            .visible_projects
            .as_ref()
            .is_some_and(|allowed| !allowed.contains(&pid))
    {
        return Ok(Err(not_found("project not found")));
    }
    plan.filters.project_id = Some(pid);
    Ok(Ok(()))
}

/// Resolves `request`'s references in `read` for `caller` and asks `read`'s shared and message
/// sources. A reference that names nothing the caller may see is `NotFound`, the same whether
/// it does not exist or is hidden.
pub fn read_sources<S: DeliveryRead + SearchRead>(
    read: &S,
    request: SearchRequest,
    owner: &Id,
    caller: &Id,
    now_utc: &str,
) -> Result<Result<Resolved, ApiError>, EngineError> {
    let employee = read.employee(caller)?;
    let Some(caller_is_person) = searcher(employee.as_ref()) else {
        return Ok(Err(not_authorized()));
    };
    let caller_emp = employee.expect("searcher accepted Some");
    let visible_projects = resolve_visible_projects(read, caller_is_person, &caller_emp)?;
    let SearchRequest {
        mut plan,
        project,
        channel,
        assignee,
        fields,
    } = request;
    plan.visible_projects = visible_projects;
    if let Err(err) = apply_project_filter(read, &mut plan, project)? {
        return Ok(Err(err));
    }
    if let Some(assignee) = assignee {
        let Some(found) = find_member(read, &assignee)? else {
            return Ok(Err(not_found("member not found")));
        };
        plan.filters.assignee_id = Some(found);
    }
    let readable = if plan.wants(SearchEntityType::Message) || channel.is_some() {
        readable_conversations(read, owner, caller, now_utc)?
    } else {
        Vec::new()
    };
    if let Some(channel) = channel {
        let Some(found) = find_conversation(&readable, &channel) else {
            return Ok(Err(not_found("conversation not found")));
        };
        plan.filters.conversation_id = Some(found.id.to_string());
    }
    let mut coverage = SearchCoverage::default();
    let mut pages = Vec::new();
    let searching = !plan.terms.is_empty();
    if plan.wants_shared() {
        coverage.searched.push(SearchSource::Shared);
        if searching {
            pages.push(read.search_shared(&plan)?);
        }
    }
    if plan.wants(SearchEntityType::Message) {
        coverage.searched.push(SearchSource::Messages);
        let scope = message_scope(&readable, &plan.filters);
        if searching && !scope.within.is_empty() {
            pages.push(read.search_messages(&plan, &scope)?);
        }
    }
    Ok(Ok(Resolved {
        caller_is_person,
        plan,
        pages,
        coverage,
        fields,
    }))
}

fn hit(scored: &Scored, plan: &SearchPlan, fields: &BTreeSet<SearchHitField>) -> SearchHit {
    let want = |field| fields.is_empty() || fields.contains(&field);
    let meta = &scored.doc.meta;
    let pick = |field, value: &Option<String>| want(field).then(|| value.clone()).flatten();
    SearchHit {
        entity_type: scored.doc.entity_type,
        entity_id: scored.doc.entity_id.clone(),
        reference: pick(SearchHitField::Ref, &meta.reference),
        title: pick(SearchHitField::Title, &meta.title),
        excerpt: want(SearchHitField::Excerpt)
            .then(|| excerpt(&scored.doc, &plan.terms))
            .flatten(),
        score: want(SearchHitField::Score).then_some(scored.score),
        updated_at: want(SearchHitField::UpdatedAt)
            .then(|| meta.updated_at.clone())
            .filter(|at| !at.is_empty()),
        status: pick(SearchHitField::Status, &meta.status),
        project_id: pick(SearchHitField::ProjectId, &meta.project_id),
        conversation_id: pick(SearchHitField::ConversationId, &meta.conversation_id),
        task_id: pick(SearchHitField::TaskId, &meta.task_id),
        author_id: pick(SearchHitField::AuthorId, &meta.author_id),
    }
}

/// The answer: every source's matches merged best first, one page of them with only the
/// fields asked for, the total, and the cursor of the next page.
pub fn answer(resolved: Resolved) -> SearchQueryResult {
    let Resolved {
        plan,
        pages,
        coverage,
        fields,
        ..
    } = resolved;
    let (hits, total, next) = merge(pages, plan.limit);
    SearchQueryResult {
        hits: hits.iter().map(|h| hit(h, &plan, &fields)).collect(),
        total,
        next_cursor: next.map(|cursor| cursor.encode()),
        coverage,
    }
}
