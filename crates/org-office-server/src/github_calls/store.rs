//! Office-local durable JSON store for GitHub repository bindings, delivery replay dedup IDs,
//! and sanitized PR/review/CI snapshots under `durable/local/offices/<office-id>/github-context/`.
//!
//! Uses `0700` directories and `0600` atomic writes via `org_platform::fs::write_regular_beneath`,
//! `O_NOFOLLOW` reads, and strict size/item caps. Zero secrets are ever stored here.

use std::fs;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};

use org_github::{
    compute_ci_overall_status, DeliveryDeduplicator, ParsedPullRequestEvent, MAX_DEDUP_DELIVERIES,
};
use org_office::Office;
use org_proto::github::{
    GitHubCheckConclusion, GitHubCheckRunView, GitHubPrReviewCommentView, GitHubPrReviewView,
    GitHubPrState, GitHubPrSummaryView, GitHubRepoBindingView,
};
use org_proto::{ApiError, ErrorCode};
use serde::{Deserialize, Serialize};

const BINDINGS_FILE: &str = "bindings.v1.json";
const DELIVERIES_FILE: &str = "deliveries.v1.json";
pub(super) const MAX_STORE_FILE_BYTES: u64 = 1024 * 1024;
/// Deterministic serialized JSON ceiling for one `StoredPrRecord` (448 KiB).
///
/// Keeps every persisted PR file strictly `<= MAX_STORE_FILE_BYTES` (1 MiB) AND ensures derived
/// `GitHubPrDetailView` (record + <=16 KiB fenced context) and `GitHubPrChecksResult` (check runs
/// + failing check summaries) fit strictly below `org_proto::MAX_FRAME_BYTES` (1 MiB).
pub(super) const MAX_PR_RECORD_BYTES: usize = 448 * 1024;
const MAX_BINDINGS: usize = 256;
const MAX_REVIEWS_PER_PR: usize = 100;
const MAX_COMMENTS_PER_PR: usize = 200;
const MAX_CHECKS_PER_PR: usize = 100;
const MAX_PRS_PER_REPO_SCAN: usize = 500;

/// Persisted mapping between an Agentic Org repository UUIDv7 and a GitHub `owner/repo` slug.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct StoredRepoBinding {
    pub repository_id: String,
    pub owner: String,
    pub repo: String,
    pub full_name: String,
    pub installation_id: u64,
    pub bound_at: String,
}

impl StoredRepoBinding {
    pub(super) fn to_view(&self, repository_key: Option<String>) -> GitHubRepoBindingView {
        GitHubRepoBindingView {
            repository_id: self.repository_id.clone(),
            repository_key,
            owner: self.owner.clone(),
            repo: self.repo.clone(),
            full_name: self.full_name.clone(),
            installation_id: self.installation_id,
            bound_at: self.bound_at.clone(),
        }
    }
}

/// Persisted sanitized PR snapshot with upserted reviews, inline review comments, and CI checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct StoredPrRecord {
    pub repository_id: String,
    pub repo_full_name: String,
    pub pr_number: u64,
    pub state: GitHubPrState,
    pub draft: bool,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_excerpt: Option<String>,
    pub html_url: String,
    pub diff_url: String,
    pub head_ref: String,
    pub head_sha: String,
    pub base_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_sha: Option<String>,
    pub author_login: String,
    #[serde(default)]
    pub linked_task_refs: Vec<String>,
    pub updated_at: String,
    #[serde(default)]
    pub reviews: Vec<GitHubPrReviewView>,
    #[serde(default)]
    pub review_comments: Vec<GitHubPrReviewCommentView>,
    #[serde(default)]
    pub check_runs: Vec<GitHubCheckRunView>,
}

impl StoredPrRecord {
    pub(super) fn from_parsed(repository_id: String, ev: ParsedPullRequestEvent) -> Self {
        Self {
            repository_id,
            repo_full_name: ev.repo_full_name,
            pr_number: ev.pr_number,
            state: ev.state,
            draft: ev.draft,
            title: ev.title,
            body_excerpt: ev.body_excerpt,
            html_url: ev.html_url,
            diff_url: ev.diff_url,
            head_ref: ev.head_ref,
            head_sha: ev.head_sha,
            base_ref: ev.base_ref,
            base_sha: ev.base_sha,
            author_login: ev.author_login,
            linked_task_refs: ev.linked_task_refs,
            updated_at: ev.updated_at,
            reviews: Vec::new(),
            review_comments: Vec::new(),
            check_runs: Vec::new(),
        }
    }

    pub(super) fn apply_pull_request(&mut self, ev: ParsedPullRequestEvent) {
        self.repo_full_name = ev.repo_full_name;
        self.state = ev.state;
        self.draft = ev.draft;
        self.title = ev.title;
        self.body_excerpt = ev.body_excerpt;
        self.html_url = ev.html_url;
        self.diff_url = ev.diff_url;
        self.head_ref = ev.head_ref;
        self.head_sha = ev.head_sha;
        self.base_ref = ev.base_ref;
        self.base_sha = ev.base_sha;
        self.author_login = ev.author_login;
        self.linked_task_refs = ev.linked_task_refs;
        self.updated_at = ev.updated_at;
    }

    pub(super) fn upsert_review(&mut self, review: GitHubPrReviewView) {
        if let Some(existing) = self
            .reviews
            .iter_mut()
            .find(|r| r.review_id == review.review_id)
        {
            *existing = review;
            return;
        }
        if self.reviews.len() >= MAX_REVIEWS_PER_PR {
            self.reviews.remove(0);
        }
        self.reviews.push(review);
    }

    pub(super) fn upsert_review_comment(&mut self, comment: GitHubPrReviewCommentView) {
        if let Some(existing) = self
            .review_comments
            .iter_mut()
            .find(|c| c.comment_id == comment.comment_id)
        {
            *existing = comment;
            return;
        }
        if self.review_comments.len() >= MAX_COMMENTS_PER_PR {
            self.review_comments.remove(0);
        }
        self.review_comments.push(comment);
    }

    pub(super) fn upsert_check_run(&mut self, check: GitHubCheckRunView) {
        if let Some(existing) = self
            .check_runs
            .iter_mut()
            .find(|c| c.check_run_id == check.check_run_id)
        {
            *existing = check;
            return;
        }
        if self.check_runs.len() >= MAX_CHECKS_PER_PR {
            self.check_runs.remove(0);
        }
        self.check_runs.push(check);
    }

    /// Deterministically trims oldest/excess review comments, reviews, annotations, and check
    /// runs (preserving recent failing checks longest) so the pretty-printed record stays
    /// strictly `<= max_bytes` on disk and within RPC wire frame limits.
    pub(super) fn trim_to_budget(&mut self, max_bytes: usize) -> Result<(), ApiError> {
        while pretty_json_len(self)? > max_bytes {
            if self.review_comments.len() > 20 {
                let drop_n = (self.review_comments.len() - 20).div_ceil(2);
                self.review_comments.drain(..drop_n);
            } else if self.reviews.len() > 20 {
                let drop_n = (self.reviews.len() - 20).div_ceil(2);
                self.reviews.drain(..drop_n);
            } else if let Some(chk) = self.check_runs.iter_mut().find(|c| c.annotations.len() > 5) {
                chk.annotations.truncate(5);
            } else if let Some(idx) = self
                .check_runs
                .iter()
                .position(|c| !c.conclusion.is_some_and(GitHubCheckConclusion::is_failure))
            {
                self.check_runs.remove(idx);
            } else if !self.review_comments.is_empty() {
                let drop_n = (self.review_comments.len() / 2).max(1);
                self.review_comments.drain(..drop_n);
            } else if !self.reviews.is_empty() {
                let drop_n = (self.reviews.len() / 2).max(1);
                self.reviews.drain(..drop_n);
            } else if let Some(chk) = self
                .check_runs
                .iter_mut()
                .find(|c| !c.annotations.is_empty())
            {
                chk.annotations.clear();
            } else if self.check_runs.len() > 1 {
                self.check_runs.remove(0);
            } else if let Some(chk) = self.check_runs.first_mut().filter(|c| c.summary.is_some()) {
                chk.summary = None;
            } else if !self.check_runs.is_empty() {
                self.check_runs.clear();
            } else if self.body_excerpt.is_some() {
                self.body_excerpt = None;
            } else {
                break;
            }
        }
        Ok(())
    }

    pub(super) fn to_summary_view(
        &self,
        repository_key: Option<String>,
        linked_task_ids: Vec<String>,
        linked_review_ids: Vec<String>,
        linked_review_keys: Vec<String>,
    ) -> GitHubPrSummaryView {
        let failing = self
            .check_runs
            .iter()
            .filter(|c| c.conclusion.is_some_and(GitHubCheckConclusion::is_failure))
            .count();
        GitHubPrSummaryView {
            repository_id: self.repository_id.clone(),
            repository_key,
            repo_full_name: self.repo_full_name.clone(),
            pr_number: self.pr_number,
            state: self.state,
            draft: self.draft,
            title: self.title.clone(),
            body_excerpt: self.body_excerpt.clone(),
            html_url: self.html_url.clone(),
            diff_url: self.diff_url.clone(),
            head_ref: self.head_ref.clone(),
            head_sha: self.head_sha.clone(),
            base_ref: self.base_ref.clone(),
            base_sha: self.base_sha.clone(),
            author_login: self.author_login.clone(),
            linked_task_refs: self.linked_task_refs.clone(),
            linked_task_ids,
            linked_review_ids,
            linked_review_keys,
            ci_status: compute_ci_overall_status(&self.check_runs),
            failing_checks_count: u32::try_from(failing).unwrap_or(u32::MAX),
            total_checks_count: u32::try_from(self.check_runs.len()).unwrap_or(u32::MAX),
            review_count: u32::try_from(self.reviews.len()).unwrap_or(u32::MAX),
            review_comment_count: u32::try_from(self.review_comments.len()).unwrap_or(u32::MAX),
            updated_at: self.updated_at.clone(),
        }
    }
}

fn context_root(office: &Office) -> PathBuf {
    office
        .durable_root()
        .join("local")
        .join("offices")
        .join(&office.info().office_id)
        .join("github-context")
}

fn repo_dir_path(office: &Office, repo_id: &str) -> Result<PathBuf, ApiError> {
    if !org_proto::is_uuid_v7(repo_id) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "invalid repository_id",
        ));
    }
    Ok(context_root(office).join("repos").join(repo_id))
}

fn verify_no_symlink_ancestors(office: &Office, target: &Path) -> Result<(), ApiError> {
    let base = office.durable_root();
    let rel = target.strip_prefix(base).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "office github-context path escapes durable root",
        )
    })?;
    let mut cur = base.to_path_buf();
    for comp in rel.components() {
        cur.push(comp);
        match fs::symlink_metadata(&cur) {
            Ok(meta) if meta.file_type().is_dir() => {}
            Ok(_) => {
                return Err(ApiError::new(
                    ErrorCode::StorageError,
                    "office github-context ancestor must be a real directory (never a symlink)",
                ));
            }
            Err(_) => {
                return Err(ApiError::new(
                    ErrorCode::StorageError,
                    "could not inspect office github-context ancestor",
                ));
            }
        }
    }
    Ok(())
}

fn ensure_owner_only_dir(office: &Office, dir: &Path) -> Result<PathBuf, ApiError> {
    let uid = org_platform::fs::effective_uid().map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "office github-context storage is unsupported on this platform",
        )
    })?;
    let root = context_root(office);
    org_platform::fs::create_owner_only_dir_all(&root)
        .and_then(|()| org_platform::fs::set_owner_only_dir(&root))
        .map_err(|_| {
            ApiError::new(
                ErrorCode::StorageError,
                "could not prepare office github-context root directory",
            )
        })?;
    if dir != root {
        let repos_dir = root.join("repos");
        org_platform::fs::create_owner_only_dir_all(&repos_dir)
            .and_then(|()| org_platform::fs::set_owner_only_dir(&repos_dir))
            .map_err(|_| {
                ApiError::new(
                    ErrorCode::StorageError,
                    "could not prepare office github-context repos directory",
                )
            })?;
        org_platform::fs::create_owner_only_dir_all(dir)
            .and_then(|()| org_platform::fs::set_owner_only_dir(dir))
            .map_err(|_| {
                ApiError::new(
                    ErrorCode::StorageError,
                    "could not prepare office github-context directory",
                )
            })?;
    }
    verify_no_symlink_ancestors(office, dir)?;
    org_platform::fs::verify_owner_only_ancestors(dir, &root, uid).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "office github-context directory failed owner-only verification",
        )
    })?;
    fs::canonicalize(dir).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "could not resolve office github-context directory",
        )
    })
}

struct ByteCounter(usize);

impl io::Write for ByteCounter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0 = self.0.saturating_add(buf.len());
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn pretty_json_len<T: Serialize>(value: &T) -> Result<usize, ApiError> {
    let mut counter = ByteCounter(0);
    serde_json::to_writer_pretty(&mut counter, value).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "could not encode office github-context JSON",
        )
    })?;
    Ok(counter.0.saturating_add(1))
}

pub(super) fn compact_json_len<T: Serialize>(value: &T) -> Result<usize, ApiError> {
    let mut counter = ByteCounter(0);
    serde_json::to_writer(&mut counter, value).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "could not encode office github-context JSON",
        )
    })?;
    Ok(counter.0)
}

fn write_json_atomic<T: Serialize>(
    office: &Office,
    dir: &Path,
    file_name: &str,
    value: &T,
) -> Result<(), ApiError> {
    let canon_dir = ensure_owner_only_dir(office, dir)?;
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "could not encode office github-context JSON",
        )
    })?;
    bytes.push(b'\n');
    if bytes.len() as u64 > MAX_STORE_FILE_BYTES {
        return Err(ApiError::new(
            ErrorCode::StorageError,
            "office github-context file exceeds size limit",
        ));
    }
    org_platform::fs::write_regular_beneath(&canon_dir, Path::new(file_name), &bytes).map_err(
        |_| {
            ApiError::new(
                ErrorCode::StorageError,
                "could not persist office github-context file",
            )
        },
    )?;
    if let Ok(file) = org_platform::fs::open_nofollow_ro(&canon_dir.join(file_name)) {
        let _ = org_platform::fs::full_sync(&file);
    }
    Ok(())
}

fn read_json_optional<T: serde::de::DeserializeOwned>(
    office: &Office,
    dir: &Path,
    file_name: &str,
) -> Result<Option<T>, ApiError> {
    let path = dir.join(file_name);
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_file() => {}
        Ok(_) => {
            return Err(ApiError::new(
                ErrorCode::StorageError,
                "office github-context entry must be a regular file",
            ));
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err(ApiError::new(
                ErrorCode::StorageError,
                "could not inspect office github-context file",
            ));
        }
    }
    let uid = org_platform::fs::effective_uid().map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "office github-context storage is unsupported on this platform",
        )
    })?;
    verify_no_symlink_ancestors(office, dir)?;
    let root = context_root(office);
    org_platform::fs::verify_owner_only_ancestors(dir, &root, uid).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "office github-context directory failed owner-only verification",
        )
    })?;
    let verified = org_platform::fs::verify_owner_only_file(&path, uid, false).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "office github-context file failed owner-only verification",
        )
    })?;
    let file = org_platform::fs::open_nofollow_ro(&verified).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "could not open office github-context file",
        )
    })?;
    let mut raw = Vec::new();
    file.take(MAX_STORE_FILE_BYTES + 1)
        .read_to_end(&mut raw)
        .map_err(|_| {
            ApiError::new(
                ErrorCode::StorageError,
                "could not read office github-context file",
            )
        })?;
    if raw.len() as u64 > MAX_STORE_FILE_BYTES {
        return Err(ApiError::new(
            ErrorCode::StorageError,
            "office github-context file exceeds size limit",
        ));
    }
    serde_json::from_slice(&raw).map(Some).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "malformed office github-context JSON",
        )
    })
}

pub(super) fn load_bindings(office: &Office) -> Result<Vec<StoredRepoBinding>, ApiError> {
    let dir = context_root(office);
    Ok(read_json_optional(office, &dir, BINDINGS_FILE)?.unwrap_or_default())
}

pub(super) fn save_bindings(
    office: &Office,
    bindings: &[StoredRepoBinding],
) -> Result<(), ApiError> {
    if bindings.len() > MAX_BINDINGS {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "maximum number of GitHub repository bindings reached",
        ));
    }
    let dir = context_root(office);
    write_json_atomic(office, &dir, BINDINGS_FILE, &bindings)
}

pub(super) fn load_dedup(office: &Office) -> Result<DeliveryDeduplicator, ApiError> {
    let dir = context_root(office);
    let ids: Vec<String> = read_json_optional(office, &dir, DELIVERIES_FILE)?.unwrap_or_default();
    Ok(DeliveryDeduplicator::from_ids(ids, MAX_DEDUP_DELIVERIES))
}

pub(super) fn save_dedup(office: &Office, dedup: &DeliveryDeduplicator) -> Result<(), ApiError> {
    let dir = context_root(office);
    write_json_atomic(office, &dir, DELIVERIES_FILE, &dedup.snapshot())
}

pub(super) fn load_pr(
    office: &Office,
    repo_id: &str,
    pr_number: u64,
) -> Result<Option<StoredPrRecord>, ApiError> {
    if pr_number == 0 {
        return Ok(None);
    }
    let dir = repo_dir_path(office, repo_id)?;
    let name = format!("pr-{pr_number}.v1.json");
    read_json_optional(office, &dir, &name)
}

pub(super) fn save_pr(office: &Office, record: &mut StoredPrRecord) -> Result<(), ApiError> {
    record.trim_to_budget(MAX_PR_RECORD_BYTES)?;
    let dir = repo_dir_path(office, &record.repository_id)?;
    let name = format!("pr-{}.v1.json", record.pr_number);
    write_json_atomic(office, &dir, &name, record)
}

pub(super) fn list_prs_for_repo(
    office: &Office,
    repo_id: &str,
) -> Result<Vec<StoredPrRecord>, ApiError> {
    let dir = repo_dir_path(office, repo_id)?;
    let entries = match fs::read_dir(&dir) {
        Ok(it) => it,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => {
            return Err(ApiError::new(
                ErrorCode::StorageError,
                "could not list office github-context repository directory",
            ));
        }
    };
    let mut out = Vec::new();
    for entry in entries.flatten().take(MAX_PRS_PER_REPO_SCAN) {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !name.starts_with("pr-") || !name.ends_with(".v1.json") {
            continue;
        }
        if let Ok(Some(record)) = read_json_optional::<StoredPrRecord>(office, &dir, &name) {
            if record.repository_id == repo_id {
                out.push(record);
            }
        }
    }
    out.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| b.pr_number.cmp(&a.pr_number))
    });
    Ok(out)
}
