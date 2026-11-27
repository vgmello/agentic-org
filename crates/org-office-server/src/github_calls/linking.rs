//! Live linking between ingested GitHub pull requests and Agentic Org tasks (`T-`) and
//! Universal Reviews (`RV-`) scoped strictly to the authorized project and caller.

use org_office::reviews::features::ReviewInspectSections;
use org_office::Office;
use org_proto::reviews::{ReviewInspectParams, ReviewListParams, ReviewTargetKind};

use super::store::StoredPrRecord;

/// Resolved task IDs and Universal Review (`RV-`) IDs/keys linked to a GitHub PR.
#[derive(Debug, Default, Clone)]
pub(super) struct ResolvedPrLinks {
    pub linked_task_ids: Vec<String>,
    pub linked_review_ids: Vec<String>,
    pub linked_review_keys: Vec<String>,
}

/// Resolves matching org tasks (from `record.linked_task_refs`, e.g. `T-A42`, and code reviews'
/// `task_id`) and Universal Reviews (`RV-` matching `(repository_id, head_sha)`,
/// `(repository_id, branch)`, or `task_id`).
///
/// Always evaluates reviews under `reader_member` (and scoped to `project_id_filter` when given)
/// so private reviews outside the caller's visibility never leak.
pub(super) fn resolve_pr_links(
    office: &Office,
    reader_member: &str,
    project_id_filter: Option<&str>,
    record: &StoredPrRecord,
) -> ResolvedPrLinks {
    let mut links = ResolvedPrLinks::default();
    resolve_task_links(office, project_id_filter, record, &mut links);
    resolve_review_links(office, reader_member, project_id_filter, record, &mut links);
    links
}

fn push_unique(list: &mut Vec<String>, value: String) {
    if !list.contains(&value) {
        list.push(value);
    }
}

fn resolve_task_links(
    office: &Office,
    project_id_filter: Option<&str>,
    record: &StoredPrRecord,
    links: &mut ResolvedPrLinks,
) {
    for task_ref in &record.linked_task_refs {
        let Ok(Ok(inspected)) = office.task_inspect(task_ref) else {
            continue;
        };
        let project_ok =
            project_id_filter.is_none_or(|pid| inspected.task.project_id.as_deref() == Some(pid));
        if project_ok {
            push_unique(&mut links.linked_task_ids, inspected.task.id);
        }
    }
}

fn resolve_review_links(
    office: &Office,
    reader_member: &str,
    project_id_filter: Option<&str>,
    record: &StoredPrRecord,
    links: &mut ResolvedPrLinks,
) {
    let list_params = ReviewListParams {
        project_id: project_id_filter.map(str::to_owned),
        target_kind: Some(ReviewTargetKind::Code),
        limit: Some(100),
        ..ReviewListParams::default()
    };
    let Ok(Ok(listed)) = office.review_list(reader_member, list_params) else {
        return;
    };

    for review in listed.reviews {
        if !review_matches_pr(
            office,
            reader_member,
            record,
            &links.linked_task_ids,
            &review,
        ) {
            continue;
        }
        push_unique(&mut links.linked_review_ids, review.id);
        push_unique(&mut links.linked_review_keys, review.key);
        if let Some(tid) = review.task_id {
            push_unique(&mut links.linked_task_ids, tid);
        }
    }
}

fn review_matches_pr(
    office: &Office,
    reader_member: &str,
    record: &StoredPrRecord,
    linked_task_ids: &[String],
    review: &org_proto::reviews::ReviewView,
) -> bool {
    if review.repository_id.as_deref() != Some(record.repository_id.as_str()) {
        return false;
    }
    let task_match = review
        .task_id
        .as_deref()
        .is_some_and(|tid| linked_task_ids.iter().any(|id| id == tid));
    let branch_match = !record.head_ref.is_empty()
        && review
            .branch
            .as_deref()
            .is_some_and(|b| b == record.head_ref);
    task_match
        || branch_match
        || review_matches_head_sha(office, reader_member, &review.id, &record.head_sha)
}

fn review_matches_head_sha(
    office: &Office,
    reader_member: &str,
    review_id: &str,
    pr_head_sha: &str,
) -> bool {
    let sha = pr_head_sha.trim();
    if sha.len() < 7 {
        return false;
    }
    let Ok(Ok(inspected)) = office.review_inspect_sections(
        reader_member,
        ReviewInspectParams {
            review: review_id.to_owned(),
            fields: None,
        },
        ReviewInspectSections {
            rounds: true,
            reviewers: false,
            findings: false,
        },
    ) else {
        return false;
    };
    inspected.rounds.iter().any(|round| {
        round
            .head_oid
            .as_deref()
            .is_some_and(|oid| sha_matches(oid, sha))
    })
}

fn sha_matches(a: &str, b: &str) -> bool {
    let a = a.trim();
    let b = b.trim();
    if a.len() < 7 || b.len() < 7 {
        return false;
    }
    a.eq_ignore_ascii_case(b)
        || (a.len() > b.len() && a[..b.len()].eq_ignore_ascii_case(b))
        || (b.len() > a.len() && b[..a.len()].eq_ignore_ascii_case(a))
}
