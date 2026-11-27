//! The consumer-owned search ports (`doc-104` P0: `pub`, so a later engine such as a
//! PostgreSQL `tsvector` one implements them as the SQLite FTS5 and memory engines do).
//!
//! Three physically separate stores answer a search, and none ever sees another's documents:
//!
//! - [`SearchRead::search_shared`]: shared organization knowledge (`office.db` on SQLite);
//! - [`SearchRead::search_messages`]: chat messages (`protected/messaging.db` on SQLite),
//!   restricted to the conversations the caller may read before anything is scored, counted
//!   or cut to a page;
//! - [`PrivateNotesSearch`]: private notes and decisions, HQ only, restricted by `ADR-0031`
//!   before anything is scored, counted or cut.
//!
//! Every engine answers with the same documents ([`super::documents`]), filters
//! ([`SearchPlan::admits`]), scores ([`super::rank`]) and order.

use org_store::{Engine, EngineError};

use crate::error::OfficeError;

use super::plan::SearchPlan;
use super::rank::SourcePage;

/// The conversations a message search reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MessageScope {
    /// Every conversation the caller may read: the corpus its scores are computed over.
    pub visible: Vec<String>,
    /// Those the search's channel and project filters keep (a subset of `visible`).
    pub within: Vec<String>,
}

/// Full-text reads of one consistent snapshot of the office store.
pub trait SearchRead {
    /// The shared documents of the types `plan` asks for that match every word and filter:
    /// their total, and the best after `plan.after`, at most `plan.fetch()` (none when
    /// counting). Every active member may see every shared document.
    fn search_shared(&self, plan: &SearchPlan) -> Result<SourcePage, EngineError>;

    /// The chat messages in `scope.within` that match every word and filter, ranked over the
    /// messages in `scope.visible` alone. A message outside `scope.visible` is never read,
    /// counted or scored.
    fn search_messages(
        &self,
        plan: &SearchPlan,
        scope: &MessageScope,
    ) -> Result<SourcePage, EngineError>;
}

/// An engine whose snapshots answer [`SearchRead`].
pub trait SearchEngine: Engine<Snapshot: SearchRead> {}

impl<E> SearchEngine for E where E: Engine<Snapshot: SearchRead> {}

/// Who searches private notes (`ADR-0031`).
#[derive(Clone, Copy, Debug)]
pub struct PrivateScope<'a> {
    pub org_id: &'a str,
    pub caller_id: &'a str,
    /// An active person reads every author's notes; an agent only its own.
    pub caller_is_person: bool,
}

impl PrivateScope<'_> {
    /// The only author whose notes the caller may see, or `None` for every author (a
    /// person). An agent's corpus is its own notes, so an `author_id` filter naming anyone
    /// else matches nothing, whatever team it leads.
    pub fn visible_author(&self) -> Option<&str> {
        (!self.caller_is_person).then_some(self.caller_id)
    }
}

/// The disposable full-text index of private notes and decisions (HQ only).
pub trait PrivateNotesSearch: Send + Sync {
    /// Whether this office holds private notes at all: `false` on a satellite.
    fn available(&self) -> bool {
        true
    }

    /// The private notes and decisions of the types `plan` asks for that `scope` may see and
    /// that match every word and filter, ranked over what `scope` may see alone.
    fn search(
        &self,
        plan: &SearchPlan,
        scope: &PrivateScope<'_>,
    ) -> Result<SourcePage, OfficeError>;
}

/// A satellite's private search: private notes live only at HQ, so it searches nothing and
/// creates nothing (`ARCH-02`).
#[derive(Debug, Default)]
pub struct SatelliteNoPrivateSearch;

impl PrivateNotesSearch for SatelliteNoPrivateSearch {
    fn available(&self) -> bool {
        false
    }

    fn search(
        &self,
        _plan: &SearchPlan,
        _scope: &PrivateScope<'_>,
    ) -> Result<SourcePage, OfficeError> {
        Ok(SourcePage::default())
    }
}
