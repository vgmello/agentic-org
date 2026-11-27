//! Hardened `org-git` bare-cache & worktree inspection helpers for [`super::ReviewService`]
//! (`TASK-92.1`).
//!
//! Keeps `org-office` and `org-reviews` completely free of `org-git` while ensuring every code
//! review operation runs through [`ReadOnlyCache`], shared [`RepoLocks`], clean-worktree pinning,
//! bounded commit diffs, and tamper-evident `read_blob_lines` snippet hashes.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use org_git::{
    diff_commits, read_blob_lines, resolve_worktree_head, BlobLinesRequest, DiffCommitsRequest,
    DiffFileStatus, GitError, GitRunner, ReadOnlyCache, RepoLocks,
};
use org_office::git::{bare_cache_path, git_clean_home_path, OfficeGitState, TaskWorkspaceRecord};
use org_office::reviews::records::ResolvedCodePin;
use org_office::Office;
use org_proto::git::CacheStateWire;
use org_proto::reviews::{
    AnchorKind, FindingAnchor, ReviewDiff, ReviewDiffFile, ReviewDiffFileStatus, ReviewDiffResult,
    ReviewTargetKind, ReviewView,
};
use org_proto::{ApiError, ErrorCode, Response, MAX_FRAME_BYTES};
use org_redact::redact_text;

const MAX_LOG_BYTES: usize = 384;
const FRAME_HEADROOM_BYTES: usize = 512;

/// Reusable per-service Git inspection handle sharing the office's [`RepoLocks`].
pub(super) struct ReviewGit {
    locks: Arc<RepoLocks>,
    runner: Mutex<Option<GitRunner>>,
}

/// Inputs for pinning a code review round against the active task worktree and bare cache.
pub(super) struct CodePinQuery<'a> {
    pub(super) caller_id: &'a str,
    pub(super) repository_id: &'a str,
    pub(super) branch: &'a str,
    pub(super) task_id: Option<&'a str>,
    pub(super) requested_base_oid: Option<&'a str>,
    pub(super) expected_head_oid: Option<&'a str>,
}

/// Inputs for computing a bounded `review.diff` from the office bare cache.
pub(super) struct DiffQuery<'a> {
    pub(super) review_id: &'a str,
    pub(super) repository_id: &'a str,
    pub(super) round_number: u32,
    pub(super) since_round: Option<u32>,
    pub(super) base_oid: &'a str,
    pub(super) head_oid: &'a str,
    pub(super) path_filter: Option<&'a str>,
    pub(super) stat_only: bool,
    pub(super) context_lines: u32,
}

impl ReviewGit {
    pub(super) fn new(locks: Arc<RepoLocks>) -> Self {
        Self {
            locks,
            runner: Mutex::new(None),
        }
    }

    fn runner(&self, office: &Office) -> Result<GitRunner, ApiError> {
        let mut guard = self.runner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = guard.as_ref() {
            return Ok(existing.clone());
        }
        let clean_home = git_clean_home_path(office.runtime_root(), &office.info().office_id);
        let created = GitRunner::new(clean_home).map_err(map_git_error)?;
        *guard = Some(created.clone());
        Ok(created)
    }

    pub(super) fn load_state(&self, office: &Office) -> OfficeGitState {
        OfficeGitState::load(office.runtime_root(), &office.info().office_id).unwrap_or_default()
    }

    /// Pins a clean worktree head (refusing dirty worktrees with `Conflict`) and verifies both
    /// base and head commits exist in the office's bare cache.
    pub(super) fn resolve_code_pin(
        &self,
        office: &Office,
        q: &CodePinQuery<'_>,
    ) -> Result<ResolvedCodePin, ApiError> {
        let state = self.load_state(office);
        let bare = require_bare_cache_dir(office, &state, q.repository_id)?;
        let runner = self.runner(office)?;
        let cache = ReadOnlyCache::new(&runner, &self.locks, &bare, false);

        if let Some(ws) = find_active_workspace(
            &state,
            q.repository_id,
            q.branch,
            q.task_id,
            Some(q.caller_id),
        ) {
            let head =
                resolve_worktree_head(&cache, &ws.admin_dir_name, Path::new(&ws.worktree_path))
                    .map_err(map_git_error)?;
            if head.dirty {
                return Err(ApiError::new(
                    ErrorCode::Conflict,
                    "worktree has uncommitted changes; commit or clean before submitting for review",
                ));
            }
            if let Some(exp) = q.expected_head_oid {
                if exp.trim() != head.head_oid {
                    return Err(ApiError::new(
                        ErrorCode::Conflict,
                        "expected_head_oid does not match the worktree HEAD",
                    ));
                }
            }
            let base_oid = q
                .requested_base_oid
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(&ws.base_commit)
                .to_owned();
            verify_commit_pair(&cache, &base_oid, &head.head_oid)?;
            return Ok(ResolvedCodePin {
                base_oid,
                head_oid: head.head_oid,
            });
        }

        // No active local worktree: only succeed if caller named explicit commits present in cache.
        let (Some(base), Some(head)) = (
            q.requested_base_oid
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            q.expected_head_oid.map(str::trim).filter(|s| !s.is_empty()),
        ) else {
            return Err(ApiError::new(
                ErrorCode::NotFound,
                "no active worktree is prepared for this repository and branch",
            ));
        };
        verify_commit_pair(&cache, base, head)?;
        Ok(ResolvedCodePin {
            base_oid: base.to_owned(),
            head_oid: head.to_owned(),
        })
    }

    /// Computes a bounded diff between `q.base_oid` and `q.head_oid` in the bare cache.
    pub(super) fn diff_review_round(
        &self,
        office: &Office,
        q: &DiffQuery<'_>,
    ) -> Result<ReviewDiffResult, ApiError> {
        let state = self.load_state(office);
        let bare = require_bare_cache_dir(office, &state, q.repository_id)?;
        let runner = self.runner(office)?;
        let cache = ReadOnlyCache::new(&runner, &self.locks, &bare, false);
        let diff = diff_commits(
            &cache,
            &DiffCommitsRequest {
                base_oid: q.base_oid,
                head_oid: q.head_oid,
                path_filter: q.path_filter,
                stat_only: q.stat_only,
                context_lines: q.context_lines,
            },
        )
        .map_err(map_git_error)?;
        let files = diff
            .files
            .into_iter()
            .map(|f| ReviewDiffFile {
                path: f.path,
                status: map_diff_status(f.status),
                added: f.added,
                deleted: f.deleted,
                binary: f.binary,
            })
            .collect();
        Ok(ReviewDiffResult {
            review_id: q.review_id.to_owned(),
            round_number: q.round_number,
            since_round: q.since_round,
            base_oid: diff.base_oid,
            head_oid: diff.head_oid,
            files,
            total_files: diff.total_files,
            files_truncated: diff.files_truncated,
            unified_diff: diff.unified_diff,
            truncated: diff.truncated,
        })
    }

    /// Produces a compact stat summary between two commits for round 2..N warm review wakes.
    pub(super) fn format_inter_round_stat(
        &self,
        office: &Office,
        repository_id: &str,
        prev_head_oid: &str,
        new_head_oid: &str,
    ) -> Option<String> {
        let state = self.load_state(office);
        let bare = require_bare_cache_dir(office, &state, repository_id).ok()?;
        let runner = self.runner(office).ok()?;
        let cache = ReadOnlyCache::new(&runner, &self.locks, &bare, false);
        let diff = diff_commits(
            &cache,
            &DiffCommitsRequest {
                base_oid: prev_head_oid,
                head_oid: new_head_oid,
                path_filter: None,
                stat_only: true,
                context_lines: 0,
            },
        )
        .ok()?;
        let mut lines = Vec::new();
        for f in diff.files.iter().take(12) {
            let line = if f.binary {
                format!("  - {} ({}, binary)", f.path, f.status.as_str())
            } else {
                format!(
                    "  - {} ({}, +{} / -{})",
                    f.path,
                    f.status.as_str(),
                    f.added,
                    f.deleted
                )
            };
            lines.push(line);
        }
        if diff.total_files as usize > lines.len() {
            lines.push(format!(
                "  ... and {} more file(s)",
                diff.total_files as usize - lines.len()
            ));
        }
        Some(format!(
            "Inter-round diff ({} file(s) changed):\n{}",
            diff.total_files,
            lines.join("\n")
        ))
    }

    /// Validates a `file_line` anchor against `head_oid`, verifying line bounds and computing (or
    /// verifying) `anchor_snippet_sha256` via `read_blob_lines`.
    pub(super) fn validate_and_hash_anchor(
        &self,
        office: &Office,
        repository_id: &str,
        head_oid: &str,
        anchor: &mut FindingAnchor,
    ) -> Result<(), ApiError> {
        if anchor.kind != AnchorKind::FileLine {
            return Ok(());
        }
        anchor
            .check()
            .map_err(|msg| ApiError::new(ErrorCode::InvalidRequest, msg))?;
        let path = anchor.path.as_deref().unwrap_or_default();
        let start_line = anchor.start_line.unwrap_or(1);
        let end_line = anchor.last_line().unwrap_or(start_line);
        let line_count = end_line - start_line + 1;

        let state = self.load_state(office);
        let bare = require_bare_cache_dir(office, &state, repository_id)?;
        let runner = self.runner(office)?;
        let cache = ReadOnlyCache::new(&runner, &self.locks, &bare, false);
        let slice = read_blob_lines(
            &cache,
            &BlobLinesRequest {
                commit_oid: head_oid,
                path,
                start_line,
                line_count,
            },
        )
        .map_err(map_git_error)?;
        if end_line > slice.total_lines || slice.end_line < end_line {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                format!(
                    "anchor line {end_line} exceeds file length ({})",
                    slice.total_lines
                ),
            ));
        }
        if let Some(given) = anchor.anchor_snippet_sha256.as_deref() {
            if given != slice.anchor_snippet_sha256 {
                return Err(ApiError::new(
                    ErrorCode::Conflict,
                    "anchor_snippet_sha256 does not match the anchored lines at the round head",
                ));
            }
        } else {
            anchor.anchor_snippet_sha256 = Some(slice.anchor_snippet_sha256);
        }
        Ok(())
    }

    /// Returns whether a code review's active task worktree has moved past `pinned_head_oid`.
    pub(super) fn is_pin_stale(
        &self,
        office: &Office,
        state: &OfficeGitState,
        review: &ReviewView,
        pinned_head_oid: &str,
    ) -> bool {
        if review.target_kind != ReviewTargetKind::Code {
            return false;
        }
        let (Some(repo_id), Some(branch)) =
            (review.repository_id.as_deref(), review.branch.as_deref())
        else {
            return false;
        };
        let Some(ws) =
            find_active_workspace(state, repo_id, branch, review.task_id.as_deref(), None)
        else {
            return false;
        };
        let (Ok(bare), Ok(runner)) = (
            require_bare_cache_dir(office, state, repo_id),
            self.runner(office),
        ) else {
            return false;
        };
        let cache = ReadOnlyCache::new(&runner, &self.locks, &bare, false);
        resolve_worktree_head(&cache, &ws.admin_dir_name, Path::new(&ws.worktree_path))
            .map(|h| h.head_oid != pinned_head_oid)
            .unwrap_or(false)
    }

    /// Returns whether a `file_line` finding's anchored snippet differs at `current_head_oid`.
    pub(super) fn is_anchor_stale(
        &self,
        office: &Office,
        state: &OfficeGitState,
        repository_id: &str,
        current_head_oid: &str,
        anchor: &FindingAnchor,
    ) -> bool {
        if anchor.kind != AnchorKind::FileLine {
            return false;
        }
        let (Some(expected_sha), Some(path), Some(start_line)) = (
            anchor.anchor_snippet_sha256.as_deref(),
            anchor.path.as_deref(),
            anchor.start_line,
        ) else {
            return false;
        };
        let end_line = anchor.last_line().unwrap_or(start_line);
        let Ok(bare) = require_bare_cache_dir(office, state, repository_id) else {
            return false;
        };
        let Ok(runner) = self.runner(office) else {
            return false;
        };
        let cache = ReadOnlyCache::new(&runner, &self.locks, &bare, false);
        match read_blob_lines(
            &cache,
            &BlobLinesRequest {
                commit_oid: current_head_oid,
                path,
                start_line,
                line_count: end_line.saturating_sub(start_line) + 1,
            },
        ) {
            Ok(slice) => {
                end_line > slice.total_lines
                    || slice.end_line < end_line
                    || slice.anchor_snippet_sha256 != expected_sha
            }
            Err(_) => true,
        }
    }
}

/// Keeps a serialized `review.diff` reply strictly within [`MAX_FRAME_BYTES`] using UTF-8-safe
/// truncation on `unified_diff` and then `files`.
pub(super) fn fit_diff_frame(id: u64, mut diff: ReviewDiffResult) -> ReviewDiffResult {
    let fits = |res: &ReviewDiffResult| {
        Response::ok::<ReviewDiff>(id, res)
            .ok()
            .and_then(|r| serde_json::to_vec(&r).ok())
            .is_some_and(|b| b.len() + FRAME_HEADROOM_BYTES <= MAX_FRAME_BYTES)
    };
    while !fits(&diff) {
        if !diff.unified_diff.is_empty() {
            let next = diff.unified_diff.len() / 2;
            let cut = diff.unified_diff.floor_char_boundary(next);
            diff.unified_diff.truncate(cut);
            diff.truncated = true;
        } else if !diff.files.is_empty() {
            let next = diff.files.len() / 2;
            diff.files.truncate(next);
            diff.files_truncated = true;
        } else {
            break;
        }
    }
    diff
}

fn verify_commit_pair(
    cache: &ReadOnlyCache<'_>,
    base_oid: &str,
    head_oid: &str,
) -> Result<(), ApiError> {
    diff_commits(
        cache,
        &DiffCommitsRequest {
            base_oid,
            head_oid,
            path_filter: None,
            stat_only: true,
            context_lines: 0,
        },
    )
    .map(|_| ())
    .map_err(map_git_error)
}

fn require_bare_cache_dir(
    office: &Office,
    state: &OfficeGitState,
    repo_id: &str,
) -> Result<PathBuf, ApiError> {
    if let Some(cache) = state.caches.get(repo_id) {
        if cache.state == CacheStateWire::Ready {
            if let Some(path) = cache.bare_path.as_deref() {
                let candidate = PathBuf::from(path);
                if candidate.join("HEAD").is_file() {
                    return Ok(candidate);
                }
            }
        }
    }
    if let Ok(parsed) = org_store::Id::parse(repo_id) {
        let canonical = bare_cache_path(office.runtime_root(), &office.info().office_id, &parsed);
        if canonical.join("HEAD").is_file() {
            return Ok(canonical);
        }
    }
    Err(ApiError::new(
        ErrorCode::NotFound,
        "repository bare cache is not prepared in this office",
    ))
}

fn find_active_workspace<'a>(
    state: &'a OfficeGitState,
    repo_id: &str,
    branch: &str,
    task_id: Option<&str>,
    caller_id: Option<&str>,
) -> Option<&'a TaskWorkspaceRecord> {
    let matches = |w: &&TaskWorkspaceRecord| {
        w.state == "active"
            && w.repository_id == repo_id
            && w.branch == branch
            && task_id.is_none_or(|t| w.task_id == t)
    };
    if let Some(caller) = caller_id {
        if let Some(own) = state
            .workspaces
            .values()
            .find(|w| matches(w) && w.employee_id == caller)
        {
            return Some(own);
        }
    }
    state.workspaces.values().find(matches)
}

fn map_diff_status(status: DiffFileStatus) -> ReviewDiffFileStatus {
    match status {
        DiffFileStatus::Added => ReviewDiffFileStatus::Added,
        DiffFileStatus::Modified => ReviewDiffFileStatus::Modified,
        DiffFileStatus::Deleted => ReviewDiffFileStatus::Deleted,
        DiffFileStatus::TypeChanged => ReviewDiffFileStatus::TypeChanged,
    }
}

pub(super) fn map_git_error(err: GitError) -> ApiError {
    let redacted = scrub_log(&err.to_string());
    eprintln!("agentic-org-office: review git error: {redacted}");
    match err {
        GitError::InvalidInput(msg) => ApiError::new(ErrorCode::InvalidRequest, msg),
        GitError::Conflict(msg) => ApiError::new(ErrorCode::Conflict, msg),
        GitError::SecurityViolation(_) => ApiError::new(
            ErrorCode::InvalidRequest,
            "Git repository integrity or security check failed",
        ),
        GitError::UnsupportedGit(_) | GitError::RemoteUnavailable(_) => ApiError::new(
            ErrorCode::Unavailable,
            "office Git inspection is unavailable",
        ),
        GitError::CommandFailed { .. } | GitError::Io { .. } => {
            ApiError::new(ErrorCode::StorageError, "Git inspection failed")
        }
    }
}

fn scrub_log(raw: &str) -> String {
    let mut cleaned: String = redact_text(raw, &[])
        .chars()
        .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
        .collect();
    if cleaned.len() > MAX_LOG_BYTES {
        let cut = cleaned.floor_char_boundary(MAX_LOG_BYTES);
        cleaned.truncate(cut);
        cleaned.push_str("...[truncated]");
    }
    cleaned
}
