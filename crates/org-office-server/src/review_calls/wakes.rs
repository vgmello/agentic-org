//! ADR-0028 review wake orchestration (`ReviewRequested`, `ReviewVerdict`, deduplicated
//! `ReviewReply`) with two-stage warm/fresh session affinity and `>= 5`-round escalation notice.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

use org_office::organization_family::records::EmployeeKind;
use org_office::reviews::records::ReviewReviewerRecord;
use org_office::Office;
use org_office_sessions::{PendingWakeItem, SessionManager, WakeKind, WakeSessionAffinity};
use org_proto::reviews::{
    ReviewFindingView, ReviewReviewerView, ReviewRoundView, ReviewSessionMode, ReviewVerdict,
    ReviewVerdictView, ReviewView,
};
use org_proto::{AgentStatus, MemberKind};

const ESCALATION_ROUND_THRESHOLD: u32 = 5;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn actor_meta(office: &Office, actor_id: &str) -> (String, bool) {
    match office.load_employee(actor_id).ok().flatten() {
        Some(emp) => (emp.name, emp.kind == EmployeeKind::Person),
        None => (actor_id.to_owned(), actor_id == office.owner_member_id()),
    }
}

/// Returns the subset of `candidates` that are active agents placed at this office (never `actor`).
fn local_active_agent_targets(
    office: &Office,
    actor_id: &str,
    candidates: BTreeSet<String>,
) -> Vec<String> {
    let Ok(snapshot) = office.org_snapshot() else {
        return Vec::new();
    };
    let local_office = office.info().office_id.as_str();
    snapshot
        .members
        .iter()
        .filter(|m| {
            m.id != actor_id
                && candidates.contains(&m.id)
                && m.kind == MemberKind::Agent
                && m.office_id.as_deref() == Some(local_office)
                && m.agent
                    .as_ref()
                    .is_some_and(|a| a.status == AgentStatus::Active)
        })
        .map(|m| m.id.clone())
        .collect()
}

fn advisory_escalation(round_number: u32) -> Option<String> {
    (round_number >= ESCALATION_ROUND_THRESHOLD).then(|| {
        format!(
            "Advisory escalation notice: this review has reached round {round_number} (>= {ESCALATION_ROUND_THRESHOLD}). Consider escalating to a person or the accountable team lead to align expectations and resolve the loop."
        )
    })
}

/// Wakes active local agent reviewers assigned on Round 1 (`review.request`).
pub(super) fn on_review_requested(
    office: &Office,
    sessions: &SessionManager,
    actor_id: &str,
    review: &ReviewView,
    round: &ReviewRoundView,
    reviewers: &[ReviewReviewerView],
) {
    let candidates: BTreeSet<String> = reviewers
        .iter()
        .filter(|r| !r.removed)
        .map(|r| r.reviewer_id.clone())
        .collect();
    let targets = local_active_agent_targets(office, actor_id, candidates);
    if targets.is_empty() {
        return;
    }
    let (sender_name, sender_is_person) = actor_meta(office, actor_id);
    let ts = now_ms();
    let fresh = round.session_mode == ReviewSessionMode::Fresh;
    let summary = format!(
        "Review requested for {} ({}): \"{}\". Round 1 ({}).",
        review.key,
        review.kind_slug,
        review.title,
        round.session_mode.as_str()
    );
    for target in targets {
        let item = PendingWakeItem {
            item_id: format!("rev-req:{}:r{}:{target}", review.id, round.round_number),
            kind: WakeKind::ReviewRequested,
            sender_id: actor_id.to_owned(),
            sender_name: sender_name.clone(),
            sender_handle: None,
            sender_relation: None,
            sender_is_person,
            conversation_id: Some(review.key.clone()),
            source_id: review.key.clone(),
            summary: summary.clone(),
            attachment_ids: Vec::new(),
            session_affinity: fresh.then_some(WakeSessionAffinity {
                resume_runtime_session_id: None,
                fresh: true,
            }),
            enqueued_at_ms: ts,
        };
        let _ = sessions.enqueue_wake(&target, item);
    }
}

/// Wakes newly routed local agent reviewers (`review.route`).
pub(super) fn on_review_routed(
    office: &Office,
    sessions: &SessionManager,
    actor_id: &str,
    review: &ReviewView,
    reviewers: &[ReviewReviewerView],
) {
    let candidates: BTreeSet<String> = reviewers
        .iter()
        .filter(|r| !r.removed && r.last_verdict_round != Some(review.current_round))
        .map(|r| r.reviewer_id.clone())
        .collect();
    let targets = local_active_agent_targets(office, actor_id, candidates);
    if targets.is_empty() {
        return;
    }
    let (sender_name, sender_is_person) = actor_meta(office, actor_id);
    let ts = now_ms();
    let modes: BTreeMap<&str, ReviewSessionMode> = reviewers
        .iter()
        .filter(|r| !r.removed)
        .map(|r| (r.reviewer_id.as_str(), r.session_mode))
        .collect();
    for target in targets {
        let fresh = modes.get(target.as_str()) == Some(&ReviewSessionMode::Fresh);
        let item = PendingWakeItem {
            item_id: format!("rev-route:{}:rev{}:{target}", review.id, review.revision),
            kind: WakeKind::ReviewRequested,
            sender_id: actor_id.to_owned(),
            sender_name: sender_name.clone(),
            sender_handle: None,
            sender_relation: None,
            sender_is_person,
            conversation_id: Some(review.key.clone()),
            source_id: review.key.clone(),
            summary: format!(
                "You were routed as reviewer on {} ({}): \"{}\" (round {}).",
                review.key, review.kind_slug, review.title, review.current_round
            ),
            attachment_ids: Vec::new(),
            session_affinity: fresh.then_some(WakeSessionAffinity {
                resume_runtime_session_id: None,
                fresh: true,
            }),
            enqueued_at_ms: ts,
        };
        let _ = sessions.enqueue_wake(&target, item);
    }
}

/// Context for a deduplicated `ReviewReply` finding-activity wake.
pub(super) struct FindingWake<'a> {
    pub(super) actor_id: &'a str,
    pub(super) review: &'a ReviewView,
    pub(super) reviewers: &'a [ReviewReviewerView],
    pub(super) finding: &'a ReviewFindingView,
    pub(super) event_id: &'a str,
    pub(super) preview: &'a str,
    pub(super) attachment_ids: &'a [String],
}

/// Wakes review participants with deduplicated `ReviewReply` wakes on finding activity.
pub(super) fn on_review_finding_activity(
    office: &Office,
    sessions: &SessionManager,
    wake: &FindingWake<'_>,
) {
    let mut candidates = BTreeSet::new();
    candidates.insert(wake.review.created_by.clone());
    candidates.insert(wake.finding.author_id.clone());
    for r in wake.reviewers {
        if !r.removed {
            candidates.insert(r.reviewer_id.clone());
        }
    }
    let targets = local_active_agent_targets(office, wake.actor_id, candidates);
    if targets.is_empty() {
        return;
    }
    let (sender_name, sender_is_person) = actor_meta(office, wake.actor_id);
    let ts = now_ms();
    let snippet: String = wake.preview.trim().chars().take(240).collect();
    let summary = format!(
        "Finding #{} ({}, {}) on {}: {}",
        wake.finding.number,
        wake.finding.severity.as_str(),
        wake.finding.status.as_str(),
        wake.review.key,
        if snippet.is_empty() {
            "status updated"
        } else {
            &snippet
        }
    );
    for target in targets {
        let item = PendingWakeItem {
            item_id: format!("rev-reply:{}:{target}", wake.event_id),
            kind: WakeKind::ReviewReply,
            sender_id: wake.actor_id.to_owned(),
            sender_name: sender_name.clone(),
            sender_handle: None,
            sender_relation: None,
            sender_is_person,
            conversation_id: Some(wake.finding.id.clone()),
            source_id: wake.review.key.clone(),
            summary: summary.clone(),
            attachment_ids: wake.attachment_ids.to_vec(),
            session_affinity: None,
            enqueued_at_ms: ts,
        };
        let _ = sessions.enqueue_wake(&target, item);
    }
}

/// Wakes the review author (and current-round submitter) when a reviewer submits a verdict.
pub(super) fn on_review_verdict(
    office: &Office,
    sessions: &SessionManager,
    actor_id: &str,
    review: &ReviewView,
    submitted_by: Option<&str>,
    verdict: &ReviewVerdictView,
    findings_count: usize,
) {
    let mut candidates = BTreeSet::new();
    candidates.insert(review.created_by.clone());
    if let Some(sub) = submitted_by {
        candidates.insert(sub.to_owned());
    }
    let targets = local_active_agent_targets(office, actor_id, candidates);
    if targets.is_empty() {
        return;
    }
    let (sender_name, sender_is_person) = actor_meta(office, actor_id);
    let ts = now_ms();
    let mut summary = format!(
        "Verdict `{}` submitted on {} round {} (status: {}, new findings: {}).",
        verdict.verdict.as_str(),
        review.key,
        verdict.round_number,
        review.status.as_str(),
        findings_count
    );
    if !verdict.summary_markdown.trim().is_empty() {
        let brief: String = verdict.summary_markdown.trim().chars().take(240).collect();
        summary.push_str(&format!("\n{brief}"));
    }
    if verdict.verdict == ReviewVerdict::ChangesRequested {
        if let Some(notice) = advisory_escalation(verdict.round_number) {
            summary.push_str(&format!("\n{notice}"));
        }
    }
    for target in targets {
        let item = PendingWakeItem {
            item_id: format!(
                "rev-verdict:{}:r{}:rev{}:{}:{target}",
                review.id, verdict.round_number, review.revision, actor_id
            ),
            kind: WakeKind::ReviewVerdict,
            sender_id: actor_id.to_owned(),
            sender_name: sender_name.clone(),
            sender_handle: None,
            sender_relation: None,
            sender_is_person,
            conversation_id: Some(review.key.clone()),
            source_id: review.key.clone(),
            summary: summary.clone(),
            attachment_ids: verdict.attachment_ids.clone(),
            session_affinity: None,
            enqueued_at_ms: ts,
        };
        let _ = sessions.enqueue_wake(&target, item);
    }
}

/// Wakes active reviewers on Round 2..N resubmission with warm session affinity (prior
/// `runtime_session_id` + inter-round diff stat + fix/regression cues) or explicit `--fresh`.
pub(super) fn on_review_resubmitted(
    office: &Office,
    sessions: &SessionManager,
    actor_id: &str,
    review: &ReviewView,
    round: &ReviewRoundView,
    reviewer_records: &[ReviewReviewerRecord],
    inter_round_stat: Option<&str>,
) {
    let active_for_round: Vec<&ReviewReviewerRecord> = reviewer_records
        .iter()
        .filter(|r| r.round_number == round.round_number && !r.removed)
        .collect();
    let candidates: BTreeSet<String> = active_for_round
        .iter()
        .map(|r| r.reviewer_id.as_str().to_owned())
        .collect();
    let targets = local_active_agent_targets(office, actor_id, candidates);
    if targets.is_empty() {
        return;
    }
    let (sender_name, sender_is_person) = actor_meta(office, actor_id);
    let ts = now_ms();
    let prev_round = round.round_number.saturating_sub(1).max(1);
    let summary = build_resubmit_summary(review, round, prev_round, inter_round_stat);

    let sid_by_reviewer: BTreeMap<&str, Option<&str>> = active_for_round
        .iter()
        .map(|r| (r.reviewer_id.as_str(), r.runtime_session_id.as_deref()))
        .collect();

    for target in targets {
        let prior_sid = (round.session_mode == ReviewSessionMode::Warm)
            .then(|| sid_by_reviewer.get(target.as_str()).copied().flatten())
            .flatten()
            .map(str::to_owned);
        let affinity = match round.session_mode {
            ReviewSessionMode::Fresh => WakeSessionAffinity {
                resume_runtime_session_id: None,
                fresh: true,
            },
            ReviewSessionMode::Warm => WakeSessionAffinity {
                resume_runtime_session_id: prior_sid,
                fresh: false,
            },
        };
        let item = PendingWakeItem {
            item_id: format!(
                "rev-resubmit:{}:r{}:{target}",
                review.id, round.round_number
            ),
            kind: WakeKind::ReviewRequested,
            sender_id: actor_id.to_owned(),
            sender_name: sender_name.clone(),
            sender_handle: None,
            sender_relation: None,
            sender_is_person,
            conversation_id: Some(review.key.clone()),
            source_id: review.key.clone(),
            summary: summary.clone(),
            attachment_ids: Vec::new(),
            session_affinity: Some(affinity),
            enqueued_at_ms: ts,
        };
        let _ = sessions.enqueue_wake(&target, item);
    }
}

fn build_resubmit_summary(
    review: &ReviewView,
    round: &ReviewRoundView,
    prev_round: u32,
    inter_round_stat: Option<&str>,
) -> String {
    let mut parts = vec![format!(
        "Resubmitted {} (\"{}\") as round {} ({}).",
        review.key,
        review.title,
        round.round_number,
        round.session_mode.as_str()
    )];
    if !round.note_markdown.trim().is_empty() {
        let note: String = round.note_markdown.trim().chars().take(280).collect();
        parts.push(format!("Author note: {note}"));
    }
    if let Some(stat) = inter_round_stat {
        parts.push(stat.to_owned());
    }
    if round.session_mode == ReviewSessionMode::Fresh {
        parts.push(format!(
            "Fresh independent validation pass: re-examine the full diff (`agent-os review diff --review {}`) and all open findings without relying on earlier round assumptions.",
            review.key
        ));
    } else {
        parts.push(format!(
            "Warm round-{} verification: inspect the inter-round diff (`agent-os review diff --review {} --since-round {}`), verify each open finding is actually fixed at the new head, and check for regressions or previously unexamined areas outside the changed lines.",
            round.round_number, review.key, prev_round
        ));
    }
    if let Some(notice) = advisory_escalation(round.round_number) {
        parts.push(notice);
    }
    parts.join("\n\n")
}
