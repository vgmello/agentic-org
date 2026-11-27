//! A validated search as the engines see it: its words, the types and filters it narrows to,
//! and the page it asks for. Every filter narrows: a document of a type a filter does not
//! apply to never passes it (an `assignee` filter keeps tasks alone).

use std::collections::BTreeSet;

use org_proto::search::SearchEntityType;

use super::rank::SearchCursor;
use super::records::SearchDoc;
use super::text::QueryTerm;

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
