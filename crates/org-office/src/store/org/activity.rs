//! IA-12 org activity reads of the log, newest first: each one read transaction, no writes.

use std::collections::{HashMap, HashSet};

use org_proto::org::{OrgChange, Position};
use rusqlite::Connection;

use crate::error::OfficeError;
use crate::organization::activity::{
    self as rules, ActivityEntry, ActivityError, ActivityPage, ActivityPosition, OrgActivityQuery,
    ProjectActivity, LOOKBACK_ROWS,
};
use crate::time;

/// Text in the stored JSON of every change that has, or resolves, a project subject (its type
/// tag or a `project_id`), so `latest_by_project` reads other rows as empty.
const PROJECT_MARK: &str = "project";

struct Row {
    seq: u64,
    command_id: String,
    at: String,
    actor: Option<String>,
    changes: Vec<OrgChange>,
}

/// One epoch's log from a top row down, read lazily in blocks; lookbacks count its rows.
#[derive(Default)]
struct Log {
    rows: Vec<Row>,
    /// Each entity's upserts as `(row, index)`, newest first.
    upserts: HashMap<(&'static str, String), Vec<(usize, usize)>>,
    end: bool,
}

/// Where a log is read from: the connection, the epoch, the top row's `seq`, and the text a
/// body needs for its changes and actor to be read.
type Source<'c> = (&'c Connection, u64, i64, Option<&'static str>);

impl Log {
    /// Whether row `k` exists, reading down to it.
    fn has(&mut self, (db, epoch, top, only): Source<'_>, k: usize) -> Result<bool, OfficeError> {
        while k >= self.rows.len() && !self.end {
            let block = (self.rows.len() + 32).min(512);
            let top = self.rows.last().map_or(top, |row| row.seq as i64 - 1);
            let mut statement = db
                .prepare_cached(
                    "SELECT seq, command_id, at,
                        CASE WHEN ?4 IS NULL OR instr(body, ?4) THEN body ELSE '[]' END,
                        CASE WHEN ?4 IS NULL THEN (SELECT principal_member FROM commands c
                            WHERE c.command_id = events.command_id) END
                     FROM events WHERE kind = 'org.changed' AND epoch = ?1 AND seq <= ?2
                     ORDER BY seq DESC LIMIT ?3",
                )
                .map_err(OfficeError::storage)?;
            let read = statement
                .query_map((epoch as i64, top, block as i64, only), |r| {
                    Ok((
                        r.get::<_, i64>(0)? as u64,
                        r.get(1)?,
                        r.get(2)?,
                        r.get::<_, String>(3)?,
                        r.get(4)?,
                    ))
                })
                .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
                .map_err(OfficeError::storage)?;
            self.end = read.len() < block;
            for (seq, command_id, at, body, actor) in read {
                let changes: Vec<OrgChange> = serde_json::from_str(&body)
                    .map_err(|_| OfficeError::Invalid("a logged entry is not readable"))?;
                for (index, change) in changes.iter().enumerate().rev() {
                    if let Some(((kind, id), true)) = rules::entity(change) {
                        let upserts = self.upserts.entry((kind, id.to_owned())).or_default();
                        upserts.push((self.rows.len(), index));
                    }
                }
                self.rows.push(Row {
                    seq,
                    command_id,
                    at,
                    actor,
                    changes,
                });
            }
        }
        Ok(k < self.rows.len())
    }

    /// The verb and subject of change `(k, i)`, against its entity's last upsert at most
    /// `LOOKBACK_ROWS` rows below row `k` (earlier changes of row `k` included).
    fn describe(
        &mut self,
        source: Source<'_>,
        k: usize,
        i: usize,
    ) -> Result<(String, rules::ActivitySubject), OfficeError> {
        let change = &self.rows[k].changes[i];
        let mut previous = None;
        if let Some(((kind, id), _)) =
            rules::entity(change).filter(|_| rules::needs_previous(change))
        {
            let key = (kind, id.to_owned());
            previous = loop {
                let older = self
                    .upserts
                    .get(&key)
                    .and_then(|upserts| {
                        upserts.get(upserts.partition_point(|&(j, x)| j < k || (j == k && x >= i)))
                    })
                    .copied();
                if older.is_some()
                    || self.rows.len() > k + LOOKBACK_ROWS
                    || !self.has(source, self.rows.len())?
                {
                    break older.filter(|&(j, _)| j <= k + LOOKBACK_ROWS);
                }
            };
        }
        let previous = previous.map(|(j, x)| &self.rows[j].changes[x]);
        Ok(rules::describe(&self.rows[k].changes[i], previous))
    }
}

/// The head (in `epoch`, at or above `ceiling`) and the oldest retained `seq` of its epoch.
fn open(
    tx: &Connection,
    epoch: u64,
    ceiling: u64,
) -> Result<(Position, Option<u64>), ActivityError> {
    let head = super::head(tx)?;
    if epoch != head.epoch || head.seq < ceiling {
        return Err(ActivityError::SnapshotRequired);
    }
    let sql = "SELECT min(seq) FROM events WHERE epoch = ?1";
    let oldest: Option<i64> = tx
        .query_row(sql, [head.epoch as i64], |r| r.get(0))
        .map_err(OfficeError::storage)?;
    Ok((head, oldest.map(|seq| seq as u64)))
}

/// One page: resolve, then filter, then stop at `limit` entries or `scan` rows.
pub(crate) fn page(
    db: &Connection,
    query: &OrgActivityQuery,
) -> Result<ActivityPage, ActivityError> {
    let tx = db.unchecked_transaction().map_err(OfficeError::storage)?;
    let (head, oldest) = open(&tx, query.epoch, query.ceiling)?;
    let mut top = query.ceiling as i64;
    if let Some(before) = query.before {
        // An epoch with no rows retains nothing below its head; nothing is above the ceiling.
        let retained = oldest.unwrap_or(head.seq + 1)..=query.ceiling;
        if before.epoch != head.epoch || !retained.contains(&before.seq) {
            return Err(ActivityError::SnapshotRequired);
        }
        top = top.min(before.seq as i64 - i64::from(before.index == 0));
    }
    let (source, mut log) = ((&*tx, head.epoch, top, None), Log::default());
    let at = |seq, index| ActivityPosition {
        epoch: head.epoch,
        seq,
        index,
    };
    let mut page = ActivityPage {
        entries: Vec::new(),
        scanned_to: None,
        frontier: query.prev_key,
        exhausted: false,
        history_truncated: !(head.epoch == super::EPOCH && oldest == Some(1)),
    };
    let mut k = 0;
    while log.has(source, k)? {
        if k == query.scan.max(1) {
            return Ok(page);
        }
        let (seq, at_ms) = (log.rows[k].seq, time::parse_rfc3339_ms(&log.rows[k].at));
        let key = rules::merge_key(page.frontier, at_ms);
        let below = query
            .before
            .filter(|b| b.seq == seq)
            .map_or(usize::MAX, |b| b.index);
        for index in (0..log.rows[k].changes.len().min(below)).rev() {
            let (verb, subject) = log.describe(source, k, index)?;
            let row = &log.rows[k];
            if !query.matches(row.actor.as_deref(), &subject) {
                continue;
            }
            if let Some(last) = page.entries.get(query.limit.max(1) - 1) {
                // A match past the limit: the page ends at its last returned entry.
                (page.scanned_to, page.frontier) = (Some(last.position), last.key);
                return Ok(page);
            }
            page.entries.push(ActivityEntry {
                position: at(seq, index),
                verb,
                at_ms,
                key,
                actor: row.actor.clone(),
                subject,
                group: row.command_id.clone(),
            });
        }
        (page.scanned_to, page.frontier) = (Some(at(seq, 0)), key);
        k += 1;
    }
    page.exhausted = true;
    Ok(page)
}

/// Per project, the `at_ms` of the first entry `page` returns with `project = id`: one pass
/// down from `ceiling`, until every project has its entry or the oldest row is read.
pub(crate) fn latest_by_project(
    db: &Connection,
    ceiling: Position,
    project_ids: &[String],
) -> Result<ProjectActivity, ActivityError> {
    let tx = db.unchecked_transaction().map_err(OfficeError::storage)?;
    let (head, _) = open(&tx, ceiling.epoch, ceiling.seq)?;
    let mut wanted: HashSet<&str> = project_ids.iter().map(String::as_str).collect();
    let (mut out, mut log) = (ProjectActivity::default(), Log::default());
    let source = (&*tx, head.epoch, ceiling.seq as i64, Some(PROJECT_MARK));
    let mut k = 0;
    while !wanted.is_empty() && log.has(source, k)? {
        let at_ms = time::parse_rfc3339_ms(&log.rows[k].at);
        for index in (0..log.rows[k].changes.len()).rev() {
            let (_, subject) = log.describe(source, k, index)?;
            if let Some(id) = subject.project.filter(|id| wanted.remove(id.as_str())) {
                match at_ms {
                    Some(at) => _ = out.latest_ms.insert(id, at),
                    None => out.degraded = true,
                }
            }
        }
        k += 1;
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
