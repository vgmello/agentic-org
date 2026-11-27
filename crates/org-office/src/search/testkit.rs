//! The memory engine's search, in pure Rust (`ENG-1`, `ENG-2`): [`SearchRead`] for the memory
//! store's snapshot and [`MemoryPrivateNotesSearch`] over the memory private notes. It scans
//! the very documents the SQLite adapter indexes ([`documents`]) and applies the same
//! visibility, filters, field weights, BM25 and tie-breakers, so both engines answer a search
//! alike. Rebuilt from the records on every read, it has nothing to delete or rebuild.

use std::collections::BTreeSet;
use std::sync::Arc;

use org_store::testkit::MemorySnapshot;
use org_store::EngineError;

use crate::error::OfficeError;
use crate::messaging::ports::MessagingRead;
use crate::private_notes::memory::MemoryPrivateNotesStore;

use super::documents;
use super::plan::SearchPlan;
use super::ports::{MessageScope, PrivateNotesSearch, PrivateScope, SearchRead};
use super::rank::{analyze_corpus, scored_page, SourcePage};
use super::records::SearchDoc;

/// The page of `visible`'s documents that match every word and filter and that `keep` keeps,
/// ranked over `visible` alone.
pub fn scan(
    visible: Vec<SearchDoc>,
    plan: &SearchPlan,
    keep: impl Fn(&SearchDoc) -> bool,
) -> SourcePage {
    let (stats, matches) = analyze_corpus(&visible, &plan.terms);
    let candidates = visible
        .into_iter()
        .zip(matches)
        .filter(|(doc, m)| m.matches_all() && plan.admits(doc) && keep(doc))
        .map(|(doc, _)| doc);
    scored_page(candidates, &stats, plan)
}

impl SearchRead for MemorySnapshot {
    fn search_shared(&self, plan: &SearchPlan) -> Result<SourcePage, EngineError> {
        Ok(scan(documents::shared_docs(self)?, plan, |_| true))
    }

    fn search_messages(
        &self,
        plan: &SearchPlan,
        scope: &MessageScope,
    ) -> Result<SourcePage, EngineError> {
        let visible: BTreeSet<&str> = scope.visible.iter().map(String::as_str).collect();
        let within: BTreeSet<&str> = scope.within.iter().map(String::as_str).collect();
        let docs = self
            .all_messages()?
            .iter()
            .filter(|m| visible.contains(m.conversation_id.as_str()))
            .map(documents::message)
            .collect();
        Ok(scan(docs, plan, |doc| {
            doc.meta
                .conversation_id
                .as_deref()
                .is_some_and(|c| within.contains(c))
        }))
    }
}

/// The memory engine's private notes search, over the memory private notes store.
pub struct MemoryPrivateNotesSearch {
    notes: Arc<MemoryPrivateNotesStore>,
}

impl MemoryPrivateNotesSearch {
    pub fn new(notes: Arc<MemoryPrivateNotesStore>) -> Self {
        Self { notes }
    }
}

impl PrivateNotesSearch for MemoryPrivateNotesSearch {
    fn search(
        &self,
        plan: &SearchPlan,
        scope: &PrivateScope<'_>,
    ) -> Result<SourcePage, OfficeError> {
        let author = scope.visible_author();
        let visible = self
            .notes
            .active_notes(scope.org_id)?
            .iter()
            .filter(|note| author.is_none_or(|a| note.author_id == a))
            .filter_map(documents::private_note)
            .collect();
        Ok(scan(visible, plan, |_| true))
    }
}
