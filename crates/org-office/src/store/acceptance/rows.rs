//! Exact ordered SQL projection mutations, captured only on the existing writer.
//! Values are typed SQL cells; JSON-looking TEXT is never parsed/reserialized.

use std::collections::BTreeMap;

use rusqlite::{
    params_from_iter,
    types::{Value as SqlValue, ValueRef},
    Connection,
};
use serde::{Deserialize, Serialize};

use crate::OfficeError;

pub(super) const TABLES: &[&str] = &[
    "meta",
    "work_feed",
    "work_items",
    "work_ids",
    "work_events",
    "work_origin_counter",
    "work_origin_log",
    "work_change_results",
    "work_number_counters",
    "work_clock",
    "selected_directory",
    "selected_directory_head",
    "selected_owner_authority",
    "selected_tasks",
    "selected_events",
    "selected_receipts",
    "selected_adoptions",
    "selected_artifacts",
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(super) enum Cell {
    Null,
    Integer(i64),
    Text(String),
    Blob(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Mutation {
    pub table: String,
    pub columns: Vec<String>,
    pub before: Option<Vec<Cell>>,
    pub after: Option<Vec<Cell>>,
}

fn storage(e: impl std::error::Error + Send + Sync + 'static) -> OfficeError {
    OfficeError::storage(e)
}

pub(super) fn columns(db: &Connection, table: &str) -> Result<Vec<String>, OfficeError> {
    if !TABLES.contains(&table) && table != "sqlite_sequence" {
        return Err(OfficeError::Invalid("unsupported frozen table"));
    }
    let mut statement = db
        .prepare(&format!("PRAGMA table_info(\"{table}\")"))
        .map_err(storage)?;
    let result = statement
        .query_map([], |r| r.get(1))
        .map_err(storage)?
        .collect::<Result<_, _>>()
        .map_err(storage);
    result
}

pub(super) fn schema_digest(db: &Connection) -> Result<String, OfficeError> {
    let mut definitions = BTreeMap::new();
    for table in TABLES {
        definitions.insert(*table, columns(db, table)?);
    }
    let text = serde_json::to_vec(&definitions).map_err(storage)?;
    Ok(org_work::selected_codec::sha256(&text))
}

fn json_row(cols: &[String], prefix: &str) -> String {
    let values = cols.iter().map(|c| format!("json_array(typeof({prefix}.\"{c}\"),CASE WHEN typeof({prefix}.\"{c}\")='blob' THEN hex({prefix}.\"{c}\") ELSE {prefix}.\"{c}\" END)")).collect::<Vec<_>>().join(",");
    format!("json_array({values})")
}

pub(super) fn install_capture(db: &Connection) -> Result<(), OfficeError> {
    db.execute_batch(
        "PRAGMA recursive_triggers=ON;
         CREATE TEMP TABLE acceptance_capture_enabled(enabled INTEGER NOT NULL);
         INSERT INTO acceptance_capture_enabled VALUES(0);
         CREATE TEMP TABLE acceptance_capture(seq INTEGER PRIMARY KEY,data TEXT NOT NULL);
         CREATE TEMP TABLE acceptance_capture_budget(rows INTEGER NOT NULL,bytes INTEGER NOT NULL);
         INSERT INTO acceptance_capture_budget VALUES(0,0);
         CREATE TEMP TRIGGER acceptance_capture_limit BEFORE INSERT ON acceptance_capture
         BEGIN
           UPDATE acceptance_capture_budget SET rows=rows+1,bytes=bytes+length(CAST(NEW.data AS BLOB));
           SELECT CASE WHEN (SELECT rows>200000 OR bytes>268435456 FROM acceptance_capture_budget)
             THEN RAISE(ABORT,'frozen preparation resource limit') END;
         END;"
    ).map_err(storage)?;
    for table in TABLES {
        let cols = columns(db, table)?;
        if cols.is_empty() {
            return Err(OfficeError::Invalid("missing acceptance projection schema"));
        }
        for (op, before, after) in [
            ("INSERT", "NULL".into(), json_row(&cols, "NEW")),
            ("UPDATE", json_row(&cols, "OLD"), json_row(&cols, "NEW")),
            ("DELETE", json_row(&cols, "OLD"), "NULL".into()),
        ] {
            let condition = if *table == "meta" {
                match op {
                    "INSERT" => "NEW.key='incarnation'",
                    "UPDATE" => "OLD.key='incarnation' OR NEW.key='incarnation'",
                    _ => "OLD.key='incarnation'",
                }
            } else {
                "1"
            };
            // Guard even raw SQL/replication paths against bypassing capture.
            db.execute_batch(&format!(
                "CREATE TEMP TRIGGER acceptance_guard_{table}_{op} BEFORE {op} ON main.\"{table}\"
                 WHEN ({condition}) AND (SELECT enabled FROM acceptance_capture_enabled)=0
                 BEGIN SELECT RAISE(ABORT,'work acceptance boundary required'); END;
                 CREATE TEMP TRIGGER acceptance_log_{table}_{op} AFTER {op} ON main.\"{table}\"
                 WHEN ({condition}) AND (SELECT enabled FROM acceptance_capture_enabled)=1
                 BEGIN INSERT INTO acceptance_capture(data) VALUES(json_object('table','{table}','before',{before},'after',{after})); END;"
            )).map_err(storage)?;
        }
    }
    for op in ["INSERT", "UPDATE", "DELETE"] {
        db.execute_batch(&format!(
            "CREATE TEMP TRIGGER acceptance_cursor_{op} BEFORE {op} ON main.selected_journal_cursor
            WHEN (SELECT enabled FROM acceptance_capture_enabled)!=2
            BEGIN SELECT RAISE(ABORT,'journal cursor requires frozen projection'); END;"
        ))
        .map_err(storage)?;
    }
    Ok(())
}

pub(super) fn mode(db: &Connection, mode: i64) -> Result<(), OfficeError> {
    db.execute(
        "UPDATE temp.acceptance_capture_enabled SET enabled=?1",
        [mode],
    )
    .map_err(storage)?;
    Ok(())
}

pub(super) fn clear(db: &Connection) -> Result<(), OfficeError> {
    db.execute_batch("DELETE FROM temp.acceptance_capture; UPDATE temp.acceptance_capture_budget SET rows=0,bytes=0;").map_err(storage)
}

fn decode_row(value: &serde_json::Value) -> Result<Option<Vec<Cell>>, OfficeError> {
    if value.is_null() {
        return Ok(None);
    }
    value
        .as_array()
        .ok_or(OfficeError::Invalid("invalid captured row"))?
        .iter()
        .map(|v| {
            let v = v
                .as_array()
                .filter(|a| a.len() == 2)
                .ok_or(OfficeError::Invalid("invalid captured cell"))?;
            match (v[0].as_str(), &v[1]) {
                (Some("null"), _) => Ok(Cell::Null),
                (Some("integer"), n) => n
                    .as_i64()
                    .map(Cell::Integer)
                    .ok_or(OfficeError::Invalid("invalid captured integer")),
                (Some("text"), serde_json::Value::String(s)) => Ok(Cell::Text(s.clone())),
                (Some("blob"), serde_json::Value::String(s)) => {
                    Ok(Cell::Blob(s.to_ascii_lowercase()))
                }
                _ => Err(OfficeError::Invalid(
                    "unsupported captured SQL representation",
                )),
            }
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

pub(super) fn captured(db: &Connection) -> Result<Vec<Mutation>, OfficeError> {
    let mut statement = db
        .prepare("SELECT data FROM temp.acceptance_capture ORDER BY seq")
        .map_err(storage)?;
    let mut rows = statement.query([]).map_err(storage)?;
    let mut mutations = Vec::new();
    while let Some(row) = rows.next().map_err(storage)? {
        let text: String = row.get(0).map_err(storage)?;
        let data: serde_json::Value = serde_json::from_str(&text).map_err(storage)?;
        let table = data["table"]
            .as_str()
            .ok_or(OfficeError::Invalid("invalid captured table"))?;
        mutations.push(Mutation {
            table: table.into(),
            columns: columns(db, table)?,
            before: decode_row(&data["before"])?,
            after: decode_row(&data["after"])?,
        });
    }
    Ok(mutations)
}

// Borrow SQL values until a bounded anchor is known to fit. In particular,
// never copy TEXT or expand BLOBs to hex merely to discover an oversized root.
#[derive(Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
enum BorrowedCell<'a> {
    Null,
    Integer(i64),
    Text(&'a str),
    Blob(Hex<'a>),
}
struct Hex<'a>(&'a [u8]);
impl std::fmt::Display for Hex<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for b in self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}
impl Serialize for Hex<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}
fn borrowed_cells<'a>(
    row: &'a rusqlite::Row<'_>,
    count: usize,
) -> Result<Vec<BorrowedCell<'a>>, OfficeError> {
    (0..count)
        .map(|i| {
            Ok(match row.get_ref(i).map_err(storage)? {
                ValueRef::Null => BorrowedCell::Null,
                ValueRef::Integer(n) => BorrowedCell::Integer(n),
                ValueRef::Text(s) => BorrowedCell::Text(std::str::from_utf8(s).map_err(storage)?),
                ValueRef::Blob(b) => BorrowedCell::Blob(Hex(b)),
                ValueRef::Real(_) => {
                    return Err(OfficeError::Invalid("unsupported legacy SQL number"))
                }
            })
        })
        .collect()
}
fn visit_rows(
    db: &Connection,
    mut visit: impl FnMut(&str, &[String], &rusqlite::Row<'_>) -> Result<(), OfficeError>,
) -> Result<(), OfficeError> {
    // sqlite_sequence has no uniqueness constraint. A malformed preexisting
    // allocator must fail before it can become an authoritative anchor.
    feed_high_water(db)?;
    for table in TABLES
        .iter()
        .copied()
        .chain(std::iter::once("sqlite_sequence"))
    {
        let cols = columns(db, table)?;
        let filter = match table {
            "meta" => " WHERE key='incarnation'",
            "sqlite_sequence" => " WHERE name='work_feed'",
            _ => "",
        };
        let mut statement = db
            .prepare(&format!(
                "SELECT {} FROM \"{table}\"{filter}",
                cols.iter()
                    .map(|c| format!("\"{c}\""))
                    .collect::<Vec<_>>()
                    .join(",")
            ))
            .map_err(storage)?;
        let mut rows = statement.query([]).map_err(storage)?;
        while let Some(row) = rows.next().map_err(storage)? {
            visit(table, &cols, row)?;
        }
    }
    Ok(())
}
struct ByteBudget(usize);
impl std::io::Write for ByteBudget {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_sub(bytes.len())
            .ok_or_else(|| std::io::Error::other("legacy checkpoint exceeds resource profile"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
#[derive(Serialize)]
struct BorrowedMutation<'a, 'b> {
    table: &'a str,
    columns: &'a [String],
    before: Option<()>,
    after: &'a [BorrowedCell<'b>],
}
pub(super) fn checkpoint(db: &Connection) -> Result<Vec<Mutation>, OfficeError> {
    checkpoint_bounded(
        db,
        org_work::selected_codec::MAX_BLOB_BYTES as usize,
        org_work::selected_codec::MAX_NODES,
    )
}
fn checkpoint_bounded(
    db: &Connection,
    bytes: usize,
    nodes: usize,
) -> Result<Vec<Mutation>, OfficeError> {
    let mut budget = ByteBudget(bytes);
    let mut remaining = nodes;
    // One aggregate budget across all tables. Both passes occur under the same
    // sole-writer transaction; source values cannot change between them.
    visit_rows(db, |table, cols, row| {
        remaining = remaining
            .checked_sub(13 + cols.len() * 6)
            .ok_or(OfficeError::Invalid(
                "legacy checkpoint exceeds resource profile",
            ))?;
        let cells = borrowed_cells(row, cols.len())?;
        serde_json::to_writer(
            &mut budget,
            &BorrowedMutation {
                table,
                columns: cols,
                before: None,
                after: &cells,
            },
        )
        .map_err(storage)
    })?;
    let mut all = Vec::new();
    visit_rows(db, |table, cols, row| {
        let values = borrowed_cells(row, cols.len())?
            .into_iter()
            .map(|c| match c {
                BorrowedCell::Null => Cell::Null,
                BorrowedCell::Integer(n) => Cell::Integer(n),
                BorrowedCell::Text(s) => Cell::Text(s.into()),
                BorrowedCell::Blob(Hex(b)) => Cell::Blob(hex(b)),
            })
            .collect();
        all.push(Mutation {
            table: table.into(),
            columns: cols.to_vec(),
            before: None,
            after: Some(values),
        });
        Ok(())
    })?;
    Ok(all)
}
struct HashWriter(sha2::Sha256);
impl std::io::Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        use sha2::Digest;
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct FrozenState(BTreeMap<(String, String), usize>);

impl FrozenState {
    fn key(table: &str, cells: &[Cell]) -> Result<(String, String), OfficeError> {
        Ok((
            table.into(),
            org_work::selected_codec::sha256(&serde_json::to_vec(cells).map_err(storage)?),
        ))
    }
    pub(super) fn extend(&mut self, mutations: &[Mutation]) -> Result<(), OfficeError> {
        for m in mutations {
            if let Some(before) = &m.before {
                let key = Self::key(&m.table, before)?;
                let n = self
                    .0
                    .get_mut(&key)
                    .ok_or(OfficeError::Invalid("frozen history preimage mismatch"))?;
                *n = n
                    .checked_sub(1)
                    .ok_or(OfficeError::Invalid("frozen history preimage mismatch"))?;
                if *n == 0 {
                    self.0.remove(&key);
                }
            }
            if let Some(after) = &m.after {
                *self.0.entry(Self::key(&m.table, after)?).or_default() += 1;
            }
        }
        Ok(())
    }
    pub(super) fn verify(&self, db: &Connection) -> Result<(), OfficeError> {
        use sha2::Digest;
        let mut actual = Self::default();
        // Accumulated acknowledged state has no new total-size admission cap.
        // Hash one borrowed row at a time; retain only the multiset of digests.
        visit_rows(db, |table, cols, row| {
            let mut hash = HashWriter(sha2::Sha256::new());
            serde_json::to_writer(&mut hash, &borrowed_cells(row, cols.len())?).map_err(storage)?;
            *actual
                .0
                .entry((table.into(), hex(&hash.0.finalize())))
                .or_default() += 1;
            Ok(())
        })?;
        if actual.0 != self.0 {
            return Err(OfficeError::Invalid(
                "projection differs from frozen history",
            ));
        }
        Ok(())
    }
}

pub(super) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(s, "{b:02x}").expect("String writes cannot fail");
    }
    s
}

pub(super) fn unhex(s: &str) -> Result<Vec<u8>, OfficeError> {
    if !s.len().is_multiple_of(2) {
        return Err(OfficeError::Invalid("invalid frozen blob"));
    }
    s.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| {
            let digit = |c: u8| match c {
                b'0'..=b'9' => Some(c - b'0'),
                b'a'..=b'f' => Some(c - b'a' + 10),
                b'A'..=b'F' => Some(c - b'A' + 10),
                _ => None,
            };
            Ok(
                digit(p[0]).ok_or(OfficeError::Invalid("invalid frozen blob"))? * 16
                    + digit(p[1]).ok_or(OfficeError::Invalid("invalid frozen blob"))?,
            )
        })
        .collect()
}

fn values(cells: &[Cell]) -> Result<Vec<SqlValue>, OfficeError> {
    cells
        .iter()
        .map(|c| {
            Ok(match c {
                Cell::Null => SqlValue::Null,
                Cell::Integer(n) => SqlValue::Integer(*n),
                Cell::Text(s) => SqlValue::Text(s.clone()),
                Cell::Blob(s) => SqlValue::Blob(unhex(s)?),
            })
        })
        .collect()
}

/// Matching complete preimages prevent replay over divergent projected state.
/// A cursor, not relaxed upserts, decides whether a transaction was already applied.
pub(super) fn apply(db: &Connection, m: &Mutation, adopting: bool) -> Result<(), OfficeError> {
    if m.columns != columns(db, &m.table)? {
        return Err(OfficeError::Invalid("frozen schema mismatch"));
    }
    if m.before
        .as_ref()
        .is_some_and(|r| r.len() != m.columns.len())
        || m.after.as_ref().is_some_and(|r| r.len() != m.columns.len())
    {
        return Err(OfficeError::Invalid("frozen row shape mismatch"));
    }
    let names = m
        .columns
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>();
    if m.table == "sqlite_sequence" {
        return apply_feed_high_water(db, m);
    }
    if let Some(before) = &m.before {
        apply_existing_row(db, m, &names, before)?;
    } else if let Some(after) = &m.after {
        let parameters = values(after)?;
        if adopting {
            let condition = names
                .iter()
                .enumerate()
                .map(|(i, c)| format!("{c} IS ?{}", i + 1))
                .collect::<Vec<_>>()
                .join(" AND ");
            let count: i64 = db
                .query_row(
                    &format!("SELECT count(*) FROM \"{}\" WHERE {condition}", m.table),
                    params_from_iter(&parameters),
                    |r| r.get(0),
                )
                .map_err(storage)?;
            if count == 1 {
                return Ok(());
            }
        }
        let slots = (1..=after.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(",");
        db.execute(
            &format!(
                "INSERT INTO \"{}\"({}) VALUES({slots})",
                m.table,
                names.join(",")
            ),
            params_from_iter(parameters),
        )
        .map_err(storage)?;
    } else {
        return Err(OfficeError::Invalid("empty frozen mutation"));
    }
    Ok(())
}

pub(super) fn feed_high_water(db: &Connection) -> Result<Option<i64>, OfficeError> {
    let mut query = db
        .prepare("SELECT seq FROM sqlite_sequence WHERE name='work_feed'")
        .map_err(storage)?;
    let mut rows = query.query([]).map_err(storage)?;
    let value = rows
        .next()
        .map_err(storage)?
        .map(|r| r.get::<_, i64>(0))
        .transpose()
        .map_err(storage)?;
    if rows.next().map_err(storage)?.is_some() || value.is_some_and(|n| n < 0) {
        return Err(OfficeError::Invalid("invalid work feed allocator"));
    }
    Ok(value)
}

pub(super) fn freeze_feed(before: Option<i64>, after: Option<i64>) -> Option<Mutation> {
    (before != after).then(|| Mutation {
        table: "sqlite_sequence".into(),
        columns: vec!["name".into(), "seq".into()],
        before: before.map(|n| vec![Cell::Text("work_feed".into()), Cell::Integer(n)]),
        after: after.map(|n| vec![Cell::Text("work_feed".into()), Cell::Integer(n)]),
    })
}

fn apply_feed_high_water(db: &Connection, m: &Mutation) -> Result<(), OfficeError> {
    let expected = match m.after.as_deref() {
        Some([Cell::Text(name), Cell::Integer(n)]) if name == "work_feed" && *n >= 0 => *n,
        _ => return Err(OfficeError::Invalid("invalid frozen feed high-water")),
    };
    let actual = feed_high_water(db)?;
    // Explicit feed INSERTs may already have raised SQLite's counter. Never
    // lower it or insert duplicate system rows while restoring an anchor.
    if actual.is_some_and(|n| n > expected) {
        return Err(OfficeError::Invalid("feed high-water conflict"));
    }
    if actual.is_some() {
        db.execute(
            "UPDATE sqlite_sequence SET seq=?1 WHERE name='work_feed'",
            [expected],
        )
        .map_err(storage)?;
    } else {
        db.execute(
            "INSERT INTO sqlite_sequence(name,seq) VALUES('work_feed',?1)",
            [expected],
        )
        .map_err(storage)?;
    }
    Ok(())
}

fn apply_existing_row(
    db: &Connection,
    m: &Mutation,
    names: &[String],
    before: &[Cell],
) -> Result<(), OfficeError> {
    let condition = names
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{c} IS ?{}", i + 1))
        .collect::<Vec<_>>()
        .join(" AND ");
    let parameters = values(before)?;
    let count: i64 = db
        .query_row(
            &format!("SELECT count(*) FROM \"{}\" WHERE {condition}", m.table),
            params_from_iter(&parameters),
            |r| r.get(0),
        )
        .map_err(storage)?;
    if count != 1 {
        return Err(OfficeError::Invalid("frozen prior row mismatch"));
    }
    if let Some(after) = &m.after {
        let set = names
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{c}=?{}", i + 1))
            .collect::<Vec<_>>()
            .join(",");
        let condition = names
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{c} IS ?{}", i + 1 + after.len()))
            .collect::<Vec<_>>()
            .join(" AND ");
        let mut params = values(after)?;
        params.extend(parameters);
        db.execute(
            &format!("UPDATE \"{}\" SET {set} WHERE {condition}", m.table),
            params_from_iter(params),
        )
        .map_err(storage)?;
    } else {
        db.execute(
            &format!("DELETE FROM \"{}\" WHERE {condition}", m.table),
            params_from_iter(parameters),
        )
        .map_err(storage)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_budget_is_aggregate_and_verification_streams_exact_typed_rows() {
        let db = super::super::super::connection_for_test(":memory:").unwrap();
        // Use the production table inventory with compact synthetic rows.
        for table in TABLES {
            if *table == "meta" {
                db.execute_batch("CREATE TABLE meta(key TEXT,value TEXT)")
                    .unwrap();
            } else {
                db.execute_batch(&format!("CREATE TABLE {table}(value BLOB)"))
                    .unwrap();
            }
        }
        db.execute_batch("CREATE TABLE allocator(id INTEGER PRIMARY KEY AUTOINCREMENT); INSERT INTO meta VALUES('incarnation','existing'); INSERT INTO work_feed VALUES(X'0100FF'); INSERT INTO selected_tasks VALUES(CAST('extension 🦀' AS BLOB)); INSERT INTO selected_events VALUES(NULL);").unwrap();
        let mut bytes = ByteBudget(usize::MAX);
        visit_rows(&db, |table, cols, row| {
            serde_json::to_writer(
                &mut bytes,
                &BorrowedMutation {
                    table,
                    columns: cols,
                    before: None,
                    after: &borrowed_cells(row, cols.len())?,
                },
            )
            .map_err(storage)
        })
        .unwrap();
        let exact = usize::MAX - bytes.0;
        assert!(checkpoint_bounded(&db, exact - 1, 200000).is_err());
        let rows = checkpoint_bounded(&db, exact, 200000).unwrap();
        let mut frozen = FrozenState::default();
        frozen.extend(&rows).unwrap();
        frozen.verify(&db).unwrap();
        db.execute("UPDATE selected_tasks SET value=X'00'", [])
            .unwrap();
        assert!(frozen.verify(&db).is_err());
        assert!(checkpoint_bounded(&db, exact, 1).is_err());
    }
}
