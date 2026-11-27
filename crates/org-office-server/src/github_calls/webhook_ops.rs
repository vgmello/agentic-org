//! Constant-time signed GitHub webhook ingestion, bounded delivery deduplication, store updates,
//! live task/`RV-` linking, and SSE change notification (`TASK-272`, AC #2).

use std::sync::Mutex;

use org_github::{
    parse_webhook_event, verify_webhook_headers_and_signature, ParsedWebhookEvent, WebhookError,
};
use org_office::Office;
use org_proto::github::{GitHubCheckRunView, GitHubWebhookIngestParams, GitHubWebhookIngestResult};
use org_proto::{ApiError, ErrorCode};

use crate::live::LiveEvents;

use super::linking::resolve_pr_links;
use super::store::{
    list_prs_for_repo, load_bindings, load_dedup, load_pr, save_dedup, save_pr, StoredPrRecord,
    StoredRepoBinding,
};
use super::vault::{load_github_webhook_secret, now_rfc3339};

struct WebhookApplyOutcome {
    changed: bool,
    ignored: bool,
    repository_id: Option<String>,
    pr_number: Option<u64>,
    linked_task_ids: Vec<String>,
    linked_review_ids: Vec<String>,
}

impl WebhookApplyOutcome {
    fn ignored() -> Self {
        Self {
            changed: false,
            ignored: true,
            repository_id: None,
            pr_number: None,
            linked_task_ids: Vec::new(),
            linked_review_ids: Vec::new(),
        }
    }
}

pub(super) fn handle_webhook_ingest(
    office: &Office,
    live: &LiveEvents,
    store_lock: &Mutex<()>,
    params: GitHubWebhookIngestParams,
) -> Result<GitHubWebhookIngestResult, ApiError> {
    let secret = load_github_webhook_secret(office)?.ok_or_else(|| {
        ApiError::new(
            ErrorCode::NotAuthorized,
            "GitHub webhook secret is not configured for this office",
        )
    })?;
    let (event, delivery_id) = verify_webhook_headers_and_signature(
        &params.event,
        &params.delivery_id,
        params.signature_256.expose(),
        params.payload.as_bytes(),
        secret.as_str(),
    )
    .map_err(map_webhook_error)?;

    let parsed =
        parse_webhook_event(&event, params.payload.as_bytes()).map_err(map_webhook_error)?;

    let (result, changed) = {
        let _guard = store_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut dedup = load_dedup(office)?;
        if !dedup.record_if_new(&delivery_id) {
            return Ok(GitHubWebhookIngestResult {
                delivery_id,
                event,
                accepted: true,
                duplicate: true,
                ignored: false,
                repository_id: None,
                pr_number: None,
                linked_task_ids: Vec::new(),
                linked_review_ids: Vec::new(),
            });
        }

        let bindings = load_bindings(office)?;
        let outcome = apply_webhook_event(office, &bindings, parsed)?;
        save_dedup(office, &dedup)?;

        (
            GitHubWebhookIngestResult {
                delivery_id,
                event,
                accepted: true,
                duplicate: false,
                ignored: outcome.ignored,
                repository_id: outcome.repository_id,
                pr_number: outcome.pr_number,
                linked_task_ids: outcome.linked_task_ids,
                linked_review_ids: outcome.linked_review_ids,
            },
            outcome.changed,
        )
    };

    if changed {
        live.record_org_changed();
    }
    Ok(result)
}

fn apply_webhook_event(
    office: &Office,
    bindings: &[StoredRepoBinding],
    parsed: ParsedWebhookEvent,
) -> Result<WebhookApplyOutcome, ApiError> {
    let find_repo = |full_name: &str| {
        bindings
            .iter()
            .find(|b| b.full_name.eq_ignore_ascii_case(full_name))
            .map(|b| b.repository_id.clone())
    };
    let owner = office.owner_member_id();

    match parsed {
        ParsedWebhookEvent::Ping { .. } | ParsedWebhookEvent::Unsupported { .. } => {
            Ok(WebhookApplyOutcome::ignored())
        }
        ParsedWebhookEvent::PullRequest(ev) => {
            let Some(repo_id) = find_repo(&ev.repo_full_name) else {
                return Ok(WebhookApplyOutcome::ignored());
            };
            let pr_num = ev.pr_number;
            let record = match load_pr(office, &repo_id, pr_num)? {
                Some(mut existing) => {
                    existing.apply_pull_request(ev);
                    existing
                }
                None => StoredPrRecord::from_parsed(repo_id.clone(), ev),
            };
            save_pr(office, &record)?;
            let links = resolve_pr_links(office, owner, None, &record);
            Ok(WebhookApplyOutcome {
                changed: true,
                ignored: false,
                repository_id: Some(repo_id),
                pr_number: Some(pr_num),
                linked_task_ids: links.linked_task_ids,
                linked_review_ids: links.linked_review_ids,
            })
        }
        ParsedWebhookEvent::PullRequestReview(ev) => {
            let Some(repo_id) = find_repo(&ev.repo_full_name) else {
                return Ok(WebhookApplyOutcome::ignored());
            };
            let Some(mut record) = load_pr(office, &repo_id, ev.pr_number)? else {
                return Ok(WebhookApplyOutcome::ignored());
            };
            record.upsert_review(ev.review);
            record.updated_at = now_rfc3339();
            save_pr(office, &record)?;
            let links = resolve_pr_links(office, owner, None, &record);
            Ok(WebhookApplyOutcome {
                changed: true,
                ignored: false,
                repository_id: Some(repo_id),
                pr_number: Some(ev.pr_number),
                linked_task_ids: links.linked_task_ids,
                linked_review_ids: links.linked_review_ids,
            })
        }
        ParsedWebhookEvent::PullRequestReviewComment(ev) => {
            let Some(repo_id) = find_repo(&ev.repo_full_name) else {
                return Ok(WebhookApplyOutcome::ignored());
            };
            let Some(mut record) = load_pr(office, &repo_id, ev.pr_number)? else {
                return Ok(WebhookApplyOutcome::ignored());
            };
            record.upsert_review_comment(ev.comment);
            record.updated_at = now_rfc3339();
            save_pr(office, &record)?;
            let links = resolve_pr_links(office, owner, None, &record);
            Ok(WebhookApplyOutcome {
                changed: true,
                ignored: false,
                repository_id: Some(repo_id),
                pr_number: Some(ev.pr_number),
                linked_task_ids: links.linked_task_ids,
                linked_review_ids: links.linked_review_ids,
            })
        }
        ParsedWebhookEvent::CheckRun(ev) => apply_check_event(
            office,
            bindings,
            &ev.repo_full_name,
            &ev.pr_numbers,
            ev.check_run,
        ),
        ParsedWebhookEvent::CheckSuite(ev) => apply_check_event(
            office,
            bindings,
            &ev.repo_full_name,
            &ev.pr_numbers,
            ev.check_run,
        ),
    }
}

fn apply_check_event(
    office: &Office,
    bindings: &[StoredRepoBinding],
    repo_full_name: &str,
    explicit_prs: &[u64],
    check_run: GitHubCheckRunView,
) -> Result<WebhookApplyOutcome, ApiError> {
    let Some(repo_id) = bindings
        .iter()
        .find(|b| b.full_name.eq_ignore_ascii_case(repo_full_name))
        .map(|b| b.repository_id.clone())
    else {
        return Ok(WebhookApplyOutcome::ignored());
    };
    let mut target_prs: Vec<StoredPrRecord> = Vec::new();
    for &pr_num in explicit_prs {
        if let Some(rec) = load_pr(office, &repo_id, pr_num)? {
            target_prs.push(rec);
        }
    }
    // Fallback: match PRs by `head_sha` when GitHub omits `pull_requests` on check runs.
    if target_prs.is_empty() && !check_run.head_sha.is_empty() {
        for rec in list_prs_for_repo(office, &repo_id)? {
            if rec.head_sha.eq_ignore_ascii_case(&check_run.head_sha) {
                target_prs.push(rec);
            }
        }
    }
    if target_prs.is_empty() {
        return Ok(WebhookApplyOutcome::ignored());
    }

    let owner = office.owner_member_id();
    let mut first_pr = None;
    let mut linked_task_ids = Vec::new();
    let mut linked_review_ids = Vec::new();
    for mut rec in target_prs {
        if first_pr.is_none() {
            first_pr = Some(rec.pr_number);
        }
        rec.upsert_check_run(check_run.clone());
        rec.updated_at = now_rfc3339();
        save_pr(office, &rec)?;
        let links = resolve_pr_links(office, owner, None, &rec);
        for id in links.linked_task_ids {
            if !linked_task_ids.contains(&id) {
                linked_task_ids.push(id);
            }
        }
        for id in links.linked_review_ids {
            if !linked_review_ids.contains(&id) {
                linked_review_ids.push(id);
            }
        }
    }
    Ok(WebhookApplyOutcome {
        changed: true,
        ignored: false,
        repository_id: Some(repo_id),
        pr_number: first_pr,
        linked_task_ids,
        linked_review_ids,
    })
}

fn map_webhook_error(err: WebhookError) -> ApiError {
    match err {
        WebhookError::PayloadTooLarge => ApiError::new(ErrorCode::TooLarge, err.to_string()),
        WebhookError::MissingHeader(_)
        | WebhookError::MalformedSignature
        | WebhookError::InvalidSignature
        | WebhookError::SecretNotConfigured => {
            ApiError::new(ErrorCode::NotAuthorized, err.to_string())
        }
        WebhookError::InvalidDeliveryId
        | WebhookError::InvalidEventName
        | WebhookError::MalformedPayload(_) => {
            ApiError::new(ErrorCode::InvalidRequest, err.to_string())
        }
    }
}
