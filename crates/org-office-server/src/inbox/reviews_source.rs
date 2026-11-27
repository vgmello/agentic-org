//! Read-only Inbox source for human-actionable universal reviews (`TASK-92.2`, `ADR-0026`, `ADR-0031`).
//!
//! Surfaces at most one deterministic card per `(review_id, current_round)` for the authenticated
//! active human caller, strictly within `may_see_project` boundaries and never reading private
//! task notes or raw secrets:
//! 1. Assigned open reviews needing the caller's verdict on the current round;
//! 2. `changes_requested` feedback for human authors or current-round submitters;
//! 3. Claimable team/role pool reviews where the caller is eligible (and never barred by
//!    `independent = true` author/submitter rules);
//! 4. Advisory `>= 5`-round escalations for leads, owner, and human stakeholders.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use org_office::organization_family::records::{EmployeeKind, EmployeeStatus};
use org_office::reviews::features::ReviewInspectSections;
use org_office::Office;
use org_proto::inbox::{
    Action, InboxClears, InboxCounts, InboxItem, InboxKind, InboxState, Open, OpenTarget,
};
use org_proto::projects::ProjectRecord;
use org_proto::reviews::{
    ReviewInspectParams, ReviewInspectResult, ReviewListParams, ReviewReviewerView,
    ReviewRoundView, ReviewRoute, ReviewStatus, ReviewVerdict, ReviewView, RoutingMode,
    REVIEW_LIST_MAX_LIMIT,
};
use org_proto::{ApiError, Call, ErrorCode, Member, MemberKind, OrgSnapshot};
use org_redact::sanitize::sanitize_single_line_display;
use serde_json::{json, Map};

use super::{Query, Source, SourceFailure, SourceRead};

const ESCALATION_ROUND_THRESHOLD: u32 = 5;
const MAX_REVIEW_LIST_PAGES: u64 = 50;
const VERDICT_SNIPPET_MAX_CHARS: usize = 200;

type SnapshotFn = Box<dyn Fn() -> Result<OrgSnapshot, ApiError> + Send + Sync>;

/// Human Inbox source projecting actionable reviews for the authenticated human caller.
pub(crate) struct ReviewInboxSource {
    office: Arc<Office>,
    office_id: String,
    snapshot_fn: SnapshotFn,
}

impl ReviewInboxSource {
    pub(crate) fn new(office: Arc<Office>) -> Self {
        let office_id = office.info().office_id.clone();
        let snapshot_office = Arc::clone(&office);
        let snapshot_fn: SnapshotFn = Box::new(move || {
            snapshot_office.org_snapshot().map_err(|error| {
                crate::logged("inbox.reviews", &error);
                ApiError::new(
                    ErrorCode::StorageError,
                    "organization snapshot unavailable for reviews inbox",
                )
            })
        });
        Self {
            office,
            office_id,
            snapshot_fn,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_snapshot_for_test(office: Arc<Office>, snapshot_fn: SnapshotFn) -> Self {
        let office_id = office.info().office_id.clone();
        Self {
            office,
            office_id,
            snapshot_fn,
        }
    }

    /// Reads actionable review Inbox items strictly authorized for `caller_id`.
    /// Only an active human employee is admitted; project-scoped reviews are isolated by
    /// `may_see_project`.
    pub(crate) fn read_for_caller(
        &self,
        caller_id: &str,
        query: &Query<'_>,
    ) -> Result<SourceRead, ApiError> {
        let snapshot = (self.snapshot_fn)()?;
        let caller = ensure_active_human_caller(&self.office, &snapshot, caller_id)?;
        let projects = load_projects_by_id(&self.office)?;
        let ctx = CallerOrgContext::build(&snapshot, caller, &projects);
        let candidates = list_active_reviews(&self.office, caller_id)?;

        let mut all_items = Vec::new();
        let mut seen_review_ids = BTreeSet::new();
        for review in candidates {
            if !seen_review_ids.insert(review.id.clone()) {
                continue;
            }
            if !ctx.may_see_project(review.project_id.as_deref()) {
                continue;
            }
            let Some(inspected) = inspect_review_metadata(&self.office, caller_id, &review.id)?
            else {
                continue;
            };
            if let Some(reason) = classify_review(&ctx, &inspected) {
                all_items.push(build_review_inbox_item(
                    &self.office_id,
                    &ctx,
                    &inspected,
                    reason,
                ));
            }
        }

        let count = all_items.len() as u64;
        let counts = InboxCounts {
            reviews: count,
            total: count,
            ..InboxCounts::default()
        };
        if query.limit == 0
            || query.state != InboxState::Open
            || query
                .kinds
                .is_some_and(|kinds| !kinds.contains(&InboxKind::Review))
        {
            return Ok(SourceRead {
                items: Vec::new(),
                counts,
            });
        }
        let items = filter_and_page_items(&snapshot, &projects, all_items, query);
        Ok(SourceRead { items, counts })
    }
}

impl Source for ReviewInboxSource {
    fn name(&self) -> &'static str {
        "reviews"
    }

    fn read(&self, query: &Query<'_>) -> Result<SourceRead, SourceFailure> {
        let owner_member_id = self.office.effective_owner_member_id();
        self.read_for_caller(&owner_member_id, query)
            .map_err(|_| SourceFailure)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReviewReason {
    AssignedPending,
    ChangesRequested,
    ClaimablePool { pool_team_id: String },
    EscalatedRound,
}

struct CallerOrgContext<'a> {
    caller: &'a Member,
    members_by_id: HashMap<&'a str, &'a Member>,
    team_parents: HashMap<&'a str, Option<&'a str>>,
    team_leads: HashMap<&'a str, Option<&'a str>>,
    projects: &'a HashMap<String, ProjectRecord>,
    caller_team_chain: BTreeSet<&'a str>,
    caller_led_teams: BTreeSet<&'a str>,
}

impl<'a> CallerOrgContext<'a> {
    fn build(
        snapshot: &'a OrgSnapshot,
        caller: &'a Member,
        projects: &'a HashMap<String, ProjectRecord>,
    ) -> Self {
        let members_by_id: HashMap<&str, &Member> = snapshot
            .members
            .iter()
            .map(|m| (m.id.as_str(), m))
            .collect();
        let team_parents: HashMap<&str, Option<&str>> = snapshot
            .teams
            .iter()
            .map(|t| (t.id.as_str(), t.owner_team_id.as_deref()))
            .collect();
        let team_leads: HashMap<&str, Option<&str>> = snapshot
            .teams
            .iter()
            .map(|t| (t.id.as_str(), t.lead_member_id.as_deref()))
            .collect();
        let caller_led_teams: BTreeSet<&str> = snapshot
            .teams
            .iter()
            .filter(|t| t.lead_member_id.as_deref() == Some(caller.id.as_str()))
            .map(|t| t.id.as_str())
            .collect();

        let mut caller_team_chain = BTreeSet::new();
        if let Some(home) = caller.home_team_id.as_deref() {
            collect_ancestors(&team_parents, home, &mut caller_team_chain);
        }
        for &led in &caller_led_teams {
            collect_ancestors(&team_parents, led, &mut caller_team_chain);
        }

        Self {
            caller,
            members_by_id,
            team_parents,
            team_leads,
            projects,
            caller_team_chain,
            caller_led_teams,
        }
    }

    fn may_see_project(&self, project_id: Option<&str>) -> bool {
        let Some(pid) = project_id else {
            return true;
        };
        if self.caller.owner {
            return true;
        }
        let Some(proj) = self.projects.get(pid) else {
            return false;
        };
        self.caller_team_chain.contains(proj.owner_team_id.as_str())
            || proj
                .contributing_team_ids
                .iter()
                .any(|tid| self.caller_team_chain.contains(tid.as_str()))
    }

    fn eligible_for_pool(&self, reviewer: &ReviewReviewerView) -> Option<String> {
        let pool_team_id = reviewer.team_id.as_deref()?;
        if self.caller.home_team_id.as_deref() == Some(pool_team_id)
            || self.caller_led_teams.contains(pool_team_id)
        {
            return Some(pool_team_id.to_owned());
        }
        if let Some(caller_role) = self.caller.role_id.as_deref() {
            let matches_role = self
                .members_by_id
                .get(reviewer.reviewer_id.as_str())
                .and_then(|m| m.role_id.as_deref())
                == Some(caller_role);
            if matches_role {
                return Some(pool_team_id.to_owned());
            }
        }
        None
    }

    fn is_accountable_team_lead(&self, review: &ReviewView) -> bool {
        let mut root_teams: Vec<&str> = Vec::new();
        if let Some(pid) = review.project_id.as_deref() {
            if let Some(proj) = self.projects.get(pid) {
                root_teams.push(proj.owner_team_id.as_str());
                for tid in &proj.contributing_team_ids {
                    root_teams.push(tid.as_str());
                }
            }
        } else if let Some(home) = self
            .members_by_id
            .get(review.created_by.as_str())
            .and_then(|m| m.home_team_id.as_deref())
        {
            root_teams.push(home);
        }

        let mut ancestors = BTreeSet::new();
        for tid in root_teams {
            collect_ancestors(&self.team_parents, tid, &mut ancestors);
        }
        ancestors
            .into_iter()
            .any(|tid| self.team_leads.get(tid).copied().flatten() == Some(self.caller.id.as_str()))
    }

    fn is_manager_of(&self, member_id: &str) -> bool {
        self.members_by_id
            .get(member_id)
            .and_then(|m| m.reports_to_member_id.as_deref())
            == Some(self.caller.id.as_str())
    }

    fn display_name(&self, member_id: &str) -> String {
        let raw = self
            .members_by_id
            .get(member_id)
            .map_or("Member", |m| m.name.as_str());
        let safe = sanitize_display(raw);
        if safe.is_empty() {
            "Member".to_owned()
        } else {
            safe
        }
    }

    fn review_team_id(&self, review: &ReviewView, reason: &ReviewReason) -> Option<String> {
        if let ReviewReason::ClaimablePool { pool_team_id } = reason {
            return Some(pool_team_id.clone());
        }
        if let Some(pid) = review.project_id.as_deref() {
            if let Some(proj) = self.projects.get(pid) {
                return Some(proj.owner_team_id.clone());
            }
        }
        self.members_by_id
            .get(review.created_by.as_str())
            .and_then(|m| m.home_team_id.clone())
    }
}

fn collect_ancestors<'a>(
    parents: &HashMap<&'a str, Option<&'a str>>,
    start: &'a str,
    out: &mut BTreeSet<&'a str>,
) {
    let mut cur = Some(start);
    while let Some(tid) = cur {
        if !out.insert(tid) {
            break;
        }
        cur = parents.get(tid).copied().flatten();
    }
}

fn ensure_active_human_caller<'a>(
    office: &Office,
    snapshot: &'a OrgSnapshot,
    caller_id: &str,
) -> Result<&'a Member, ApiError> {
    let member = snapshot
        .members
        .iter()
        .find(|m| m.id == caller_id && m.kind == MemberKind::Person)
        .ok_or_else(|| {
            ApiError::new(
                ErrorCode::NotAuthorized,
                "only an active person may view review inbox items",
            )
        })?;
    let employee = office.load_employee(caller_id).map_err(|error| {
        crate::logged("inbox.reviews.employee", &error);
        ApiError::new(
            ErrorCode::StorageError,
            "employee directory unavailable for reviews inbox",
        )
    })?;
    let active_person = employee
        .is_some_and(|e| e.kind == EmployeeKind::Person && e.status == EmployeeStatus::Active);
    if !active_person {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "only an active person may view review inbox items",
        ));
    }
    Ok(member)
}

fn load_projects_by_id(office: &Office) -> Result<HashMap<String, ProjectRecord>, ApiError> {
    let listed = office.list_projects(true).map_err(|error| {
        crate::logged("inbox.reviews.projects", &error);
        ApiError::new(
            ErrorCode::StorageError,
            "project directory unavailable for reviews inbox",
        )
    })?;
    Ok(listed
        .projects
        .into_iter()
        .map(|p| (p.id.clone(), p))
        .collect())
}

fn list_active_reviews(office: &Office, caller_id: &str) -> Result<Vec<ReviewView>, ApiError> {
    let mut all = Vec::new();
    for status in [ReviewStatus::Open, ReviewStatus::ChangesRequested] {
        let mut offset = 0u64;
        for _ in 0..MAX_REVIEW_LIST_PAGES {
            let page = office
                .review_list(
                    caller_id,
                    ReviewListParams {
                        status: Some(status),
                        limit: Some(REVIEW_LIST_MAX_LIMIT),
                        offset: Some(offset),
                        ..ReviewListParams::default()
                    },
                )
                .map_err(|error| {
                    crate::logged("inbox.reviews.list", &error);
                    ApiError::new(
                        ErrorCode::StorageError,
                        "reviews store unavailable for inbox",
                    )
                })??;
            let fetched = page.reviews.len() as u64;
            all.extend(page.reviews);
            offset = offset.saturating_add(fetched);
            if fetched == 0 || offset >= page.total {
                break;
            }
        }
    }
    Ok(all)
}

fn inspect_review_metadata(
    office: &Office,
    caller_id: &str,
    review_id: &str,
) -> Result<Option<ReviewInspectResult>, ApiError> {
    let res = office
        .review_inspect_sections(
            caller_id,
            ReviewInspectParams {
                review: review_id.to_owned(),
                fields: None,
            },
            ReviewInspectSections {
                rounds: true,
                reviewers: true,
                findings: false,
            },
        )
        .map_err(|error| {
            crate::logged("inbox.reviews.inspect", &error);
            ApiError::new(
                ErrorCode::StorageError,
                "reviews store unavailable for inbox inspection",
            )
        })?;
    match res {
        Ok(inspected) => Ok(Some(inspected)),
        Err(refusal) if refusal.code == ErrorCode::NotFound => Ok(None),
        Err(refusal) => Err(refusal),
    }
}

fn reviewer_needs_verdict(reviewer: &ReviewReviewerView, current_round: u32) -> bool {
    reviewer.last_verdict_round != Some(current_round)
        || !matches!(
            reviewer.last_verdict,
            Some(ReviewVerdict::Approve | ReviewVerdict::ChangesRequested)
        )
}

fn classify_review(
    ctx: &CallerOrgContext<'_>,
    inspected: &ReviewInspectResult,
) -> Option<ReviewReason> {
    let review = &inspected.review;
    if !matches!(
        review.status,
        ReviewStatus::Open | ReviewStatus::ChangesRequested
    ) {
        return None;
    }
    let caller_id = ctx.caller.id.as_str();
    let is_creator_or_submitter = is_author_or_submitter(inspected, caller_id);
    let barred_independent = review.independent && is_creator_or_submitter;
    let active_reviewers: Vec<&ReviewReviewerView> =
        inspected.reviewers.iter().filter(|r| !r.removed).collect();
    let caller_reviewer = active_reviewers
        .iter()
        .copied()
        .find(|r| r.reviewer_id == caller_id);

    if is_assigned_pending(review, barred_independent, caller_reviewer) {
        return Some(ReviewReason::AssignedPending);
    }
    if review.status == ReviewStatus::ChangesRequested && is_creator_or_submitter {
        return Some(ReviewReason::ChangesRequested);
    }
    if review.status == ReviewStatus::Open && !barred_independent && caller_reviewer.is_none() {
        if let Some(pool_team_id) =
            find_claimable_pool(ctx, &active_reviewers, review.current_round)
        {
            return Some(ReviewReason::ClaimablePool { pool_team_id });
        }
    }
    if is_escalation_stakeholder(
        ctx,
        review,
        is_creator_or_submitter,
        caller_reviewer.is_some(),
        &active_reviewers,
    ) {
        return Some(ReviewReason::EscalatedRound);
    }
    None
}

fn is_author_or_submitter(inspected: &ReviewInspectResult, caller_id: &str) -> bool {
    let review = &inspected.review;
    if review.created_by == caller_id {
        return true;
    }
    inspected
        .rounds
        .iter()
        .find(|r| r.round_number == review.current_round)
        .is_some_and(|r| r.submitted_by == caller_id)
}

fn is_assigned_pending(
    review: &ReviewView,
    barred_independent: bool,
    caller_reviewer: Option<&ReviewReviewerView>,
) -> bool {
    review.status == ReviewStatus::Open
        && !barred_independent
        && caller_reviewer.is_some_and(|r| reviewer_needs_verdict(r, review.current_round))
}

fn find_claimable_pool(
    ctx: &CallerOrgContext<'_>,
    active_reviewers: &[&ReviewReviewerView],
    current_round: u32,
) -> Option<String> {
    active_reviewers.iter().find_map(|reviewer| {
        if reviewer.routing == RoutingMode::Pool && reviewer_needs_verdict(reviewer, current_round)
        {
            ctx.eligible_for_pool(reviewer)
        } else {
            None
        }
    })
}

fn is_escalation_stakeholder(
    ctx: &CallerOrgContext<'_>,
    review: &ReviewView,
    is_creator_or_submitter: bool,
    is_assigned_reviewer: bool,
    active_reviewers: &[&ReviewReviewerView],
) -> bool {
    if review.current_round < ESCALATION_ROUND_THRESHOLD {
        return false;
    }
    ctx.caller.owner
        || ctx.is_accountable_team_lead(review)
        || is_creator_or_submitter
        || is_assigned_reviewer
        || ctx.is_manager_of(&review.created_by)
        || active_reviewers
            .iter()
            .any(|r| ctx.is_manager_of(&r.reviewer_id))
}

fn sanitize_display(raw: &str) -> String {
    sanitize_single_line_display(&org_redact::redact_text(raw, &[]))
}

fn latest_changes_requested_verdict(
    round: Option<&ReviewRoundView>,
) -> Option<&org_proto::reviews::ReviewVerdictView> {
    round?
        .verdicts
        .iter()
        .rev()
        .find(|v| v.verdict == ReviewVerdict::ChangesRequested)
}

fn build_review_inbox_item(
    office_id: &str,
    ctx: &CallerOrgContext<'_>,
    inspected: &ReviewInspectResult,
    reason: ReviewReason,
) -> InboxItem {
    let review = &inspected.review;
    let current_round = inspected
        .rounds
        .iter()
        .find(|r| r.round_number == review.current_round);
    let safe_title = sanitize_display(&review.title);
    let display_title = if safe_title.is_empty() {
        review.key.as_str()
    } else {
        safe_title.as_str()
    };
    let safe_kind = sanitize_display(&review.kind_slug);
    let escalated = review.current_round >= ESCALATION_ROUND_THRESHOLD;

    let summary = match &reason {
        ReviewReason::AssignedPending => {
            format!("Review {}: {display_title}", review.key)
        }
        ReviewReason::ChangesRequested => {
            format!("Changes requested on {}: {display_title}", review.key)
        }
        ReviewReason::ClaimablePool { .. } => {
            format!("Claim pool review {}: {display_title}", review.key)
        }
        ReviewReason::EscalatedRound => format!(
            "Escalation (round {}) on {}: {display_title}",
            review.current_round, review.key
        ),
    };

    let context = build_context(review, current_round, &safe_kind, &reason, ctx);
    let recommendation = build_recommendation(review, &reason, escalated);
    let member = match &reason {
        ReviewReason::ChangesRequested => latest_changes_requested_verdict(current_round)
            .map(|v| v.reviewer_id.clone())
            .or_else(|| Some(review.created_by.clone())),
        _ => Some(review.created_by.clone()),
    };
    let team = ctx.review_team_id(review, &reason);
    let actions = build_review_actions(review, &reason, ctx.caller.id.as_str());
    let created_at_ms = org_office::parse_rfc3339_ms(&review.updated_at)
        .or_else(|| org_office::parse_rfc3339_ms(&review.created_at));

    InboxItem {
        reference: format!("{office_id}:review:{}:r{}", review.id, review.current_round),
        source: "reviews".into(),
        kind: InboxKind::Review,
        member,
        project: review.project_id.clone(),
        team,
        summary,
        context,
        recommendation,
        options: None,
        created_at_ms,
        expires_at_ms: None,
        state: InboxState::Open,
        handled_at_ms: None,
        outcome: None,
        clears: InboxClears::ReviewDecided,
        dismissable: false,
        actions,
        attachments: Vec::new(),
    }
}

fn build_context(
    review: &ReviewView,
    current_round: Option<&ReviewRoundView>,
    safe_kind: &str,
    reason: &ReviewReason,
    ctx: &CallerOrgContext<'_>,
) -> Option<String> {
    let mut parts = vec![format!(
        "{} ({}) · round {}",
        review.target_kind.as_str(),
        if safe_kind.is_empty() {
            "review"
        } else {
            safe_kind
        },
        review.current_round
    )];
    if review.open_findings > 0 {
        parts.push(format!("{} open finding(s)", review.open_findings));
    }
    match reason {
        ReviewReason::ChangesRequested => {
            if let Some(verdict) = latest_changes_requested_verdict(current_round) {
                let reviewer_name = ctx.display_name(&verdict.reviewer_id);
                let snippet: String = verdict
                    .summary_markdown
                    .trim()
                    .chars()
                    .take(VERDICT_SNIPPET_MAX_CHARS)
                    .collect();
                let safe_snippet = sanitize_display(&snippet);
                if safe_snippet.is_empty() {
                    parts.push(format!("requested by {reviewer_name}"));
                } else {
                    parts.push(format!("{reviewer_name}: {safe_snippet}"));
                }
            }
        }
        ReviewReason::ClaimablePool { .. } => {
            parts.push("eligible team review pool".to_owned());
        }
        ReviewReason::EscalatedRound => {
            parts.push(format!("status {}", review.status.as_str()));
        }
        ReviewReason::AssignedPending => {
            parts.push(format!("by {}", ctx.display_name(&review.created_by)));
        }
    }
    let joined = sanitize_display(&parts.join(" · "));
    (!joined.is_empty()).then_some(joined)
}

fn build_recommendation(
    review: &ReviewView,
    reason: &ReviewReason,
    escalated: bool,
) -> Option<String> {
    if escalated {
        return Some(format!(
            "Advisory escalation: review {} reached round {} (>= {ESCALATION_ROUND_THRESHOLD}). Align expectations or resolve the loop.",
            review.key, review.current_round
        ));
    }
    match reason {
        ReviewReason::ClaimablePool { .. } => {
            Some("Claim this team pool review or open it to inspect.".to_owned())
        }
        ReviewReason::ChangesRequested => {
            Some("Address open findings and resubmit a new round.".to_owned())
        }
        ReviewReason::AssignedPending | ReviewReason::EscalatedRound => None,
    }
}

fn build_review_actions(
    review: &ReviewView,
    reason: &ReviewReason,
    caller_id: &str,
) -> Vec<Action> {
    let mut actions = Vec::new();
    if matches!(reason, ReviewReason::ClaimablePool { .. }) {
        let mut params = Map::new();
        params.insert("review".into(), json!(review.key));
        params.insert("expected_revision".into(), json!(review.revision));
        params.insert(
            "routing".into(),
            json!({
                "mode": "explicit",
                "reviewer_ids": [caller_id],
            }),
        );
        actions.push(Action::Call {
            label: "Claim review".into(),
            op: ReviewRoute::METHOD.into(),
            params,
            fresh: vec!["command_id".into()],
            input: None,
        });
    }
    actions.push(Action::Open {
        label: "Open review".into(),
        open: Open {
            target: OpenTarget::Review,
            id: review.key.clone(),
        },
    });
    actions
}

fn filter_and_page_items(
    snapshot: &OrgSnapshot,
    projects: &HashMap<String, ProjectRecord>,
    items: Vec<InboxItem>,
    query: &Query<'_>,
) -> Vec<InboxItem> {
    let mut filtered: Vec<InboxItem> = items
        .into_iter()
        .filter(|item| {
            matches_scope_filters(snapshot, projects, item, query)
                && query.after.is_none_or(|after| after.admits(item))
                && query.admits_files(item)
        })
        .collect();
    filtered.sort_by(|a, b| {
        b.created_at_ms
            .cmp(&a.created_at_ms)
            .then_with(|| a.reference.cmp(&b.reference))
    });
    filtered.truncate(query.limit.min(500));
    filtered
}

fn matches_scope_filters(
    snapshot: &OrgSnapshot,
    projects: &HashMap<String, ProjectRecord>,
    item: &InboxItem,
    query: &Query<'_>,
) -> bool {
    if let Some(project) = query.project {
        if item.project.as_deref() != Some(project) {
            return false;
        }
    }
    if let Some(team) = query.team {
        let team_matches = item.team.as_deref() == Some(team)
            || item.project.as_deref().is_some_and(|pid| {
                projects.get(pid).is_some_and(|proj| {
                    proj.owner_team_id == team
                        || proj.contributing_team_ids.iter().any(|t| t == team)
                }) || snapshot
                    .project_teams
                    .iter()
                    .any(|link| link.project_id == pid && link.team_id == team)
            });
        if !team_matches {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests;
