//! The per-repository bare cache: integrity check, fetch, and recorded cache state. Both
//! `worktree.prepare` and `repo.fetch` fetch the origin through [`fetch_recording_state`].

use std::path::Path;

use org_git::{fetch_cache, verify_cache_integrity, GitError, GitRunner, RemoteSpec};
use org_office::git::RepoCacheState;
use org_office::organization_family::records::Repository;
use org_proto::git::CacheStateWire;
use org_proto::ApiError;

use super::redaction::{
    map_git_error, map_remote_git_error, redact_git_error_for_log, sanitized_remote_git_reason,
    scrub_git_log_text,
};
use super::GitService;

/// What one origin fetch did to the cache, after its state was recorded.
pub(super) enum FetchOutcome<T> {
    /// The fetch succeeded and the cache is `ready`.
    Fetched(T),
    /// The origin was unreachable, but the existing cache stays usable and its state is kept.
    /// Carries the raw Git detail, which only goes to a log through the redaction choke point.
    OfflineKept(String),
    /// The fetch failed and the cache is recorded `failed` with a sanitized reason.
    Failed(GitError),
}

/// Runs one origin `fetch` into `bare_path` under the caller's repository lock and records the
/// cache state: `cloning` while a cache that is not yet usable is filled, `ready` on success,
/// `failed` unless a usable cache merely could not reach its origin. `Err` only when persisting
/// a successful state fails.
pub(super) fn fetch_recording_state<T>(
    svc: &GitService,
    repo: &Repository,
    bare_path: &Path,
    remote: Option<&RemoteSpec>,
    fetch: impl FnOnce() -> Result<T, GitError>,
) -> Result<FetchOutcome<T>, ApiError> {
    let usable = cache_is_usable(svc, repo, bare_path);
    if !usable {
        record_cache_state(svc, repo, bare_path, CacheStateWire::Cloning, None);
    }
    match fetch() {
        Ok(value) => {
            let mut state_guard = svc.state.lock().unwrap_or_else(|e| e.into_inner());
            state_guard.caches.insert(
                repo.id.as_str().to_owned(),
                cache_state(repo, bare_path, CacheStateWire::Ready, None),
            );
            svc.save_state_locked(&state_guard)?;
            Ok(FetchOutcome::Fetched(value))
        }
        Err(GitError::RemoteUnavailable(detail)) if usable => Ok(FetchOutcome::OfflineKept(detail)),
        Err(err) => {
            eprintln!(
                "agentic-org-office: cache fetch failed for {}: {}",
                repo.id,
                redact_git_error_for_log(&err)
            );
            let reason = sanitized_remote_git_reason(&err, remote);
            record_cache_state(svc, repo, bare_path, CacheStateWire::Failed, Some(reason));
            Ok(FetchOutcome::Failed(err))
        }
    }
}

/// Whether the cache at `bare_path` is on disk and recorded `ready`. A failed or interrupted
/// first fill leaves a `HEAD` behind with no usable refs, recorded `failed` or `cloning` (or
/// not at all), so `HEAD` alone never makes a cache usable.
fn cache_is_usable(svc: &GitService, repo: &Repository, bare_path: &Path) -> bool {
    if !bare_path.join("HEAD").is_file() {
        return false;
    }
    let state_guard = svc.state.lock().unwrap_or_else(|e| e.into_inner());
    state_guard
        .caches
        .get(repo.id.as_str())
        .is_some_and(|cache| cache.state == CacheStateWire::Ready)
}

fn cache_state(
    repo: &Repository,
    bare_path: &Path,
    state: CacheStateWire,
    reason: Option<String>,
) -> RepoCacheState {
    RepoCacheState {
        repository_id: repo.id.as_str().to_owned(),
        state,
        bare_path: Some(bare_path.to_string_lossy().into_owned()),
        reason,
    }
}

/// Records a transitional or failed state; persisting it is best effort.
fn record_cache_state(
    svc: &GitService,
    repo: &Repository,
    bare_path: &Path,
    state: CacheStateWire,
    reason: Option<String>,
) {
    let mut state_guard = svc.state.lock().unwrap_or_else(|e| e.into_inner());
    state_guard.caches.insert(
        repo.id.as_str().to_owned(),
        cache_state(repo, bare_path, state, reason),
    );
    let _ = svc.save_state_locked(&state_guard);
}

pub(super) fn ensure_bare_cache_ready(
    svc: &GitService,
    runner: &GitRunner,
    repo: &Repository,
    bare_path: &Path,
    remote: Result<&RemoteSpec, &ApiError>,
) -> Result<(), ApiError> {
    let remote = match remote {
        Ok(r) => r,
        Err(_) if cache_is_usable(svc, repo, bare_path) => {
            verify_cache_integrity(bare_path, false).map_err(map_git_error)?;
            return Ok(());
        }
        Err(err) => return Err(err.clone()),
    };
    match fetch_recording_state(svc, repo, bare_path, Some(remote), || {
        fetch_cache(runner, bare_path, remote, false)
    })? {
        FetchOutcome::Fetched(()) => Ok(()),
        FetchOutcome::OfflineKept(detail) => {
            eprintln!(
                "agentic-org-office: offline fetch fallback for cache {}: {}",
                repo.id,
                scrub_git_log_text(&detail)
            );
            Ok(())
        }
        FetchOutcome::Failed(err) => {
            let repo_ref = repo.key.as_deref().unwrap_or(repo.id.as_str());
            Err(map_remote_git_error(err, remote, repo_ref))
        }
    }
}
