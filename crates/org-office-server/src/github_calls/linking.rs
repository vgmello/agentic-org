//! Live linking between ingested GitHub pull requests and Agentic Org tasks (`T-`) and
//! Universal Reviews (`RV-`) scoped strictly to the authorized project and caller.

use std::collections::HashMap;

use org_office::reviews::features::ReviewInspectSections;
use org_office::Office;
use org_proto::reviews::{ReviewInspectParams, ReviewListParams, ReviewTargetKind, ReviewView};

use super::store::StoredPrRecord;

/// Resolved task IDs and Universal Review (`RV-`) IDs/keys linked to a GitHub PR.
#[derive(Debug, Default, Clone)]
pub(super) struct ResolvedPrLinks {
    pub linked_task_ids: Vec<String>,
    pub linked_review_ids: Vec<String>,
    pub linked_review_keys: Vec<String>,
}

/// Per-request cache of project code reviews, round head OIDs, and task lookups.
///
/// Fetches `office.review_list` ONCE per call (eliminating N×M amplification in `github.pr.list`)
/// and lazily inspects each review's rounds at most once when SHA matching is needed.
pub(super) struct ProjectReviewLinkCache<'a> {
    office: &'a Office,
    reader_member: &'a str,
    reviews: Vec<ReviewView>,
    round_oids_by_review: HashMap<String, Vec<String>>,
    task_by_ref: HashMap<String, Option<(String, Option<String>)>>,
}

impl<'a> ProjectReviewLinkCache<'a> {
    /// Loads the caller-visible code reviews once for `project_id_filter`.
    pub(super) fn load(
        office: &'a Office,
        reader_member: &'a str,
        project_id_filter: Option<&str>,
    ) -> Self {
        let list_params = ReviewListParams {
            project_id: project_id_filter.map(str::to_owned),
            target_kind: Some(ReviewTargetKind::Code),
            limit: Some(100),
            ..ReviewListParams::default()
        };
        let reviews = office
            .review_list(reader_member, list_params)
            .ok()
            .and_then(Result::ok)
            .map_or_else(Vec::new, |listed| listed.reviews);
        Self {
            office,
            reader_member,
            reviews,
            round_oids_by_review: HashMap::new(),
            task_by_ref: HashMap::new(),
        }
    }

    /// Resolves matching org tasks and Universal Reviews for one PR record using the cached index.
    pub(super) fn resolve(
        &mut self,
        project_id_filter: Option<&str>,
        record: &StoredPrRecord,
    ) -> ResolvedPrLinks {
        let mut links = ResolvedPrLinks::default();
        self.resolve_task_links(project_id_filter, record, &mut links);
        self.resolve_review_links(record, &mut links);
        links
    }

    fn resolve_task_links(
        &mut self,
        project_id_filter: Option<&str>,
        record: &StoredPrRecord,
        links: &mut ResolvedPrLinks,
    ) {
        for task_ref in &record.linked_task_refs {
            let entry = self.task_by_ref.entry(task_ref.clone()).or_insert_with(|| {
                self.office
                    .task_inspect(task_ref)
                    .ok()
                    .and_then(Result::ok)
                    .map(|inspected| (inspected.task.id, inspected.task.project_id))
            });
            let Some((task_id, task_project_id)) = entry else {
                continue;
            };
            let project_ok =
                project_id_filter.is_none_or(|pid| task_project_id.as_deref() == Some(pid));
            if project_ok {
                push_unique(&mut links.linked_task_ids, task_id.clone());
            }
        }
    }

    fn resolve_review_links(&mut self, record: &StoredPrRecord, links: &mut ResolvedPrLinks) {
        let mut matched: Vec<(String, String, Option<String>)> = Vec::new();
        for idx in 0..self.reviews.len() {
            if self.reviews[idx].repository_id.as_deref() != Some(record.repository_id.as_str()) {
                continue;
            }
            let task_match = self.reviews[idx]
                .task_id
                .as_deref()
                .is_some_and(|tid| links.linked_task_ids.iter().any(|id| id == tid));
            let branch_match = !record.head_ref.is_empty()
                && self.reviews[idx]
                    .branch
                    .as_deref()
                    .is_some_and(|b| b == record.head_ref);
            let sha_match = if task_match || branch_match {
                false
            } else {
                let review_id = self.reviews[idx].id.clone();
                self.review_matches_head_sha(&review_id, &record.head_sha)
            };
            if task_match || branch_match || sha_match {
                let review = &self.reviews[idx];
                matched.push((
                    review.id.clone(),
                    review.key.clone(),
                    review.task_id.clone(),
                ));
            }
        }
        for (id, key, task_id) in matched {
            push_unique(&mut links.linked_review_ids, id);
            push_unique(&mut links.linked_review_keys, key);
            if let Some(tid) = task_id {
                push_unique(&mut links.linked_task_ids, tid);
            }
        }
    }

    fn review_matches_head_sha(&mut self, review_id: &str, pr_head_sha: &str) -> bool {
        let sha = pr_head_sha.trim();
        if !is_valid_git_sha_prefix(sha) {
            return false;
        }
        let oids = self
            .round_oids_by_review
            .entry(review_id.to_owned())
            .or_insert_with(|| {
                let Ok(Ok(inspected)) = self.office.review_inspect_sections(
                    self.reader_member,
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
                    return Vec::new();
                };
                inspected
                    .rounds
                    .into_iter()
                    .filter_map(|round| round.head_oid)
                    .collect()
            });
        oids.iter().any(|oid| sha_matches(oid, sha))
    }
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
    let mut cache = ProjectReviewLinkCache::load(office, reader_member, project_id_filter);
    cache.resolve(project_id_filter, record)
}

fn push_unique(list: &mut Vec<String>, value: String) {
    if !list.contains(&value) {
        list.push(value);
    }
}

fn is_valid_git_sha_prefix(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    (7..=64).contains(&bytes.len()) && bytes.iter().all(u8::is_ascii_hexdigit)
}

/// Compares two Git SHA strings (full or >= 7-hex-digit abbreviated prefix) using raw byte slices
/// after requiring non-empty ASCII hex, avoiding any `&str` byte-index slicing panic on untrusted
/// multibyte UTF-8 input.
pub(super) fn sha_matches(a: &str, b: &str) -> bool {
    let a = a.trim();
    let b = b.trim();
    if !is_valid_git_sha_prefix(a) || !is_valid_git_sha_prefix(b) {
        return false;
    }
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    let prefix_len = ab.len().min(bb.len());
    ab[..prefix_len].eq_ignore_ascii_case(&bb[..prefix_len])
}
