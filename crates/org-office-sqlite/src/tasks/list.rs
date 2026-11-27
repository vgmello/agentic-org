//! `tasks.list` in SQL (`TASK-105`): every filter of a [`TaskListQuery`], its order, offset
//! and limit run in SQLite, so a page costs a count, the page, its four hydration reads and
//! its five rollup reads, whatever the organization holds.
//!
//! Each filter states the same rule as the `MemoryEngine` reference scan (`ENG-1`):
//!
//! - an open scope is written `status NOT IN ('done', 'cancelled')` verbatim, the term of the
//!   partial indexes `tasks_open_by_project` and `tasks_open_by_team`, whose columns are the
//!   default order, so an open page is read in order straight from the index;
//! - `done_since` compares instants: `updated_at` is read as Unix milliseconds by
//!   `rfc3339_ms`, the connection's binding of the reference scan's own RFC 3339 parser, never
//!   compared as text (`...00Z` and `...00.000Z` are one instant) and never by SQLite's date
//!   functions, which round sub-millisecond digits and read times RFC 3339 refuses;
//! - text search is ASCII-lowercase substring search (`lower`, `instr`), never `LIKE`, so `%`
//!   and `_` in a query are plain characters;
//! - a `modified_file` path matches itself and the paths under it by whole components.

use std::collections::BTreeMap;

use org_proto::tasks::{ChecklistSection, LabelMatchMode, TaskSortMode, TaskStatus};
use org_store::{EngineError, Id};
use rusqlite::{types::Value, Connection};

use super::super::organization::{id, number, text, whole, META};
use super::hydrate::{hydrate, Owners};
use super::rows::task_row;
use super::{fetch, id_list, CHUNK, TASK_COLS};
use org_office::tasks::query::{
    LabelFilter, TaskListPage, TaskListQuery, TaskRollup, TaskStatusSet,
};

/// The open statuses, as the open-task partial indexes state them.
const OPEN: &str = "t.status NOT IN ('done', 'cancelled')";

/// A `WHERE` clause over `tasks t` and the values it binds; `?1` is the organization.
pub(super) struct Clause {
    sql: String,
    values: Vec<Value>,
}

impl Clause {
    fn bind(&mut self, value: Value) -> String {
        self.values.push(value);
        format!("?{}", self.values.len())
    }

    fn and(&mut self, term: &str) {
        self.sql.push_str(" AND ");
        self.sql.push_str(term);
    }

    /// `column = ?` for a filter that is set.
    fn equal(&mut self, column: &str, wanted: Option<&str>) {
        if let Some(wanted) = wanted {
            let at = self.bind(text(wanted));
            self.and(&format!("{column} = {at}"));
        }
    }
}

/// The page `query` asks for, and how many tasks match in all.
pub(super) fn page(
    db: &Connection,
    scope: &str,
    query: &TaskListQuery,
) -> Result<TaskListPage, EngineError> {
    let clause = filter(scope, query)?;
    let count = format!("SELECT COUNT(*) FROM tasks t WHERE {}", clause.sql);
    let total = fetch(db, &count, clause.values.clone(), |r| number(r, 0))?
        .pop()
        .unwrap_or(0);
    if query.limit == Some(0) || query.offset >= total {
        return Ok(TaskListPage {
            tasks: Vec::new(),
            total,
        });
    }
    let (sql, values) = page_statement(clause, query);
    let rows = fetch(db, &sql, values, task_row)?;
    let tasks = hydrate(db, scope, rows, Owners::Listed)?;
    Ok(TaskListPage { tasks, total })
}

/// The page's statement and its values: the filter, the order, the limit and the offset.
pub(super) fn page_statement(clause: Clause, query: &TaskListQuery) -> (String, Vec<Value>) {
    let mut values = clause.values;
    let limit = query
        .limit
        .map_or(-1, |limit| i64::try_from(limit).unwrap_or(i64::MAX));
    values.push(Value::Integer(limit));
    let limit_at = values.len();
    values.push(Value::Integer(
        i64::try_from(query.offset).unwrap_or(i64::MAX),
    ));
    let offset_at = values.len();
    let sql = format!(
        "SELECT {TASK_COLS}, {META} FROM tasks t WHERE {} ORDER BY {} \
         LIMIT ?{limit_at} OFFSET ?{offset_at}",
        clause.sql,
        order_by(query.sort)
    );
    (sql, values)
}

fn order_by(sort: TaskSortMode) -> &'static str {
    match sort {
        TaskSortMode::Order => "t.rank_order IS NULL, t.rank_order, t.office_code, t.number, t.id",
        TaskSortMode::Number => "t.office_code, t.number, t.id",
        TaskSortMode::UpdatedDesc => "t.updated_at DESC, t.number DESC, t.id",
    }
}

pub(super) fn filter(scope: &str, query: &TaskListQuery) -> Result<Clause, EngineError> {
    let mut clause = Clause {
        sql: "t.organization_id = ?1".to_owned(),
        values: vec![text(scope)],
    };
    if let Some(kind) = &query.task_type {
        let at = clause.bind(text(kind));
        clause.and(&format!("t.type = {at} COLLATE NOCASE"));
    }
    statuses(&mut clause, query)?;
    clause.equal("t.team_id", query.team_id.as_deref());
    clause.equal("t.assigned_team_id", query.assigned_team_id.as_deref());
    clause.equal("t.assignee_id", query.assignee_id.as_deref());
    clause.equal("t.project_id", query.project_id.as_deref());
    if query.unassigned {
        clause.and("t.assignee_id IS NULL");
    }
    if let Some(labels) = &query.labels {
        carries(&mut clause, labels);
    }
    if let Some(target) = &query.depends_on {
        let at = clause.bind(text(target.as_str()));
        clause.and(&format!(
            "t.id IN (SELECT k.source_id FROM links k WHERE k.organization_id = ?1 \
             AND k.source_kind = 'task' AND k.kind = 'depends_on' \
             AND k.target_kind = 'task' AND k.target_id = {at})"
        ));
    }
    if let Some(source) = &query.dependent_of {
        let at = clause.bind(text(source.as_str()));
        clause.and(&format!(
            "t.id IN (SELECT k.target_id FROM links k WHERE k.organization_id = ?1 \
             AND k.source_kind = 'task' AND k.source_id = {at} AND k.kind = 'depends_on')"
        ));
    }
    if let Some(q) = &query.text {
        mentions(&mut clause, q);
    }
    if let Some(path) = &query.modified_file {
        let at = clause.bind(text(path));
        clause.and(&format!(
            "EXISTS (SELECT 1 FROM task_files f WHERE f.organization_id = t.organization_id \
             AND f.task_id = t.id AND f.role = 'modified' \
             AND (f.path = {at} OR f.path = './' || {at} \
               OR (f.path >= {at} || '/' AND f.path < {at} || '0') \
               OR (f.path >= './' || {at} || '/' AND f.path < './' || {at} || '0')))"
        ));
    }
    Ok(clause)
}

/// The status set, and `done_since` for the `done` tasks it admits. Status names are the
/// closed enum's own, never caller text.
fn statuses(clause: &mut Clause, query: &TaskListQuery) -> Result<(), EngineError> {
    let set = query.statuses;
    if set == TaskStatusSet::OPEN {
        clause.and(OPEN);
    } else if set.is_empty() {
        clause.and("0");
    } else if set != TaskStatusSet::ALL {
        let names: Vec<String> = set.iter().map(|s| format!("'{}'", s.as_str())).collect();
        clause.and(&format!("t.status IN ({})", names.join(", ")));
    }
    if let Some(since) = query.done_since_ms {
        if set.contains(TaskStatus::Done) {
            let at = clause.bind(whole(since)?);
            clause.and(&format!(
                "(t.status <> 'done' OR rfc3339_ms(t.updated_at) >= {at})"
            ));
        }
    }
    Ok(())
}

fn carries(clause: &mut Clause, filter: &LabelFilter) {
    let has = |at: &str| {
        format!(
            "EXISTS (SELECT 1 FROM task_labels l WHERE l.organization_id = t.organization_id \
             AND l.task_id = t.id AND l.label = {at})"
        )
    };
    // No labels: every task carries all of none, and none carries any of none.
    if filter.labels.is_empty() {
        if filter.mode == LabelMatchMode::Any {
            clause.and("0");
        }
        return;
    }
    let terms: Vec<String> = filter
        .labels
        .iter()
        .map(|label| {
            let at = clause.bind(text(label));
            has(&at)
        })
        .collect();
    match filter.mode {
        LabelMatchMode::All => clause.and(&terms.join(" AND ")),
        LabelMatchMode::Any => clause.and(&format!("({})", terms.join(" OR "))),
    }
}

/// `q` (already lowercase) in the key, title, body, a label or a file path.
fn mentions(clause: &mut Clause, q: &str) {
    let at = clause.bind(text(q));
    clause.and(&format!(
        "(instr(lower(t.key), {at}) > 0 OR instr(lower(t.title), {at}) > 0 \
         OR instr(lower(t.body_markdown), {at}) > 0 \
         OR EXISTS (SELECT 1 FROM task_labels l WHERE l.organization_id = t.organization_id \
           AND l.task_id = t.id AND instr(l.label, {at}) > 0) \
         OR EXISTS (SELECT 1 FROM task_files f WHERE f.organization_id = t.organization_id \
           AND f.task_id = t.id AND instr(lower(f.path), {at}) > 0))"
    ));
}

/// The rollup of each of `ids`: five statements per [`CHUNK`] of IDs.
pub(super) fn rollups(
    db: &Connection,
    scope: &str,
    ids: &[Id],
) -> Result<BTreeMap<Id, TaskRollup>, EngineError> {
    let mut out: BTreeMap<Id, TaskRollup> = ids
        .iter()
        .map(|id| (id.clone(), TaskRollup::default()))
        .collect();
    // Each ID once, so a repeated ID can neither repeat links nor split across chunks.
    let unique: Vec<Id> = out.keys().cloned().collect();
    for chunk in unique.chunks(CHUNK) {
        let mut values = vec![text(scope)];
        let targets = id_list(chunk, &mut values);
        let steps = fetch(
            db,
            &format!(
                "SELECT target_id, section, COUNT(*), SUM(done <> 0) FROM checklist_steps \
                 WHERE organization_id = ?1 AND target_kind = 'task' AND target_id IN {targets} \
                 GROUP BY target_id, section"
            ),
            values.clone(),
            |r| {
                let section: String = r.get(1)?;
                let section = ChecklistSection::parse(&section)
                    .ok_or_else(|| super::bad_col(1, "invalid section"))?;
                Ok((id(r, 0)?, section, number(r, 2)?, number(r, 3)?))
            },
        )?;
        for (task, section, total, done) in steps {
            out.entry(task).or_default().add_steps(section, total, done);
        }
        for (table, set) in [
            ("comments", set_comments as fn(&mut TaskRollup, u64)),
            ("watchers", set_watchers),
        ] {
            let counts = fetch(
                db,
                &format!(
                    "SELECT target_id, COUNT(*) FROM {table} WHERE organization_id = ?1 \
                     AND target_kind = 'task' AND target_id IN {targets} GROUP BY target_id"
                ),
                values.clone(),
                |r| Ok((id(r, 0)?, number(r, 1)?)),
            )?;
            for (task, count) in counts {
                set(out.entry(task).or_default(), count);
            }
        }
        let upstream = fetch(
            db,
            &format!(
                "SELECT source_id, target_id FROM links WHERE organization_id = ?1 \
                 AND source_kind = 'task' AND source_id IN {targets} AND kind = 'depends_on' \
                 ORDER BY source_id, target_id"
            ),
            values.clone(),
            |r| Ok((id(r, 0)?, id(r, 1)?)),
        )?;
        for (task, target) in upstream {
            out.entry(task).or_default().depends_on.push(target);
        }
        let downstream = fetch(
            db,
            &format!(
                "SELECT target_id, source_id FROM links WHERE organization_id = ?1 \
                 AND source_kind = 'task' AND kind = 'depends_on' AND target_kind = 'task' \
                 AND target_id IN {targets} ORDER BY target_id, source_id"
            ),
            values,
            |r| Ok((id(r, 0)?, id(r, 1)?)),
        )?;
        for (task, source) in downstream {
            out.entry(task).or_default().dependents.push(source);
        }
    }
    Ok(out)
}

fn set_comments(rollup: &mut TaskRollup, count: u64) {
    rollup.comment_count = count;
}

fn set_watchers(rollup: &mut TaskRollup, count: u64) {
    rollup.watcher_count = count;
}
