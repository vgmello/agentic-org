//! SQLite adapter for universal reviews (`office_review_counters`, `reviews`, `review_rounds`,
//! `review_reviewers`, `review_findings`, `review_finding_replies`, `review_events`).

use org_proto::reviews::{
    FindingAnchor, FindingSeverity, FindingStatus, ReviewSessionMode, ReviewStatus,
    ReviewTargetKind, ReviewVerdict, RoutingMode,
};
use org_store::{EngineError, Id};
use rusqlite::{
    params,
    types::{Type, Value},
    Connection, Row,
};

use org_office::reviews::ports::{ReviewListQuery, ReviewsRead, ReviewsWrite};
use org_office::reviews::records::{
    canonical_json, OfficeReviewCounterRecord, ReviewDocument, ReviewEventRecord,
    ReviewFindingRecord, ReviewFindingReplyRecord, ReviewRecord, ReviewReviewerRecord,
    ReviewRoundRecord, SCHEMA_VERSION,
};

use super::failed;
use super::organization::{all, id, number, one, optional_id as opt_id, text as txt, whole, View};
use super::{SqliteSnapshot, SqliteTx};

const REVIEW_COLS: &str = "id, office_id, office_code, number, project_id, task_id, target_kind, \
    repository_id, branch, target_ref, kind_slug, category_slug, title, body_markdown, independent, \
    status, current_round, session_mode, guidance_refs_json, criteria_json, closed_reason, \
    revision, created_by, created_at, updated_at, closed_at";
const ROUND_COLS: &str = "review_id, round_number, base_oid, head_oid, session_mode, submitted_by, submitted_at, note_markdown";
const REVIEWER_COLS: &str =
    "review_id, round_number, reviewer_id, routing, team_id, session_mode, \
    assigned_by, assigned_at, removed, verdict, summary_markdown, finding_ids_json, \
    attachment_ids_json, verdict_at, runtime_session_id";
const FINDING_COLS: &str = "id, review_id, number, round_number, author_id, severity, status, \
    anchor_json, body_markdown, attachment_ids_json, resolved_by, resolved_at, resolved_in_round, \
    revision, created_at, updated_at";
const REPLY_COLS: &str = "id, finding_id, review_id, round_number, author_id, body_markdown, set_status, attachment_ids_json, created_at";
const EVENT_COLS: &str = "review_id, revision, event_type, actor_id, operation_id, payload_json, payload_sha256, created_at";

fn bad(c: usize, w: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(c, Type::Text, w.into())
}
fn u32_c(r: &Row<'_>, c: usize) -> rusqlite::Result<u32> {
    u32::try_from(r.get::<_, i64>(c)?).map_err(|_| bad(c, "u32"))
}
fn opt_u32(r: &Row<'_>, c: usize) -> rusqlite::Result<Option<u32>> {
    r.get::<_, Option<i64>>(c)?
        .map(|v| u32::try_from(v).map_err(|_| bad(c, "u32")))
        .transpose()
}
fn bool_c(r: &Row<'_>, c: usize) -> rusqlite::Result<bool> {
    match r.get::<_, i64>(c)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(bad(c, "bool")),
    }
}
fn en<T>(r: &Row<'_>, c: usize, p: fn(&str) -> Option<T>, w: &str) -> rusqlite::Result<T> {
    p(&r.get::<_, String>(c)?).ok_or_else(|| bad(c, w))
}
fn opt_en<T>(
    r: &Row<'_>,
    c: usize,
    p: fn(&str) -> Option<T>,
    w: &str,
) -> rusqlite::Result<Option<T>> {
    r.get::<_, Option<String>>(c)?
        .map(|s| p(&s).ok_or_else(|| bad(c, w)))
        .transpose()
}
fn js<T: serde::de::DeserializeOwned>(r: &Row<'_>, c: usize, w: &str) -> rusqlite::Result<T> {
    serde_json::from_str(&r.get::<_, String>(c)?).map_err(|_| bad(c, w))
}
fn js_str<T: serde::Serialize>(v: &T) -> String {
    canonical_json(&serde_json::to_value(v).expect("JSON"))
}
fn i64_u64(v: u64) -> Result<i64, EngineError> {
    i64::try_from(v).map_err(|_| EngineError::new("number too large"))
}
fn exec(db: &Connection, sql: &str, p: impl rusqlite::Params) -> Result<(), EngineError> {
    db.prepare_cached(sql)
        .and_then(|mut s| s.execute(p))
        .map_err(|e| failed(db, "writing review row", &e))?;
    Ok(())
}

fn review_row(r: &Row<'_>) -> rusqlite::Result<ReviewRecord> {
    Ok(ReviewRecord {
        id: id(r, 0)?,
        office_id: id(r, 1)?,
        office_code: r.get(2)?,
        number: number(r, 3)?,
        project_id: opt_id(r, 4)?,
        task_id: opt_id(r, 5)?,
        target_kind: en(r, 6, ReviewTargetKind::parse, "target")?,
        repository_id: opt_id(r, 7)?,
        branch: r.get(8)?,
        target_ref: r.get(9)?,
        kind_slug: r.get(10)?,
        category_slug: r.get(11)?,
        title: r.get(12)?,
        body_markdown: r.get(13)?,
        independent: bool_c(r, 14)?,
        status: en(r, 15, ReviewStatus::parse, "status")?,
        current_round: u32_c(r, 16)?,
        session_mode: en(r, 17, ReviewSessionMode::parse, "mode")?,
        guidance_refs: js(r, 18, "refs")?,
        criteria: js(r, 19, "criteria")?,
        closed_reason: r.get(20)?,
        revision: number(r, 21)?,
        created_by: id(r, 22)?,
        created_at: r.get(23)?,
        updated_at: r.get(24)?,
        closed_at: r.get(25)?,
    })
}

fn round_row(r: &Row<'_>) -> rusqlite::Result<ReviewRoundRecord> {
    Ok(ReviewRoundRecord {
        review_id: id(r, 0)?,
        round_number: u32_c(r, 1)?,
        base_oid: r.get(2)?,
        head_oid: r.get(3)?,
        session_mode: en(r, 4, ReviewSessionMode::parse, "mode")?,
        submitted_by: id(r, 5)?,
        submitted_at: r.get(6)?,
        note_markdown: r.get(7)?,
    })
}

fn reviewer_row(r: &Row<'_>) -> rusqlite::Result<ReviewReviewerRecord> {
    let fids = js::<Vec<String>>(r, 11, "fids")?
        .into_iter()
        .map(|s| Id::parse(&s).map_err(|_| bad(11, "id")))
        .collect::<rusqlite::Result<_>>()?;
    Ok(ReviewReviewerRecord {
        review_id: id(r, 0)?,
        round_number: u32_c(r, 1)?,
        reviewer_id: id(r, 2)?,
        routing: en(r, 3, RoutingMode::parse, "routing")?,
        team_id: opt_id(r, 4)?,
        session_mode: en(r, 5, ReviewSessionMode::parse, "mode")?,
        assigned_by: id(r, 6)?,
        assigned_at: r.get(7)?,
        removed: bool_c(r, 8)?,
        verdict: opt_en(r, 9, ReviewVerdict::parse, "verdict")?,
        summary_markdown: r.get(10)?,
        finding_ids: fids,
        attachment_ids: js(r, 12, "atts")?,
        verdict_at: r.get(13)?,
        runtime_session_id: r.get(14)?,
    })
}

fn finding_row(r: &Row<'_>) -> rusqlite::Result<ReviewFindingRecord> {
    Ok(ReviewFindingRecord {
        id: id(r, 0)?,
        review_id: id(r, 1)?,
        number: number(r, 2)?,
        round_number: u32_c(r, 3)?,
        author_id: id(r, 4)?,
        severity: en(r, 5, FindingSeverity::parse, "sev")?,
        status: en(r, 6, FindingStatus::parse, "status")?,
        anchor: js::<FindingAnchor>(r, 7, "anchor")?,
        body_markdown: r.get(8)?,
        attachment_ids: js(r, 9, "atts")?,
        resolved_by: opt_id(r, 10)?,
        resolved_at: r.get(11)?,
        resolved_in_round: opt_u32(r, 12)?,
        revision: number(r, 13)?,
        created_at: r.get(14)?,
        updated_at: r.get(15)?,
    })
}

fn reply_row(r: &Row<'_>) -> rusqlite::Result<ReviewFindingReplyRecord> {
    Ok(ReviewFindingReplyRecord {
        id: id(r, 0)?,
        finding_id: id(r, 1)?,
        review_id: id(r, 2)?,
        round_number: u32_c(r, 3)?,
        author_id: id(r, 4)?,
        body_markdown: r.get(5)?,
        set_status: opt_en(r, 6, FindingStatus::parse, "st")?,
        attachment_ids: js(r, 7, "atts")?,
        created_at: r.get(8)?,
    })
}

fn event_row(r: &Row<'_>) -> rusqlite::Result<ReviewEventRecord> {
    Ok(ReviewEventRecord {
        review_id: id(r, 0)?,
        revision: number(r, 1)?,
        event_type: r.get(2)?,
        actor_id: id(r, 3)?,
        operation_id: r.get(4)?,
        payload_json: r.get(5)?,
        payload_sha256: r.get(6)?,
        created_at: r.get(7)?,
    })
}

fn counter_row(r: &Row<'_>) -> rusqlite::Result<OfficeReviewCounterRecord> {
    Ok(OfficeReviewCounterRecord {
        office_id: id(r, 0)?,
        office_code: r.get(1)?,
        next_number: number(r, 2)?,
    })
}

fn by_id<T>(
    v: &impl View,
    t: &str,
    c: &str,
    k: &Id,
    m: fn(&Row<'_>) -> rusqlite::Result<T>,
) -> Result<Option<T>, EngineError> {
    let (db, s) = v.view()?;
    one(
        db,
        &format!("SELECT {c} FROM {t} WHERE organization_id = ?1 AND id = ?2"),
        vec![txt(s), txt(k.as_str())],
        m,
    )
}

fn by_rev<T>(
    v: &impl View,
    t: &str,
    c: &str,
    o: &str,
    k: &Id,
    m: fn(&Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>, EngineError> {
    let (db, s) = v.view()?;
    all(
        db,
        &format!("SELECT {c} FROM {t} WHERE organization_id = ?1 AND review_id = ?2 ORDER BY {o}"),
        vec![txt(s), txt(k.as_str())],
        m,
    )
}

fn list_reviews_query(
    v: &impl View,
    q: &ReviewListQuery,
) -> Result<Vec<ReviewRecord>, EngineError> {
    let (db, s) = v.view()?;
    let mut sql = format!("SELECT {REVIEW_COLS} FROM reviews r WHERE r.organization_id = ?1");
    let mut vals = vec![txt(s)];
    let mut add = |col: &str, val: Value| {
        vals.push(val);
        sql.push_str(&format!(" AND {col} = ?{}", vals.len()));
    };
    if let Some(pid) = &q.project_id {
        add("r.project_id", txt(pid.as_str()));
    }
    if let Some(tid) = &q.task_id {
        add("r.task_id", txt(tid.as_str()));
    }
    if let Some(st) = q.status {
        add("r.status", txt(st.as_str()));
    }
    if let Some(tk) = q.target_kind {
        add("r.target_kind", txt(tk.as_str()));
    }
    if let Some(cb) = &q.created_by {
        add("r.created_by", txt(cb.as_str()));
    }
    if let Some(rev) = &q.reviewer_id {
        vals.push(txt(rev.as_str()));
        sql.push_str(&format!(" AND EXISTS (SELECT 1 FROM review_reviewers rr WHERE rr.organization_id = r.organization_id AND rr.review_id = r.id AND rr.round_number = r.current_round AND rr.reviewer_id = ?{} AND rr.removed = 0)", vals.len()));
    }
    sql.push_str(" ORDER BY r.updated_at DESC, r.id DESC");
    all(db, &sql, vals, review_row)
}

macro_rules! sqlite_reviews_read {
    ($($v:ty),+) => {$(
        impl ReviewsRead for $v {
            fn review(&self, k: &Id) -> Result<Option<ReviewRecord>, EngineError> { by_id(self, "reviews", REVIEW_COLS, k, review_row) }
            fn review_by_key(&self, c: &str, n: u64) -> Result<Option<ReviewRecord>, EngineError> { let (db, s) = self.view()?; one(db, &format!("SELECT {REVIEW_COLS} FROM reviews WHERE organization_id=?1 AND office_code=?2 AND number=?3"), vec![txt(s), txt(c), whole(n)?], review_row) }
            fn list_reviews(&self, q: &ReviewListQuery) -> Result<Vec<ReviewRecord>, EngineError> { list_reviews_query(self, q) }
            fn max_review_number(&self, c: &str) -> Result<u64, EngineError> { let (db, s) = self.view()?; Ok(one(db, "SELECT MAX(number) FROM reviews WHERE organization_id=?1 AND office_code=?2", vec![txt(s), txt(c)], |r| r.get::<_, Option<i64>>(0))?.flatten().and_then(|n| u64::try_from(n).ok()).unwrap_or(0)) }
            fn office_review_counter(&self, c: &str) -> Result<Option<OfficeReviewCounterRecord>, EngineError> { let (db, s) = self.view()?; one(db, "SELECT office_id, office_code, next_number FROM office_review_counters WHERE organization_id=?1 AND office_code=?2", vec![txt(s), txt(c)], counter_row) }
            fn office_review_counters(&self) -> Result<Vec<OfficeReviewCounterRecord>, EngineError> { let (db, s) = self.view()?; all(db, "SELECT office_id, office_code, next_number FROM office_review_counters WHERE organization_id=?1 ORDER BY office_code", vec![txt(s)], counter_row) }
            fn review_rounds(&self, k: &Id) -> Result<Vec<ReviewRoundRecord>, EngineError> { by_rev(self, "review_rounds", ROUND_COLS, "round_number", k, round_row) }
            fn review_reviewers(&self, k: &Id) -> Result<Vec<ReviewReviewerRecord>, EngineError> { by_rev(self, "review_reviewers", REVIEWER_COLS, "round_number, reviewer_id", k, reviewer_row) }
            fn review_findings(&self, k: &Id) -> Result<Vec<ReviewFindingRecord>, EngineError> { by_rev(self, "review_findings", FINDING_COLS, "number", k, finding_row) }
            fn review_finding_by_id(&self, k: &Id) -> Result<Option<ReviewFindingRecord>, EngineError> { by_id(self, "review_findings", FINDING_COLS, k, finding_row) }
            fn review_finding_replies(&self, k: &Id) -> Result<Vec<ReviewFindingReplyRecord>, EngineError> { by_rev(self, "review_finding_replies", REPLY_COLS, "created_at, id", k, reply_row) }
            fn review_finding_reply_by_id(&self, k: &Id) -> Result<Option<ReviewFindingReplyRecord>, EngineError> { by_id(self, "review_finding_replies", REPLY_COLS, k, reply_row) }
            fn review_events(&self, k: &Id) -> Result<Vec<ReviewEventRecord>, EngineError> { by_rev(self, "review_events", EVENT_COLS, "revision", k, event_row) }
            fn review_child_parent_ids(&self) -> Result<Vec<Id>, EngineError> { let (db, s) = self.view()?; all(db, "SELECT review_id FROM review_rounds WHERE organization_id=?1 UNION SELECT review_id FROM review_reviewers WHERE organization_id=?1 UNION SELECT review_id FROM review_findings WHERE organization_id=?1 UNION SELECT review_id FROM review_finding_replies WHERE organization_id=?1 UNION SELECT review_id FROM review_events WHERE organization_id=?1 ORDER BY 1", vec![txt(s)], |r| id(r, 0)) }
        }
    )+};
}
sqlite_reviews_read!(SqliteTx, SqliteSnapshot);

fn put_doc(db: &Connection, s: &str, doc: &ReviewDocument) -> Result<(), EngineError> {
    let r = &doc.review;
    exec(db, "INSERT INTO reviews (organization_id, id, office_id, office_code, number, project_id, task_id, target_kind, repository_id, branch, target_ref, kind_slug, category_slug, title, body_markdown, independent, status, current_round, session_mode, guidance_refs_json, criteria_json, closed_reason, revision, created_by, created_at, updated_at, closed_at, schema_version) \
              VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28) \
              ON CONFLICT (organization_id, id) DO UPDATE SET project_id=excluded.project_id, task_id=excluded.task_id, target_kind=excluded.target_kind, repository_id=excluded.repository_id, branch=excluded.branch, target_ref=excluded.target_ref, kind_slug=excluded.kind_slug, category_slug=excluded.category_slug, title=excluded.title, body_markdown=excluded.body_markdown, independent=excluded.independent, status=excluded.status, current_round=excluded.current_round, session_mode=excluded.session_mode, guidance_refs_json=excluded.guidance_refs_json, criteria_json=excluded.criteria_json, closed_reason=excluded.closed_reason, revision=excluded.revision, updated_at=excluded.updated_at, closed_at=excluded.closed_at",
        params![s, r.id.as_str(), r.office_id.as_str(), &r.office_code, i64_u64(r.number)?, r.project_id.as_ref().map(Id::as_str), r.task_id.as_ref().map(Id::as_str), r.target_kind.as_str(), r.repository_id.as_ref().map(Id::as_str), r.branch.as_deref(), r.target_ref.as_deref(), &r.kind_slug, r.category_slug.as_deref(), &r.title, &r.body_markdown, i64::from(r.independent), r.status.as_str(), i64::from(r.current_round), r.session_mode.as_str(), js_str(&r.guidance_refs), js_str(&r.criteria), r.closed_reason.as_deref(), i64_u64(r.revision)?, r.created_by.as_str(), &r.created_at, &r.updated_at, r.closed_at.as_deref(), i64_u64(SCHEMA_VERSION)?])?;
    put_reviewers(db, s, &r.id, &doc.reviewers)?;
    put_findings(db, s, &r.id, &doc.findings)?;
    put_replies(db, s, &r.id, &doc.replies)
}

fn put_reviewers(
    db: &Connection,
    s: &str,
    rid: &Id,
    rows: &[ReviewReviewerRecord],
) -> Result<(), EngineError> {
    let keep: std::collections::BTreeSet<(u32, &str)> = rows
        .iter()
        .map(|rr| (rr.round_number, rr.reviewer_id.as_str()))
        .collect();
    let existing = all(db, "SELECT round_number, reviewer_id FROM review_reviewers WHERE organization_id=?1 AND review_id=?2", vec![txt(s), txt(rid.as_str())], |row| Ok((u32_c(row, 0)?, row.get::<_, String>(1)?)))?;
    for (rn, rev_id) in existing {
        if !keep.contains(&(rn, rev_id.as_str())) {
            exec(db, "DELETE FROM review_reviewers WHERE organization_id=?1 AND review_id=?2 AND round_number=?3 AND reviewer_id=?4", params![s, rid.as_str(), i64::from(rn), rev_id])?;
        }
    }
    for rr in rows {
        let fids: Vec<&str> = rr.finding_ids.iter().map(Id::as_str).collect();
        exec(db, "INSERT INTO review_reviewers (organization_id, review_id, round_number, reviewer_id, routing, team_id, session_mode, assigned_by, assigned_at, removed, verdict, summary_markdown, finding_ids_json, attachment_ids_json, verdict_at, runtime_session_id) \
                  VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16) \
                  ON CONFLICT (organization_id, review_id, round_number, reviewer_id) DO UPDATE SET routing=excluded.routing, team_id=excluded.team_id, session_mode=excluded.session_mode, assigned_by=excluded.assigned_by, assigned_at=excluded.assigned_at, removed=excluded.removed, verdict=excluded.verdict, summary_markdown=excluded.summary_markdown, finding_ids_json=excluded.finding_ids_json, attachment_ids_json=excluded.attachment_ids_json, verdict_at=excluded.verdict_at, runtime_session_id=excluded.runtime_session_id",
            params![s, rr.review_id.as_str(), i64::from(rr.round_number), rr.reviewer_id.as_str(), rr.routing.as_str(), rr.team_id.as_ref().map(Id::as_str), rr.session_mode.as_str(), rr.assigned_by.as_str(), &rr.assigned_at, i64::from(rr.removed), rr.verdict.map(|v| v.as_str()), &rr.summary_markdown, js_str(&fids), js_str(&rr.attachment_ids), rr.verdict_at.as_deref(), rr.runtime_session_id.as_deref()])?;
    }
    Ok(())
}

fn put_findings(
    db: &Connection,
    s: &str,
    rid: &Id,
    rows: &[ReviewFindingRecord],
) -> Result<(), EngineError> {
    if rows.is_empty() {
        return Ok(());
    }
    for f in rows {
        if one(
            db,
            "SELECT review_id FROM review_findings WHERE organization_id=?1 AND id=?2",
            vec![txt(s), txt(f.id.as_str())],
            |row| row.get::<_, String>(0),
        )?
        .is_some_and(|ex| ex != rid.as_str())
        {
            return Err(EngineError::new(format!(
                "finding {} belongs to another review",
                f.id
            )));
        }
        exec(db, "INSERT INTO review_findings (organization_id, id, review_id, number, round_number, author_id, severity, status, anchor_kind, anchor_json, body_markdown, attachment_ids_json, resolved_by, resolved_at, resolved_in_round, revision, created_at, updated_at) \
                  VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18) \
                  ON CONFLICT (organization_id, id) DO UPDATE SET severity=excluded.severity, status=excluded.status, anchor_kind=excluded.anchor_kind, anchor_json=excluded.anchor_json, body_markdown=excluded.body_markdown, attachment_ids_json=excluded.attachment_ids_json, resolved_by=excluded.resolved_by, resolved_at=excluded.resolved_at, resolved_in_round=excluded.resolved_in_round, revision=excluded.revision, updated_at=excluded.updated_at",
            params![s, f.id.as_str(), f.review_id.as_str(), i64_u64(f.number)?, i64::from(f.round_number), f.author_id.as_str(), f.severity.as_str(), f.status.as_str(), f.anchor.kind.as_str(), js_str(&f.anchor), &f.body_markdown, js_str(&f.attachment_ids), f.resolved_by.as_ref().map(Id::as_str), f.resolved_at.as_deref(), f.resolved_in_round.map(i64::from), i64_u64(f.revision)?, &f.created_at, &f.updated_at])?;
    }
    Ok(())
}

fn put_replies(
    db: &Connection,
    s: &str,
    rid: &Id,
    rows: &[ReviewFindingReplyRecord],
) -> Result<(), EngineError> {
    let sql = format!(
        "SELECT {REPLY_COLS} FROM review_finding_replies WHERE organization_id=?1 AND review_id=?2"
    );
    let existing: std::collections::BTreeMap<Id, ReviewFindingReplyRecord> =
        all(db, &sql, vec![txt(s), txt(rid.as_str())], reply_row)?
            .into_iter()
            .map(|rep| (rep.id.clone(), rep))
            .collect();
    let next_ids: std::collections::BTreeSet<&Id> = rows.iter().map(|rep| &rep.id).collect();
    if existing.keys().any(|id| !next_ids.contains(id)) {
        return Err(EngineError::new("immutable review_finding_reply omitted"));
    }
    for rep in rows {
        if let Some(ex) = existing.get(&rep.id) {
            if ex != rep {
                return Err(EngineError::new(format!(
                    "immutable reply {} modified",
                    rep.id
                )));
            }
            continue;
        }
        exec(db, "INSERT INTO review_finding_replies (organization_id, id, finding_id, review_id, round_number, author_id, body_markdown, set_status, attachment_ids_json, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![s, rep.id.as_str(), rep.finding_id.as_str(), rep.review_id.as_str(), i64::from(rep.round_number), rep.author_id.as_str(), &rep.body_markdown, rep.set_status.map(|st| st.as_str()), js_str(&rep.attachment_ids), &rep.created_at])?;
    }
    Ok(())
}

impl ReviewsWrite for SqliteTx {
    fn put_review_document(&mut self, doc: &ReviewDocument) -> Result<(), EngineError> {
        let (db, s) = self.view()?;
        put_doc(db, s, doc)
    }
    fn put_review_round(&mut self, r: &ReviewRoundRecord) -> Result<(), EngineError> {
        let (db, s) = self.view()?;
        let sql = format!("SELECT {ROUND_COLS} FROM review_rounds WHERE organization_id=?1 AND review_id=?2 AND round_number=?3");
        if let Some(ex) = one(
            db,
            &sql,
            vec![
                txt(s),
                txt(r.review_id.as_str()),
                Value::Integer(i64::from(r.round_number)),
            ],
            round_row,
        )? {
            if ex == *r {
                return Ok(());
            }
            return Err(EngineError::new(format!(
                "immutable review round {} modified",
                r.round_number
            )));
        }
        exec(db, "INSERT INTO review_rounds (organization_id, review_id, round_number, base_oid, head_oid, session_mode, submitted_by, submitted_at, note_markdown) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![s, r.review_id.as_str(), i64::from(r.round_number), r.base_oid.as_deref(), r.head_oid.as_deref(), r.session_mode.as_str(), r.submitted_by.as_str(), &r.submitted_at, &r.note_markdown])
    }
    fn put_review_event(&mut self, r: &ReviewEventRecord) -> Result<(), EngineError> {
        let (db, s) = self.view()?;
        if let Some(sha) = one(db, "SELECT payload_sha256 FROM review_events WHERE organization_id=?1 AND review_id=?2 AND revision=?3", vec![txt(s), txt(r.review_id.as_str()), whole(r.revision)?], |row| row.get::<_, String>(0))? {
            if sha == r.payload_sha256 { return Ok(()); }
            return Err(EngineError::new(format!("immutable review event conflict at rev {}", r.revision)));
        }
        exec(db, "INSERT INTO review_events (organization_id, review_id, revision, event_type, actor_id, operation_id, payload_json, payload_sha256, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![s, r.review_id.as_str(), i64_u64(r.revision)?, &r.event_type, r.actor_id.as_str(), r.operation_id.as_deref(), &r.payload_json, &r.payload_sha256, &r.created_at])
    }
    fn put_office_review_counter(
        &mut self,
        r: &OfficeReviewCounterRecord,
    ) -> Result<(), EngineError> {
        let (db, s) = self.view()?;
        exec(db, "INSERT INTO office_review_counters (organization_id, office_code, office_id, next_number) VALUES (?1,?2,?3,?4) ON CONFLICT (organization_id, office_code) DO UPDATE SET office_id=excluded.office_id, next_number=excluded.next_number",
            params![s, &r.office_code, r.office_id.as_str(), i64_u64(r.next_number)?])
    }
    fn remove_office_review_counter(&mut self, code: &str) -> Result<(), EngineError> {
        let (db, s) = self.view()?;
        exec(
            db,
            "DELETE FROM office_review_counters WHERE organization_id=?1 AND office_code=?2",
            params![s, code],
        )
    }
}
