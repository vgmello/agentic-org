//! A listing of every row of the projection, for the suites that compare one projection
//! with another of this engine. Never part of a production build. The adapter-private search
//! index (its FTS5 virtual tables, their shadow tables and its rowid-keyed documents) is left
//! out: it is derived from the rows listed here, and its own tests compare what it answers.

use org_store::{testkit::Dump, Engine};
use rusqlite::types::Value;

use super::SqliteEngine;

/// The tables whose row order is their file's order: a row's place is part of it.
const LISTS: [&str; 2] = ["repositories", "office_codes"];

fn dump_connection(db: &rusqlite::Connection, prefix: &str, skip: &[&str], out: &mut Vec<String>) {
    let tables = "SELECT name FROM sqlite_schema WHERE type = 'table' \
                  AND name NOT LIKE 'sqlite_%'";
    let mut names = db.prepare(tables).expect("the list of tables");
    let names = names.query_map([], |row| row.get::<_, String>(0));
    let names: Vec<String> = names.and_then(Iterator::collect).expect("table names");
    for table in names {
        let search_index = super::search::DUMP_SKIPPED_PREFIXES
            .iter()
            .any(|prefix| table.starts_with(prefix));
        if skip.contains(&table.as_str()) || search_index {
            continue;
        }
        let ordered = LISTS.contains(&table.as_str());
        let order = if ordered { " ORDER BY rowid" } else { "" };
        let mut rows = db
            .prepare(&format!("SELECT * FROM {table}{order}"))
            .expect("a table to dump");
        let width = rows.column_count();
        let mut rows = rows.query([]).expect("rows to dump");
        let mut place = 0;
        while let Some(row) = rows.next().expect("a row to dump") {
            let values: Vec<Value> = (0..width)
                .map(|column| row.get(column).expect("a value to dump"))
                .collect();
            let at = if ordered {
                format!(" {place:06}")
            } else {
                String::new()
            };
            out.push(format!("{prefix}{table}{at} {values:?}"));
            place += 1;
        }
    }
}

impl Dump for SqliteEngine {
    fn dump(&self) -> Vec<String> {
        let snapshot = self.snapshot().expect("a snapshot to dump");
        let mut lines = Vec::new();
        dump_connection(snapshot.db(), "", &["store_schema"], &mut lines);
        dump_connection(
            snapshot.msg_db(),
            "messaging.",
            &[
                "messaging_schema",
                "messaging_cursor",
                "messaging_local_meta",
            ],
            &mut lines,
        );
        lines.sort();
        lines
    }
}

#[cfg(test)]
impl SqliteEngine {
    /// The projection as a build with `ddl` after this build's schema makes it.
    pub(crate) fn open_with(
        path: &std::path::Path,
        ddl: &'static str,
    ) -> Result<Self, org_store::EngineError> {
        Self::open_as(path, super::schema::Schema::current().with(ddl))
    }

    /// The digest of the DDL this projection is made from.
    pub(crate) fn schema_digest(&self) -> String {
        self.schema.digest()
    }
}
