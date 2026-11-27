//! Chat messages in `protected/messaging.db`, one document each in `message_search_docs` +
//! `message_search_fts`, beside the messages and nowhere else. A search reads only the
//! messages of the conversations the caller may read: that restriction is in every statement,
//! before anything is counted, scored or cut, and the BM25 statistics are those of those
//! messages alone, so an outsider never learns that a DM or another conversation's message
//! exists, how many there are, or a word of them.

use std::sync::LazyLock;

use org_proto::search::SearchEntityType;
use org_store::EngineError;
use rusqlite::{params, Connection, OptionalExtension, Row};

use org_office::search::documents::{self, FORMAT};
use org_office::search::plan::SearchPlan;
use org_office::search::ports::MessageScope;
use org_office::search::rank::{doc_len, SourcePage};
use org_office::search::records::{DocMeta, SearchDoc};

use super::super::{failed, SqliteTx};
use super::{fields, fts5, text, Clause, Ranked};
use org_office::messaging::ports::MessagingRead;

const TABLES: &str = "
CREATE TABLE message_search_dirty (
  organization_id TEXT NOT NULL,
  message_id TEXT NOT NULL,
  PRIMARY KEY (organization_id, message_id)
) STRICT, WITHOUT ROWID;
CREATE TABLE message_search_docs (
  doc INTEGER PRIMARY KEY,
  organization_id TEXT NOT NULL,
  message_id TEXT NOT NULL,
  conversation_id TEXT NOT NULL,
  author_id TEXT,
  title TEXT,
  updated_at TEXT NOT NULL,
  token_count INTEGER NOT NULL CHECK (token_count >= 0),
  UNIQUE (organization_id, message_id)
) STRICT;
CREATE INDEX message_search_docs_by_conversation
  ON message_search_docs(organization_id, conversation_id);
";

/// The adapter-private DDL of the message index in `protected/messaging.db`, after the
/// contract's: its tables, its FTS5 index, and the triggers that queue each changed message.
pub(crate) static DDL: LazyLock<String> = LazyLock::new(|| {
    let mut ddl = format!("-- message search documents, format {FORMAT}\n{TABLES}");
    ddl.push_str(&fts5::create_index("message_search_fts"));
    for (event, rows) in [
        ("insert", &["NEW"][..]),
        ("update", &["OLD", "NEW"][..]),
        ("delete", &["OLD"][..]),
    ] {
        let queue: String = rows
            .iter()
            .map(|row| {
                format!(
                    " INSERT INTO message_search_dirty (organization_id, message_id) \
                     SELECT {row}.organization_id, {row}.id WHERE NOT EXISTS (SELECT 1 \
                     FROM message_search_dirty WHERE organization_id = {row}.organization_id \
                     AND message_id = {row}.id);"
                )
            })
            .collect();
        ddl.push_str(&format!(
            "CREATE TRIGGER message_search_queue_{event} AFTER {event} ON messages \
             BEGIN{queue} END;\n"
        ));
    }
    ddl
});

/// Replaces the document of message `id` with `doc`, or removes it.
pub(super) fn put(
    db: &Connection,
    organization: &str,
    id: &str,
    doc: Option<&SearchDoc>,
) -> rusqlite::Result<()> {
    let old: Option<i64> = db
        .prepare_cached(
            "SELECT doc FROM message_search_docs WHERE organization_id = ?1 AND message_id = ?2",
        )?
        .query_row(params![organization, id], |row| row.get(0))
        .optional()?;
    if let Some(old) = old {
        db.prepare_cached("DELETE FROM message_search_fts WHERE rowid = ?1")?
            .execute([old])?;
        db.prepare_cached("DELETE FROM message_search_docs WHERE doc = ?1")?
            .execute([old])?;
    }
    let Some(doc) = doc else {
        return Ok(());
    };
    let m = &doc.meta;
    db.prepare_cached(
        "INSERT INTO message_search_docs (doc, organization_id, message_id, conversation_id, \
         author_id, title, updated_at, token_count) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?
    .execute(params![
        old,
        organization,
        id,
        m.conversation_id.as_deref().unwrap_or_default(),
        m.author_id,
        m.title,
        m.updated_at,
        i64::try_from(doc_len(doc)).unwrap_or(i64::MAX),
    ])?;
    let row = db.last_insert_rowid();
    let [a, b, c, d, e] = &doc.fields;
    db.prepare_cached(
        "INSERT INTO message_search_fts (rowid, f_ref, f_title, f_tags, f_body, f_extra) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?
    .execute(params![row, a, b, c, d, e])?;
    Ok(())
}

/// Rebuilds every queued message document of `organization` from the messages `tx` holds,
/// and empties the queue.
pub(super) fn refresh(tx: &SqliteTx, organization: &str) -> Result<(), EngineError> {
    let db = tx.msg_db();
    let doing = |e: rusqlite::Error| failed(db, "queuing message search documents", &e);
    let queue: Vec<(String, String)> = db
        .prepare_cached("SELECT organization_id, message_id FROM message_search_dirty")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect()
        })
        .map_err(doing)?;
    if queue.is_empty() {
        return Ok(());
    }
    for (org, id) in queue.iter().filter(|(org, _)| org == organization) {
        let doc = tx.message(id)?.as_ref().map(documents::message);
        put(db, org, id, doc.as_ref())
            .map_err(|e| failed(db, "indexing a message search document", &e))?;
    }
    db.prepare_cached("DELETE FROM message_search_dirty")
        .and_then(|mut statement| statement.execute([]))
        .map_err(doing)?;
    Ok(())
}

const COLUMNS: &str = "d.message_id, d.conversation_id, d.author_id, d.title, d.updated_at";

fn doc(row: &Row<'_>) -> rusqlite::Result<SearchDoc> {
    Ok(SearchDoc {
        entity_type: SearchEntityType::Message,
        entity_id: row.get(5)?,
        meta: DocMeta {
            conversation_id: Some(row.get(6)?),
            author_id: row.get(7)?,
            title: row.get(8)?,
            updated_at: row.get(9)?,
            ..DocMeta::default()
        },
        fields: fields(row)?,
    })
}

/// `d.conversation_id` is one of `ids`.
fn in_conversations(organization: &str, ids: &[String]) -> Clause {
    let ids = serde_json::to_string(ids).unwrap_or_else(|_| "[]".to_owned());
    Clause::new(
        "d.organization_id = ? AND d.conversation_id IN (SELECT value FROM json_each(?))",
        [text(organization), text(&ids)],
    )
}

/// The message page `plan` asks for, from `scope.within`, ranked over `scope.visible`.
pub(super) fn search(
    db: &Connection,
    organization: &str,
    plan: &SearchPlan,
    scope: &MessageScope,
) -> rusqlite::Result<SourcePage> {
    let f = &plan.filters;
    let never = f.status.is_some()
        || f.assignee_id.is_some()
        || f.label.is_some()
        || f.modified_file.is_some();
    if !plan.wants(SearchEntityType::Message)
        || plan.terms.is_empty()
        || scope.within.is_empty()
        || never
    {
        return Ok(SourcePage::default());
    }
    let visible = in_conversations(organization, &scope.visible);
    let mut keep = in_conversations(organization, &scope.within);
    if let Some(author) = &f.author_id {
        keep.and("d.author_id = ?", [text(author)]);
    }
    let index = Ranked {
        db,
        fts: "message_search_fts",
        docs: "message_search_docs",
        columns: COLUMNS,
        build: doc,
    };
    index.page(plan, &visible, &keep)
}
