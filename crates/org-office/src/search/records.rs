//! What search indexes: one document per entity, its text in five weighted fields, and what
//! a hit shows and the filters read. Built from the domain records by [`super::documents`]
//! the same way for every engine.

use std::fmt;

use org_proto::search::SearchEntityType;

/// How many weighted text fields a document has.
pub const FIELD_COUNT: usize = 5;

/// The BM25 weight of each field, in field order: the key or reference, the title and file
/// paths, labels, tags and headings, the body (why and alternatives too), then comments and
/// checklists.
pub const FIELD_WEIGHTS: [f64; FIELD_COUNT] = [10.0, 6.0, 4.0, 2.0, 1.0];

/// The position of each field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Ref = 0,
    Title = 1,
    Tags = 2,
    Body = 3,
    Extra = 4,
}

/// What a hit shows and the filters read, beside the text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocMeta {
    pub reference: Option<String>,
    pub title: Option<String>,
    pub status: Option<String>,
    pub project_id: Option<String>,
    pub assignee_id: Option<String>,
    pub author_id: Option<String>,
    pub conversation_id: Option<String>,
    pub task_id: Option<String>,
    /// The canonical time of the last change; empty when the record has none.
    pub updated_at: String,
    /// A task's labels or a private note's tags, normalized.
    pub labels: Vec<String>,
    /// The repository paths a task modifies.
    pub modified_files: Vec<String>,
}

/// One searchable document.
#[derive(Clone, PartialEq)]
pub struct SearchDoc {
    pub entity_type: SearchEntityType,
    pub entity_id: String,
    pub meta: DocMeta,
    /// The text of each field, in field order.
    pub fields: [String; FIELD_COUNT],
}

impl SearchDoc {
    pub fn field(&self, field: Field) -> &str {
        &self.fields[field as usize]
    }
}

impl fmt::Debug for SearchDoc {
    /// Never the text: a private note's or a DM's words never reach a log.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SearchDoc")
            .field("entity_type", &self.entity_type)
            .field("entity_id", &self.entity_id)
            .field("reference", &self.meta.reference)
            .field("updated_at", &self.meta.updated_at)
            .field("fields", &"<redacted>")
            .finish_non_exhaustive()
    }
}
