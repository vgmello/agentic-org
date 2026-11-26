//! The per-repository bare cache: integrity check, fetch, and recorded cache state.

use std::path::Path;

use org_git::{fetch_cache, verify_cache_integrity, GitError, GitRunner, RemoteSpec};
use org_office::git::RepoCacheState;
use org_office::organization_family::records::Repository;
use org_proto::git::CacheStateWire;
use org_proto::ApiError;

use super::redaction::{
    map_git_error, redact_git_error_for_log, sanitized_git_reason, scrub_git_log_text,
};
use super::GitService;

pub(super) fn ensure_bare_cache_ready(
    svc: &GitService,
    runner: &GitRunner,
    repo: &Repository,
    bare_path: &Path,
    remote: Result<&RemoteSpec, &ApiError>,
) -> Result<(), ApiError> {
    let already_exists = bare_path.join("HEAD").is_file();
    let remote = match remote {
        Ok(r) => r,
        Err(_) if already_exists => {
            verify_cache_integrity(bare_path, false).map_err(map_git_error)?;
            return Ok(());
        }
        Err(err) => return Err(err.clone()),
    };
    if !already_exists {
        let mut state_guard = svc.state.lock().unwrap_or_else(|e| e.into_inner());
        state_guard.caches.insert(
            repo.id.as_str().to_owned(),
            RepoCacheState {
                repository_id: repo.id.as_str().to_owned(),
                state: CacheStateWire::Cloning,
                bare_path: Some(bare_path.to_string_lossy().into_owned()),
                reason: None,
            },
        );
        let _ = svc.save_state_locked(&state_guard);
    }
    let op_res = fetch_cache(runner, bare_path, remote, false);
    let mut state_guard = svc.state.lock().unwrap_or_else(|e| e.into_inner());
    match op_res {
        Ok(()) => {
            state_guard.caches.insert(
                repo.id.as_str().to_owned(),
                RepoCacheState {
                    repository_id: repo.id.as_str().to_owned(),
                    state: CacheStateWire::Ready,
                    bare_path: Some(bare_path.to_string_lossy().into_owned()),
                    reason: None,
                },
            );
            svc.save_state_locked(&state_guard)?;
            Ok(())
        }
        Err(GitError::RemoteUnavailable(detail)) if already_exists => {
            eprintln!(
                "agentic-org-office: offline fetch fallback for cache {}: {}",
                repo.id,
                scrub_git_log_text(&detail)
            );
            Ok(())
        }
        Err(err) => {
            let sanitized = sanitized_git_reason(&err);
            eprintln!(
                "agentic-org-office: cache fetch failed for {}: {}",
                repo.id,
                redact_git_error_for_log(&err)
            );
            state_guard.caches.insert(
                repo.id.as_str().to_owned(),
                RepoCacheState {
                    repository_id: repo.id.as_str().to_owned(),
                    state: CacheStateWire::Failed,
                    bare_path: Some(bare_path.to_string_lossy().into_owned()),
                    reason: Some(sanitized),
                },
            );
            let _ = svc.save_state_locked(&state_guard);
            Err(map_git_error(err))
        }
    }
}
