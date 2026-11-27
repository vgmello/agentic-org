//! On-demand GitHub PR, review, comment, and CI check-run sync via short-lived per-repository
//! GitHub App installation tokens.

use org_proto::github::{GitHubCheckRunView, GitHubPrReviewCommentView, GitHubPrReviewView};
use serde::Deserialize;
use zeroize::Zeroizing;

use crate::config::{ValidatedApiBaseUrl, ValidatedRepoSlug};
use crate::error::GitHubError;
use crate::http::{
    GitHubHttpRequest, HttpMethod, DEFAULT_HTTP_TIMEOUT, MAX_GITHUB_HTTP_RESPONSE_BYTES,
};
use crate::jwt::GitHubAppPrivateKey;
use crate::token::InstallationTokenManager;
use crate::webhook::{
    normalize_annotations, normalize_check_run, normalize_pull_request, normalize_review,
    normalize_review_comment, ParsedPullRequestEvent, RawAnnotation, RawCheckRun, RawPullRequest,
    RawReview, RawReviewComment,
};

const MAX_SYNC_PRS: usize = 20;
const MAX_ANNOTATION_FETCHES_PER_PR: usize = 5;

/// Fully hydrated, sanitized pull request snapshot fetched from GitHub REST APIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncedPullRequest {
    pub pr: ParsedPullRequestEvent,
    pub reviews: Vec<GitHubPrReviewView>,
    pub review_comments: Vec<GitHubPrReviewCommentView>,
    pub check_runs: Vec<GitHubCheckRunView>,
}

/// Executes bounded read-only PR and CI check syncs using repository-scoped installation tokens.
pub struct GitHubSyncClient<'a> {
    tokens: &'a InstallationTokenManager,
    app_id: u64,
    installation_id: u64,
    api_base: &'a ValidatedApiBaseUrl,
    private_key: &'a GitHubAppPrivateKey,
}

impl<'a> GitHubSyncClient<'a> {
    pub fn new(
        tokens: &'a InstallationTokenManager,
        app_id: u64,
        installation_id: u64,
        api_base: &'a ValidatedApiBaseUrl,
        private_key: &'a GitHubAppPrivateKey,
    ) -> Self {
        Self {
            tokens,
            app_id,
            installation_id,
            api_base,
            private_key,
        }
    }

    /// Syncs a single PR (`pr_number`) including its reviews, inline review comments, and CI
    /// check runs (plus bounded failure annotations).
    pub fn sync_pull_request(
        &self,
        slug: &ValidatedRepoSlug,
        pr_number: u64,
        now_epoch_secs: u64,
    ) -> Result<SyncedPullRequest, GitHubError> {
        if pr_number == 0 {
            return Err(GitHubError::InvalidConfig(
                "pr_number must be positive".into(),
            ));
        }
        let token = self.tokens.get_token(
            self.app_id,
            self.installation_id,
            self.api_base,
            self.private_key,
            slug.repo(),
            now_epoch_secs,
        )?;
        let raw_pr: RawPullRequest = self.get_json(
            &format!("/repos/{}/{}/pulls/{pr_number}", slug.owner(), slug.repo()),
            &token,
        )?;
        let pr = normalize_pull_request(slug.full_name(), raw_pr)
            .map_err(|e| GitHubError::MalformedResponse(e.to_string()))?;
        self.hydrate_pr(slug, pr, &token)
    }

    /// Syncs up to `max_prs` (capped at 20) open PRs for `slug`.
    pub fn sync_open_pull_requests(
        &self,
        slug: &ValidatedRepoSlug,
        max_prs: usize,
        now_epoch_secs: u64,
    ) -> Result<Vec<SyncedPullRequest>, GitHubError> {
        let cap = max_prs.clamp(1, MAX_SYNC_PRS);
        let token = self.tokens.get_token(
            self.app_id,
            self.installation_id,
            self.api_base,
            self.private_key,
            slug.repo(),
            now_epoch_secs,
        )?;
        let raw_prs: Vec<RawPullRequest> = self.get_json(
            &format!(
                "/repos/{}/{}/pulls?state=open&per_page={cap}",
                slug.owner(),
                slug.repo()
            ),
            &token,
        )?;
        let mut out = Vec::with_capacity(raw_prs.len().min(cap));
        for raw_pr in raw_prs.into_iter().take(cap) {
            let pr = normalize_pull_request(slug.full_name(), raw_pr)
                .map_err(|e| GitHubError::MalformedResponse(e.to_string()))?;
            out.push(self.hydrate_pr(slug, pr, &token)?);
        }
        Ok(out)
    }

    fn hydrate_pr(
        &self,
        slug: &ValidatedRepoSlug,
        pr: ParsedPullRequestEvent,
        token: &Zeroizing<String>,
    ) -> Result<SyncedPullRequest, GitHubError> {
        let raw_reviews: Vec<RawReview> = self.get_json(
            &format!(
                "/repos/{}/{}/pulls/{}/reviews?per_page=50",
                slug.owner(),
                slug.repo(),
                pr.pr_number
            ),
            token,
        )?;
        let reviews = raw_reviews
            .into_iter()
            .take(50)
            .map(normalize_review)
            .collect();

        let raw_comments: Vec<RawReviewComment> = self.get_json(
            &format!(
                "/repos/{}/{}/pulls/{}/comments?per_page=50",
                slug.owner(),
                slug.repo(),
                pr.pr_number
            ),
            token,
        )?;
        let review_comments = raw_comments
            .into_iter()
            .take(50)
            .map(normalize_review_comment)
            .collect();

        let check_runs = self.fetch_check_runs(slug, &pr.head_sha, token)?;

        Ok(SyncedPullRequest {
            pr,
            reviews,
            review_comments,
            check_runs,
        })
    }

    fn fetch_check_runs(
        &self,
        slug: &ValidatedRepoSlug,
        head_sha: &str,
        token: &Zeroizing<String>,
    ) -> Result<Vec<GitHubCheckRunView>, GitHubError> {
        if head_sha.is_empty()
            || head_sha.len() > 64
            || !head_sha.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Ok(Vec::new());
        }
        #[derive(Deserialize)]
        struct CheckRunsEnvelope {
            #[serde(default)]
            check_runs: Vec<RawCheckRun>,
        }
        let env: CheckRunsEnvelope = self.get_json(
            &format!(
                "/repos/{}/{}/commits/{head_sha}/check-runs?per_page=50",
                slug.owner(),
                slug.repo()
            ),
            token,
        )?;
        let mut out = Vec::with_capacity(env.check_runs.len().min(50));
        let mut fetched_annotations = 0usize;
        for raw in env.check_runs.into_iter().take(50) {
            let needs_annotations = raw
                .output
                .as_ref()
                .is_some_and(|o| o.annotations.is_empty() && o.annotations_count > 0);
            let mut view = normalize_check_run(raw);
            if needs_annotations
                && fetched_annotations < MAX_ANNOTATION_FETCHES_PER_PR
                && view.conclusion.is_some_and(|c| c.is_failure())
            {
                if let Ok(raw_anns) = self.get_json::<Vec<RawAnnotation>>(
                    &format!(
                        "/repos/{}/{}/check-runs/{}/annotations?per_page=20",
                        slug.owner(),
                        slug.repo(),
                        view.check_run_id
                    ),
                    token,
                ) {
                    view.annotations = normalize_annotations(raw_anns);
                }
                fetched_annotations += 1;
            }
            out.push(view);
        }
        Ok(out)
    }

    fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        api_path: &str,
        token: &Zeroizing<String>,
    ) -> Result<T, GitHubError> {
        let url = self.api_base.endpoint(api_path)?;
        let req = GitHubHttpRequest {
            method: HttpMethod::Get,
            url,
            bearer_token: token.clone(),
            json_body: None,
            timeout: DEFAULT_HTTP_TIMEOUT,
        };
        let resp = self.tokens.http().send(&req)?;
        if resp.body.len() > MAX_GITHUB_HTTP_RESPONSE_BYTES {
            return Err(GitHubError::MalformedResponse(
                "GitHub API response exceeded 2 MiB".into(),
            ));
        }
        match resp.status {
            200..=299 => serde_json::from_slice(&resp.body).map_err(|_| {
                GitHubError::MalformedResponse("could not decode GitHub API JSON".into())
            }),
            401 | 403 => Err(GitHubError::AuthRejected(resp.status)),
            404 => Err(GitHubError::NotFound),
            429 => Err(GitHubError::RateLimited),
            code => Err(GitHubError::Transport(format!(
                "GitHub API returned HTTP {code}"
            ))),
        }
    }
}
