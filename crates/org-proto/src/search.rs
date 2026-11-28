//! Wire types and the typed RPC for unified cross-entity search (`TASK-91`): tasks,
//! organization processes and standards, projects, members, teams and roles, the chat messages
//! the caller may read, and, asked for on their own, the caller's permitted private notes and
//! decisions (`ADR-0031`).
//!
//! `Debug` redacts the query text and every hit's title and excerpt, so what a caller searched
//! for, or found, never reaches a log.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::Call;

/// Largest accepted query text, in UTF-8 bytes.
pub const SEARCH_MAX_QUERY_BYTES: usize = 512;
/// Most query words searched; later words are ignored.
pub const SEARCH_MAX_TERMS: usize = 16;
/// Page size when `limit` is absent.
pub const SEARCH_DEFAULT_LIMIT: u32 = 20;
/// Largest accepted page.
pub const SEARCH_MAX_LIMIT: u32 = 100;
/// Largest reference filter (`project`, `channel`, `assignee`, `status`, `label`), in bytes.
pub const SEARCH_MAX_REF_BYTES: usize = 128;
/// Largest `modified_file` path, in bytes (the task file grammar's bound).
pub const SEARCH_MAX_PATH_BYTES: usize = 1024;
/// Largest `cursor`, in bytes.
pub const SEARCH_MAX_CURSOR_BYTES: usize = 256;
/// Most characters (Unicode scalar values) in one hit's excerpt.
pub const SEARCH_EXCERPT_MAX_CHARS: usize = 240;

/// What one hit is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchEntityType {
    Task,
    Process,
    Project,
    Document,
    Feature,
    Member,
    Team,
    Role,
    Message,
    /// A private note or plan: searched only when asked for, never beside shared knowledge.
    PrivateNote,
    /// A private decision: searched only when asked for, never beside shared knowledge.
    PrivateDecision,
}

impl SearchEntityType {
    /// Every type, in wire order.
    pub const ALL: [Self; 11] = [
        Self::Task,
        Self::Process,
        Self::Project,
        Self::Document,
        Self::Feature,
        Self::Member,
        Self::Team,
        Self::Role,
        Self::Message,
        Self::PrivateNote,
        Self::PrivateDecision,
    ];

    /// The shared organization knowledge (project-scoped types filtered by caller visibility).
    pub const SHARED: [Self; 8] = [
        Self::Task,
        Self::Process,
        Self::Project,
        Self::Document,
        Self::Feature,
        Self::Member,
        Self::Team,
        Self::Role,
    ];

    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Process => "process",
            Self::Project => "project",
            Self::Document => "document",
            Self::Feature => "feature",
            Self::Member => "member",
            Self::Team => "team",
            Self::Role => "role",
            Self::Message => "message",
            Self::PrivateNote => "private_note",
            Self::PrivateDecision => "private_decision",
        }
    }

    /// Parses the wire spelling.
    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == raw)
    }

    /// Whether this type is a private note or decision.
    pub const fn is_private(self) -> bool {
        matches!(self, Self::PrivateNote | Self::PrivateDecision)
    }
}

/// A field of a hit that `fields` may ask for. `entity_type` and `entity_id` always come.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchHitField {
    Ref,
    Title,
    Excerpt,
    Score,
    UpdatedAt,
    Status,
    ProjectId,
    ConversationId,
    TaskId,
    AuthorId,
}

impl SearchHitField {
    /// Every optional field, in wire order.
    pub const ALL: [Self; 10] = [
        Self::Ref,
        Self::Title,
        Self::Excerpt,
        Self::Score,
        Self::UpdatedAt,
        Self::Status,
        Self::ProjectId,
        Self::ConversationId,
        Self::TaskId,
        Self::AuthorId,
    ];

    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ref => "ref",
            Self::Title => "title",
            Self::Excerpt => "excerpt",
            Self::Score => "score",
            Self::UpdatedAt => "updated_at",
            Self::Status => "status",
            Self::ProjectId => "project_id",
            Self::ConversationId => "conversation_id",
            Self::TaskId => "task_id",
            Self::AuthorId => "author_id",
        }
    }

    /// Parses the wire spelling.
    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|field| field.as_str() == raw)
    }
}

/// Parameters of `search.query`. Every filter narrows: a hit of a type the filter does not
/// apply to is left out (an `assignee` filter keeps tasks alone).
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchQueryParams {
    /// The words to find (1..=[`SEARCH_MAX_QUERY_BYTES`] bytes). Punctuation and search-engine
    /// operators are plain text: every word is matched as itself and as a prefix.
    pub q: String,
    /// The types to search. None: every shared type and the messages the caller may read.
    /// `private_note` and `private_decision` are searched only on their own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub types: Vec<SearchEntityType>,
    /// A project (UUIDv7, `P-<n>` or slug): its tasks, processes, project channels and the
    /// project itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// A conversation the caller may read (UUIDv7, `#name` or name): its messages alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    /// A task, process, member, team or project status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// A repository path a task modifies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_file: Option<String>,
    /// A task's assignee (UUIDv7 or handle).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    /// A task label or a private note tag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// A message's sender or a private note's author (UUIDv7). An agent sees only its own
    /// private notes: another author's ID answers it nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_id: Option<String>,
    /// Answer the total alone, with no hits.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub count_only: bool,
    /// 1..=[`SEARCH_MAX_LIMIT`]; defaults to [`SEARCH_DEFAULT_LIMIT`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// The `next_cursor` of the previous page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// The optional hit fields to return; empty returns them all.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<SearchHitField>,
}

impl fmt::Debug for SearchQueryParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SearchQueryParams")
            .field("q", &format_args!("<{} bytes redacted>", self.q.len()))
            .field("types", &self.types)
            .field("project", &self.project)
            .field("channel", &self.channel)
            .field("status", &self.status)
            .field("modified_file", &self.modified_file)
            .field("assignee", &self.assignee)
            .field("label", &self.label)
            .field("author_id", &self.author_id)
            .field("count_only", &self.count_only)
            .field("limit", &self.limit)
            .field("cursor", &self.cursor)
            .field("fields", &self.fields)
            .finish()
    }
}

/// One search hit. Hits come best first: higher `score`, then later `updated_at`, then
/// `entity_id`.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    pub entity_type: SearchEntityType,
    pub entity_id: String,
    /// The readable reference: `T-HQ12`, `P-3`, `TEAM-2`, a handle, a process key.
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// At most [`SEARCH_EXCERPT_MAX_CHARS`] characters around the first match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
    /// Field-weighted BM25 over what the caller may see; higher is better.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_id: Option<String>,
}

impl fmt::Debug for SearchHit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let redacted = |value: &Option<String>| value.as_ref().map(|_| "<redacted>");
        f.debug_struct("SearchHit")
            .field("entity_type", &self.entity_type)
            .field("entity_id", &self.entity_id)
            .field("reference", &self.reference)
            .field("title", &redacted(&self.title))
            .field("excerpt", &redacted(&self.excerpt))
            .field("score", &self.score)
            .field("updated_at", &self.updated_at)
            .field("status", &self.status)
            .field("project_id", &self.project_id)
            .field("conversation_id", &self.conversation_id)
            .field("task_id", &self.task_id)
            .field("author_id", &self.author_id)
            .finish()
    }
}

/// One of the three physically separate stores a search reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchSource {
    /// Shared organization knowledge (`office.db`).
    Shared,
    /// Chat messages (`protected/messaging.db`).
    Messages,
    /// Private notes and decisions (HQ only).
    PrivateNotes,
}

/// Why a source the request asked for was not searched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchOmissionReason {
    /// Private notes live only at HQ: a satellite holds none.
    HqOnly,
}

/// A source the request asked for that this office did not search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchOmission {
    pub source: SearchSource,
    pub reason: SearchOmissionReason,
}

/// What an answer covers: the sources searched and those the request asked for but this
/// office could not search.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchCoverage {
    pub searched: Vec<SearchSource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub omitted: Vec<SearchOmission>,
}

/// Response of `search.query`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SearchQueryResult {
    /// Empty when `count_only`.
    #[serde(default)]
    pub hits: Vec<SearchHit>,
    /// Every visible hit matching the query and filters, across all pages.
    pub total: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub coverage: SearchCoverage,
}

/// `search.query`
pub struct SearchQuery;
impl Call for SearchQuery {
    const METHOD: &'static str = "search.query";
    type Params = SearchQueryParams;
    type Result = SearchQueryResult;
}

#[cfg(test)]
mod tests;
