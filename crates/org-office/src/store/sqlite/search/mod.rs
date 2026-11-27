//! The SQLite adapter of search (`TASK-91`). Its FTS5 indexes are adapter-private: no data
//! contract's DDL (`schema.sql`, `messaging-schema.sql`, `private-notes-schema.sql`) holds a
//! virtual table, and each index lives beside the rows it indexes and nowhere else:
//!
//! - `office.db`: shared organization knowledge ([`shared`]);
//! - `protected/messaging.db`: chat messages ([`messages`]);
//! - HQ's disposable `protected/private/task-notes-search.db`: private notes and decisions
//!   ([`private`]).
//!
//! The first two follow the projection. Triggers on the projected tables queue each changed
//! entity, and the writer's commit rebuilds every queued document from the records inside the
//! same transaction ([`refresh`]), so a full rebuild of the projection (`runtime/<office-id>/`
//! deleted) rebuilds them with everything else. Shared knowledge, which every active member
//! may see, is ranked by FTS5's own `bm25()`. Messages and private notes are ranked by
//! [`crate::search::rank`] over the documents the caller may see alone, so nothing the caller
//! may not see is ever counted, scored or cut to a page.

mod fts5;
mod messages;
pub(crate) mod private;
mod shared;
#[cfg(test)]
mod tests;

use org_store::EngineError;
use rusqlite::types::Value;
use rusqlite::{params_from_iter, Connection, Row};

use crate::search::plan::SearchPlan;
use crate::search::ports::{MessageScope, SearchRead};
use crate::search::rank::{scored_page, CorpusStats, SourcePage};
use crate::search::records::{SearchDoc, FIELD_COUNT};

use super::organization::View;
use super::{failed, SqliteSnapshot, SqliteTx};

pub(super) use messages::DDL as MESSAGING_SEARCH_DDL;
pub(super) use shared::DDL as SHARED_SEARCH_DDL;

/// The names every search table of a projection starts with: rowid-keyed, derived from rows a
/// projection's dump already lists, so a dump leaves them out (FTS5's shadow tables too).
#[cfg(any(test, feature = "testkit"))]
pub(super) const DUMP_SKIPPED_PREFIXES: [&str; 2] = ["search_", "message_search_"];

/// A `WHERE` clause made only of fixed fragments, and its bound values.
#[derive(Clone, Debug, Default)]
struct Clause {
    sql: String,
    values: Vec<Value>,
}

impl Clause {
    fn new(fragment: &str, values: impl IntoIterator<Item = Value>) -> Self {
        Self {
            sql: fragment.to_owned(),
            values: values.into_iter().collect(),
        }
    }

    fn and(&mut self, fragment: &str, values: impl IntoIterator<Item = Value>) {
        self.sql.push_str(" AND ");
        self.sql.push_str(fragment);
        self.values.extend(values);
    }
}

fn text(value: &str) -> Value {
    Value::Text(value.to_owned())
}

/// The five indexed fields of a row whose first five columns are they.
fn fields(row: &Row<'_>) -> rusqlite::Result<[String; FIELD_COUNT]> {
    Ok([
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
    ])
}

/// One index whose hits are ranked in Rust over the documents the caller may see.
struct Ranked<'a> {
    db: &'a Connection,
    fts: &'static str,
    docs: &'static str,
    /// The `d.` columns a hit reads after its five fields.
    columns: &'static str,
    /// A document from a row of the five fields, then `columns`.
    build: fn(&Row<'_>) -> rusqlite::Result<SearchDoc>,
}

impl Ranked<'_> {
    /// How many documents `keep` keeps that match `matched`.
    fn count(&self, matched: &str, keep: &Clause) -> rusqlite::Result<u64> {
        let (fts, docs) = (self.fts, self.docs);
        let sql = format!(
            "SELECT count(*) FROM {fts} JOIN {docs} AS d ON d.doc = {fts}.rowid \
             WHERE {fts} MATCH ? AND {}",
            keep.sql
        );
        let values = std::iter::once(text(matched)).chain(keep.values.iter().cloned());
        let count: i64 = self
            .db
            .prepare_cached(&sql)?
            .query_row(params_from_iter(values), |row| row.get(0))?;
        Ok(u64::try_from(count).unwrap_or(0))
    }

    /// The statistics of the documents `visible` keeps.
    fn stats(&self, visible: &Clause, plan: &SearchPlan) -> rusqlite::Result<CorpusStats> {
        let sql = format!(
            "SELECT count(*), coalesce(sum(d.token_count), 0) FROM {} AS d WHERE {}",
            self.docs, visible.sql
        );
        let (docs, tokens): (i64, i64) = self
            .db
            .prepare_cached(&sql)?
            .query_row(params_from_iter(visible.values.iter()), |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?;
        let term_docs = plan
            .terms
            .iter()
            .map(|term| self.count(&fts5::phrase(term), visible))
            .collect::<rusqlite::Result<_>>()?;
        Ok(CorpusStats {
            docs: u64::try_from(docs).unwrap_or(0),
            tokens: u64::try_from(tokens).unwrap_or(0),
            term_docs,
        })
    }

    /// Every document `keep` keeps that matches `matched`.
    fn candidates(&self, matched: &str, keep: &Clause) -> rusqlite::Result<Vec<SearchDoc>> {
        let (fts, docs, columns) = (self.fts, self.docs, self.columns);
        let sql = format!(
            "SELECT f.f_ref, f.f_title, f.f_tags, f.f_body, f.f_extra, {columns} \
             FROM (SELECT rowid AS doc, {cols} FROM {fts} WHERE {fts} MATCH ?) AS f \
             JOIN {docs} AS d ON d.doc = f.doc WHERE {}",
            keep.sql,
            cols = fts5::COLUMNS,
        );
        let values = std::iter::once(text(matched)).chain(keep.values.iter().cloned());
        let mut statement = self.db.prepare_cached(&sql)?;
        let rows = statement.query_map(params_from_iter(values), self.build)?;
        rows.collect()
    }

    /// The page `plan` asks of the documents `keep` keeps, ranked over those `visible` keeps
    /// (`keep` is `visible` and the filters).
    fn page(
        &self,
        plan: &SearchPlan,
        visible: &Clause,
        keep: &Clause,
    ) -> rusqlite::Result<SourcePage> {
        let matched = fts5::match_all(&plan.terms);
        if plan.count_only {
            return Ok(SourcePage {
                total: self.count(&matched, keep)?,
                hits: Vec::new(),
            });
        }
        let stats = self.stats(visible, plan)?;
        let candidates = self.candidates(&matched, keep)?;
        Ok(scored_page(candidates, &stats, plan))
    }
}

impl SearchRead for SqliteSnapshot {
    fn search_shared(&self, plan: &SearchPlan) -> Result<SourcePage, EngineError> {
        let (db, scope) = self.view()?;
        shared::search(db, scope, plan).map_err(|e| failed(db, "searching shared knowledge", &e))
    }

    fn search_messages(
        &self,
        plan: &SearchPlan,
        scope: &MessageScope,
    ) -> Result<SourcePage, EngineError> {
        let (_, organization) = self.view()?;
        let db = self.msg_db();
        messages::search(db, organization, plan, scope)
            .map_err(|e| failed(db, "searching messages", &e))
    }
}

/// Rebuilds the documents the transaction's changes queued, in both databases, before the
/// writer commits. A transaction of no organization queues nothing it could rebuild.
pub(super) fn refresh(tx: &SqliteTx) -> Result<(), EngineError> {
    let Some(organization) = tx.organization.as_ref().map(|o| o.as_str().to_owned()) else {
        return Ok(());
    };
    shared::refresh(tx, &organization)?;
    messages::refresh(tx, &organization)
}
