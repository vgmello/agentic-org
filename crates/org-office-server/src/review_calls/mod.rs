//! Universal reviews RPC service (`TASK-92.1`, `ADR-0026`, `ADR-0028`).
//!
//! Composes [`Office`] (domain engine), [`org_git`] bare-cache/worktree inspection, and
//! [`SessionManager`] review wakes inside `org-office-server` so `org-office` and `org-reviews`
//! keep zero `org-git` dependency.

mod attachments;
mod fields;
mod git;
mod read_ops;
mod scope;
#[cfg(test)]
mod tests;
mod wakes;
mod write_ops;

use std::sync::Arc;

use org_git::RepoLocks;
use org_office::Office;
use org_office_sessions::SessionManager;
use org_proto::reviews::{
    ReviewClose, ReviewDiff, ReviewFinding, ReviewInspect, ReviewList, ReviewRequest,
    ReviewResubmit, ReviewRoute, ReviewVerdictCall,
};
use org_proto::{ApiError, Call, ErrorCode, Request, Response};

use crate::live::LiveEvents;

use self::git::ReviewGit;
use self::write_ops::WriteDeps;

/// Matches all 9 `review.*` RPCs using explicit `request.is::<Call>()` checks so the host
/// operation inventory AST scanner discovers every served method directly in `org-office-server`.
pub(crate) fn is_review_method(request: &Request) -> bool {
    request.is::<ReviewRequest>()
        || request.is::<ReviewList>()
        || request.is::<ReviewInspect>()
        || request.is::<ReviewDiff>()
        || request.is::<ReviewRoute>()
        || request.is::<ReviewFinding>()
        || request.is::<ReviewVerdictCall>()
        || request.is::<ReviewResubmit>()
        || request.is::<ReviewClose>()
}

/// Live review orchestration service composed over [`Office`], [`RepoLocks`], and optional
/// [`SessionManager`].
pub(crate) struct ReviewService {
    office: Arc<Office>,
    live: Arc<LiveEvents>,
    git: ReviewGit,
    sessions: Option<Arc<SessionManager>>,
    sync_wake: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl ReviewService {
    pub(crate) fn new(
        office: Arc<Office>,
        live: Arc<LiveEvents>,
        locks: Arc<RepoLocks>,
        sessions: Option<Arc<SessionManager>>,
    ) -> Self {
        Self {
            office,
            live,
            git: ReviewGit::new(locks),
            sessions,
            sync_wake: None,
        }
    }

    pub(crate) fn with_sync_wake(mut self, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        self.sync_wake = Some(wake);
        self
    }

    pub(crate) fn dispatch(&self, member: Option<&str>, request: &Request) -> Option<Response> {
        if !is_review_method(request) {
            return None;
        }
        let Some(member) = member else {
            return Some(Response::error(
                request.id,
                ApiError::new(ErrorCode::NotBound, "connection is not bound"),
            ));
        };
        if self
            .office
            .sync_status()
            .is_ok_and(|s| s.initial_snapshot_required)
        {
            return Some(Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::Conflict,
                    "satellite office is awaiting a complete HQ snapshot",
                ),
            ));
        }
        self.dispatch_read(member, request)
            .or_else(|| self.dispatch_write(member, request))
    }

    fn dispatch_read(&self, member: &str, request: &Request) -> Option<Response> {
        if request.is::<ReviewList>() {
            return Some(parse_and_run::<ReviewList>(request, |p| {
                read_ops::handle_list(&self.office, &self.git, request.id, member, p)
            }));
        }
        if request.is::<ReviewInspect>() {
            return Some(parse_and_run::<ReviewInspect>(request, |p| {
                read_ops::handle_inspect(&self.office, &self.git, request.id, member, p)
            }));
        }
        if request.is::<ReviewDiff>() {
            return Some(parse_and_run::<ReviewDiff>(request, |p| {
                read_ops::handle_diff(&self.office, &self.git, request.id, member, p)
            }));
        }
        None
    }

    fn dispatch_write(&self, member: &str, request: &Request) -> Option<Response> {
        let deps = WriteDeps {
            office: &self.office,
            live: &self.live,
            git: &self.git,
            sessions: self.sessions.as_deref(),
            sync_wake: self.sync_wake.as_deref(),
        };
        if request.is::<ReviewRequest>() {
            return Some(parse_and_run::<ReviewRequest>(request, |p| {
                write_ops::handle_request(&deps, request.id, member, p)
            }));
        }
        if request.is::<ReviewRoute>() {
            return Some(parse_and_run::<ReviewRoute>(request, |p| {
                write_ops::handle_route(&deps, request.id, member, p)
            }));
        }
        if request.is::<ReviewFinding>() {
            return Some(parse_and_run::<ReviewFinding>(request, |p| {
                write_ops::handle_finding(&deps, request.id, member, p)
            }));
        }
        if request.is::<ReviewVerdictCall>() {
            return Some(parse_and_run::<ReviewVerdictCall>(request, |p| {
                write_ops::handle_verdict(&deps, request.id, member, p)
            }));
        }
        if request.is::<ReviewResubmit>() {
            return Some(parse_and_run::<ReviewResubmit>(request, |p| {
                write_ops::handle_resubmit(&deps, request.id, member, p)
            }));
        }
        if request.is::<ReviewClose>() {
            return Some(parse_and_run::<ReviewClose>(request, |p| {
                write_ops::handle_close(&deps, request.id, member, p)
            }));
        }
        None
    }
}

fn parse_and_run<C: Call>(request: &Request, f: impl FnOnce(C::Params) -> Response) -> Response {
    match request.params::<C>() {
        Ok(params) => f(params),
        Err(_) => Response::error(
            request.id,
            ApiError::new(
                ErrorCode::InvalidRequest,
                format!("the params of `{}` are not valid", C::METHOD),
            ),
        ),
    }
}
