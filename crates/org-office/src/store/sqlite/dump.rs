//! A listing of every row of the projection, for the suites that compare one projection
//! with another of this engine. Never part of a production build.

use org_store::{testkit::Dump, Engine};
use rusqlite::types::Value;

use super::SqliteEngine;

/// The tables whose row order is their file's order: a row's place is part of it.
const LISTS: [&str; 2] = ["repositories", "office_codes"];

impl Dump for SqliteEngine {
    fn dump(&self) -> Vec<String> {
        let snapshot = self.snapshot().expect("a snapshot to dump");
        let db = snapshot.db();
        // The digest row says what the projection was made from, not what it holds.
        let tables = "SELECT name FROM sqlite_schema WHERE type = 'table' \
                      AND name NOT LIKE 'sqlite_%' AND name <> 'store_schema'";
        let mut names = db.prepare(tables).expect("the list of tables");
        let names = names.query_map([], |row| row.get::<_, String>(0));
        let names: Vec<String> = names.and_then(Iterator::collect).expect("table names");
        let mut lines = Vec::new();
        for table in names {
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
                lines.push(format!("{table}{at} {values:?}"));
                place += 1;
            }
        }
        lines.sort();
        lines
    }
}
