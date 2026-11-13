//! The owner's work activity (IA-12 decision 7): read-only walks of the work feed, newest
//! first, each in one read transaction. No DDL: `work_events` has no `feed_seq` index, so a
//! target's events in a feed row are found by primary-key seeks (versions grow with
//! `feed_seq` and are contiguous within one row), never a table scan.

use std::collections::HashMap;
use std::sync::Arc;

use org_proto::work::WorkEvent;
use org_work::{
    LatestActivity, StoreError, WorkActivityEntry, WorkActivityError, WorkActivityPage,
    WorkActivityPosition, WorkActivityQuery, WorkActivityReads,
};
use rusqlite::{params, Connection, OptionalExtension};

use crate::store::{origin, Store};
use crate::OfficeError;

/// [`WorkActivityReads`] over office.db's read pool (query-only connections).
pub(crate) struct OfficeWorkActivity(pub(crate) Arc<Store>);

impl OfficeWorkActivity {
    fn read<R>(
        &self,
        work: impl FnOnce(&Connection) -> Result<R, OfficeError>,
    ) -> Result<R, StoreError> {
        self.0
            .read(|db| {
                // Roll back explicitly: a failed ROLLBACK must not return a reader to the pool
                // still inside this snapshot.
                let tx = sql(db.unchecked_transaction())?;
                let out = work(&tx);
                sql(tx.rollback())?;
                out
            })
            .map_err(StoreError::new)
    }
}

impl WorkActivityReads for OfficeWorkActivity {
    fn head(&self) -> Result<(Option<String>, u64), WorkActivityError> {
        Ok(self.read(|db| {
            let seq: Option<i64> =
                sql(db.query_row("SELECT max(seq) FROM work_feed", [], |r| r.get(0)))?;
            Ok((origin::stored_incarnation(db)?, unsigned(seq.unwrap_or(0))?))
        })?)
    }

    fn activity_page(
        &self,
        query: &WorkActivityQuery,
    ) -> Result<WorkActivityPage, WorkActivityError> {
        self.read(|db| {
            let stored = origin::stored_incarnation(db)?;
            if stored != query.incarnation && (query.incarnation.is_some() || query.ceiling > 0) {
                return Ok(Err(WorkActivityError::SnapshotRequired));
            }
            if let Some(before) = &query.before {
                if !stored_position(db, query.ceiling, before)? {
                    return Ok(Err(WorkActivityError::InvalidPosition));
                }
            }
            page(db, query).map(Ok)
        })?
    }

    fn latest_by_project(
        &self,
        ceiling: u64,
        project_ids: &[String],
    ) -> Result<Vec<(String, LatestActivity)>, WorkActivityError> {
        Ok(self.read(|db| latest_by_project(db, ceiling, project_ids))?)
    }
}

/// Whether `before` is a stored event at its feed row at or below the ceiling, or the start
/// of such a row (`(feed_seq, "", 0)`).
fn stored_position(
    db: &Connection,
    ceiling: u64,
    before: &WorkActivityPosition,
) -> Result<bool, OfficeError> {
    let (Ok(seq), Ok(version)) = (
        i64::try_from(before.feed_seq),
        i64::try_from(before.version),
    ) else {
        return Ok(false);
    };
    if before.feed_seq > ceiling {
        return Ok(false);
    }
    // An event's feed_seq references its work_feed row (whose targets list every put), so a
    // stored (target, version) at `seq` is in that row.
    let found: Option<i64> = if before.target_id.is_empty() {
        let row = "SELECT seq FROM work_feed WHERE seq = ?1 AND ?2 = 0";
        sql(db
            .query_row(row, params![seq, version], |r| r.get(0))
            .optional())?
    } else {
        let event = "SELECT feed_seq FROM work_events WHERE target_id = ?1 AND version = ?2";
        let at = params![before.target_id, version];
        sql(db.query_row(event, at, |r| r.get(0)).optional())?
    };
    Ok(found == Some(seq))
}

fn page(db: &Connection, q: &WorkActivityQuery) -> Result<WorkActivityPage, OfficeError> {
    // At least one row and one entry, so every page moves on.
    let (scan, limit) = (q.scan.max(1), q.limit.max(1) as usize);
    let resume = q.before.as_ref();
    let start = resume.map_or(q.ceiling, |b| b.feed_seq);
    let mut walk = Walk::new(db);
    let mut feed = sql(
        db.prepare("SELECT seq, targets FROM work_feed WHERE seq <= ?1 ORDER BY seq DESC LIMIT ?2")
    )?;
    let mut rows = sql(feed.query(params![bound(start), i64::from(scan) + 2]))?;
    let (mut key, mut scanned, mut scanned_to) = (q.prev_key, 0, None);
    let mut entries: Vec<WorkActivityEntry> = Vec::new();
    while let Some(row) = sql(rows.next())? {
        let seq = column(row, 0)?;
        let partial = resume.filter(|b| b.feed_seq == seq);
        // The rest of a row the previous page started doesn't count against the cap again,
        // so every page that scans moves on.
        if partial.is_none() {
            if scanned == scan {
                return Ok(WorkActivityPage {
                    entries,
                    scanned_to,
                    frontier: key,
                    exhausted: false,
                });
            }
            scanned += 1;
        }
        let targets: String = sql(row.get(1))?;
        let mut targets: Vec<String> =
            serde_json::from_str(&targets).map_err(OfficeError::storage)?;
        targets.sort_unstable_by(|a, b| b.cmp(a));
        targets.dedup();
        // The start of the row: where the next page resumes when it holds no events.
        scanned_to = Some(WorkActivityPosition {
            feed_seq: seq,
            target_id: String::new(),
            version: 0,
        });
        for target in targets {
            if let Some(b) = partial {
                if target > b.target_id {
                    continue;
                } else if target == b.target_id {
                    walk.lowest.insert(target.clone(), b.version);
                }
            }
            for (version, event) in walk.events_at(&target, seq)? {
                let at_ms = org_work::parse_ms(&event.at);
                key = at_ms.map_or(key, |at| at.min(key));
                let position = WorkActivityPosition {
                    feed_seq: seq,
                    target_id: target.clone(),
                    version,
                };
                if q.involves(&event) && walk.in_project(&target, q.project.as_deref())? {
                    // More matches than fit (limit ≥ 1, so one was returned): resume right
                    // after the last one returned.
                    if let Some(last) = entries.last().filter(|_| entries.len() == limit) {
                        let (scanned_to, frontier) = (Some(last.position.clone()), last.key);
                        return Ok(WorkActivityPage {
                            entries,
                            scanned_to,
                            frontier,
                            exhausted: false,
                        });
                    }
                    let position = position.clone();
                    entries.push(WorkActivityEntry {
                        position,
                        event,
                        at_ms,
                        key,
                    });
                }
                scanned_to = Some(position);
            }
        }
    }
    Ok(WorkActivityPage {
        entries,
        scanned_to,
        frontier: key,
        exhausted: true,
    })
}

fn latest_by_project(
    db: &Connection,
    ceiling: u64,
    projects: &[String],
) -> Result<Vec<(String, LatestActivity)>, OfficeError> {
    let mut query = sql(db.prepare(
        "SELECT id, json_extract(data, '$.project_id') FROM work_items
         WHERE json_extract(data, '$.project_id') IN (SELECT value FROM json_each(?1))",
    ))?;
    let ids = serde_json::to_string(projects).map_err(OfficeError::storage)?;
    let mut rows = sql(query.query([ids]))?;
    let walk = Walk::new(db);
    let mut newest: HashMap<String, (u64, String, u64)> = HashMap::new();
    while let Some(row) = sql(rows.next())? {
        let (id, project): (String, String) = (sql(row.get(0))?, sql(row.get(1))?);
        if let Some((version, seq)) = walk.last_at(&id, ceiling)? {
            let candidate = (seq, id, version);
            if newest.get(&project).is_none_or(|best| candidate > *best) {
                newest.insert(project, candidate);
            }
        }
    }
    projects
        .iter()
        .map(|project| {
            let latest = match newest.get(project) {
                Some((_, id, version)) => sql(db.query_row(
                    "SELECT json_extract(data, '$.at') FROM work_events
                     WHERE target_id = ?1 AND version = ?2",
                    params![id, bound(*version)],
                    |r| r.get::<_, Option<String>>(0),
                ))?
                .as_deref()
                .and_then(org_work::parse_ms)
                .map_or(LatestActivity::Unknown, LatestActivity::At),
                None => LatestActivity::None,
            };
            Ok((project.clone(), latest))
        })
        .collect()
}

struct Walk<'a> {
    db: &'a Connection,
    /// Each target's lowest version read on this page: its next older events are below it.
    lowest: HashMap<String, u64>,
    in_project: HashMap<String, bool>,
}

impl<'a> Walk<'a> {
    fn new(db: &'a Connection) -> Self {
        Self {
            db,
            lowest: HashMap::new(),
            in_project: HashMap::new(),
        }
    }

    /// The target's events in feed row `seq`, newest version first.
    fn events_at(&mut self, target: &str, seq: u64) -> Result<Vec<(u64, WorkEvent)>, OfficeError> {
        let seen = self
            .lowest
            .get(target)
            .map(|lowest| lowest.saturating_sub(1));
        // Unseen on this page: its newest event is usually in this row; when the target
        // changed since, search its history for the row.
        let found = match self.read_down(target, seq, seen.unwrap_or(u64::MAX))? {
            Some(found) => found,
            None if seen.is_none() => match self.last_at(target, seq)? {
                Some((version, _)) => self
                    .read_down(target, seq, version)?
                    .ok_or_else(inconsistent)?,
                None => Vec::new(),
            },
            None => return Err(inconsistent()),
        };
        if let Some((version, _)) = found.last() {
            self.lowest.insert(target.to_owned(), *version);
        }
        Ok(found)
    }

    /// The target's events in row `seq` at or below version `top`, newest first; `None` when
    /// the first one read is in a newer row.
    fn read_down(
        &self,
        target: &str,
        seq: u64,
        top: u64,
    ) -> Result<Option<Vec<(u64, WorkEvent)>>, OfficeError> {
        let mut query = sql(self.db.prepare_cached(
            "SELECT version, feed_seq, data FROM work_events
             WHERE target_id = ?1 AND version <= ?2 ORDER BY version DESC",
        ))?;
        let mut rows = sql(query.query(params![target, bound(top)]))?;
        let mut found = Vec::new();
        while let Some(row) = sql(rows.next())? {
            let version = column(row, 0)?;
            match column(row, 1)? {
                feed if feed > seq && found.is_empty() => return Ok(None),
                feed if feed > seq => return Err(inconsistent()),
                feed if feed < seq => break,
                _ => {}
            }
            let data: String = sql(row.get(2))?;
            let event: WorkEvent = serde_json::from_str(&data).map_err(OfficeError::storage)?;
            if event.target_id != target || event.version != version {
                return Err(inconsistent());
            }
            found.push((version, event));
        }
        Ok(Some(found))
    }

    /// The target's newest `(version, feed_seq)` at or below feed row `seq`: a binary search
    /// over its contiguous versions, since `feed_seq` grows with the version.
    fn last_at(&self, target: &str, seq: u64) -> Result<Option<(u64, u64)>, OfficeError> {
        let point = |sql_text: &str, version: u64| -> Result<Option<(u64, u64)>, OfficeError> {
            let mut q = sql(self.db.prepare_cached(sql_text))?;
            let row = sql(q
                .query_row(params![target, bound(version)], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .optional())?;
            row.map(|(v, f): (i64, i64)| Ok((unsigned(v)?, unsigned(f)?)))
                .transpose()
        };
        let newest =
            "SELECT version, feed_seq FROM work_events WHERE target_id = ?1 AND version <= ?2
             ORDER BY version DESC LIMIT 1";
        let Some((top, top_feed)) = point(newest, u64::MAX)? else {
            return Ok(None);
        };
        if top_feed <= seq {
            return Ok(Some((top, top_feed)));
        }
        // Invariant: version `low` (0: none) is at or below `seq`; version `high` is above.
        let (mut low, mut high, mut low_feed) = (0, top, 0);
        while high - low > 1 {
            let middle = low + (high - low) / 2;
            let exact =
                "SELECT version, feed_seq FROM work_events WHERE target_id = ?1 AND version = ?2";
            match point(exact, middle)?.ok_or_else(inconsistent)?.1 {
                feed if feed <= seq => (low, low_feed) = (middle, feed),
                _ => high = middle,
            }
        }
        Ok((low > 0).then_some((low, low_feed)))
    }

    /// Whether the target is currently in `project` (always, with no project filter).
    fn in_project(&mut self, target: &str, project: Option<&str>) -> Result<bool, OfficeError> {
        let Some(project) = project else {
            return Ok(true);
        };
        if let Some(&known) = self.in_project.get(target) {
            return Ok(known);
        }
        let mut q = sql(self.db.prepare_cached(
            "SELECT EXISTS (SELECT 1 FROM work_items
             WHERE id = ?1 AND json_extract(data, '$.project_id') = ?2)",
        ))?;
        let known = sql(q.query_row(params![target, project], |r| r.get(0)))?;
        self.in_project.insert(target.to_owned(), known);
        Ok(known)
    }
}

fn sql<T>(result: rusqlite::Result<T>) -> Result<T, OfficeError> {
    result.map_err(OfficeError::storage)
}

fn column(row: &rusqlite::Row<'_>, index: usize) -> Result<u64, OfficeError> {
    unsigned(sql(row.get(index))?)
}

fn inconsistent() -> OfficeError {
    OfficeError::Invalid("stored work feed and events are inconsistent")
}

/// SQLite's integers are signed; every stored one fits, so a larger bound saturates.
fn bound(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn unsigned(value: i64) -> Result<u64, OfficeError> {
    u64::try_from(value).map_err(|_| inconsistent())
}

#[cfg(test)]
#[path = "tests/activity.rs"]
mod tests;
