//! Bug calls (§4.5, state machine §1.10): `bug.report`, `bug.update`, `bug.triage`,
//! `bug.assign`, `bug.mark_fixed`, `bug.verify`, `bug.close`, `bug.reopen`; and the two derived
//! people of a bug (§1.4): its suggested triager and verifier.
//!
//! A bug is `new → triaged → in_progress → fixed → closed`. Its move to `in_progress` and
//! `fixed` normally follows its fix tasks (see [`crate::fixes`]); `bug.mark_fixed` is the hand
//! route. A `new` bug whose suggested triager is the owner waits on the owner (§6): the calls
//! that can change either side keep that span in line.

use org_proto::org::{AgentStatus, MemberKind};
use org_proto::work::{
    BugAssignParams, BugCloseParams, BugMarkFixedParams, BugReopenParams, BugReportParams,
    BugReportedFrom, BugTriageParams, BugUpdateParams, BugVerifyParams, DoneAnyway, FieldChange,
    ItemKind, ItemStatus, LinkKind, LinkTarget, Priority, Resolution, Severity, VerifyOutcome,
    WorkEventKind, WORK_ENVIRONMENT_MAX_CHARS, WORK_REASON_MAX_CHARS, WORK_TEXT_MAX_CHARS,
    WORK_TITLE_MAX_CHARS,
};
use org_proto::ErrorCode;
use serde_json::Value;

use crate::aspects::Aspect;
use crate::common::{
    distinct, expect_kind, known_priority, load, new_steps, not_deleted, require_ulid, unused,
};
use crate::engine::{Ctx, Draft, Origin, Plan};
use crate::fail::{invalid, not_found, refuse, Fail};
use crate::fixes;
use crate::items::DOC_KEY_MAX_CHARS;
use crate::model::{BugFields, StoredItem, StoredLink};
use crate::ports::{OrgView, StoreError};
use crate::tasks::{priority_word, status_word};
use crate::text::{multi_line, one_line};
use crate::waiting;

// ---- the derived people ------------------------------------------------------------------

/// Whether `member_id` can still be named in a suggestion: a member who is there and, for an
/// agent, not offboarded.
fn usable(org: &dyn OrgView, member_id: &str) -> Result<bool, StoreError> {
    Ok(org.member(member_id)?.is_some_and(|m| {
        m.kind != MemberKind::Agent
            || m.agent
                .as_ref()
                .is_some_and(|a| a.status != AgentStatus::Offboarded)
    }))
}

/// The bug's suggested triager (§1.4): its area's lead, else the area's team's lead, else the
/// owner. A deleted area stands for its replacement (its open bugs are moving there).
pub(crate) fn suggested_triager(
    org: &dyn OrgView,
    area_id: Option<&str>,
) -> Result<String, StoreError> {
    let mut area = match area_id {
        Some(id) => org.area(id)?,
        None => None,
    };
    // A chain of replacements ends at a live area; a loop or a long chain is a broken
    // organization, answered with the owner rather than followed.
    for _ in 0..8 {
        match area.as_ref().filter(|a| a.deleted) {
            Some(a) => match &a.replaced_by {
                Some(next) => area = org.area(next)?,
                None => break,
            },
            None => break,
        }
    }
    if let Some(area) = area.filter(|a| !a.deleted) {
        if let Some(lead) = &area.lead_member_id {
            if usable(org, lead)? {
                return Ok(lead.clone());
            }
        }
        if let Some(team) = &area.team_id {
            if let Some(lead) = org.team_lead(team)? {
                if usable(org, &lead)? {
                    return Ok(lead);
                }
            }
        }
    }
    org.owner_id()
}

/// The bug's suggested verifier (§1.4): its reporter if still a member, else the suggested
/// triager.
pub(crate) fn suggested_verifier(
    org: &dyn OrgView,
    bug: &StoredItem,
) -> Result<String, StoreError> {
    if usable(org, &bug.created_by)? {
        return Ok(bug.created_by.clone());
    }
    suggested_triager(org, bug.bug.area_id.as_deref())
}

/// Brings the bug's triage span in line with its state: open while it is `new` and its
/// suggested triager is the owner. `origin` is the event that may have changed either side.
pub(crate) fn sync_triage(
    org: &dyn OrgView,
    draft: &mut Draft,
    origin: Origin,
) -> Result<(), Fail> {
    let owner = org.owner_id()?;
    let wanted = draft.item.status == ItemStatus::New
        && suggested_triager(org, draft.item.bug.area_id.as_deref())? == owner;
    waiting::triage(draft, &owner, wanted, origin);
    Ok(())
}

// ---- shared checks -----------------------------------------------------------------------

fn load_bug(ctx: &mut Ctx, id: &str) -> Result<StoredItem, Fail> {
    let item = load(ctx, id)?;
    expect_kind(&item, ItemKind::Bug)?;
    Ok(item)
}

/// An area the caller names must exist (`not_found`).
fn area_exists(ctx: &Ctx, area_id: &str) -> Result<org_proto::org::Area, Fail> {
    ctx.org
        .area(area_id)?
        .ok_or_else(|| not_found("no such area"))
}

/// ... and be a live area of the bug's project.
fn area_in_project(area: &org_proto::org::Area, project_id: &str) -> Result<(), Fail> {
    if area.project_id != project_id {
        return Err(invalid("the area belongs to another project"));
    }
    if area.deleted {
        return Err(invalid("the area is deleted"));
    }
    Ok(())
}

fn known_severity(severity: Severity) -> Result<Severity, Fail> {
    match severity {
        Severity::Critical | Severity::Major | Severity::Minor | Severity::Trivial => Ok(severity),
        _ => Err(invalid(
            "`severity` must be critical, major, minor or trivial",
        )),
    }
}

pub(crate) fn severity_word(s: Severity) -> &'static str {
    match s {
        Severity::Critical => "critical",
        Severity::Major => "major",
        Severity::Minor => "minor",
        Severity::Trivial => "trivial",
        _ => "unknown",
    }
}

/// A bug a task may fix (`fixes`, C8): a bug of the task's project that isn't deleted.
pub(crate) fn check_fixable(project_id: &str, bug: &StoredItem) -> Result<(), Fail> {
    if bug.kind != ItemKind::Bug {
        return Err(invalid("a task fixes a bug, and the item isn't one"));
    }
    if bug.project_id.as_deref() != Some(project_id) {
        return Err(invalid("a task fixes only bugs of its own project"));
    }
    not_deleted(bug)
}

fn wrong_state(what: &str, status: ItemStatus) -> Fail {
    invalid(format!(
        "wrong state: a bug that is {} can't be {what}",
        status_word(status)
    ))
}

fn optional<T>(
    value: Option<&str>,
    check: impl Fn(&str) -> Result<T, Fail>,
) -> Result<Option<T>, Fail> {
    value.map(check).transpose()
}

// ---- bug.report --------------------------------------------------------------------------

/// `bug.report`: a bug in `new`, numbered `B-n` in its project, unassigned, priority `normal`.
pub(crate) fn report(ctx: &mut Ctx, p: &BugReportParams) -> Result<Plan, Fail> {
    let project = ctx
        .org
        .project(&p.project_id)?
        .ok_or_else(|| not_found("no such project"))?;
    let area = match &p.area_id {
        Some(id) => Some(area_exists(ctx, id)?),
        None => None,
    };
    let from_target = match p.from.as_ref().map(|f| &f.to) {
        Some(LinkTarget::Item { item_id }) => Some(
            ctx.txn
                .get(item_id)?
                .ok_or_else(|| not_found("no such work item to report from"))?,
        ),
        Some(LinkTarget::Doc { project_id, .. }) => {
            ctx.org
                .project(project_id)?
                .ok_or_else(|| not_found("no such project"))?;
            None
        }
        _ => None,
    };

    require_ulid("item_id", &p.item_id)?;
    let title = one_line("title", &p.title, WORK_TITLE_MAX_CHARS)?;
    let what_happened = multi_line("what_happened", &p.what_happened, WORK_TEXT_MAX_CHARS)?;
    let expected = optional(p.expected.as_deref(), |t| {
        multi_line("expected", t, WORK_TEXT_MAX_CHARS)
    })?;
    let steps = optional(p.steps.as_deref(), |t| {
        multi_line("steps", t, WORK_TEXT_MAX_CHARS)
    })?;
    let environment = optional(p.environment.as_deref(), |t| {
        multi_line("environment", t, WORK_ENVIRONMENT_MAX_CHARS)
    })?;
    let severity = known_severity(p.severity)?;
    if p.affects_feature_id.is_some() {
        return Err(invalid(
            "`affects_feature_id` comes with features (a later phase)",
        ));
    }
    let checklist = new_steps(&p.checklist, 0)?;
    let link_id = p.from.as_ref().map(|f| f.link_id.as_str());
    distinct(
        std::iter::once(p.item_id.as_str())
            .chain(checklist.iter().map(|s| s.id.as_str()))
            .chain(link_id),
    )?;
    let reported_from = match &p.from {
        Some(from) => Some(reported_from(ctx.caller_id(), from, from_target.as_ref())?),
        None => None,
    };
    let area_id = match area {
        Some(area) => {
            area_in_project(&area, &project.id)?;
            area.id
        }
        None => ctx
            .org
            .general_area(&project.id)?
            .ok_or_else(|| invalid("the project has no General area"))?,
    };
    if project.archived {
        return Err(invalid("the project is archived: it takes no new items"));
    }
    let mut ids = vec![p.item_id.as_str()];
    ids.extend(checklist.iter().map(|s| s.id.as_str()));
    ids.extend(link_id);
    unused(ctx, &ids)?;

    let item = StoredItem {
        kind: ItemKind::Bug,
        status: ItemStatus::New,
        priority: Some(Priority::Normal),
        bug: BugFields {
            area_id: Some(area_id),
            severity: Some(severity),
            what_happened,
            expected,
            steps,
            environment,
            ..BugFields::default()
        },
        checklist,
        links: reported_from.into_iter().collect(),
        ..crate::tasks::new_task(ctx, &p.item_id, &project.id, title, Priority::Normal)
    };
    let mut draft = ctx.draft_new(item);
    let at = draft.at_ms();
    for l in &mut draft.item.links {
        l.created_at_ms = at;
    }
    let cause = draft.record(WorkEventKind::Created, Origin::Caller);
    sync_triage(ctx.org, &mut draft, Origin::System(cause))?;
    Ok(Plan::of(vec![draft]))
}

/// The `reported_from` link of a report: to a task, or to a doc by its key.
fn reported_from(
    caller: &str,
    from: &BugReportedFrom,
    target: Option<&StoredItem>,
) -> Result<StoredLink, Fail> {
    require_ulid("link_id", &from.link_id)?;
    let to = match (&from.to, target) {
        (LinkTarget::Item { item_id }, Some(t)) => {
            if t.kind != ItemKind::Task {
                return Err(invalid("a bug is reported from a task or a doc"));
            }
            if t.deleted {
                return Err(invalid("the task to report from is deleted"));
            }
            LinkTarget::Item {
                item_id: item_id.clone(),
            }
        }
        (
            LinkTarget::Doc {
                project_id,
                doc_key,
            },
            _,
        ) => LinkTarget::Doc {
            project_id: project_id.clone(),
            doc_key: one_line("doc_key", doc_key, DOC_KEY_MAX_CHARS)?,
        },
        _ => return Err(invalid("unknown link target")),
    };
    Ok(StoredLink {
        id: from.link_id.clone(),
        kind: LinkKind::ReportedFrom,
        to,
        created_by: caller.to_owned(),
        created_at_ms: 0,
        removed: false,
    })
}

// ---- bug.update --------------------------------------------------------------------------

/// `bug.update`: only the fields given change; a `clear_` flag removes an optional field. A
/// new area on a `new` bug is a reroute (TRI-3): the suggested triager changes, nothing else.
pub(crate) fn update(ctx: &mut Ctx, p: &BugUpdateParams) -> Result<Plan, Fail> {
    let item = load_bug(ctx, &p.item_id)?;
    let area = match &p.area_id {
        Some(id) => Some(area_exists(ctx, id)?),
        None => None,
    };

    let given = p.title.is_some()
        || p.what_happened.is_some()
        || p.expected.is_some()
        || p.clear_expected
        || p.steps.is_some()
        || p.clear_steps
        || p.environment.is_some()
        || p.clear_environment
        || p.severity.is_some()
        || p.priority.is_some()
        || p.area_id.is_some()
        || p.affects_feature_id.is_some()
        || p.clear_affects_feature;
    if !given {
        return Err(invalid("nothing to change"));
    }
    for (value, clear, name) in [
        (p.expected.is_some(), p.clear_expected, "expected"),
        (p.steps.is_some(), p.clear_steps, "steps"),
        (p.environment.is_some(), p.clear_environment, "environment"),
    ] {
        if value && clear {
            return Err(invalid(format!("`{name}` is given with its `clear_` flag")));
        }
    }
    if p.affects_feature_id.is_some() || p.clear_affects_feature {
        return Err(invalid(
            "`affects_feature_id` comes with features (a later phase)",
        ));
    }
    let title = optional(p.title.as_deref(), |t| {
        one_line("title", t, WORK_TITLE_MAX_CHARS)
    })?;
    let what_happened = optional(p.what_happened.as_deref(), |t| {
        multi_line("what_happened", t, WORK_TEXT_MAX_CHARS)
    })?;
    let expected = optional(p.expected.as_deref(), |t| {
        multi_line("expected", t, WORK_TEXT_MAX_CHARS)
    })?;
    let steps = optional(p.steps.as_deref(), |t| {
        multi_line("steps", t, WORK_TEXT_MAX_CHARS)
    })?;
    let environment = optional(p.environment.as_deref(), |t| {
        multi_line("environment", t, WORK_ENVIRONMENT_MAX_CHARS)
    })?;
    let severity = p.severity.map(known_severity).transpose()?;
    let priority = known_priority(p.priority)?;
    if let (Some(area), Some(project)) = (&area, &item.project_id) {
        area_in_project(area, project)?;
    }

    // The fields this call writes, in a fixed order, with their new values.
    let mut changes: Vec<(&str, Option<String>)> = vec![];
    if let Some(t) = title {
        changes.push(("title", Some(t)));
    }
    if let Some(t) = what_happened {
        changes.push(("what_happened", Some(t)));
    }
    if expected.is_some() || p.clear_expected {
        changes.push(("expected", expected));
    }
    if steps.is_some() || p.clear_steps {
        changes.push(("steps", steps));
    }
    if environment.is_some() || p.clear_environment {
        changes.push(("environment", environment));
    }
    if let Some(s) = severity {
        changes.push(("severity", Some(severity_word(s).to_owned())));
    }
    if let Some(pr) = priority {
        changes.push(("priority", Some(priority_word(pr).to_owned())));
    }
    if let Some(a) = &area {
        changes.push(("area_id", Some(a.id.clone())));
    }
    let names: Vec<&str> = changes.iter().map(|(n, _)| *n).collect();
    ctx.check_expected(
        &item,
        p.expected_version,
        |a| matches!(a, Aspect::Field(f) if names.contains(&f.as_str())),
    )?;
    not_deleted(&item)?;
    if area.is_some() && item.status == ItemStatus::Closed {
        return Err(invalid("a closed bug keeps its area"));
    }

    let mut draft = ctx.draft(item);
    let mut fields = vec![];
    for (name, value) in changes {
        let it = &mut draft.item;
        let from = match name {
            "title" => Some(std::mem::replace(
                &mut it.title,
                value.clone().unwrap_or_default(),
            )),
            "what_happened" => Some(std::mem::replace(
                &mut it.bug.what_happened,
                value.clone().unwrap_or_default(),
            )),
            "expected" => std::mem::replace(&mut it.bug.expected, value.clone()),
            "steps" => std::mem::replace(&mut it.bug.steps, value.clone()),
            "environment" => std::mem::replace(&mut it.bug.environment, value.clone()),
            "severity" => {
                let before = it.bug.severity.map(|s| severity_word(s).to_owned());
                it.bug.severity = severity;
                before
            }
            "priority" => {
                let before = it.priority.map(|p| priority_word(p).to_owned());
                it.priority = priority;
                before
            }
            "area_id" => std::mem::replace(&mut it.bug.area_id, value.clone()),
            _ => unreachable!("only the fields above are written"),
        };
        fields.push(FieldChange {
            field: name.to_owned(),
            from: from.map(Value::String),
            to: value.map(Value::String),
        });
    }
    let cause = draft.record(WorkEventKind::FieldsChanged { fields }, Origin::Caller);
    if area.is_some() {
        sync_triage(ctx.org, &mut draft, Origin::System(cause))?;
    }
    Ok(Plan::of(vec![draft]))
}

// ---- bug.triage --------------------------------------------------------------------------

/// `bug.triage`: `new → triaged`. Confirms the area and severity, sets the priority and,
/// when given, the assignee.
pub(crate) fn triage(ctx: &mut Ctx, p: &BugTriageParams) -> Result<Plan, Fail> {
    let item = load_bug(ctx, &p.item_id)?;
    let area = area_exists(ctx, &p.area_id)?;
    let severity = known_severity(p.severity)?;
    let priority =
        known_priority(Some(p.priority))?.ok_or_else(|| invalid("`priority` is required"))?;
    if let Some(project) = &item.project_id {
        area_in_project(&area, project)?;
    }
    if let Some(assignee) = &p.assignee_id {
        ctx.check_recipient(assignee)?;
    }
    let assigns = p.assignee_id.is_some();
    ctx.check_expected(&item, p.expected_version, |a| match a {
        Aspect::Status => true,
        Aspect::Field(f) => ["area_id", "severity", "priority"].contains(&f.as_str()),
        Aspect::Assignee => assigns,
        _ => false,
    })?;
    not_deleted(&item)?;
    if item.status != ItemStatus::New {
        return Err(wrong_state("triaged", item.status));
    }

    let mut draft = ctx.draft(item);
    {
        let it = &mut draft.item;
        it.status = ItemStatus::Triaged;
        it.bug.area_id = Some(area.id.clone());
        it.bug.severity = Some(severity);
        it.bug.severity_confirmed = true;
        it.priority = Some(priority);
        if assigns {
            it.assignee_id = p.assignee_id.clone();
        }
    }
    let cause = draft.record(
        WorkEventKind::Triaged {
            area_id: area.id,
            severity,
            priority,
            assignee_id: p.assignee_id.clone(),
        },
        Origin::Caller,
    );
    sync_triage(ctx.org, &mut draft, Origin::System(cause.clone()))?;
    fixes::after_triage(ctx, &mut draft, cause)?;
    Ok(Plan::of(vec![draft]))
}

// ---- bug.assign --------------------------------------------------------------------------

/// `bug.assign`: `assignee_id` absent unassigns. Not on a closed bug.
pub(crate) fn assign(ctx: &mut Ctx, p: &BugAssignParams) -> Result<Plan, Fail> {
    let item = load_bug(ctx, &p.item_id)?;
    if let Some(assignee) = &p.assignee_id {
        ctx.check_recipient(assignee)?;
    }
    ctx.check_expected(&item, p.expected_version, |a| *a == Aspect::Assignee)?;
    not_deleted(&item)?;
    if item.status == ItemStatus::Closed {
        return Err(wrong_state("assigned", item.status));
    }
    let mut draft = ctx.draft(item);
    let from = std::mem::replace(&mut draft.item.assignee_id, p.assignee_id.clone());
    draft.record(
        WorkEventKind::Assigned {
            from,
            to: p.assignee_id.clone(),
        },
        Origin::Caller,
    );
    Ok(Plan::of(vec![draft]))
}

// ---- bug.mark_fixed ----------------------------------------------------------------------

/// `bug.mark_fixed`: `triaged` or `in_progress → fixed` by hand, with "what fixed it" and the
/// step guard of Done.
pub(crate) fn mark_fixed(ctx: &mut Ctx, p: &BugMarkFixedParams) -> Result<Plan, Fail> {
    let item = load_bug(ctx, &p.item_id)?;
    let note = multi_line("note", &p.note, WORK_REASON_MAX_CHARS)?;
    let anyway_reason = optional(p.anyway_reason.as_deref(), |r| {
        one_line("anyway_reason", r, WORK_REASON_MAX_CHARS)
    })?;
    ctx.check_expected(&item, p.expected_version, |a| *a == Aspect::Status)?;
    not_deleted(&item)?;
    let from = item.status;
    if !matches!(from, ItemStatus::Triaged | ItemStatus::InProgress) {
        return Err(wrong_state("marked fixed", from));
    }
    let mut anyway = None;
    let left = item.required_left();
    if !left.is_empty() {
        let Some(reason) = anyway_reason else {
            let names: Vec<String> = left.iter().map(|s| format!("\"{}\"", s.text)).collect();
            let n = left.len();
            return Err(refuse(
                ErrorCode::ChecklistIncomplete,
                format!(
                    "{n} required step{} {} done: {}",
                    if n == 1 { "" } else { "s" },
                    if n == 1 { "isn't" } else { "aren't" },
                    names.join(", ")
                ),
            ));
        };
        anyway = Some(DoneAnyway {
            reason,
            steps: left.iter().map(|s| s.id.clone()).collect(),
        });
    }

    let mut draft = ctx.draft(item);
    draft.item.status = ItemStatus::Fixed;
    draft.item.bug.closed_reason = Some(note.clone());
    draft.item.bug.done_anyway = anyway.clone();
    draft.record(
        WorkEventKind::StatusChanged {
            from,
            to: ItemStatus::Fixed,
            resolution: None,
            reason: Some(note),
            anyway,
        },
        Origin::Caller,
    );
    Ok(Plan::of(vec![draft]))
}

// ---- bug.verify --------------------------------------------------------------------------

/// `bug.verify`: on a `fixed` bug. `fixed` closes it as verified; `still_happens` sends it back
/// to `triaged` (assignee and fix links kept) and counts a reopening.
pub(crate) fn verify(ctx: &mut Ctx, p: &BugVerifyParams) -> Result<Plan, Fail> {
    let item = load_bug(ctx, &p.item_id)?;
    let outcome = match p.outcome {
        VerifyOutcome::Fixed | VerifyOutcome::StillHappens => p.outcome,
        _ => return Err(invalid("`outcome` must be fixed or still_happens")),
    };
    let note = optional(p.note.as_deref(), |n| {
        multi_line("note", n, WORK_REASON_MAX_CHARS)
    })?;
    if outcome == VerifyOutcome::StillHappens && note.is_none() {
        return Err(invalid("saying it still happens needs a note"));
    }
    ctx.check_expected(&item, p.expected_version, |a| *a == Aspect::Status)?;
    not_deleted(&item)?;
    if item.status != ItemStatus::Fixed {
        return Err(wrong_state("verified", item.status));
    }

    let by_reporter = ctx.caller_id() == item.created_by;
    let mut draft = ctx.draft(item);
    {
        let it = &mut draft.item;
        it.bug.done_anyway = None;
        match outcome {
            VerifyOutcome::Fixed => {
                it.status = ItemStatus::Closed;
                it.resolution = Some(Resolution::Verified);
            }
            _ => {
                it.status = ItemStatus::Triaged;
                it.bug.reopen_count += 1;
                it.bug.closed_reason = None;
            }
        }
    }
    let cause = draft.record(
        WorkEventKind::Verified {
            outcome,
            by_reporter,
            note,
        },
        Origin::Caller,
    );
    if outcome == VerifyOutcome::Fixed {
        waiting::round_closed(&mut draft, &cause);
    }
    Ok(Plan::of(vec![draft]))
}

// ---- bug.close ---------------------------------------------------------------------------

/// `bug.close`: as a duplicate of another bug of the project, or as won't fix / can't
/// reproduce with a reason. A duplicate's reporter starts watching the original.
pub(crate) fn close(ctx: &mut Ctx, p: &BugCloseParams) -> Result<Plan, Fail> {
    let item = load_bug(ctx, &p.item_id)?;
    let original = match &p.duplicate_of {
        Some(id) => Some(
            ctx.txn
                .get(id)?
                .ok_or_else(|| not_found("no such bug to be a duplicate of"))?,
        ),
        None => None,
    };

    let (resolution, reason) = match (&original, p.resolution, p.reason.as_deref()) {
        (Some(_), None | Some(Resolution::Duplicate), None) => (Resolution::Duplicate, None),
        (Some(_), _, _) => {
            return Err(invalid(
                "a duplicate is closed with `duplicate_of` alone, without a reason",
            ))
        }
        (None, Some(r @ (Resolution::WontFix | Resolution::CantReproduce)), Some(reason)) => {
            (r, Some(one_line("reason", reason, WORK_REASON_MAX_CHARS)?))
        }
        (None, Some(Resolution::WontFix | Resolution::CantReproduce), None) => {
            return Err(invalid("closing a bug needs a reason"))
        }
        (None, _, _) => {
            return Err(invalid(
                "close a bug with `duplicate_of`, or with `resolution` wont_fix or cant_reproduce and a `reason`",
            ))
        }
    };
    if let Some(original) = &original {
        if original.kind != ItemKind::Bug {
            return Err(invalid("the original is not a bug"));
        }
        if original.id == item.id {
            return Err(invalid("a bug can't be a duplicate of itself"));
        }
        if original.project_id != item.project_id {
            return Err(invalid("the original is in another project"));
        }
        if original.deleted {
            return Err(invalid("the original is deleted"));
        }
        if original.status == ItemStatus::Closed
            && original.resolution == Some(Resolution::Duplicate)
        {
            return Err(invalid(
                "the original was itself closed as a duplicate: use its original",
            ));
        }
    }
    ctx.check_expected(&item, p.expected_version, |a| *a == Aspect::Status)?;
    not_deleted(&item)?;
    let from = item.status;
    if from == ItemStatus::Closed {
        return Err(wrong_state("closed", from));
    }

    let reporter = item.created_by.clone();
    let mut draft = ctx.draft(item);
    {
        let it = &mut draft.item;
        it.status = ItemStatus::Closed;
        it.resolution = Some(resolution);
        it.bug.closed_reason = reason.clone();
        it.bug.duplicate_of = original.as_ref().map(|o| o.id.clone());
        it.bug.done_anyway = None;
    }
    let cause = draft.record(
        WorkEventKind::StatusChanged {
            from,
            to: ItemStatus::Closed,
            resolution: Some(resolution),
            reason: original.as_ref().map(|o| o.id.clone()).or(reason),
            anyway: None,
        },
        Origin::Caller,
    );
    waiting::round_closed(&mut draft, &cause);
    let mut drafts = vec![draft];
    if let Some(original) = original.filter(|o| !o.watchers.contains(&reporter)) {
        let mut od = ctx.draft(original);
        od.item.watchers.push(reporter.clone());
        od.record(
            WorkEventKind::Watched {
                member_id: reporter,
                watching: true,
            },
            Origin::Caller,
        );
        drafts.push(od);
    }
    Ok(Plan::of(drafts))
}

// ---- bug.reopen --------------------------------------------------------------------------

/// `bug.reopen`: `closed → triaged`, or `new` for a duplicate (so triage looks again, with
/// `duplicate_of` cleared). Counts a reopening and starts a new round.
pub(crate) fn reopen(ctx: &mut Ctx, p: &BugReopenParams) -> Result<Plan, Fail> {
    let item = load_bug(ctx, &p.item_id)?;
    let note = multi_line("note", &p.note, WORK_REASON_MAX_CHARS)?;
    ctx.check_expected(&item, p.expected_version, |a| *a == Aspect::Status)?;
    not_deleted(&item)?;
    if item.status != ItemStatus::Closed {
        return Err(wrong_state("reopened", item.status));
    }

    let owner_id = ctx.org.owner_id()?;
    let was_duplicate = item.resolution == Some(Resolution::Duplicate);
    let mut draft = ctx.draft(item);
    {
        let it = &mut draft.item;
        it.status = if was_duplicate {
            ItemStatus::New
        } else {
            ItemStatus::Triaged
        };
        it.resolution = None;
        it.bug.closed_reason = None;
        it.bug.duplicate_of = None;
        it.bug.done_anyway = None;
        it.bug.reopen_count += 1;
    }
    let cause = draft.record(WorkEventKind::Reopened { note: Some(note) }, Origin::Caller);
    waiting::round_reopened(&mut draft, &owner_id, &cause);
    sync_triage(ctx.org, &mut draft, Origin::System(cause))?;
    Ok(Plan::of(vec![draft]))
}
