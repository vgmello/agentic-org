//! Project-scoped GitHub PR list, detail inspection, CI failure analysis, and on-demand
//! installation-token sync handlers (`TASK-272`, AC #3).

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use org_github::{
    compute_ci_overall_status, format_fenced_pr_context, GitHubError, GitHubSyncClient,
    InstallationTokenManager, ValidatedRepoSlug,
};
use org_office::organization_family::records::Repository;
use org_office::Office;
use org_proto::github::{
    GitHubCheckConclusion, GitHubFailureSummaryView, GitHubPrChecksParams, GitHubPrChecksResult,
    GitHubPrDetailView, GitHubPrInspectParams, GitHubPrListParams, GitHubPrListResult,
    GitHubPrSummaryView, GitHubPrSyncParams, GitHubPrSyncResult,
    GITHUB_PR_LIST_BODY_EXCERPT_MAX_BYTES, GITHUB_PR_LIST_MAX_LIMIT,
    GITHUB_PR_LIST_MAX_RESPONSE_BYTES,
};
use org_proto::{ApiError, ErrorCode};

use crate::live::LiveEvents;

use super::linking::{resolve_pr_links, ProjectReviewLinkCache};
use super::scope::resolve_authorized_project_repos;
use super::store::{
    compact_json_len, list_prs_for_repo, load_bindings, load_pr, save_pr, StoredPrRecord,
    MAX_PR_RECORD_BYTES,
};
use super::vault::{load_github_app_meta, load_github_app_private_key, now_rfc3339};

pub(super) fn handle_pr_list(
    office: &Office,
    member: &str,
    params: GitHubPrListParams,
) -> Result<GitHubPrListResult, ApiError> {
    let auth = resolve_authorized_project_repos(
        office,
        member,
        &params.project,
        params.repository.as_deref(),
    )?;
    let limit = params
        .limit
        .unwrap_or(50)
        .clamp(1, GITHUB_PR_LIST_MAX_LIMIT) as usize;

    let task_filter_id = match params
        .task
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(tref) => match office.task_inspect(tref) {
            Ok(Ok(inspected)) if inspected.task.project_id.as_deref() == Some(&auth.project_id) => {
                Some((inspected.task.id, inspected.task.key))
            }
            _ => {
                return Ok(GitHubPrListResult {
                    project_id: auth.project_id,
                    pull_requests: Vec::new(),
                    has_more: false,
                });
            }
        },
        None => None,
    };

    let mut link_cache = ProjectReviewLinkCache::load(office, member, Some(&auth.project_id));
    let mut summaries = Vec::new();
    for repo in &auth.repos {
        for rec in list_prs_for_repo(office, repo.id.as_str())? {
            if let Some(state) = params.state {
                if rec.state != state {
                    continue;
                }
            }
            let links = link_cache.resolve(Some(&auth.project_id), &rec);
            if let Some((ref tid, ref tkey)) = task_filter_id {
                let matches_task = links.linked_task_ids.iter().any(|id| id == tid)
                    || rec
                        .linked_task_refs
                        .iter()
                        .any(|r| r.eq_ignore_ascii_case(tkey));
                if !matches_task {
                    continue;
                }
            }
            summaries.push(rec.to_summary_view(
                repo.key.clone(),
                links.linked_task_ids,
                links.linked_review_ids,
                links.linked_review_keys,
            ));
        }
    }
    summaries.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| b.pr_number.cmp(&a.pr_number))
    });
    let (pull_requests, has_more) = cap_pr_list_summaries(&auth.project_id, summaries, limit)?;
    Ok(GitHubPrListResult {
        project_id: auth.project_id,
        pull_requests,
        has_more,
    })
}

fn cap_pr_list_summaries(
    project_id: &str,
    mut summaries: Vec<GitHubPrSummaryView>,
    limit: usize,
) -> Result<(Vec<GitHubPrSummaryView>, bool), ApiError> {
    let mut has_more = summaries.len() > limit;
    summaries.truncate(limit);

    // Use `has_more: false` (5 bytes vs 4 bytes for `true`) as the conservative wire upper bound.
    let base_bytes = compact_json_len(&GitHubPrListResult {
        project_id: project_id.to_owned(),
        pull_requests: Vec::new(),
        has_more: false,
    })?;
    let mut used_bytes = base_bytes;
    let mut bounded = Vec::with_capacity(summaries.len());
    for mut summary in summaries {
        if let Some(excerpt) = summary.body_excerpt.as_deref() {
            if excerpt.len() > GITHUB_PR_LIST_BODY_EXCERPT_MAX_BYTES {
                summary.body_excerpt = Some(truncate_utf8_bytes(
                    excerpt,
                    GITHUB_PR_LIST_BODY_EXCERPT_MAX_BYTES,
                ));
            }
        }
        let item_bytes = compact_json_len(&summary)?;
        let sep = usize::from(!bounded.is_empty());
        let next_bytes = used_bytes.saturating_add(sep).saturating_add(item_bytes);
        if next_bytes > GITHUB_PR_LIST_MAX_RESPONSE_BYTES {
            has_more = true;
            break;
        }
        used_bytes = next_bytes;
        bounded.push(summary);
    }
    Ok((bounded, has_more))
}

fn truncate_utf8_bytes(input: &str, max_bytes: usize) -> String {
    if input.len() <= max_bytes {
        return input.to_owned();
    }
    const SUFFIX: &str = "…";
    if max_bytes <= SUFFIX.len() {
        let cut = input.floor_char_boundary(max_bytes);
        return input[..cut].to_owned();
    }
    let cut = input.floor_char_boundary(max_bytes - SUFFIX.len());
    let mut out = String::with_capacity(cut + SUFFIX.len());
    out.push_str(&input[..cut]);
    out.push_str(SUFFIX);
    out
}

pub(super) fn handle_pr_inspect(
    office: &Office,
    member: &str,
    params: GitHubPrInspectParams,
) -> Result<GitHubPrDetailView, ApiError> {
    let auth = resolve_authorized_project_repos(
        office,
        member,
        &params.project,
        params.repository.as_deref(),
    )?;
    let (repo_key, mut record) =
        find_pr_in_authorized_repos(office, &auth.repos, params.pr_number)?;
    record.trim_to_budget(MAX_PR_RECORD_BYTES)?;
    let links = resolve_pr_links(office, member, Some(&auth.project_id), &record);
    let summary = record.to_summary_view(
        repo_key,
        links.linked_task_ids,
        links.linked_review_ids,
        links.linked_review_keys,
    );
    let fenced_untrusted_context = format_fenced_pr_context(
        &summary,
        &record.reviews,
        &record.review_comments,
        &record.check_runs,
    );
    Ok(GitHubPrDetailView {
        project_id: auth.project_id,
        pr: summary,
        reviews: record.reviews,
        review_comments: record.review_comments,
        check_runs: record.check_runs,
        fenced_untrusted_context,
    })
}

pub(super) fn handle_pr_checks(
    office: &Office,
    member: &str,
    params: GitHubPrChecksParams,
) -> Result<GitHubPrChecksResult, ApiError> {
    let auth = resolve_authorized_project_repos(
        office,
        member,
        &params.project,
        params.repository.as_deref(),
    )?;
    let (_repo_key, mut record) =
        find_pr_in_authorized_repos(office, &auth.repos, params.pr_number)?;
    record.trim_to_budget(MAX_PR_RECORD_BYTES)?;
    let links = resolve_pr_links(office, member, Some(&auth.project_id), &record);
    let overall_status = compute_ci_overall_status(&record.check_runs);
    let failure_summaries: Vec<GitHubFailureSummaryView> = record
        .check_runs
        .iter()
        .filter_map(|c| {
            let conclusion = c.conclusion?;
            if !conclusion.is_failure() {
                return None;
            }
            Some(GitHubFailureSummaryView {
                check_run_id: c.check_run_id,
                name: c.name.clone(),
                conclusion,
                details_url: c.details_url.clone(),
                title: c.title.clone(),
                summary: c.summary.clone(),
                annotations: c.annotations.clone(),
            })
        })
        .collect();
    let check_runs = if params.only_failures {
        record
            .check_runs
            .into_iter()
            .filter(|c| c.conclusion.is_some_and(GitHubCheckConclusion::is_failure))
            .collect()
    } else {
        record.check_runs
    };
    Ok(GitHubPrChecksResult {
        project_id: auth.project_id,
        repository_id: record.repository_id,
        pr_number: record.pr_number,
        head_sha: record.head_sha,
        overall_status,
        linked_task_ids: links.linked_task_ids,
        linked_review_ids: links.linked_review_ids,
        linked_review_keys: links.linked_review_keys,
        check_runs,
        failure_summaries,
    })
}

pub(super) fn handle_pr_sync(
    office: &Office,
    live: &LiveEvents,
    tokens: &InstallationTokenManager,
    store_lock: &Mutex<()>,
    member: &str,
    params: GitHubPrSyncParams,
) -> Result<GitHubPrSyncResult, ApiError> {
    let auth = resolve_authorized_project_repos(
        office,
        member,
        &params.project,
        Some(&params.repository),
    )?;
    let repo = auth
        .repos
        .first()
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "repository not found"))?;
    let repo_id = repo.id.as_str().to_owned();

    // Load binding and unwrapped key before network I/O; never hold `store_lock` across I/O.
    let (binding, config, private_key) = {
        let _guard = store_lock.lock().unwrap_or_else(|e| e.into_inner());
        let bindings = load_bindings(office)?;
        let binding = bindings
            .into_iter()
            .find(|b| b.repository_id == repo_id)
            .ok_or_else(|| {
                ApiError::new(
                    ErrorCode::Conflict,
                    "repository is not bound to a GitHub owner/repo",
                )
            })?;
        let (config, private_key) = load_github_app_private_key(office)?.ok_or_else(|| {
            ApiError::new(
                ErrorCode::Conflict,
                "GitHub App is not configured for this office",
            )
        })?;
        (binding, config, private_key)
    };

    let slug = ValidatedRepoSlug::new(&binding.owner, &binding.repo)
        .map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))?;
    let sync_client = GitHubSyncClient::new(
        tokens,
        config.app_id,
        binding.installation_id,
        &config.api_base_url,
        &private_key,
    );
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let synced = match params.pr_number {
        Some(pr_number) => vec![sync_client
            .sync_pull_request(&slug, pr_number, now_secs)
            .map_err(map_github_error)?],
        None => sync_client
            .sync_open_pull_requests(&slug, 20, now_secs)
            .map_err(map_github_error)?,
    };

    let mut synced_prs = Vec::with_capacity(synced.len());
    {
        let _guard = store_lock.lock().unwrap_or_else(|e| e.into_inner());
        // Re-verify App custody and repository binding after releasing `store_lock` across network
        // I/O so concurrent revocation or unbinding never persists stale PR snapshots.
        if !load_github_app_meta(office)?.is_some_and(|meta| meta.app_id == config.app_id) {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "GitHub App configuration was revoked or changed during sync",
            ));
        }
        let still_bound = load_bindings(office)?.iter().any(|b| {
            b.repository_id == repo_id
                && b.full_name.eq_ignore_ascii_case(&binding.full_name)
                && b.installation_id == binding.installation_id
        });
        if !still_bound {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "repository GitHub binding was removed or changed during sync",
            ));
        }
        for item in synced {
            let pr_num = item.pr.pr_number;
            let mut record = match load_pr(office, &repo_id, pr_num)? {
                Some(mut existing) => {
                    existing.apply_pull_request(item.pr);
                    existing
                }
                None => StoredPrRecord::from_parsed(repo_id.clone(), item.pr),
            };
            for review in item.reviews {
                record.upsert_review(review);
            }
            for comment in item.review_comments {
                record.upsert_review_comment(comment);
            }
            for check in item.check_runs {
                record.upsert_check_run(check);
            }
            save_pr(office, &mut record)?;
            synced_prs.push(pr_num);
        }
    }
    if !synced_prs.is_empty() {
        live.record_org_changed();
    }
    Ok(GitHubPrSyncResult {
        project_id: auth.project_id,
        repository_id: repo_id,
        synced_prs,
        synced_at: now_rfc3339(),
    })
}

fn find_pr_in_authorized_repos(
    office: &Office,
    repos: &[Repository],
    pr_number: u64,
) -> Result<(Option<String>, StoredPrRecord), ApiError> {
    if pr_number == 0 {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "pr_number must be positive",
        ));
    }
    let mut found: Vec<(Option<String>, StoredPrRecord)> = Vec::new();
    for repo in repos {
        if let Some(rec) = load_pr(office, repo.id.as_str(), pr_number)? {
            found.push((repo.key.clone(), rec));
        }
    }
    match found.len() {
        0 => Err(ApiError::new(ErrorCode::NotFound, "pull request not found")),
        1 => Ok(found.remove(0)),
        _ => Err(ApiError::new(
            ErrorCode::Conflict,
            "multiple repositories in this project have that PR number; specify `repository`",
        )),
    }
}

fn map_github_error(err: GitHubError) -> ApiError {
    match err {
        GitHubError::InvalidConfig(_)
        | GitHubError::InvalidPrivateKey(_)
        | GitHubError::InvalidEndpoint(_) => {
            ApiError::new(ErrorCode::InvalidRequest, err.to_string())
        }
        GitHubError::AuthRejected(_) => ApiError::new(ErrorCode::NotAuthorized, err.to_string()),
        GitHubError::NotFound => ApiError::new(ErrorCode::NotFound, err.to_string()),
        GitHubError::RateLimited => ApiError::new(ErrorCode::RateLimited, err.to_string()),
        GitHubError::Timeout => ApiError::new(ErrorCode::Timeout, err.to_string()),
        GitHubError::SigningFailed(_)
        | GitHubError::Transport(_)
        | GitHubError::MalformedResponse(_) => {
            ApiError::new(ErrorCode::Unavailable, err.to_string())
        }
    }
}
