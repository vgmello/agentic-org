//! A validated search as the engines see it: its words, the types and filters it narrows to,
//! and the page it asks for. Every filter narrows: a document of a type a filter does not
//! apply to never passes it (an `assignee` filter keeps tasks alone).

use std::collections::BTreeSet;

use org_proto::search::{SearchEntityType, SEARCH_MAX_CURSOR_BYTES};
use org_proto::{ApiError, ErrorCode};

use super::records::SearchDoc;
use super::text::QueryTerm;

fn invalid_cursor() -> ApiError {
    ApiError::new(
        ErrorCode::InvalidRequest,
        "cursor is the next_cursor of a previous page",
    )
}

/// Where a page ended: its last hit's ranking key.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchCursor {
    pub score: f64,
    pub updated_at: String,
    pub entity_type: SearchEntityType,
    pub entity_id: String,
}

impl SearchCursor {
    /// The score's exact bits, then the rest, `~`-separated.
    pub fn encode(&self) -> String {
        format!(
            "{:016x}~{}~{}~{}",
            self.score.to_bits(),
            self.entity_type.as_str(),
            self.updated_at,
            self.entity_id
        )
    }

    /// Parses a `next_cursor`.
    pub fn parse(raw: &str) -> Result<Self, ApiError> {
        if raw.len() > SEARCH_MAX_CURSOR_BYTES {
            return Err(invalid_cursor());
        }
        let mut parts = raw.splitn(4, '~');
        let (Some(bits), Some(kind), Some(updated_at), Some(entity_id)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(invalid_cursor());
        };
        let plain = |text: &str, extra: &[char]| {
            text.chars()
                .all(|c| c.is_ascii_alphanumeric() || extra.contains(&c))
        };
        let score = (bits.len() == 16)
            .then(|| u64::from_str_radix(bits, 16).ok())
            .flatten()
            .map(f64::from_bits)
            .filter(|s| s.is_finite() && *s >= 0.0)
            .ok_or_else(invalid_cursor)?;
        let entity_type = SearchEntityType::parse(kind).ok_or_else(invalid_cursor)?;
        if !plain(updated_at, &['-', ':', '.', '+'])
            || entity_id.is_empty()
            || !plain(entity_id, &['-'])
        {
            return Err(invalid_cursor());
        }
        Ok(Self {
            score,
            updated_at: updated_at.to_owned(),
            entity_type,
            entity_id: entity_id.to_owned(),
        })
    }
}

/// The filters of one search, each resolved to the value the documents hold.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filters {
    /// A project's UUIDv7: its tasks, processes and private notes, and the project itself. A
    /// message's project is its conversation's: the message scope narrows to it.
    pub project_id: Option<String>,
    pub status: Option<String>,
    pub modified_file: Option<String>,
    pub assignee_id: Option<String>,
    /// A normalized task label or private note tag.
    pub label: Option<String>,
    /// A message's sender or a private note's author.
    pub author_id: Option<String>,
    /// A conversation's UUIDv7: its messages alone (the message scope narrows to it).
    pub conversation_id: Option<String>,
}

/// One search, ready for the engines.
#[derive(Clone, Debug)]
pub struct SearchPlan {
    /// The query words; never empty when an engine is asked.
    pub terms: Vec<QueryTerm>,
    pub types: BTreeSet<SearchEntityType>,
    pub filters: Filters,
    /// Where the previous page ended.
    pub after: Option<SearchCursor>,
    /// The page size.
    pub limit: usize,
    /// Count every match, answer no hit.
    pub count_only: bool,
}

impl SearchPlan {
    /// How many hits a source answers: one more than a page, so the caller knows more follow.
    pub fn fetch(&self) -> usize {
        self.limit.saturating_add(1)
    }

    pub fn wants(&self, kind: SearchEntityType) -> bool {
        self.types.contains(&kind)
    }

    /// The shared types this search asks for, in wire order.
    pub fn shared_types(&self) -> Vec<SearchEntityType> {
        SearchEntityType::SHARED
            .into_iter()
            .filter(|kind| self.wants(*kind))
            .collect()
    }

    /// Whether this search asks for any shared type.
    pub fn wants_shared(&self) -> bool {
        SearchEntityType::SHARED
            .iter()
            .any(|kind| self.wants(*kind))
    }

    /// Whether this search asks for private notes or decisions.
    pub fn wants_private(&self) -> bool {
        self.wants(SearchEntityType::PrivateNote) || self.wants(SearchEntityType::PrivateDecision)
    }

    /// Whether `doc` is of a type asked for and passes every filter (the message scope's
    /// conversations aside). Every engine filters by exactly this rule.
    pub fn admits(&self, doc: &SearchDoc) -> bool {
        let f = &self.filters;
        let m = &doc.meta;
        let same = |want: &Option<String>, have: &Option<String>| {
            want.as_ref().is_none_or(|w| have.as_ref() == Some(w))
        };
        let scoped = doc.entity_type == SearchEntityType::Message
            || (f.conversation_id.is_none() && same(&f.project_id, &m.project_id));
        self.wants(doc.entity_type)
            && scoped
            && same(&f.status, &m.status)
            && same(&f.assignee_id, &m.assignee_id)
            && same(&f.author_id, &m.author_id)
            && f.label.as_ref().is_none_or(|l| m.labels.contains(l))
            && f.modified_file
                .as_ref()
                .is_none_or(|p| m.modified_files.contains(p))
    }
}
