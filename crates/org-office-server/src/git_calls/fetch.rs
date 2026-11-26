//! `repo.fetch` (`TASK-173`): an on-demand, office-mediated refresh of one repository's bare
//! cache from its origin, so every task worktree of this office sees newly pushed
//! `origin/*` branches. Credentials stay office-side ([`resolve_remote_spec`]); the agent's
//! sandbox never gets them and never needs to write `refs/remotes`.
//!
//! One fetch per repository runs at a time ([`FetchGate`]), and a repository is fetched at most
//! once per cooldown; refused calls get `rate_limited` with `retry_after_ms`. The fetch holds
//! the repository lock, so it serializes with `worktree.prepare` and `git.push` on that cache.

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use org_git::{refresh_cache, CacheRefresh, GitError};
use org_office::git::bare_cache_path;
use org_office::organization_family::records::Repository;
use org_proto::git::{
    CacheStateWire, RepoFetchParams, RepoFetchResult, REPO_FETCH_MAX_LISTED_BRANCHES,
};
use org_proto::{ApiError, ErrorCode};

use super::cache::{fetch_recording_state, FetchOutcome};
use super::credentials::resolve_remote_spec;
use super::redaction::{map_git_error, map_office_error, scrub_git_log_text};
use super::GitService;

/// Least time between the end of one origin fetch of a repository and the start of the next.
pub(super) const FETCH_COOLDOWN: Duration = Duration::from_secs(10);

const OFFLINE_KEPT: &str =
    "Git remote is unreachable or refused the fetch; the office cache was left unchanged";
const OFFLINE_UNPREPARED: &str =
    "Git remote is unreachable or refused the fetch; the office cache is not ready";

/// Per-repository single flight and cooldown for `repo.fetch`. Its map lock is held only to
/// check and update a slot, never across the fetch itself.
pub(super) struct FetchGate {
    cooldown: Duration,
    slots: Mutex<BTreeMap<String, FetchSlot>>,
}

#[derive(Default)]
struct FetchSlot {
    in_flight: bool,
    last_finished: Option<Instant>,
}

impl FetchGate {
    pub(super) fn new(cooldown: Duration) -> Self {
        Self {
            cooldown,
            slots: Mutex::new(BTreeMap::new()),
        }
    }

    /// Claims the repository's fetch slot, or refuses with `rate_limited`. Callers pass only
    /// resolved repository IDs, so the map is bounded by the registered repositories.
    pub(super) fn begin(&self, repository_id: &str) -> Result<FetchPermit<'_>, ApiError> {
        let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        let slot = slots.entry(repository_id.to_owned()).or_default();
        if slot.in_flight {
            return Err(rate_limited(
                self.cooldown,
                "a fetch of this repository is already running in this office",
            ));
        }
        if let Some(wait) = slot
            .last_finished
            .and_then(|done| self.cooldown.checked_sub(done.elapsed()))
            .filter(|wait| !wait.is_zero())
        {
            return Err(rate_limited(
                wait,
                "this repository was fetched moments ago",
            ));
        }
        slot.in_flight = true;
        Ok(FetchPermit {
            gate: self,
            repository_id: repository_id.to_owned(),
        })
    }
}

/// Releases the slot and starts the cooldown when dropped, also when the fetch fails or panics.
pub(super) struct FetchPermit<'a> {
    gate: &'a FetchGate,
    repository_id: String,
}

impl Drop for FetchPermit<'_> {
    fn drop(&mut self) {
        let mut slots = self
            .gate
            .slots
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(slot) = slots.get_mut(&self.repository_id) {
            slot.in_flight = false;
            slot.last_finished = Some(Instant::now());
        }
    }
}

fn rate_limited(wait: Duration, why: &str) -> ApiError {
    let retry_after_ms = u64::try_from(wait.as_millis()).unwrap_or(u64::MAX).max(1);
    let seconds = retry_after_ms.div_ceil(1000);
    let mut error = ApiError::new(
        ErrorCode::RateLimited,
        format!("{why}; retry in {seconds}s"),
    );
    error.retry_after_ms = Some(retry_after_ms);
    error
}

impl GitService {
    /// Any member bound to this office may refresh a registered repository's cache: the fetch
    /// is read-only against the origin and changes only `origin/*` refs, tags and objects.
    pub(super) fn handle_fetch(
        &self,
        params: RepoFetchParams,
    ) -> Result<RepoFetchResult, ApiError> {
        let repo = self
            .office
            .resolve_repository(&params.repository)
            .map_err(map_office_error)?
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "repository not found"))?;
        let remote = resolve_remote_spec(&self.office, &repo)?;
        let _permit = self.fetch_gate.begin(repo.id.as_str())?;
        let runner = self.runner()?;
        let office_id = self.office.info().office_id.clone();
        let bare_path = bare_cache_path(self.office.runtime_root(), &office_id, &repo.id);
        let outcome = self
            .locks
            .with_lock(&bare_path, || {
                Ok(fetch_recording_state(self, &repo, &bare_path, || {
                    refresh_cache(&runner, &bare_path, &remote, false)
                }))
            })
            .map_err(map_git_error)??;
        let refresh = match outcome {
            FetchOutcome::Fetched(refresh) => refresh,
            FetchOutcome::OfflineKept(detail) => {
                eprintln!(
                    "agentic-org-office: repo.fetch kept the cache of {}: {}",
                    repo.id,
                    scrub_git_log_text(&detail)
                );
                return Err(ApiError::new(ErrorCode::Unavailable, OFFLINE_KEPT));
            }
            FetchOutcome::Failed(GitError::RemoteUnavailable(_)) => {
                return Err(ApiError::new(ErrorCode::Unavailable, OFFLINE_UNPREPARED));
            }
            FetchOutcome::Failed(err) => return Err(map_git_error(err)),
        };
        self.live.record_mailbox_change();
        Ok(fetch_result(office_id, &repo, refresh, now_rfc3339()))
    }
}

fn fetch_result(
    office_id: String,
    repo: &Repository,
    refresh: CacheRefresh,
    fetched_at: String,
) -> RepoFetchResult {
    let head_oid = refresh.head_oid().map(str::to_owned);
    RepoFetchResult {
        office_id,
        repository_id: repo.id.as_str().to_owned(),
        repository_key: repo.key.clone(),
        state: CacheStateWire::Ready,
        default_branch: refresh.default_branch,
        head_oid,
        branch_count: count(refresh.branches.len()),
        added_count: count(refresh.added.len()),
        added: capped(refresh.added),
        updated_count: count(refresh.updated.len()),
        updated: capped(refresh.updated),
        pruned_count: count(refresh.pruned.len()),
        pruned: capped(refresh.pruned),
        fetched_at,
    }
}

fn capped(mut names: Vec<String>) -> Vec<String> {
    names.truncate(REPO_FETCH_MAX_LISTED_BRANCHES);
    names
}

fn count(len: usize) -> u64 {
    u64::try_from(len).unwrap_or(u64::MAX)
}

fn now_rfc3339() -> String {
    let epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    org_proto::format_rfc3339_ms(epoch_ms).unwrap_or_else(|| "1970-01-01T00:00:00.000Z".into())
}
