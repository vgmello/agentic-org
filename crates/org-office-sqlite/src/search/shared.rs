//! Shared organization knowledge in `office.db`: tasks (key, title, files, labels, headings,
//! body, details, public comments and checklist steps), organization processes and standards,
//! projects, members, teams and roles, one document each in `search_docs` + `search_fts`,
//! ranked by FTS5's field-weighted `bm25()`: every active member may see every one of them,
//! so the whole index is the caller's corpus.
//!
//! An entity type joins by one row in [`SOURCES`] per table its document reads and one
//! builder in [`org_office::search::documents`] (organization documents, `TASK-55`, will).

use std::sync::LazyLock;

use org_proto::search::SearchEntityType;
use org_store::EngineError;
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};

use org_office::search::documents::{self, FORMAT};
use org_office::search::plan::SearchPlan;
use org_office::search::rank::{Scored, SourcePage};
use org_office::search::records::{DocMeta, SearchDoc, FIELD_WEIGHTS};

use super::super::{failed, SqliteTx};
use super::{fields, fts5, text, Clause};

/// Each projected table a shared document reads: the table, the document's type, and the
/// column naming its entity.
const SOURCES: [(&str, SearchEntityType, &str); 11] = [
    ("tasks", SearchEntityType::Task, "id"),
    ("task_labels", SearchEntityType::Task, "task_id"),
    ("task_files", SearchEntityType::Task, "task_id"),
    ("task_details", SearchEntityType::Task, "task_id"),
    ("comments", SearchEntityType::Task, "target_id"),
    ("checklist_steps", SearchEntityType::Task, "target_id"),
    ("projects", SearchEntityType::Project, "id"),
    ("employees", SearchEntityType::Member, "id"),
    ("teams", SearchEntityType::Team, "id"),
    ("roles", SearchEntityType::Role, "id"),
    ("structure_documents", SearchEntityType::Process, "id"),
];

const TABLES: &str = "
CREATE TABLE search_dirty (
  organization_id TEXT NOT NULL,
  entity_type TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  PRIMARY KEY (organization_id, entity_type, entity_id)
) STRICT, WITHOUT ROWID;
CREATE TABLE search_docs (
  doc INTEGER PRIMARY KEY,
  organization_id TEXT NOT NULL,
  entity_type TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  reference TEXT,
  title TEXT,
  status TEXT,
  project_id TEXT,
  assignee_id TEXT,
  author_id TEXT,
  updated_at TEXT NOT NULL,
  token_count INTEGER NOT NULL CHECK (token_count >= 0),
  UNIQUE (organization_id, entity_type, entity_id)
) STRICT;
";

/// The adapter-private DDL of the shared index in `office.db`, after the contract's: its
/// tables, its FTS5 index, and one trigger per source table and event that queues the
/// changed entity. It names the document format, so documents built another way are rebuilt.
/// A trigger queues with `NOT EXISTS`, never `OR IGNORE`: the statement that fires it may
/// carry its own conflict policy, which SQLite would apply to the trigger's insert instead.
pub(crate) static DDL: LazyLock<String> = LazyLock::new(|| {
    let mut ddl = format!("-- search documents, format {FORMAT}\n{TABLES}");
    ddl.push_str(&fts5::create_index("search_fts"));
    for (table, kind, column) in SOURCES {
        let kind = kind.as_str();
        for (event, rows) in [
            ("insert", &["NEW"][..]),
            ("update", &["OLD", "NEW"][..]),
            ("delete", &["OLD"][..]),
        ] {
            let queue: String = rows
                .iter()
                .map(|row| {
                    format!(
                        " INSERT INTO search_dirty (organization_id, entity_type, entity_id) \
                         SELECT {row}.organization_id, '{kind}', {row}.{column} \
                         WHERE NOT EXISTS (SELECT 1 FROM search_dirty \
                         WHERE organization_id = {row}.organization_id \
                         AND entity_type = '{kind}' AND entity_id = {row}.{column});"
                    )
                })
                .collect();
            ddl.push_str(&format!(
                "CREATE TRIGGER search_queue_{table}_{event} AFTER {event} ON {table} \
                 BEGIN{queue} END;\n"
            ));
        }
    }
    ddl
});

/// Replaces the document of `(kind, id)` with `doc`, or removes it when `doc` is `None`. A
/// replaced document keeps its row.
pub(super) fn put(
    db: &Connection,
    organization: &str,
    kind: &str,
    id: &str,
    doc: Option<&SearchDoc>,
) -> rusqlite::Result<()> {
    let old: Option<i64> = db
        .prepare_cached(
            "SELECT doc FROM search_docs \
             WHERE organization_id = ?1 AND entity_type = ?2 AND entity_id = ?3",
        )?
        .query_row(params![organization, kind, id], |row| row.get(0))
        .optional()?;
    if let Some(old) = old {
        db.prepare_cached("DELETE FROM search_fts WHERE rowid = ?1")?
            .execute([old])?;
        db.prepare_cached("DELETE FROM search_docs WHERE doc = ?1")?
            .execute([old])?;
    }
    let Some(doc) = doc else {
        return Ok(());
    };
    let m = &doc.meta;
    db.prepare_cached(
        "INSERT INTO search_docs (doc, organization_id, entity_type, entity_id, reference, \
         title, status, project_id, assignee_id, author_id, updated_at, token_count) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
    )?
    .execute(params![
        old,
        organization,
        kind,
        id,
        m.reference,
        m.title,
        m.status,
        m.project_id,
        m.assignee_id,
        m.author_id,
        m.updated_at,
        i64::try_from(org_office::search::rank::doc_len(doc)).unwrap_or(i64::MAX),
    ])?;
    let row = db.last_insert_rowid();
    let [a, b, c, d, e] = &doc.fields;
    db.prepare_cached(
        "INSERT INTO search_fts (rowid, f_ref, f_title, f_tags, f_body, f_extra) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?
    .execute(params![row, a, b, c, d, e])?;
    Ok(())
}

/// The entities queued in `search_dirty`, oldest queue first.
fn queued(db: &Connection) -> rusqlite::Result<Vec<(String, String, String)>> {
    let mut statement =
        db.prepare_cached("SELECT organization_id, entity_type, entity_id FROM search_dirty")?;
    let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
    rows.collect()
}

/// Rebuilds every queued shared document of `organization` from the records `tx` holds, and
/// empties the queue.
pub(super) fn refresh(tx: &SqliteTx, organization: &str) -> Result<(), EngineError> {
    let db = tx.db();
    let doing = |e: rusqlite::Error| failed(db, "queuing search documents", &e);
    let queue = queued(db).map_err(doing)?;
    if queue.is_empty() {
        return Ok(());
    }
    for (org, kind, id) in queue.iter().filter(|(org, ..)| org == organization) {
        let doc = match SearchEntityType::parse(kind) {
            Some(parsed) => documents::shared_doc(tx, parsed, id)?,
            None => None,
        };
        put(db, org, kind, id, doc.as_ref())
            .map_err(|e| failed(db, "indexing a search document", &e))?;
    }
    db.prepare_cached("DELETE FROM search_dirty")
        .and_then(|mut statement| statement.execute([]))
        .map_err(doing)?;
    Ok(())
}

/// What a shared hit reads beside its five fields.
const COLUMNS: &str = "d.entity_type, d.entity_id, d.reference, d.title, d.status, \
     d.project_id, d.assignee_id, d.author_id, d.updated_at, f.score";

fn hit(row: &Row<'_>) -> rusqlite::Result<Scored> {
    let kind: String = row.get(5)?;
    let entity_type = SearchEntityType::parse(&kind).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            5,
            rusqlite::types::Type::Text,
            "an unknown search entity type".into(),
        )
    })?;
    let doc = SearchDoc {
        entity_type,
        entity_id: row.get(6)?,
        meta: DocMeta {
            reference: row.get(7)?,
            title: row.get(8)?,
            status: row.get(9)?,
            project_id: row.get(10)?,
            assignee_id: row.get(11)?,
            author_id: row.get(12)?,
            updated_at: row.get(13)?,
            ..DocMeta::default()
        },
        fields: fields(row)?,
    };
    Ok(Scored {
        doc,
        score: row.get(14)?,
    })
}

/// What a shared search keeps: the organization, the types asked for, and every filter.
fn filters(organization: &str, plan: &SearchPlan) -> Clause {
    let types = plan.shared_types();
    let marks = vec!["?"; types.len()].join(", ");
    let mut keep = Clause::new(
        &format!("d.organization_id = ? AND d.entity_type IN ({marks})"),
        std::iter::once(text(organization)).chain(types.iter().map(|t| text(t.as_str()))),
    );
    let f = &plan.filters;
    for (column, value) in [
        ("d.project_id = ?", &f.project_id),
        ("d.status = ?", &f.status),
        ("d.assignee_id = ?", &f.assignee_id),
        ("d.author_id = ?", &f.author_id),
    ] {
        if let Some(value) = value {
            keep.and(column, [text(value)]);
        }
    }
    if let Some(label) = &f.label {
        keep.and(
            "d.entity_type = 'task' AND EXISTS (SELECT 1 FROM task_labels AS l \
             WHERE l.organization_id = d.organization_id AND l.task_id = d.entity_id \
             AND l.label = ?)",
            [text(label)],
        );
    }
    if let Some(path) = &f.modified_file {
        keep.and(
            "d.entity_type = 'task' AND EXISTS (SELECT 1 FROM task_files AS p \
             WHERE p.organization_id = d.organization_id AND p.task_id = d.entity_id \
             AND p.role = 'modified' AND p.path = ?)",
            [text(path)],
        );
    }
    keep
}

/// The shared page `plan` asks for, ranked by FTS5's weighted `bm25()` (negated: higher is
/// better) with the same tie-breakers as every engine.
pub(super) fn search(
    db: &Connection,
    organization: &str,
    plan: &SearchPlan,
) -> rusqlite::Result<SourcePage> {
    if !plan.wants_shared() || plan.terms.is_empty() || plan.filters.conversation_id.is_some() {
        return Ok(SourcePage::default());
    }
    let matched = fts5::match_all(&plan.terms);
    let keep = filters(organization, plan);
    let with_match = || std::iter::once(text(&matched)).chain(keep.values.iter().cloned());
    let count_sql = format!(
        "SELECT count(*) FROM search_fts JOIN search_docs AS d ON d.doc = search_fts.rowid \
         WHERE search_fts MATCH ? AND {}",
        keep.sql
    );
    let total: i64 = db
        .prepare_cached(&count_sql)?
        .query_row(params_from_iter(with_match()), |row| row.get(0))?;
    let total = u64::try_from(total).unwrap_or(0);
    if plan.count_only {
        return Ok(SourcePage {
            total,
            hits: Vec::new(),
        });
    }
    let mut ranked = keep.clone();
    if let Some(after) = &plan.after {
        ranked.and(
            "(f.score < ? OR (f.score = ? AND (d.updated_at < ? \
             OR (d.updated_at = ? AND d.entity_id > ?))))",
            [
                Value::Real(after.score),
                Value::Real(after.score),
                text(&after.updated_at),
                text(&after.updated_at),
                text(&after.entity_id),
            ],
        );
    }
    let weights = FIELD_WEIGHTS.map(|w| format!("{w:.1}")).join(", ");
    let sql = format!(
        "SELECT f.f_ref, f.f_title, f.f_tags, f.f_body, f.f_extra, {COLUMNS} FROM \
         (SELECT rowid AS doc, {cols}, -bm25(search_fts, {weights}) AS score \
          FROM search_fts WHERE search_fts MATCH ?) AS f \
         JOIN search_docs AS d ON d.doc = f.doc WHERE {} \
         ORDER BY f.score DESC, d.updated_at DESC, d.entity_id ASC LIMIT ?",
        ranked.sql,
        cols = fts5::COLUMNS,
    );
    let fetch = i64::try_from(plan.fetch()).unwrap_or(i64::MAX);
    let values = std::iter::once(text(&matched))
        .chain(ranked.values.iter().cloned())
        .chain(std::iter::once(Value::Integer(fetch)));
    let mut statement = db.prepare_cached(&sql)?;
    let hits = statement
        .query_map(params_from_iter(values), hit)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(SourcePage { total, hits })
}
