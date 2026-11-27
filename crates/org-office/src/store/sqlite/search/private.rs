//! The disposable full-text index of HQ's private notes, plans and decisions (`TASK-91`,
//! `TASK-111`, `ADR-0031`): `runtime/<office-id>/protected/private/task-notes-search.db`, an
//! owner-only (`0700` folder, `0600` file) copy of the indexed text of the notes in
//! `durable/private/task-notes.db`, which it only ever reads.
//!
//! - It sits inside `protected/`, the runtime folder every agent sandbox masks, never beside
//!   it: a private note's words are never readable from an agent's session.
//! - It is deleted and made anew every time the office opens, and filled from the notes on the
//!   first search; after that a search brings it up to date first whenever the notes database
//!   changed (`PRAGMA data_version`), by each note's revision. Deleting `runtime/` loses
//!   nothing.
//! - `temp_store = MEMORY` on both connections: no sort or temporary table of note text ever
//!   reaches a temporary file.
//! - A satellite never opens or creates it ([`crate::search::ports::SatelliteNoPrivateSearch`]).
//!
//! Indexed: the title (6x), the tags (4x), the why, alternatives and body (2x) and the kind
//! (1x). A search keeps an agent to its own notes, and lets an active person read every
//! author's, before anything is counted, scored or cut; the scores are computed over those
//! notes alone.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use org_proto::private_notes::PrivateNoteKind;
use org_proto::search::SearchEntityType;
use rusqlite::{params, Connection, OpenFlags, Row};

use crate::error::OfficeError;
use crate::private_fs::{
    absolute, private_dir, private_file, tighten_dir, tighten_sqlite_sidecars,
};
use crate::search::documents::{self, FORMAT};
use crate::search::plan::SearchPlan;
use crate::search::ports::{PrivateNotesSearch, PrivateScope};
use crate::search::rank::{doc_len, SourcePage};
use crate::search::records::{DocMeta, SearchDoc};
use crate::store::layout;

use super::super::connection::{connect_reader, remove_database};
use super::super::private_notes::load_note_for_index;
use super::{fields, fts5, text, Clause, Ranked};

/// The index's file name, in `runtime/<office-id>/protected/private/`.
pub(crate) const INDEX_FILE: &str = "task-notes-search.db";

const TABLES: &str = "
CREATE TABLE private_search_docs (
  doc INTEGER PRIMARY KEY,
  organization_id TEXT NOT NULL,
  note_id TEXT NOT NULL,
  revision INTEGER NOT NULL CHECK (revision >= 1),
  kind TEXT NOT NULL,
  author_id TEXT NOT NULL,
  task_id TEXT,
  project_id TEXT,
  title TEXT,
  updated_at TEXT NOT NULL,
  token_count INTEGER NOT NULL CHECK (token_count >= 0),
  UNIQUE (organization_id, note_id)
) STRICT;
CREATE INDEX private_search_docs_by_author ON private_search_docs(organization_id, author_id);
CREATE TABLE private_search_tags (
  doc INTEGER NOT NULL,
  tag TEXT NOT NULL,
  PRIMARY KEY (doc, tag)
) STRICT, WITHOUT ROWID;
";

fn storage(error: impl std::error::Error + Send + Sync + 'static) -> OfficeError {
    OfficeError::storage(error)
}

/// The folder of the index: `runtime/<office-id>/protected/private/`.
pub(crate) fn index_folder(runtime_root: &Path, office_id: &str) -> PathBuf {
    runtime_root
        .join(office_id)
        .join("protected")
        .join("private")
}

struct Connections {
    /// `durable/private/task-notes.db`, read only (`query_only`, never checkpoints).
    notes: Connection,
    index: Connection,
    /// The notes database's `data_version` the index was last brought up to.
    synced: Option<i64>,
}

/// HQ's private notes search over its disposable FTS5 index.
pub struct SqlitePrivateNotesSearch {
    organization: String,
    connections: Mutex<Connections>,
}

impl SqlitePrivateNotesSearch {
    /// Deletes and makes anew the index of the notes in `<durable_root>/private/task-notes.db`
    /// (opened first, by the private notes store), which the first search fills.
    pub fn open(
        durable_root: &Path,
        runtime_root: &Path,
        office_id: &str,
        organization: &str,
    ) -> Result<Self, OfficeError> {
        let protected = runtime_root.join(office_id).join("protected");
        let folder = index_folder(runtime_root, office_id);
        private_dir(&folder).map_err(storage)?;
        tighten_dir(&protected).map_err(storage)?;
        tighten_dir(&folder).map_err(storage)?;
        let path = absolute(folder.join(INDEX_FILE)).map_err(storage)?;
        remove_database(&path).map_err(storage)?;
        drop(private_file(&path).map_err(storage)?);
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let index = Connection::open_with_flags(&path, flags).map_err(storage)?;
        index
            .execute_batch(&format!(
                "PRAGMA temp_store = MEMORY; PRAGMA journal_mode = DELETE; \
                 PRAGMA synchronous = OFF; -- private search index, format {FORMAT}\n{TABLES}{}",
                fts5::create_index("private_search_fts"),
            ))
            .map_err(storage)?;
        tighten_sqlite_sidecars(&path).map_err(storage)?;
        let notes_path = layout::private_notes(durable_root).join("task-notes.db");
        let notes = connect_reader(&notes_path).map_err(storage)?;
        notes
            .execute_batch("PRAGMA temp_store = MEMORY;")
            .map_err(storage)?;
        Ok(Self {
            organization: organization.to_owned(),
            connections: Mutex::new(Connections {
                notes,
                index,
                synced: None,
            }),
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connections>, OfficeError> {
        self.connections
            .lock()
            .map_err(|_| OfficeError::Invalid("private notes search index lock poisoned"))
    }
}

/// The revision of every note of `organization` in the notes database, active or not.
fn note_revisions(
    notes: &Connection,
    organization: &str,
) -> rusqlite::Result<BTreeMap<String, (i64, bool)>> {
    let mut statement = notes.prepare_cached(
        "SELECT id, revision, deleted_at IS NULL FROM private_notes WHERE organization_id = ?1",
    )?;
    let rows = statement.query_map([organization], |row| {
        Ok((row.get(0)?, (row.get(1)?, row.get(2)?)))
    })?;
    rows.collect()
}

/// The revision of every note the index holds.
fn indexed_revisions(
    index: &Connection,
    organization: &str,
) -> rusqlite::Result<BTreeMap<String, (i64, i64)>> {
    let mut statement = index.prepare_cached(
        "SELECT note_id, doc, revision FROM private_search_docs WHERE organization_id = ?1",
    )?;
    let rows = statement.query_map([organization], |row| {
        Ok((row.get(0)?, (row.get(1)?, row.get(2)?)))
    })?;
    rows.collect()
}

fn remove(index: &Connection, doc: i64) -> rusqlite::Result<()> {
    for sql in [
        "DELETE FROM private_search_fts WHERE rowid = ?1",
        "DELETE FROM private_search_tags WHERE doc = ?1",
        "DELETE FROM private_search_docs WHERE doc = ?1",
    ] {
        index.prepare_cached(sql)?.execute([doc])?;
    }
    Ok(())
}

fn insert(
    index: &Connection,
    organization: &str,
    revision: u64,
    doc: &SearchDoc,
) -> rusqlite::Result<()> {
    let m = &doc.meta;
    let kind = doc.field(crate::search::records::Field::Extra);
    index
        .prepare_cached(
            "INSERT INTO private_search_docs (organization_id, note_id, revision, kind, \
             author_id, task_id, project_id, title, updated_at, token_count) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )?
        .execute(params![
            organization,
            doc.entity_id,
            i64::try_from(revision).unwrap_or(i64::MAX),
            kind,
            m.author_id,
            m.task_id,
            m.project_id,
            m.title,
            m.updated_at,
            i64::try_from(doc_len(doc)).unwrap_or(i64::MAX),
        ])?;
    let row = index.last_insert_rowid();
    for tag in &m.labels {
        index
            .prepare_cached("INSERT OR IGNORE INTO private_search_tags (doc, tag) VALUES (?1, ?2)")?
            .execute(params![row, tag])?;
    }
    let [a, b, c, d, e] = &doc.fields;
    index
        .prepare_cached(
            "INSERT INTO private_search_fts (rowid, f_ref, f_title, f_tags, f_body, f_extra) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?
        .execute(params![row, a, b, c, d, e])?;
    Ok(())
}

/// Brings the index up to the notes database when it changed since the last search: every
/// note whose revision differs is indexed again, every note gone or tombstoned leaves.
fn sync(c: &mut Connections, organization: &str) -> Result<(), OfficeError> {
    let version: i64 = c
        .notes
        .query_row("PRAGMA data_version", [], |row| row.get(0))
        .map_err(storage)?;
    if c.synced == Some(version) {
        return Ok(());
    }
    let notes = note_revisions(&c.notes, organization).map_err(storage)?;
    let indexed = indexed_revisions(&c.index, organization).map_err(storage)?;
    let tx = c.index.transaction().map_err(storage)?;
    for (note_id, (doc, revision)) in &indexed {
        if notes.get(note_id) != Some(&(*revision, true)) {
            remove(&tx, *doc).map_err(storage)?;
        }
    }
    for (note_id, (revision, active)) in &notes {
        let current = indexed.get(note_id).is_some_and(|(_, r)| r == revision);
        if !*active || current {
            continue;
        }
        let Some(note) = load_note_for_index(&c.notes, organization, note_id)? else {
            continue;
        };
        if let Some(doc) = documents::private_note(&note) {
            insert(&tx, organization, note.revision, &doc).map_err(storage)?;
        }
    }
    tx.commit().map_err(storage)?;
    c.synced = Some(version);
    Ok(())
}

const COLUMNS: &str = "d.note_id, d.kind, d.author_id, d.task_id, d.project_id, d.title, \
     d.updated_at, (SELECT group_concat(t.tag, char(10)) FROM private_search_tags AS t \
     WHERE t.doc = d.doc)";

fn doc(row: &Row<'_>) -> rusqlite::Result<SearchDoc> {
    let kind: String = row.get(6)?;
    let tags: Option<String> = row.get(12)?;
    let mut labels: Vec<String> = tags
        .map(|t| t.split('\n').map(str::to_owned).collect())
        .unwrap_or_default();
    labels.sort();
    Ok(SearchDoc {
        entity_type: if kind == PrivateNoteKind::Decision.as_str() {
            SearchEntityType::PrivateDecision
        } else {
            SearchEntityType::PrivateNote
        },
        entity_id: row.get(5)?,
        meta: DocMeta {
            author_id: Some(row.get(7)?),
            task_id: row.get(8)?,
            project_id: row.get(9)?,
            title: row.get(10)?,
            updated_at: row.get(11)?,
            labels,
            ..DocMeta::default()
        },
        fields: fields(row)?,
    })
}

/// The kinds the asked-for types name.
fn kinds(plan: &SearchPlan) -> Vec<&'static str> {
    let mut kinds = Vec::new();
    if plan.wants(SearchEntityType::PrivateNote) {
        kinds.extend([
            PrivateNoteKind::Note.as_str(),
            PrivateNoteKind::Plan.as_str(),
        ]);
    }
    if plan.wants(SearchEntityType::PrivateDecision) {
        kinds.push(PrivateNoteKind::Decision.as_str());
    }
    kinds
}

fn search(
    index: &Connection,
    plan: &SearchPlan,
    scope: &PrivateScope<'_>,
) -> rusqlite::Result<SourcePage> {
    let f = &plan.filters;
    let never = f.status.is_some()
        || f.assignee_id.is_some()
        || f.modified_file.is_some()
        || f.conversation_id.is_some();
    let kinds = kinds(plan);
    if kinds.is_empty() || plan.terms.is_empty() || never {
        return Ok(SourcePage::default());
    }
    let mut visible = Clause::new("d.organization_id = ?", [text(scope.org_id)]);
    if let Some(author) = scope.visible_author() {
        visible.and("d.author_id = ?", [text(author)]);
    }
    let mut keep = visible.clone();
    let marks = vec!["?"; kinds.len()].join(", ");
    keep.and(
        &format!("d.kind IN ({marks})"),
        kinds.iter().map(|k| text(k)),
    );
    if let Some(author) = &f.author_id {
        keep.and("d.author_id = ?", [text(author)]);
    }
    if let Some(project) = &f.project_id {
        keep.and("d.project_id = ?", [text(project)]);
    }
    if let Some(label) = &f.label {
        keep.and(
            "EXISTS (SELECT 1 FROM private_search_tags AS t WHERE t.doc = d.doc AND t.tag = ?)",
            [text(label)],
        );
    }
    let ranked = Ranked {
        db: index,
        fts: "private_search_fts",
        docs: "private_search_docs",
        columns: COLUMNS,
        build: doc,
    };
    ranked.page(plan, &visible, &keep)
}

impl PrivateNotesSearch for SqlitePrivateNotesSearch {
    fn search(
        &self,
        plan: &SearchPlan,
        scope: &PrivateScope<'_>,
    ) -> Result<SourcePage, OfficeError> {
        if scope.org_id != self.organization {
            return Ok(SourcePage::default());
        }
        let mut connections = self.lock()?;
        sync(&mut connections, &self.organization)?;
        search(&connections.index, plan, scope).map_err(storage)
    }
}
